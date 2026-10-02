use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn project(root: &Path) {
    for directory in [".wcode/design", "src", "tests", "crates/child/src"] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    for (path, content) in [
        (".wcode/project.yaml", "name: scope-fixture\n"),
        (".wcode/design/product.yaml", "schema_version: 1\nid: product:scope\nname: Scope fixture\nvision: Check only explicitly selected projects.\n"),
        (".wcode/design/requirements.yaml", "- schema_version: 1\n  id: REQ-SCOPE-001\n  title: Parent project owns its child\n  intent: One project configuration covers the repository.\n  priority: high\n  implemented_by: [component:scope]\n  acceptance: [AC-SCOPE-001]\n"),
        (".wcode/design/components.yaml", "- schema_version: 1\n  id: component:scope\n  name: Scope\n  responsibilities: [Return a value]\n  implementation:\n    - kind: file\n      path: src/lib.rs\n"),
        (".wcode/design/acceptance.yaml", "- schema_version: 1\n  id: AC-SCOPE-001\n  title: Value is available\n  statement: The function returns its expected value.\n  verification:\n    - kind: test\n      path: tests/value.rs\n      symbol: value_is_available\n"),
        ("Cargo.toml", "[package]\nname='scope-fixture'\nversion='0.1.0'\nedition='2021'\n[workspace]\nmembers=['crates/child']\n"),
        ("src/lib.rs", "pub fn value() -> u32 { 1 }\n"),
        ("tests/value.rs", "#[test]\nfn value_is_available() { assert_eq!(scope_fixture::value(), 1); }\n"),
        ("crates/child/Cargo.toml", "[package]\nname='scope-child'\nversion='0.1.0'\nedition='2021'\n"),
        ("crates/child/src/lib.rs", "pub fn child() {}\n"),
    ] {
        fs::write(root.join(path), content).unwrap();
    }
}

fn run(roots: &[&Path], check: bool, emit_json: bool) -> Output {
    let state = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_wcode"));
    command
        .current_dir(roots[0])
        .env("WCODE_STATE_DIR", state.path())
        .args(["--read-only", "--no-exec", "--no-semantic", "--allow-sleep"]);
    for root in roots {
        command.arg("--workspace").arg(root);
    }
    command.arg("intelligence");
    if emit_json {
        command.arg("--json");
    }
    if check {
        command.arg("--check");
    }
    command.output().unwrap()
}

fn inspect(roots: &[&Path], check: bool) -> (bool, Value) {
    let output = run(roots, check, true);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid report: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), value)
}

#[test]
fn intelligence_checks_parent_design_without_requiring_duplicate_child_design() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let (success, value) = inspect(&[root.path()], true);
    assert!(success, "{}", value["check"]);
    assert_eq!(value["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["workspaces"][0]["traceability"]["design_to_implementation"]["percent"],
        100
    );
    assert!(!root.path().join("crates/child/.wcode").exists());
}

#[test]
fn intelligence_still_rejects_an_explicit_child_without_design() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let child = root.path().join("crates/child");
    let (success, value) = inspect(&[&child], true);
    assert!(!success);
    assert_eq!(value["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(value["workspaces"][0]["design"]["initialized"], false);
    assert!(value["check"]["failures"]
        .as_array()
        .unwrap()
        .iter()
        .any(|failure| { failure.as_str().unwrap().contains("uninitialized") }));
}

#[test]
fn intelligence_checks_every_explicit_root_and_never_hides_a_failure() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    project(root.path());
    fs::create_dir(other.path().join("src")).unwrap();
    fs::write(other.path().join("src/lib.rs"), "pub fn other() {}\n").unwrap();
    fs::write(
        other.path().join("Cargo.toml"),
        "[package]\nname='other'\nversion='0.1.0'\n",
    )
    .unwrap();
    let (success, value) = inspect(&[root.path(), other.path()], true);
    assert!(!success);
    let entries = value["workspaces"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["design"]["valid"], true);
    assert_eq!(entries[1]["design"]["initialized"], false);
    assert_eq!(value["check"]["passed"], false);
}

#[test]
fn intelligence_report_without_check_uses_the_same_explicit_scope() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let (success, value) = inspect(&[root.path()], false);
    assert!(success);
    assert_eq!(value["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(value["check"]["passed"], true);
}

#[test]
fn duplicate_explicit_roots_are_checked_once() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let (success, value) = inspect(&[root.path(), root.path()], true);
    assert!(success, "{}", value["check"]);
    assert_eq!(value["workspaces"].as_array().unwrap().len(), 1);
}

#[test]
fn malformed_parent_design_cannot_be_hidden_by_discovered_children() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    fs::write(
        root.path().join(".wcode/design/product.yaml"),
        "schema_version: [\n",
    )
    .unwrap();
    let (success, value) = inspect(&[root.path()], true);
    assert!(!success);
    assert_eq!(value["check"]["passed"], false);
    assert_eq!(value["workspaces"].as_array().unwrap().len(), 1);
    assert_eq!(value["workspaces"][0]["design"]["valid"], false);
    assert!(!value["check"]["failures"].as_array().unwrap().is_empty());
}

#[test]
fn text_report_uses_a_functional_heading_without_a_release_number() {
    let root = tempfile::tempdir().unwrap();
    project(root.path());
    let output = run(&[root.path()], true, false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().next(), Some("WCode project status"));
    assert!(!text.contains("WCode Intelligence"));
}
