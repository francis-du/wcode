use super::*;

#[test]
fn focused_rust_lib_test_filter_is_exact_and_bounded() {
    for values in [
        vec!["test", "--lib", "focused_smoke"],
        vec!["test", "--locked", "--lib", "module::focused_smoke"],
    ] {
        assert!(validate_verification_command_shape("cargo", &args(&values)).is_ok());
    }
    for values in [
        vec!["test", "--lib", "--nocapture"],
        vec!["test", "--lib", "../focused"],
        vec![
            "test",
            "--locked",
            "--lib",
            "focused_smoke",
            "--",
            "--nocapture",
        ],
        vec!["test", "--workspace", "--lib", "focused_smoke"],
    ] {
        assert!(
            validate_verification_command_shape("cargo", &args(&values)).is_err(),
            "lib-focused verification unexpectedly accepted {values:?}"
        );
    }
}
