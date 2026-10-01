use super::*;

fn scan_limits(entries: usize, depth: usize, contracts: usize) -> ScanLimits {
    ScanLimits {
        entries,
        depth,
        contracts,
    }
}

fn write_manifest(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}

#[test]
fn profile_discovery_entry_budget_requires_actual_omission() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), "Cargo.toml", "[package]\nname = \"demo\"\n");
    let exact = scan_with_limits(root.path(), scan_limits(2, 8, 64));
    assert!(exact.completeness.complete);
    assert_eq!(exact.completeness.entries_seen, 2);
    write_manifest(root.path(), "README.md", "extra\n");
    let partial = scan_with_limits(root.path(), scan_limits(2, 8, 64));
    assert!(!partial.completeness.complete);
    assert_eq!(partial.completeness.entries_seen, 3);
    assert!(partial
        .completeness
        .reasons
        .iter()
        .any(|reason| reason == "scan_entry_budget"));
    assert_eq!(partial.manifest_files.len(), 1);
}

#[test]
fn profile_discovery_depth_is_explicit_unknown_and_recovers() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        root.path(),
        "pkg/Cargo.toml",
        "[package]\nname = \"nested\"\n",
    );
    let shallow = scan_with_limits(root.path(), scan_limits(100, 0, 64));
    assert!(!shallow.completeness.complete);
    assert!(shallow
        .completeness
        .reasons
        .iter()
        .any(|reason| reason == "scan_depth_unknown"));
    assert!(shallow.candidate_dirs.is_empty());
    let recovered = scan_with_limits(root.path(), scan_limits(100, 1, 64));
    assert!(recovered.completeness.complete);
    assert_eq!(recovered.completeness.manifest_candidates, 1);
}

#[test]
fn profile_discovery_contract_budget_requires_extra_config() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), "buf.yaml", "version: v1\n");
    let exact = scan_with_limits(root.path(), scan_limits(100, 8, 1));
    assert!(exact.completeness.complete);
    assert_eq!(exact.completeness.contract_configs_seen, 1);
    write_manifest(root.path(), "buf.gen.yaml", "version: v1\n");
    let partial = scan_with_limits(root.path(), scan_limits(100, 8, 1));
    assert!(!partial.completeness.complete);
    assert_eq!(partial.completeness.contract_configs_seen, 2);
    assert_eq!(partial.contract_configs.len(), 2);
    assert!(partial
        .completeness
        .reasons
        .iter()
        .any(|reason| reason == "contract_config_budget"));
}

#[test]
fn profile_discovery_real_io_error_is_relative_and_recovers() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing");
    let partial = scan(&missing);
    assert!(!partial.completeness.complete);
    assert!(partial.completeness.errors_total > 0);
    assert_eq!(partial.completeness.errors[0].path, ".");
    assert_eq!(partial.completeness.errors[0].kind, "not_found");
    assert!(partial
        .completeness
        .reasons
        .iter()
        .any(|reason| reason == "scan_io_error"));
    let public = serde_json::to_string(&partial.completeness).unwrap();
    assert!(!public.contains(&root.path().to_string_lossy().to_string()));
    fs::create_dir(&missing).unwrap();
    assert!(scan(&missing).completeness.complete);
}

#[test]
fn profile_discovery_expected_exclusions_are_not_errors() {
    let root = tempfile::tempdir().unwrap();
    for path in [".venv/pkg/package.json", "target/pkg/package.json"] {
        write_manifest(root.path(), path, "{}");
    }
    let discovery = scan(root.path());
    assert!(discovery.completeness.complete);
    assert!(discovery.candidate_dirs.is_empty());
    assert!(discovery.completeness.errors.is_empty());
}

#[test]
fn profile_discovery_island_limit_distinguishes_exact_bound_and_recovers_cache() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), "Cargo.toml", "[package]\nname = \"root\"\n");
    for index in 0..MAX_PROFILE_ISLANDS {
        write_manifest(root.path(), &format!("node-{index:02}/package.json"), "{}");
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (exact, hit) = harness.load_project_profile(&workspace).unwrap();
    assert!(!hit);
    assert!(exact.discovery.complete);
    assert_eq!(exact.discovery.islands_returned, MAX_PROFILE_ISLANDS + 1);
    assert!(harness.load_project_profile(&workspace).unwrap().1);
    write_manifest(root.path(), "node-extra/package.json", "{}");
    let (partial, hit) = harness.load_project_profile(&workspace).unwrap();
    assert!(!hit);
    assert!(!partial.discovery.complete);
    assert!(partial
        .discovery
        .reasons
        .iter()
        .any(|reason| reason == "island_budget"));
    assert_eq!(partial.discovery.islands_returned, MAX_PROFILE_ISLANDS + 1);
    assert!(partial
        .recommended_checks
        .iter()
        .any(|check| check.id == "rust-check"));
    assert!(
        !harness.load_project_profile(&workspace).unwrap().1,
        "partial inventories cannot become complete via a cache hit"
    );
    fs::remove_file(root.path().join("node-extra/package.json")).unwrap();
    fs::remove_dir(root.path().join("node-extra")).unwrap();
    let (recovered, hit) = harness.load_project_profile(&workspace).unwrap();
    assert!(!hit);
    assert!(recovered.discovery.complete);
    assert!(harness.load_project_profile(&workspace).unwrap().1);
}

#[test]
fn profile_discovery_content_fingerprint_detects_same_size_and_restored_mtime() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("package.json");
    let before = r#"{"scripts":{"lint":"echo a"}}"#;
    let after = r#"{"scripts":{"lint":"echo b"}}"#;
    assert_eq!(before.len(), after.len());
    fs::write(&path, before).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    assert!(!harness.load_project_profile(&workspace).unwrap().1);
    assert!(harness.load_project_profile(&workspace).unwrap().1);
    fs::write(&path, after).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(
        !harness.load_project_profile(&workspace).unwrap().1,
        "same metadata must not hide a native check-definition edit"
    );
    assert!(harness.load_project_profile(&workspace).unwrap().1);
}

#[test]
fn profile_discovery_source_budget_is_partial_and_recovery_is_fresh() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), "Cargo.toml", "[package]\nname = \"root\"\n");
    write_manifest(
        root.path(),
        "package.json",
        &" ".repeat(MAX_PROFILE_SOURCE_BYTES as usize + 1),
    );
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (partial, _) = harness.load_project_profile(&workspace).unwrap();
    assert!(!partial.discovery.complete);
    assert!(partial
        .discovery
        .reasons
        .iter()
        .any(|reason| reason == "profile_source_budget"));
    assert!(partial
        .recommended_checks
        .iter()
        .any(|check| check.id == "rust-check"));
    assert!(!harness.load_project_profile(&workspace).unwrap().1);
    write_manifest(
        root.path(),
        "package.json",
        r#"{"scripts":{"lint":"echo check"}}"#,
    );
    let (recovered, hit) = harness.load_project_profile(&workspace).unwrap();
    assert!(!hit);
    assert!(recovered.discovery.complete);
    assert!(recovered
        .recommended_checks
        .iter()
        .any(|check| check.id == "node-lint"));
}

#[test]
fn profile_discovery_project_context_exposes_bounded_completeness() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), "Cargo.toml", "[package]\nname = \"root\"\n");
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let context = harness.project_context("demo", &workspace).unwrap();
    assert!(context.discovery.complete);
    assert_eq!(context.discovery.islands_returned, 1);
    let public = serde_json::to_value(context).unwrap();
    assert_eq!(public["discovery"]["complete"], true);
    assert!(public["discovery"]["errors"].is_array());
    assert!(public["discovery"]["reasons"].is_array());
}

#[cfg(unix)]
#[test]
fn profile_discovery_does_not_follow_symlink_trees_or_check_sources() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write_manifest(
        outside.path(),
        "pkg/package.json",
        r#"{"scripts":{"lint":"secret"}}"#,
    );
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked")).unwrap();
    let mut discovery = scan(root.path());
    assert!(discovery.completeness.complete);
    assert!(discovery.candidate_dirs.is_empty());
    std::os::unix::fs::symlink(
        outside.path().join("pkg/package.json"),
        root.path().join("package.json"),
    )
    .unwrap();
    let mut hasher = DefaultHasher::new();
    fingerprint_sources(root.path(), &mut discovery, &mut hasher);
    assert!(!discovery.completeness.complete);
    assert!(discovery
        .completeness
        .errors
        .iter()
        .any(|issue| issue.path == "package.json" && issue.kind == "symlink"));
    let public = serde_json::to_string(&discovery.completeness).unwrap();
    assert!(!public.contains("secret"));
    assert!(!public.contains(&outside.path().to_string_lossy().to_string()));
}

#[cfg(unix)]
#[test]
fn profile_discovery_error_details_are_bounded_without_raw_messages() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private"), "token=never-public").unwrap();
    for name in PROFILE_FILES {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(outside.path().join("private"), path).unwrap();
    }
    let mut discovery = scan(root.path());
    let mut hasher = DefaultHasher::new();
    fingerprint_sources(root.path(), &mut discovery, &mut hasher);
    assert!(!discovery.completeness.complete);
    assert!(discovery.completeness.diagnostics_truncated);
    assert!(discovery.completeness.errors_total > MAX_DISCOVERY_DIAGNOSTICS);
    assert_eq!(
        discovery.completeness.errors.len(),
        MAX_DISCOVERY_DIAGNOSTICS
    );
    assert!(discovery.completeness.reasons.len() <= MAX_DISCOVERY_DIAGNOSTICS);
    let public = serde_json::to_string(&discovery.completeness).unwrap();
    assert!(!public.contains("never-public"));
    assert!(!public.contains(&outside.path().to_string_lossy().to_string()));
}

#[test]
fn profile_discovery_invalid_encoding_and_json_are_partial_not_cached() {
    for invalid in [&[0xff][..], &b"{broken"[..], &b"[]"[..]] {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), "Cargo.toml", "[package]\nname = \"root\"\n");
        fs::write(root.path().join("package.json"), invalid).unwrap();
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let harness = ToolHarness::new(2).unwrap();
        let (partial, hit) = harness.load_project_profile(&workspace).unwrap();
        assert!(!hit);
        assert!(!partial.discovery.complete);
        assert!(partial
            .discovery
            .reasons
            .iter()
            .any(|reason| reason == "profile_source_invalid"));
        assert!(partial
            .discovery
            .errors
            .iter()
            .any(|issue| issue.path == "package.json"));
        assert!(partial
            .recommended_checks
            .iter()
            .any(|check| check.id == "rust-check"));
        assert!(!harness.load_project_profile(&workspace).unwrap().1);
        write_manifest(
            root.path(),
            "package.json",
            r#"{"scripts":{"lint":"echo check"}}"#,
        );
        let (recovered, hit) = harness.load_project_profile(&workspace).unwrap();
        assert!(!hit);
        assert!(recovered.discovery.complete);
        assert!(recovered
            .recommended_checks
            .iter()
            .any(|check| check.id == "node-lint"));
        assert!(harness.load_project_profile(&workspace).unwrap().1);
    }
}

#[test]
fn profile_discovery_actual_toml_and_contract_parse_failures_are_partial() {
    for (path, invalid) in [("Cargo.toml", "["), ("codegen.yaml", "schema: [")] {
        let root = tempfile::tempdir().unwrap();
        write_manifest(root.path(), "Cargo.toml", "[package]\nname = \"root\"\n");
        write_manifest(root.path(), path, invalid);
        let workspace = Workspace::new(root.path(), true, false).unwrap();
        let harness = ToolHarness::new(2).unwrap();
        let (partial, _) = harness.load_project_profile(&workspace).unwrap();
        assert!(!partial.discovery.complete, "{path}");
        assert!(partial
            .discovery
            .reasons
            .iter()
            .any(|reason| reason == "profile_source_invalid"));
        assert!(!harness.load_project_profile(&workspace).unwrap().1);
    }
}

#[test]
fn profile_discovery_fingerprint_and_node_parser_share_captured_body() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(
        root.path(),
        "package.json",
        r#"{"scripts":{"lint":"echo old"}}"#,
    );
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let (_, captured) = super::super::project_fingerprint(&workspace);
    write_manifest(
        root.path(),
        "package.json",
        r#"{"scripts":{"test":"echo new"}}"#,
    );
    let (built, issues) = with_captured_sources(workspace.root(), &captured, || {
        super::super::build_project_profile(&workspace, &captured)
    });
    let built = built.unwrap();
    assert!(issues.complete);
    assert!(built
        .recommended_checks
        .iter()
        .any(|check| check.id == "node-lint"));
    assert!(!built
        .recommended_checks
        .iter()
        .any(|check| check.id == "node-test"));
    let latest = ToolHarness::new(2)
        .unwrap()
        .load_project_profile(&workspace)
        .unwrap()
        .0;
    assert!(latest
        .recommended_checks
        .iter()
        .any(|check| check.id == "node-test"));
    assert!(!latest
        .recommended_checks
        .iter()
        .any(|check| check.id == "node-lint"));
}

#[test]
fn profile_discovery_contract_parser_uses_capture_and_restores_scope() {
    let root = tempfile::tempdir().unwrap();
    write_manifest(root.path(), "codegen.yaml", "schema: schema.graphql\n");
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let (_, captured) = super::super::project_fingerprint(&workspace);
    write_manifest(root.path(), "codegen.yaml", "schema: [");
    let (built, issues) = with_captured_sources(workspace.root(), &captured, || {
        super::super::build_project_profile(&workspace, &captured)
    });
    assert!(issues.complete);
    assert!(!built
        .unwrap()
        .contracts
        .diagnostics
        .iter()
        .any(|issue| issue.reason == "invalid_static_config"));
    let latest = ToolHarness::new(2)
        .unwrap()
        .load_project_profile(&workspace)
        .unwrap()
        .0;
    assert!(!latest.discovery.complete);
    assert!(latest
        .contracts
        .diagnostics
        .iter()
        .any(|issue| issue.reason == "invalid_static_config"));
}

#[test]
fn profile_discovery_uncaptured_new_input_is_not_parsed_live() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let (_, captured) = super::super::project_fingerprint(&workspace);
    assert!(captured.completeness.complete);
    let missing = captured.policy_sources(workspace.root()).unwrap();
    assert!(missing
        .iter()
        .any(|source| source.path == "requirements-dev.txt" && source.sha256.is_none()));
    let path = workspace.root().join("requirements-dev.txt");
    fs::write(&path, "pytest\n").unwrap();
    let (text, issues) = with_captured_sources(workspace.root(), &captured, || {
        super::super::read_small_text(&path)
    });
    assert!(text.is_none());
    assert!(!issues.complete);
    assert!(issues
        .reasons
        .iter()
        .any(|reason| reason == "profile_capture_incomplete"));
    assert_eq!(issues.errors[0].path, "requirements-dev.txt");
    assert_eq!(issues.errors[0].kind, "uncaptured_input");
    assert_eq!(captured.policy_sources(workspace.root()).unwrap(), missing);
    assert_eq!(
        super::super::read_small_text(&path).as_deref(),
        Some("pytest\n")
    );
    let latest = super::super::capture_policy_profile(&workspace).unwrap();
    assert!(latest.discovery.complete);
    assert!(latest.policy_sources.iter().any(|source| {
        source.path == "requirements-dev.txt"
            && source.sha256.as_deref()
                == Some(format!("{:x}", Sha256::digest(b"pytest\n")).as_str())
    }));
}

#[test]
fn profile_discovery_absent_and_binary_captures_remain_complete() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let absent_path = workspace.root().join("requirements-dev.txt");
    let (_, absent) = super::super::project_fingerprint(&workspace);
    let (text, issues) = with_captured_sources(workspace.root(), &absent, || {
        super::super::read_small_text(&absent_path)
    });
    assert!(text.is_none());
    assert!(issues.complete);
    assert!(issues.errors.is_empty());

    let bytes = [0xff, 0x00, 0x81];
    let binary_path = workspace.root().join("bun.lockb");
    fs::write(&binary_path, bytes).unwrap();
    let (_, binary) = super::super::project_fingerprint(&workspace);
    assert!(binary.completeness.complete);
    let (text, issues) = with_captured_sources(workspace.root(), &binary, || {
        super::super::read_small_text(&binary_path)
    });
    assert!(text.is_none());
    assert!(issues.complete);
    let profile = super::super::capture_policy_profile(&workspace).unwrap();
    assert!(profile.discovery.complete);
    assert!(profile.policy_sources.iter().any(|source| {
        source.path == "bun.lockb"
            && source.sha256.as_deref() == Some(format!("{:x}", Sha256::digest(bytes)).as_str())
    }));
    assert!(profile
        .policy_sources
        .iter()
        .any(|source| source.path == "requirements-dev.txt" && source.sha256.is_none()));
}

#[cfg(unix)]
#[test]
fn profile_discovery_uncaptured_metadata_errors_are_partial() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let (_, captured) = super::super::project_fingerprint(&workspace);
    fs::write(workspace.root().join("new-parent"), "not a directory").unwrap();
    let path = workspace.root().join("new-parent/requirements-dev.txt");
    let (text, issues) = with_captured_sources(workspace.root(), &captured, || {
        super::super::read_small_text(&path)
    });
    assert!(text.is_none());
    assert!(!issues.complete);
    assert!(issues
        .reasons
        .iter()
        .any(|reason| reason == "profile_capture_incomplete"));
    assert_eq!(issues.errors[0].path, "new-parent/requirements-dev.txt");
    assert_eq!(issues.errors[0].kind, "not_directory");
}
