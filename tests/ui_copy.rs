use std::{path::Path, process::Command};

#[test]
fn ui_copy_and_runtime_capabilities_stay_consistent() {
    let output = Command::new("node")
        .args(["--test", "tests/unit/ui/plain_copy.cjs"])
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
        .output()
        .expect("Node is required for the UI regression tests");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("tests 5"),
        "the five UI regressions must actually run"
    );
}
