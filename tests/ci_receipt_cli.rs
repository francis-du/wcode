use std::process::Command;

fn import_args() -> Vec<&'static str> {
    vec![
        "import-ci-receipt",
        "--plan-id",
        "VP-1",
        "--store",
        "receipts.jsonl",
        "--check-id",
        "9",
        "--repository",
        "francis-du/wcode",
        "--repository-id",
        "1234",
        "--app-id",
        "7",
        "--check-name",
        "wcode/change-acceptance",
    ]
}

fn without_flag(flag: &str) -> Vec<&'static str> {
    let args = import_args();
    let mut filtered = Vec::new();
    let mut skip_value = false;
    for argument in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        if argument == flag {
            skip_value = true;
            continue;
        }
        filtered.push(argument);
    }
    filtered
}

#[test]
fn ci_receipt_import_exposes_the_declared_binding() {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
        .current_dir(root.path())
        .args(["import-ci-receipt", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for option in [
        "--plan-id",
        "--store",
        "--check-id",
        "--repository",
        "--repository-id",
        "--app-id",
        "--check-name",
        "--json",
    ] {
        assert!(help.contains(option), "missing {option}");
    }
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn ci_receipt_import_rejects_read_only_and_disabled_execution() {
    let root = tempfile::tempdir().unwrap();
    for prefix in [
        vec!["--no-monitor", "--no-semantic", "--read-only"],
        vec!["--no-monitor", "--no-semantic", "--no-exec"],
    ] {
        let mut arguments = prefix.clone();
        arguments.extend(import_args());
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(&arguments)
            .output()
            .unwrap();
        assert!(!output.status.success(), "silently accepted {arguments:?}");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

#[test]
fn ci_receipt_import_requires_the_full_binding_before_reading_the_store() {
    let root = tempfile::tempdir().unwrap();
    for omitted in [
        "--plan-id",
        "--store",
        "--check-id",
        "--repository",
        "--repository-id",
        "--app-id",
        "--check-name",
    ] {
        let arguments = without_flag(omitted);
        let output = Command::new(env!("CARGO_BIN_EXE_wcode"))
            .current_dir(root.path())
            .args(&arguments)
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted without {omitted}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("required"),
            "expected a missing-argument error for {omitted}"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}
