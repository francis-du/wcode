use super::*;
use crate::design::{self, Priority, VerificationRef};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const FOCUSED_TEST_PROVIDER: &str = "design-traceability";
pub(super) const FOCUSED_TEST_PRECISION: &str = "declared+syntax";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FocusedTestRunner {
    Rust,
    Python,
    Go,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct FocusedTestCandidate {
    pub(super) priority: u8,
    pub(super) source_path: String,
    pub(super) requirement: String,
    pub(super) acceptance: String,
    pub(super) test_path: String,
    pub(super) test_symbol: String,
    pub(super) island: String,
    pub(super) runner: FocusedTestRunner,
}

pub(super) fn focused_quick_test(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
) -> Option<CheckSpec> {
    let design = harness.intelligence.design_load(workspace).ok()?;
    if !design.initialized || design.error_count() > 0 {
        return None;
    }
    focused_quick_test_from_design(
        harness,
        workspace_id,
        workspace,
        profile,
        snapshot,
        &design.state,
    )
}

pub(super) fn focused_quick_test_from_design(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    profile: &ProjectProfile,
    snapshot: Option<&Value>,
    state: &design::DesignState,
) -> Option<CheckSpec> {
    let changed_paths = exact_changed_paths(snapshot)?;
    let candidates = declared_test_candidates(state, profile, &changed_paths);
    for candidate in candidates {
        if !resolved_test_symbol(
            harness,
            workspace_id,
            workspace,
            &candidate.test_path,
            &candidate.test_symbol,
        ) {
            continue;
        }
        if let Some(check) = focused_test_check(profile, &candidate) {
            return Some(check);
        }
    }
    None
}

fn exact_changed_paths(snapshot: Option<&Value>) -> Option<BTreeSet<String>> {
    let snapshot = snapshot?
        .get("available")
        .and_then(Value::as_bool)
        .filter(|available| *available)
        .and(snapshot)
        .filter(|snapshot| snapshot.get("truncated").and_then(Value::as_bool) != Some(true))?;
    let paths = snapshot
        .get("files")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|file| file.get("path").and_then(Value::as_str))
        .filter(|path| !path.trim().is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    (!paths.is_empty()).then_some(paths)
}

pub(super) fn declared_test_candidates(
    state: &design::DesignState,
    profile: &ProjectProfile,
    changed_paths: &BTreeSet<String>,
) -> Vec<FocusedTestCandidate> {
    let mut component_sources = BTreeMap::<String, Vec<String>>::new();
    for component in state.components.values() {
        let sources = component
            .implementation
            .iter()
            .map(|reference| reference.path())
            .filter(|path| changed_paths.contains(*path))
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !sources.is_empty() {
            component_sources.insert(component.id.clone(), sources);
        }
    }

    let mut candidates = Vec::new();
    for requirement in state.requirements.values() {
        let mut sources = requirement
            .implemented_by
            .iter()
            .filter_map(|component| component_sources.get(component))
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        sources.sort();
        sources.dedup();
        if sources.is_empty() {
            continue;
        }
        for acceptance_id in &requirement.acceptance {
            let Some(acceptance) = state.acceptance.get(acceptance_id) else {
                continue;
            };
            for verification in &acceptance.verification {
                let VerificationRef::Test { path, symbol } = verification else {
                    continue;
                };
                let Some(island) = island_for_path(profile, path) else {
                    continue;
                };
                let Some(runner) = focused_test_runner(profile, island, path, symbol) else {
                    continue;
                };
                for source_path in &sources {
                    if island_for_path(profile, source_path).map(|source| source.id.as_str())
                        != Some(island.id.as_str())
                    {
                        continue;
                    }
                    candidates.push(FocusedTestCandidate {
                        priority: priority_rank(requirement.priority),
                        source_path: source_path.clone(),
                        requirement: requirement.id.clone(),
                        acceptance: acceptance.id.clone(),
                        test_path: path.clone(),
                        test_symbol: symbol.clone(),
                        island: island.id.clone(),
                        runner,
                    });
                }
            }
        }
    }
    candidates.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.requirement.cmp(&right.requirement))
            .then_with(|| left.acceptance.cmp(&right.acceptance))
            .then_with(|| left.test_path.cmp(&right.test_path))
            .then_with(|| left.test_symbol.cmp(&right.test_symbol))
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    candidates.dedup_by(|left, right| {
        left.island == right.island
            && left.test_path == right.test_path
            && left.test_symbol == right.test_symbol
    });
    candidates
}

fn island_for_path<'a>(profile: &'a ProjectProfile, path: &str) -> Option<&'a ProjectIsland> {
    profile
        .islands
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

fn focused_test_runner(
    profile: &ProjectProfile,
    island: &ProjectIsland,
    path: &str,
    symbol: &str,
) -> Option<FocusedTestRunner> {
    if island.project_types.iter().any(|kind| kind == "rust")
        && path.ends_with(".rs")
        && simple_test_identifier(symbol)
        && rust_test_check(profile, &island.id).is_some()
    {
        return Some(FocusedTestRunner::Rust);
    }
    if island.project_types.iter().any(|kind| kind == "python")
        && path.ends_with(".py")
        && simple_python_test_symbol(symbol)
        && python_test_check(profile, &island.id).is_some()
    {
        return Some(FocusedTestRunner::Python);
    }
    if island.project_types.iter().any(|kind| kind == "go")
        && path.ends_with("_test.go")
        && simple_go_test_symbol(symbol)
        && go_test_check(profile, &island.id).is_some()
    {
        return Some(FocusedTestRunner::Go);
    }
    None
}

pub(super) fn focused_test_check(
    profile: &ProjectProfile,
    candidate: &FocusedTestCandidate,
) -> Option<CheckSpec> {
    let island = profile
        .islands
        .iter()
        .find(|island| island.id == candidate.island)?;
    let (base, args) = match candidate.runner {
        FocusedTestRunner::Rust => {
            let base = rust_test_check(profile, &candidate.island)?;
            let mut args = base.args.clone();
            args.push(candidate.test_symbol.clone());
            (base, args)
        }
        FocusedTestRunner::Python => {
            let base = python_test_check(profile, &candidate.island)?;
            let relative = island_relative_path(&island.root, &candidate.test_path)?;
            (
                base,
                vec![
                    "-q".to_owned(),
                    format!("{relative}::{}", candidate.test_symbol),
                ],
            )
        }
        FocusedTestRunner::Go => {
            let base = go_test_check(profile, &candidate.island)?;
            let package = go_package_for_test(&island.root, &candidate.test_path)?;
            (
                base,
                vec![
                    "test".to_owned(),
                    package,
                    "-run".to_owned(),
                    format!("^{}$", candidate.test_symbol),
                ],
            )
        }
    };
    Some(CheckSpec {
        id: format!("focused-{}", base.id),
        level: "quick".to_owned(),
        phase: 1,
        program: base.program.clone(),
        args,
        cwd: base.cwd.clone(),
        island: base.island.clone(),
        languages: base.languages.clone(),
        reason: format!(
            "Prioritize Design-declared test {}::{} because direct change {} maps to {} via {}; provider={FOCUSED_TEST_PROVIDER}, precision={FOCUSED_TEST_PRECISION}.",
            candidate.test_path,
            candidate.test_symbol,
            candidate.source_path,
            candidate.requirement,
            candidate.acceptance,
        ),
    })
}

fn rust_test_check<'a>(profile: &'a ProjectProfile, island: &str) -> Option<&'a CheckSpec> {
    profile.recommended_checks.iter().find(|check| {
        check.island == island
            && check.program == "cargo"
            && matches!(
                check
                    .args
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
                ["test"] | ["test", "--locked"]
            )
    })
}

fn python_test_check<'a>(profile: &'a ProjectProfile, island: &str) -> Option<&'a CheckSpec> {
    profile.recommended_checks.iter().find(|check| {
        check.island == island
            && check.program == "pytest"
            && check.args.iter().map(String::as_str).eq(["-q"])
    })
}

fn go_test_check<'a>(profile: &'a ProjectProfile, island: &str) -> Option<&'a CheckSpec> {
    profile.recommended_checks.iter().find(|check| {
        check.island == island
            && check.program == "go"
            && check.args.iter().map(String::as_str).eq(["test", "./..."])
    })
}

fn island_relative_path(island_root: &str, path: &str) -> Option<String> {
    let relative = if island_root == "." {
        path
    } else {
        path.strip_prefix(island_root)?.strip_prefix('/')?
    };
    if relative.is_empty()
        || relative.starts_with('-')
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
    {
        return None;
    }
    Some(relative.to_owned())
}

fn go_package_for_test(island_root: &str, path: &str) -> Option<String> {
    let relative = island_relative_path(island_root, path)?;
    if !relative.ends_with("_test.go") {
        return None;
    }
    let parent = relative
        .rsplit_once('/')
        .map(|(parent, _)| parent)
        .unwrap_or_default();
    Some(if parent.is_empty() {
        ".".to_owned()
    } else {
        format!("./{parent}")
    })
}

fn resolved_test_symbol(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
    path: &str,
    symbol: &str,
) -> bool {
    let Ok(info) = workspace.path_info(path) else {
        return false;
    };
    if info.kind != "file" {
        return false;
    }
    let Ok(found) = harness.find_symbol(workspace_id, workspace, symbol, path, Some("function"), 8)
    else {
        return false;
    };
    found
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|result| {
            result.get("path").and_then(Value::as_str) == Some(path)
                && result.get("name").and_then(Value::as_str) == Some(symbol)
        })
}

fn simple_test_identifier(symbol: &str) -> bool {
    if symbol.is_empty() || symbol.len() > 128 {
        return false;
    }
    let mut chars = symbol.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn simple_python_test_symbol(symbol: &str) -> bool {
    symbol.starts_with("test_") && simple_test_identifier(symbol)
}

fn simple_go_test_symbol(symbol: &str) -> bool {
    let Some(suffix) = symbol.strip_prefix("Test") else {
        return false;
    };
    let Some(first) = suffix.chars().next() else {
        return false;
    };
    !first.is_ascii_lowercase() && simple_test_identifier(symbol)
}

fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::Critical => 4,
        Priority::High => 3,
        Priority::Medium => 2,
        Priority::Low => 1,
    }
}
