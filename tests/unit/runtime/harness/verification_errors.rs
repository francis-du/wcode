use super::*;

fn fixture_check() -> CheckSpec {
    CheckSpec {
        id: "python-tests".into(),
        level: "full".into(),
        phase: 1,
        program: "pytest".into(),
        args: vec!["-q".into()],
        cwd: ".".into(),
        island: ".".into(),
        languages: vec!["python".into()],
        reason: "Run the Python test suite.".into(),
    }
}

#[test]
fn missing_verification_executable_reports_an_actionable_recovery() {
    let source = std::io::Error::new(std::io::ErrorKind::NotFound, "fixture missing executable");
    let error = anyhow::Error::new(source).context("failed to start command");
    let message = verification_command_error(&fixture_check(), &error);

    assert!(message.contains("verification executable `pytest` is unavailable"));
    assert!(message.contains("install the project toolchain"));
    assert!(message.contains("retry verify_project"));
    assert!(message.contains("failed to start command"));
    assert!(message.contains("fixture missing executable"));
}

#[test]
fn non_missing_verification_errors_preserve_the_full_error_chain() {
    let source = anyhow::anyhow!("fixture policy rejection");
    let error = source.context("verification command rejected");
    let message = verification_command_error(&fixture_check(), &error);

    assert_eq!(
        message,
        "verification command rejected: fixture policy rejection"
    );
}
