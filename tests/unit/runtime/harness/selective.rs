use super::*;

fn selective_fixture(root: &Path) -> (Workspace, ToolHarness) {
    fs::create_dir_all(root.join("src/target")).unwrap();
    fs::create_dir_all(root.join("src/noise")).unwrap();
    fs::write(
        root.join("src/target/worker.rs"),
        "pub fn worker() {\n    let diagnostic_marker = 42;\n    let _ = diagnostic_marker;\n}\n",
    )
    .unwrap();
    for index in 0..128 {
        fs::write(
            root.join(format!("src/noise/irrelevant_{index}.rs")),
            format!("pub fn irrelevant_{index}() {{}}\n"),
        )
        .unwrap();
    }
    (
        Workspace::new(root, true, false).unwrap(),
        ToolHarness::new(4).unwrap(),
    )
}

#[test]
fn selective_context_avoids_unrelated_indexing() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    for pass in ["cold", "warm"] {
        let started = Instant::now();
        let pack = harness
            .agent_context("demo", &workspace, "src/target/worker.rs:2", 4_000, &[])
            .unwrap();
        let elapsed_us = started.elapsed().as_micros();
        let outline = harness
            .file_outline("demo", &workspace, "src/target/worker.rs", 100)
            .unwrap();
        let indexed = outline["index"]["indexed_files"].as_u64().unwrap();
        eprintln!("selective_context pass={pass} unrelated_files=128 indexed_files={indexed} elapsed_us={elapsed_us}");
        assert_eq!(pack["targets"][0]["qualified_name"], "worker");
        assert!(pack["hot_source"][0]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("diagnostic_marker"));
        assert_eq!(pack["files"][0]["sha256"], pack["hot_source"][0]["sha256"]);
        assert_eq!(
            indexed, 1,
            "an exact location should not index unrelated source"
        );
        assert_eq!(pack["repo_map"]["deferred"], true);
    }
}

#[test]
fn selective_context_retains_relationship_and_scope_discovery() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "inspect callers of src/target/worker.rs:2",
            4_000,
            &[],
        )
        .unwrap();
    assert_ne!(pack["repo_map"]["deferred"], true);
    assert_eq!(pack["targets"][0]["qualified_name"], "worker");
    let ordinary = harness
        .agent_context("demo", &workspace, "irrelevant_42", 4_000, &[])
        .unwrap();
    assert!(ordinary["targets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|target| target["qualified_name"] == "irrelevant_42"));
}

#[test]
fn selective_context_keeps_guidance_and_design_verification() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    fs::write(
        root.path().join("AGENTS.md"),
        "Preserve the guidance_sentinel contract.\n",
    )
    .unwrap();
    fs::create_dir_all(root.path().join(".wcode/design")).unwrap();
    fs::write(
        root.path().join(".wcode/project.yaml"),
        "schema_version: 1\nname: demo\n",
    )
    .unwrap();
    fs::write(root.path().join(".wcode/design/requirements.yaml"), "- schema_version: 1\n  id: REQ-WORKER\n  title: worker\n  intent: Inspect src/target/worker.rs behavior.\n  priority: high\n  implemented_by: [component:worker]\n  acceptance: [AC-WORKER]\n  constraints: []\n  risk: {}\n").unwrap();
    fs::write(root.path().join(".wcode/design/components.yaml"), "- schema_version: 1\n  id: component:worker\n  name: worker\n  responsibilities: [worker]\n  depends_on: []\n  constraints: []\n  implementation:\n    - kind: symbol\n      path: src/target/worker.rs\n      symbol: worker\n").unwrap();
    fs::write(root.path().join(".wcode/design/acceptance.yaml"), "- schema_version: 1\n  id: AC-WORKER\n  title: worker\n  statement: worker obeys its contract.\n  verification:\n    - kind: test\n      path: src/target/worker.rs\n      symbol: worker\n").unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "src/target/worker.rs:2", 8_000, &[])
        .unwrap();
    assert!(pack["guidance"].to_string().contains("guidance_sentinel"));
    assert!(pack["design"].to_string().contains("REQ-WORKER"));
    assert!(pack["tests"]
        .to_string()
        .contains("src/target/worker.rs::worker"));
    assert_eq!(pack["readiness"]["verify"], "ready");
}

#[test]
fn task_guidance_extracts_relevant_lines_instead_of_repository_overview() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    fs::write(
        root.path().join("README.md"),
        "# Demo\nrepository_overview_sentinel is generic project marketing.\n\n## Release\nUnrelated release notes live here.\n\n## Worker timeout\nworker_timeout_rule: edits to worker must preserve the diagnostic timeout boundary.\nFollow worker_timeout_rule before changing worker behavior.\n",
    )
    .unwrap();

    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "fix worker_timeout_rule in worker",
            4_000,
            &[],
        )
        .unwrap();
    let guidance = pack["guidance"].to_string();
    assert!(guidance.contains("worker_timeout_rule"));
    assert!(!guidance.contains("repository_overview_sentinel"));
}

#[test]
fn selective_missing_location_does_not_trigger_global_search() {
    let root = tempfile::tempdir().unwrap();
    let (workspace, harness) = selective_fixture(root.path());
    let pack = harness
        .agent_context("demo", &workspace, "src/missing.rs:2", 4_000, &[])
        .unwrap();
    assert!(pack["targets"].as_array().unwrap().is_empty());
    assert!(pack["hot_source"].as_array().unwrap().is_empty());
    assert_eq!(pack["retrieval"]["resolved"], 0);
    let outline = harness
        .file_outline("demo", &workspace, "src/target/worker.rs", 100)
        .unwrap();
    assert_eq!(outline["index"]["indexed_files"], 1);
}

#[test]
fn selective_context_preserves_multilanguage_originals_and_budgets() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for (path, source, line) in [
        ("worker.py", "def worker():\n    return 42\n", 2),
        (
            "worker.ts",
            "export function worker() {\n  return 42;\n}\n",
            2,
        ),
        (
            "worker.go",
            "package fixture\nfunc worker() int {\n  return 42\n}\n",
            3,
        ),
        ("settings.json", "{\n  \"value\": 42\n}\n", 2),
    ] {
        fs::write(root.path().join(path), source).unwrap();
        for budget in [1_000, 4_000] {
            let pack = harness
                .agent_context("demo", &workspace, &format!("{path}:{line}"), budget, &[])
                .unwrap();
            assert_eq!(pack["files"][0]["path"], path);
            assert_eq!(pack["hot_source"][0]["body"]["start_line"], line);
            assert!(pack["hot_source"][0]["body"]["content"]
                .as_str()
                .unwrap()
                .contains("42"));
            assert_eq!(pack["hot_source"][0]["sha256"], pack["files"][0]["sha256"]);
            assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
            assert_eq!(pack["repo_map"]["deferred"], true);
        }
    }
}

#[tokio::test]
async fn oversized_verification_plan_fails_before_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = crate::workspace::Workspaces::new([root.path()], false, true).unwrap();
    workspaces.revoke_command(None, "git").unwrap();
    let (id, workspace) = workspaces.select(None).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let mut oversized = (*profile).clone();
    oversized.recommended_checks = (0..=MAX_VERIFICATION_CHECKS)
        .map(|index| CheckSpec {
            id: format!("fixture-{index}"),
            level: "full".to_owned(),
            phase: 0,
            program: "git".to_owned(),
            args: vec!["diff".to_owned(), "--check".to_owned()],
            cwd: ".".to_owned(),
            island: "workspace".to_owned(),
            languages: Vec::new(),
            reason: "over-capacity fixture".to_owned(),
        })
        .collect();
    harness
        .project_cache
        .lock()
        .unwrap()
        .get_mut(workspace.root())
        .unwrap()
        .profile = Arc::new(oversized);
    let monitor = TaskMonitor::new([id.clone()]);
    let error = harness
        .verify_project_mode(id, &workspace, ("full", false), 1, &monitor)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no checks executed"));
    assert!(workspaces.authorization_requests(10).is_empty());
    assert!(crate::evidence_store::load(&workspace).unwrap().is_empty());
}

#[tokio::test]
async fn mixed_language_verification_accounts_for_every_inferred_check() {
    let root = tempfile::tempdir().unwrap();
    for (path, content) in [
        ("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'\n"),
        ("pyproject.toml", "[project]\nname='fixture'\n"),
        ("go.mod", "module fixture\n"),
        ("Makefile", "check:\nlint:\ntest:\n"),
        ("package.json", "{\"scripts\":{\"lint\":\"node --version\",\"typecheck\":\"node --version\",\"check\":\"node --version\",\"format:check\":\"node --version\",\"test\":\"node --version\",\"build\":\"node --version\"}}"),
    ] {
        fs::write(root.path().join(path), content).unwrap();
    }
    let workspaces = crate::workspace::Workspaces::new([root.path()], false, true).unwrap();
    // Exercise the real scheduler without launching any external program.
    for program in ["cargo", "npm", "pytest", "go", "make"] {
        workspaces.revoke_command(None, program).unwrap();
    }
    let (id, workspace) = workspaces.select(None).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let expected = profile
        .recommended_checks
        .iter()
        .map(|check| check.id.clone())
        .collect::<BTreeSet<_>>();
    for required in [
        "rust-check",
        "node-lint",
        "python-tests",
        "go-vet",
        "go-tests",
        "make-check",
    ] {
        assert!(
            expected.contains(required),
            "mixed-language profile must retain native check {required}"
        );
    }
    assert!(expected.len() >= 16);
    let monitor = TaskMonitor::new([id.clone()]);
    let report = harness
        .verify_project_mode(id, &workspace, ("full", false), 1, &monitor)
        .await
        .unwrap();
    let actual = report
        .checks
        .iter()
        .map(|check| check.id.clone())
        .collect::<BTreeSet<_>>();
    eprintln!(
        "mixed_verification inferred={} accounted={}",
        expected.len(),
        actual.len()
    );
    assert_eq!(
        actual, expected,
        "full diagnostics must not silently drop inferred checks"
    );
    assert!(!report.passed);
    assert!(report.skipped_checks.is_empty());
}

#[test]
fn polyglot_language_islands_bind_checks_to_manifest_roots_and_changed_islands() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("bridge/src")).unwrap();
    fs::create_dir_all(root.path().join("service/src/main/java")).unwrap();
    fs::create_dir_all(root.path().join("ios/Sources/App")).unwrap();
    fs::create_dir_all(root.path().join("mobile/lib")).unwrap();
    fs::create_dir_all(root.path().join("beam/lib")).unwrap();
    fs::create_dir_all(root.path().join("ml/lib")).unwrap();
    fs::create_dir_all(root.path().join("ruby/spec")).unwrap();
    fs::create_dir_all(root.path().join("php/src")).unwrap();
    fs::create_dir_all(root.path().join("web/src")).unwrap();
    fs::create_dir_all(root.path().join("shared/src")).unwrap();
    fs::create_dir_all(root.path().join("tools/tests")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='root-rust'\nversion='0.1.0'\n\n[dependencies]\nbridge={path='bridge'}\n",
    )
    .unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn root() {}\n").unwrap();
    fs::write(
        root.path().join("bridge/Cargo.toml"),
        "[package]\nname='bridge'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("bridge/pyproject.toml"),
        "[project]\nname='bridge-tools'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("bridge/src/lib.rs"),
        "pub fn bridge() {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("service/pom.xml"),
        "<project><modelVersion>4.0.0</modelVersion></project>\n",
    )
    .unwrap();
    fs::write(
        root.path().join("ios/Package.swift"),
        "// swift-tools-version: 5.9\n",
    )
    .unwrap();
    fs::write(
        root.path().join("mobile/pubspec.yaml"),
        "name: mobile\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
    )
    .unwrap();
    fs::write(root.path().join("mobile/lib/app.dart"), "void main() {}\n").unwrap();
    fs::write(
        root.path().join("beam/mix.exs"),
        "defmodule Beam.MixProject do\n  use Mix.Project\n  def project, do: [app: :beam, version: \"0.1.0\"]\nend\n",
    )
    .unwrap();
    fs::write(
        root.path().join("beam/lib/beam.ex"),
        "defmodule Beam do\nend\n",
    )
    .unwrap();
    fs::write(root.path().join("ml/dune-project"), "(lang dune 3.0)\n").unwrap();
    fs::write(root.path().join("ml/lib/core.ml"), "let value = 1\n").unwrap();
    fs::write(
        root.path().join("ruby/Gemfile"),
        "source 'https://rubygems.org'\ngem 'rubocop'\ngem 'rspec'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("ruby/spec/smoke_spec.rb"),
        "RSpec.describe('smoke') { it { expect(1).to eq(1) } }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("php/composer.json"),
        r#"{"name":"demo/php","require-dev":{"phpstan/phpstan":"^2","phpunit/phpunit":"^11"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("php/src/App.php"),
        "<?php final class App {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("shared/package.json"),
        r#"{"name":"shared-ui","scripts":{"test":"node --test"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("shared/src/index.ts"),
        "export const shared = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("web/package.json"),
        r#"{"dependencies":{"shared-ui":"file:../shared"},"scripts":{"lint":"eslint .","test":"node --test","build":"tsc -b"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("web/tsconfig.json"),
        r#"{"compilerOptions":{"strict":true}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("web/src/app.ts"),
        "export const value: number = 1;\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tools/pyproject.toml"),
        "[project]\nname='tools'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tools/tests/test_smoke.py"),
        "def test_ok(): assert True\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let roots = profile
        .islands
        .iter()
        .map(|island| island.root.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        roots,
        BTreeSet::from([
            ".", "beam", "bridge", "ios", "ml", "mobile", "php", "ruby", "service", "shared",
            "tools", "web",
        ])
    );
    let root_island = profile
        .islands
        .iter()
        .find(|island| island.root == ".")
        .unwrap();
    assert!(root_island.dependencies.iter().any(|dependency| {
        dependency.island == "bridge"
            && dependency.kind == "cargo_path"
            && dependency.provider == "manifest-dependency"
            && dependency.precision == "structural"
    }));
    let web = profile
        .islands
        .iter()
        .find(|island| island.root == "web")
        .unwrap();
    assert!(web.project_types.contains(&"node".to_owned()));
    assert!(web.languages.contains(&"type-script".to_owned()));
    assert!(web.manifests.contains(&"web/package.json".to_owned()));
    assert!(web.check_ids.iter().all(|id| id.starts_with("web:")));
    assert!(web
        .dependencies
        .iter()
        .any(|dependency| { dependency.island == "shared" && dependency.kind == "package_local" }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "web:node-lint" && check.cwd == "web" && check.island == "web"
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "tools:python-tests"
            && check.cwd == "tools"
            && check.island == "tools"
            && check.languages.contains(&"python".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "service:java-maven-compile"
            && check.level == "quick"
            && check.cwd == "service"
            && check.languages.contains(&"java".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "service:java-maven-test"
            && check.cwd == "service"
            && check.languages.contains(&"java".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "ios:swift-build"
            && check.level == "quick"
            && check.cwd == "ios"
            && check.languages.contains(&"swift".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "ios:swift-test"
            && check.cwd == "ios"
            && check.languages.contains(&"swift".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "mobile:dart-analyze"
            && check.level == "quick"
            && check.cwd == "mobile"
            && check.languages.contains(&"dart".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "beam:elixir-compile"
            && check.level == "quick"
            && check.cwd == "beam"
            && check.languages.contains(&"elixir".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "ml:ocaml-build"
            && check.level == "quick"
            && check.cwd == "ml"
            && check.languages.contains(&"ocaml".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "ruby:ruby-rubocop"
            && check.level == "quick"
            && check.cwd == "ruby"
            && check.languages.contains(&"ruby".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "ruby:ruby-rspec"
            && check.level == "full"
            && check.cwd == "ruby"
            && check.languages.contains(&"ruby".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "php:php-phpstan"
            && check.level == "quick"
            && check.cwd == "php"
            && check.languages.contains(&"php".to_owned())
    }));
    assert!(profile.recommended_checks.iter().any(|check| {
        check.id == "php:php-phpunit"
            && check.level == "full"
            && check.cwd == "php"
            && check.languages.contains(&"php".to_owned())
    }));

    let web_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"web/src/app.ts"}]
    });
    let web_plan = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&web_snapshot),
        "full",
    );
    assert!(web_plan.iter().any(|check| check.id == "git-diff-check"));
    assert!(web_plan.iter().any(|check| check.island == "web"));
    assert!(!web_plan.iter().any(|check| check.island == "."));
    assert!(!web_plan.iter().any(|check| check.island == "tools"));

    let mixed_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"web/src/app.ts"}, {"path":"tools/tests/test_smoke.py"}]
    });
    let mixed_plan = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&mixed_snapshot),
        "full",
    );
    let affected = mixed_plan
        .iter()
        .filter(|check| check.island != "workspace")
        .map(|check| check.island.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(affected, BTreeSet::from(["tools", "web"]));
    assert!(!mixed_plan.iter().any(|check| check.island == "."));

    let root_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"src/lib.rs"}]
    });
    let root_plan = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&root_snapshot),
        "full",
    );
    assert!(root_plan.iter().any(|check| check.island == "."));
    assert!(!root_plan.iter().any(|check| check.island == "web"));
    assert!(!root_plan.iter().any(|check| check.island == "tools"));

    let bridge_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"bridge/src/lib.rs"}]
    });
    let bridge_plan = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&bridge_snapshot),
        "full",
    );
    assert!(bridge_plan.iter().any(|check| check.island == "bridge"));
    assert!(bridge_plan.iter().any(|check| check.island == "."));
    assert!(!bridge_plan.iter().any(|check| check.island == "web"));

    let shared_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"shared/src/index.ts"}]
    });
    let shared_plan = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&shared_snapshot),
        "full",
    );
    assert!(shared_plan.iter().any(|check| check.island == "shared"));
    assert!(shared_plan.iter().any(|check| check.island == "web"));
    assert!(!shared_plan.iter().any(|check| check.island == "."));
}

#[test]
fn polyglot_verification_gaps_are_scoped_to_the_affected_island() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("mobile/lib")).unwrap();
    fs::create_dir_all(root.path().join("legacy")).unwrap();
    fs::write(
        root.path().join("mobile/pubspec.yaml"),
        "name: mobile\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
    )
    .unwrap();
    fs::write(root.path().join("mobile/lib/app.dart"), "void main() {}\n").unwrap();
    fs::write(
        root.path().join("legacy/composer.json"),
        "{\"name\":\"demo/legacy\"}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("legacy/index.php"),
        "<?php echo 'legacy';\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();
    let mobile = profile
        .islands
        .iter()
        .find(|island| island.root == "mobile")
        .unwrap();
    assert_eq!(mobile.verification_status, "native");
    assert!(mobile.verification_gaps.is_empty());
    let legacy = profile
        .islands
        .iter()
        .find(|island| island.root == "legacy")
        .unwrap();
    assert_eq!(legacy.verification_status, "manifest_only");
    assert_eq!(legacy.verification_gaps, vec!["php"]);

    let mobile_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"mobile/lib/app.dart"}]
    });
    assert!(
        super::super::harness_profile::verification_gaps_for_snapshot(
            &profile,
            Some(&mobile_snapshot),
            "quick",
        )
        .is_empty()
    );

    let legacy_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"legacy/index.php"}]
    });
    let gaps = super::super::harness_profile::verification_gaps_for_snapshot(
        &profile,
        Some(&legacy_snapshot),
        "quick",
    );
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].root, "legacy");
    assert_eq!(gaps[0].project_types, vec!["php"]);
    assert_eq!(gaps[0].level, "quick");
}

#[test]
fn polyglot_quick_verification_promotes_a_bounded_full_fallback_and_full_stays_strict() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("py/src")).unwrap();
    fs::create_dir_all(root.path().join("web/src")).unwrap();
    fs::write(
        root.path().join("py/pyproject.toml"),
        "[project]\nname='py'\nversion='0.1.0'\n",
    )
    .unwrap();
    fs::write(root.path().join("py/src/app.py"), "VALUE = 1\n").unwrap();
    fs::write(
        root.path().join("web/package.json"),
        r#"{"scripts":{"lint":"eslint ."},"devDependencies":{"eslint":"latest"}}"#,
    )
    .unwrap();
    fs::write(
        root.path().join("web/src/app.js"),
        "export const value = 1;\n",
    )
    .unwrap();

    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(2).unwrap();
    let (profile, _) = harness.load_project_profile(&workspace).unwrap();

    let py_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"py/src/app.py"}]
    });
    let quick = super::super::harness_profile::verification_checks_for_snapshot(
        &profile,
        Some(&py_snapshot),
        "quick",
    );
    assert!(quick.iter().any(|check| {
        check.id == "py:python-tests" && check.reason.starts_with("Quick fallback for python:")
    }));
    assert!(
        super::super::harness_profile::verification_gaps_for_snapshot(
            &profile,
            Some(&py_snapshot),
            "quick",
        )
        .is_empty()
    );

    let web_snapshot = json!({
        "available": true,
        "truncated": false,
        "files": [{"path":"web/src/app.js"}]
    });
    let full_gaps = super::super::harness_profile::verification_gaps_for_snapshot(
        &profile,
        Some(&web_snapshot),
        "full",
    );
    assert_eq!(full_gaps.len(), 1);
    assert_eq!(full_gaps[0].root, "web");
    assert_eq!(full_gaps[0].project_types, vec!["node"]);
    assert_eq!(full_gaps[0].level, "full");
}
