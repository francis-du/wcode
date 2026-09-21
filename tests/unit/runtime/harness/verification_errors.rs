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
fn default_verification_timeout_gives_only_rust_release_a_bounded_cold_build_floor() {
    let normal = fixture_check();
    assert_eq!(verification_check_timeout_seconds(&normal, 120), 120);

    let mut build = fixture_check();
    build.id = "rust-release-build".into();
    build.phase = 3;
    assert_eq!(verification_check_timeout_seconds(&build, 120), 300);

    let mut other_build = build.clone();
    other_build.id = "java-gradle-build".into();
    assert_eq!(verification_check_timeout_seconds(&other_build, 120), 120);

    let mut quick_build = build.clone();
    quick_build.level = "quick".into();
    assert_eq!(verification_check_timeout_seconds(&quick_build, 120), 120);

    assert_eq!(verification_check_timeout_seconds(&build, 60), 60);
    assert_eq!(verification_check_timeout_seconds(&build, 600), 600);
    assert_eq!(verification_check_timeout_seconds(&build, 2_400), 1_800);
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
