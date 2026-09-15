use super::*;
use std::path::Component;
use toml_edit::{DocumentMut, Item};

pub(super) struct DiscoveredProjectIsland {
    pub(super) absolute_root: PathBuf,
    pub(super) descriptor: ProjectIsland,
}

pub(super) fn discover_nested_project_islands(
    workspace_root: &Path,
    root_project_types: &[String],
    candidate_dirs: &[PathBuf],
) -> Vec<DiscoveredProjectIsland> {
    let mut discovered = Vec::<DiscoveredProjectIsland>::new();
    for directory in candidate_dirs.iter().cloned() {
        let manifest_names = manifest_file_names(&directory);
        let types = project_types_for_manifests(&directory, &manifest_names);
        if types.is_empty() {
            continue;
        }
        let project_types = types.into_iter().collect::<Vec<_>>();
        if project_types.len() == 1
            && project_types[0] == "cmake"
            && is_flutter_platform_cmake_scaffold(workspace_root, &directory)
        {
            continue;
        }
        let inherited = discovered
            .iter()
            .filter(|island| directory.starts_with(&island.absolute_root))
            .max_by_key(|island| island.absolute_root.components().count())
            .map(|island| island.descriptor.project_types.as_slice())
            .unwrap_or(root_project_types);
        if !inherited.is_empty()
            && project_types
                .iter()
                .all(|project_type| inherited.contains(project_type))
        {
            continue;
        }
        let relative = portable_relative(workspace_root, &directory);
        if relative == "." {
            continue;
        }
        let manifests = manifest_names
            .iter()
            .map(|manifest| format!("{relative}/{manifest}"))
            .collect::<Vec<_>>();
        discovered.push(DiscoveredProjectIsland {
            absolute_root: directory,
            descriptor: ProjectIsland {
                id: relative.clone(),
                root: relative,
                languages: languages_for_project_types(&project_types),
                project_types,
                manifests,
                check_ids: Vec::new(),
                dependencies: Vec::new(),
                verification_status: "pending",
                verification_gaps: Vec::new(),
                provider: "manifest-discovery",
                precision: "structural",
            },
        });
        if discovered.len() >= MAX_PROFILE_ISLANDS {
            break;
        }
    }
    discovered
}

const DOTNET_MANIFEST_EXTENSIONS: &[&str] = &["sln", "slnx", "csproj", "fsproj", "vbproj"];

pub(super) fn is_manifest_file_name(name: &str) -> bool {
    if MANIFEST_FILES.contains(&name) {
        return true;
    }
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            DOTNET_MANIFEST_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

pub(super) fn manifest_file_names(root: &Path) -> Vec<String> {
    let mut names = MANIFEST_FILES
        .iter()
        .filter(|manifest| root.join(manifest).is_file())
        .map(|manifest| (*manifest).to_owned())
        .collect::<Vec<_>>();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries
            .filter_map(Result::ok)
            .take(MAX_PROFILE_SCAN_ENTRIES)
        {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !MANIFEST_FILES.iter().any(|manifest| *manifest == name)
                && is_manifest_file_name(&name)
            {
                names.push(name);
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

pub(super) fn project_types_for_manifests(root: &Path, manifests: &[String]) -> BTreeSet<String> {
    let mut types = BTreeSet::new();
    for manifest in manifests {
        match manifest.as_str() {
            "Cargo.toml" => {
                types.insert("rust".to_owned());
            }
            "package.json" | "tsconfig.json" => {
                types.insert("node".to_owned());
            }
            "deno.json" | "deno.jsonc" => {
                types.insert("deno".to_owned());
            }
            "pyproject.toml" | "requirements.txt" => {
                types.insert("python".to_owned());
            }
            "go.mod" => {
                types.insert("go".to_owned());
            }
            "pom.xml" | "build.gradle" | "build.gradle.kts" => {
                types.insert("java".to_owned());
            }
            "Package.swift" => {
                types.insert("swift".to_owned());
            }
            "pubspec.yaml" => {
                let flutter = read_small_text(&root.join(manifest))
                    .is_some_and(|content| content.to_ascii_lowercase().contains("sdk: flutter"));
                types.insert(if flutter { "flutter" } else { "dart" }.to_owned());
            }
            "mix.exs" => {
                types.insert("elixir".to_owned());
            }
            "Gemfile" => {
                types.insert("ruby".to_owned());
            }
            "composer.json" => {
                types.insert("php".to_owned());
            }
            "dune-project" => {
                types.insert("ocaml".to_owned());
            }
            "DESCRIPTION" => {
                types.insert("r".to_owned());
            }
            "CMakeLists.txt" => {
                types.insert("cmake".to_owned());
            }
            "Makefile" => {
                types.insert("make".to_owned());
            }
            _ if manifest.ends_with(".csproj") => {
                types.insert("dotnet-csharp".to_owned());
            }
            _ if manifest.ends_with(".fsproj") => {
                types.insert("dotnet-fsharp".to_owned());
            }
            _ if manifest.ends_with(".vbproj") => {
                types.insert("dotnet-vb".to_owned());
            }
            _ if manifest.ends_with(".sln") || manifest.ends_with(".slnx") => {
                types.insert("dotnet".to_owned());
            }
            _ => {}
        }
    }
    types
}

fn is_flutter_platform_cmake_scaffold(workspace_root: &Path, directory: &Path) -> bool {
    directory
        .ancestors()
        .take_while(|candidate| candidate.starts_with(workspace_root))
        .any(|candidate| {
            let Some(platform) = candidate.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            if !matches!(platform, "linux" | "windows") {
                return false;
            }
            let Some(app_root) = candidate.parent() else {
                return false;
            };
            app_root.join("pubspec.yaml").is_file()
                && candidate.join("CMakeLists.txt").is_file()
                && candidate.join("flutter/CMakeLists.txt").is_file()
                && candidate.join("runner/CMakeLists.txt").is_file()
        })
}

pub(super) fn languages_for_project_types(project_types: &[String]) -> Vec<String> {
    let mut languages = BTreeSet::new();
    for project_type in project_types {
        match project_type.as_str() {
            "rust" | "python" | "go" | "java" | "swift" | "dart" | "elixir" | "ruby" | "php"
            | "r" => {
                languages.insert(project_type.clone());
            }
            "flutter" => {
                languages.insert("dart".to_owned());
            }
            "node" | "deno" => {
                languages.insert("java-script".to_owned());
                languages.insert("type-script".to_owned());
                languages.insert("tsx".to_owned());
            }
            "dotnet-csharp" => {
                languages.insert("c-sharp".to_owned());
            }
            "ocaml" => {
                languages.insert("ocaml".to_owned());
                languages.insert("ocaml-interface".to_owned());
            }
            "cmake" => {
                languages.insert("c".to_owned());
                languages.insert("cpp".to_owned());
            }
            _ => {}
        }
    }
    languages.into_iter().collect()
}

pub(super) fn profile_excluded_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".wcode"
            | "target"
            | "node_modules"
            | "build"
            | "coverage"
            | ".dart_tool"
            | ".build"
            | ".gradle"
            | ".swiftpm"
            | "ephemeral"
            | "Pods"
            | ".symlinks"
            | ".plugin_symlinks"
            | "DerivedData"
            | ".venv"
            | "venv"
            | "dist"
            | ".next"
            | ".cache"
            | "vendor"
    )
}

pub(super) fn attach_manifest_dependencies(workspace_root: &Path, islands: &mut [ProjectIsland]) {
    let roots = islands
        .iter()
        .map(|island| (island.id.clone(), island.root.clone()))
        .collect::<Vec<_>>();
    for island in islands {
        let (verification_status, verification_gaps) = verification_coverage(island);
        island.verification_status = verification_status;
        island.verification_gaps = verification_gaps;
        let absolute_root = if island.root == "." {
            workspace_root.to_path_buf()
        } else {
            workspace_root.join(&island.root)
        };
        let mut references = Vec::new();
        references.extend(cargo_path_references(&absolute_root));
        references.extend(package_path_references(&absolute_root));
        references.extend(tsconfig_path_references(&absolute_root));
        references.extend(go_replace_references(&absolute_root));
        let mut dependencies = Vec::new();
        let mut seen = BTreeSet::new();
        for reference in references {
            let Some(target) =
                resolve_island_reference(&island.id, &island.root, &reference.path, &roots)
            else {
                continue;
            };
            if !seen.insert((
                target.clone(),
                reference.kind.clone(),
                reference.evidence.clone(),
            )) {
                continue;
            }
            dependencies.push(ProjectIslandDependency {
                island: target,
                kind: reference.kind,
                evidence: reference.evidence,
                provider: "manifest-dependency",
                precision: "structural",
            });
        }
        dependencies.sort_by(|left, right| {
            (&left.island, &left.kind, &left.evidence).cmp(&(
                &right.island,
                &right.kind,
                &right.evidence,
            ))
        });
        island.dependencies = dependencies;
    }
}

fn verification_coverage(island: &ProjectIsland) -> (&'static str, Vec<String>) {
    if island.project_types.len() == 1 && island.project_types[0] == "generic" {
        return ("unknown", Vec::new());
    }
    let check_kinds = island
        .check_ids
        .iter()
        .filter_map(|id| id.rsplit(':').next())
        .collect::<Vec<_>>();
    let mut gaps = island
        .project_types
        .iter()
        .filter(|project_type| {
            let Some(prefix) = project_type_check_prefix(project_type) else {
                return true;
            };
            !check_kinds.iter().any(|id| id.starts_with(prefix))
        })
        .cloned()
        .collect::<Vec<_>>();
    gaps.sort();
    gaps.dedup();
    let status = if gaps.is_empty() {
        "native"
    } else if island.check_ids.is_empty() {
        "manifest_only"
    } else {
        "partial"
    };
    (status, gaps)
}

#[derive(Clone, Debug)]
pub(crate) struct ProjectIslandVerificationGap {
    pub island: String,
    pub root: String,
    pub project_types: Vec<String>,
    pub level: String,
}

#[derive(Debug)]
struct ManifestReference {
    path: String,
    kind: String,
    evidence: String,
}

fn cargo_path_references(root: &Path) -> Vec<ManifestReference> {
    let Some(content) = read_small_text(&root.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(document) = content.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let mut references = Vec::new();
    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        collect_cargo_dependency_table(document.get(section), section, &mut references);
    }
    if let Some(workspace) = document.get("workspace").and_then(Item::as_table) {
        collect_cargo_dependency_table(
            workspace.get("dependencies"),
            "workspace.dependencies",
            &mut references,
        );
    }
    references
}

fn collect_cargo_dependency_table(
    item: Option<&Item>,
    section: &str,
    references: &mut Vec<ManifestReference>,
) {
    let Some(table) = item.and_then(Item::as_table) else {
        return;
    };
    for (name, dependency) in table {
        let path = dependency
            .as_table()
            .and_then(|table| table.get("path"))
            .and_then(Item::as_str)
            .or_else(|| {
                dependency
                    .as_value()
                    .and_then(|value| value.as_inline_table())
                    .and_then(|table| table.get("path"))
                    .and_then(|value| value.as_str())
            });
        let Some(path) = path else {
            continue;
        };
        references.push(ManifestReference {
            path: path.to_owned(),
            kind: "cargo_path".to_owned(),
            evidence: format!("Cargo.toml:{section}.{name}.path"),
        });
    }
}

fn package_path_references(root: &Path) -> Vec<ManifestReference> {
    let Some(content) = read_small_text(&root.join("package.json")) else {
        return Vec::new();
    };
    let Ok(package) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    let mut references = Vec::new();
    for section in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        let Some(dependencies) = package.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (name, value) in dependencies {
            let Some(value) = value.as_str() else {
                continue;
            };
            let Some(path) = value
                .strip_prefix("file:")
                .or_else(|| value.strip_prefix("link:"))
            else {
                continue;
            };
            references.push(ManifestReference {
                path: path.to_owned(),
                kind: "package_local".to_owned(),
                evidence: format!("package.json:{section}.{name}"),
            });
        }
    }
    references
}

fn tsconfig_path_references(root: &Path) -> Vec<ManifestReference> {
    let Some(content) = read_small_text(&root.join("tsconfig.json")) else {
        return Vec::new();
    };
    let Ok(config) = serde_json::from_str::<Value>(&content) else {
        return Vec::new();
    };
    config
        .get("references")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.get("path").and_then(Value::as_str))
        .enumerate()
        .map(|(index, path)| ManifestReference {
            path: path.to_owned(),
            kind: "ts_project_reference".to_owned(),
            evidence: format!("tsconfig.json:references[{index}].path"),
        })
        .collect()
}

fn go_replace_references(root: &Path) -> Vec<ManifestReference> {
    let Some(content) = read_small_text(&root.join("go.mod")) else {
        return Vec::new();
    };
    let mut references = Vec::new();
    let mut in_replace_block = false;
    for (index, line) in content.lines().enumerate() {
        let line = line.split("//").next().unwrap_or_default().trim();
        if line == "replace (" {
            in_replace_block = true;
            continue;
        }
        if in_replace_block && line == ")" {
            in_replace_block = false;
            continue;
        }
        let candidate = if let Some(rest) = line.strip_prefix("replace ") {
            rest.trim()
        } else if in_replace_block {
            line
        } else {
            continue;
        };
        let Some((_, replacement)) = candidate.split_once("=>") else {
            continue;
        };
        let path = replacement.split_whitespace().next().unwrap_or_default();
        if path.is_empty() || !(path.starts_with('.') || path.starts_with('/')) {
            continue;
        }
        references.push(ManifestReference {
            path: path.to_owned(),
            kind: "go_replace".to_owned(),
            evidence: format!("go.mod:replace:line{}", index + 1),
        });
    }
    references
}

fn resolve_island_reference(
    from_id: &str,
    from_root: &str,
    reference: &str,
    roots: &[(String, String)],
) -> Option<String> {
    let reference = Path::new(reference);
    if reference.is_absolute() {
        return None;
    }
    let base = if from_root == "." {
        PathBuf::new()
    } else {
        PathBuf::from(from_root)
    };
    let normalized = normalize_workspace_relative(&base.join(reference))?;
    roots
        .iter()
        .filter(|(id, root)| id != from_id && island_owns_path(root, &normalized))
        .max_by_key(|(_, root)| {
            if root == "." {
                0
            } else {
                root.split('/').count()
            }
        })
        .map(|(id, _)| id.clone())
}

fn normalize_workspace_relative(path: &Path) -> Option<String> {
    let mut normalized = Vec::<String>::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => {
                normalized.push(component.to_string_lossy().to_string());
            }
            Component::ParentDir => {
                normalized.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized.join("/")
    })
}

fn island_owns_path(root: &str, path: &str) -> bool {
    root == "."
        || path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

const MAX_VERIFICATION_IMPACT_REASONS: usize = 128;

#[cfg(test)]
pub(crate) fn verification_checks_for_snapshot(
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
    level: &str,
) -> Vec<CheckSpec> {
    let impact = verification_impact_for_snapshot(profile, snapshot);
    verification_checks_for_impact(profile, &impact, level)
}

pub(crate) fn verification_checks_for_impact(
    profile: &ProjectProfile,
    impact: &ProjectVerificationImpact,
    level: &str,
) -> Vec<CheckSpec> {
    let targets_island = |island: &str| {
        !impact.selective
            || impact
                .affected_islands
                .binary_search_by(|candidate| candidate.as_str().cmp(island))
                .is_ok()
    };
    let mut plan = profile
        .recommended_checks
        .iter()
        .filter(|check| level == "full" || check.level == "quick")
        .filter(|check| check.island == "workspace" || targets_island(&check.island))
        .cloned()
        .collect::<Vec<_>>();

    if level == "quick" {
        // Quick verification must still execute a real project-owned gate for
        // every affected language island. If an ecosystem exposes only a full
        // gate (for example pytest), promote one bounded full check instead of
        // silently treating the workspace-level diff check as sufficient.
        for island in profile
            .islands
            .iter()
            .filter(|island| targets_island(&island.id))
        {
            for project_type in &island.project_types {
                let Some(prefix) = project_type_check_prefix(project_type) else {
                    continue;
                };
                let has_quick = profile.recommended_checks.iter().any(|check| {
                    check.island == island.id
                        && check.level == "quick"
                        && check_id_has_prefix(check, prefix)
                });
                if has_quick {
                    continue;
                }
                if let Some(mut fallback) = profile
                    .recommended_checks
                    .iter()
                    .filter(|check| {
                        check.island == island.id
                            && check.level == "full"
                            && check_id_has_prefix(check, prefix)
                    })
                    .min_by(|left, right| {
                        left.phase
                            .cmp(&right.phase)
                            .then_with(|| left.id.cmp(&right.id))
                    })
                    .cloned()
                {
                    fallback.reason = format!(
                        "Quick fallback for {project_type}: no dedicated quick gate is declared; {}",
                        fallback.reason
                    );
                    if !plan.iter().any(|check| check.id == fallback.id) {
                        plan.push(fallback);
                    }
                }
            }
        }
    }

    sort_checks(&mut plan);
    plan
}

#[cfg(test)]
pub(crate) fn verification_gaps_for_snapshot(
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
    level: &str,
) -> Vec<ProjectIslandVerificationGap> {
    let impact = verification_impact_for_snapshot(profile, snapshot);
    verification_gaps_for_impact(profile, &impact, level)
}

pub(crate) fn verification_gaps_for_impact(
    profile: &ProjectProfile,
    impact: &ProjectVerificationImpact,
    level: &str,
) -> Vec<ProjectIslandVerificationGap> {
    profile
        .islands
        .iter()
        .filter(|island| {
            island.project_types != ["generic"]
                && (!impact.selective || impact.affected_islands.binary_search(&island.id).is_ok())
        })
        .filter_map(|island| {
            let mut project_types = island
                .project_types
                .iter()
                .filter(|project_type| {
                    let Some(prefix) = project_type_check_prefix(project_type) else {
                        return true;
                    };
                    !project_type_has_level_coverage(profile, &island.id, prefix, level)
                })
                .cloned()
                .collect::<Vec<_>>();
            project_types.sort();
            project_types.dedup();
            (!project_types.is_empty()).then(|| ProjectIslandVerificationGap {
                island: island.id.clone(),
                root: island.root.clone(),
                project_types,
                level: level.to_owned(),
            })
        })
        .collect()
}

pub(crate) fn verification_impact_for_snapshot(
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
) -> ProjectVerificationImpact {
    let Some(snapshot) = snapshot else {
        return broad_verification_impact(profile, "worktree_snapshot", "snapshot_unavailable");
    };
    if snapshot.get("available").and_then(Value::as_bool) != Some(true) {
        return broad_verification_impact(profile, "worktree_snapshot", "snapshot_unavailable");
    }
    if snapshot.get("truncated").and_then(Value::as_bool) == Some(true) {
        return broad_verification_impact(profile, "worktree_snapshot", "snapshot_truncated");
    }
    let Some(files) = snapshot.get("files").and_then(Value::as_array) else {
        return broad_verification_impact(profile, "worktree_snapshot", "changed_files_missing");
    };
    if files.is_empty() {
        return broad_verification_impact(profile, "worktree_snapshot", "changed_files_empty");
    }

    let mut affected = BTreeSet::new();
    let mut reasons = BTreeSet::new();
    for path in files
        .iter()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
    {
        let mut matched = false;
        if let Some(island) = owning_island(&profile.islands, path) {
            matched = true;
            affected.insert(island.id.clone());
            reasons.insert(ProjectVerificationImpactReason {
                island: island.id.clone(),
                kind: "direct_change",
                source: path.to_owned(),
                relationship: "manifest_ownership".to_owned(),
                evidence: format!("nearest manifest root {}", island.root),
                provider: island.provider,
                precision: island.precision,
            });
        }
        for bridge in super::contracts::matching_contract_bridges(&profile.contracts, path) {
            matched = true;
            affected.insert(bridge.consumer_island.clone());
            reasons.insert(ProjectVerificationImpactReason {
                island: bridge.consumer_island.clone(),
                kind: "contract_bridge",
                source: path.to_owned(),
                relationship: bridge.kind.to_owned(),
                evidence: bridge.evidence.clone(),
                provider: bridge.provider,
                precision: bridge.precision,
            });
        }
        if !matched {
            return broad_verification_impact(profile, path, "unresolved_change_ownership");
        }
    }

    propagate_dependents_with_provenance(&profile.islands, &mut affected, &mut reasons);
    let truncated = reasons.len() > MAX_VERIFICATION_IMPACT_REASONS;
    let mut reasons = reasons.into_iter().collect::<Vec<_>>();
    reasons.truncate(MAX_VERIFICATION_IMPACT_REASONS);
    ProjectVerificationImpact {
        selective: true,
        affected_islands: affected.into_iter().collect(),
        reasons,
        truncated,
        provider: "workspace-impact",
        precision: "structural",
    }
}

fn broad_verification_impact(
    profile: &ProjectProfile,
    source: &str,
    evidence: &str,
) -> ProjectVerificationImpact {
    let affected_islands = profile
        .islands
        .iter()
        .map(|island| island.id.clone())
        .collect::<Vec<_>>();
    let mut reasons = affected_islands
        .iter()
        .map(|island| ProjectVerificationImpactReason {
            island: island.clone(),
            kind: "broad_fallback",
            source: source.to_owned(),
            relationship: "broad_verification".to_owned(),
            evidence: evidence.to_owned(),
            provider: "workspace-impact",
            precision: "deterministic",
        })
        .collect::<Vec<_>>();
    let truncated = reasons.len() > MAX_VERIFICATION_IMPACT_REASONS;
    reasons.truncate(MAX_VERIFICATION_IMPACT_REASONS);
    ProjectVerificationImpact {
        selective: false,
        affected_islands,
        reasons,
        truncated,
        provider: "workspace-impact",
        precision: "deterministic",
    }
}

fn propagate_dependents_with_provenance(
    islands: &[ProjectIsland],
    affected: &mut BTreeSet<String>,
    reasons: &mut BTreeSet<ProjectVerificationImpactReason>,
) {
    loop {
        let before = affected.len();
        for island in islands {
            for dependency in &island.dependencies {
                if !affected.contains(&dependency.island) {
                    continue;
                }
                reasons.insert(ProjectVerificationImpactReason {
                    island: island.id.clone(),
                    kind: "manifest_dependency",
                    source: dependency.island.clone(),
                    relationship: dependency.kind.clone(),
                    evidence: dependency.evidence.clone(),
                    provider: dependency.provider,
                    precision: dependency.precision,
                });
                affected.insert(island.id.clone());
            }
        }
        if affected.len() == before {
            break;
        }
    }
}

fn project_type_has_level_coverage(
    profile: &ProjectProfile,
    island: &str,
    prefix: &str,
    level: &str,
) -> bool {
    profile.recommended_checks.iter().any(|check| {
        check.island == island
            && check_id_has_prefix(check, prefix)
            && match level {
                "full" => check.level == "full",
                // A quick request may promote one bounded full check when an
                // ecosystem has no cheaper native gate.
                "quick" => matches!(check.level.as_str(), "quick" | "full"),
                _ => false,
            }
    })
}

fn check_id_has_prefix(check: &CheckSpec, prefix: &str) -> bool {
    check
        .id
        .rsplit(':')
        .next()
        .is_some_and(|id| id.starts_with(prefix))
}

fn project_type_check_prefix(project_type: &str) -> Option<&'static str> {
    match project_type {
        "rust" => Some("rust-"),
        "node" => Some("node-"),
        "python" => Some("python-"),
        "go" => Some("go-"),
        "java" => Some("java-"),
        "swift" => Some("swift-"),
        "dart" => Some("dart-"),
        "flutter" => Some("flutter-"),
        "deno" => Some("deno-"),
        "dotnet" | "dotnet-csharp" | "dotnet-fsharp" | "dotnet-vb" => Some("dotnet-"),
        "elixir" => Some("elixir-"),
        "ocaml" => Some("ocaml-"),
        "ruby" => Some("ruby-"),
        "php" => Some("php-"),
        "r" => Some("r-"),
        "make" => Some("make-"),
        "cmake" => Some("cmake-"),
        _ => None,
    }
}

fn owning_island<'a>(islands: &'a [ProjectIsland], path: &str) -> Option<&'a ProjectIsland> {
    islands
        .iter()
        .filter(|island| {
            island.root == "."
                || path == island.root
                || path
                    .strip_prefix(&island.root)
                    .is_some_and(|suffix| suffix.starts_with('/'))
        })
        .max_by_key(|island| {
            if island.root == "." {
                0
            } else {
                island.root.split('/').count()
            }
        })
}

fn portable_relative(root: &Path, path: &Path) -> String {
    let Ok(relative) = path.strip_prefix(root) else {
        return ".".to_owned();
    };
    let value = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if value.is_empty() {
        ".".to_owned()
    } else {
        value
    }
}
