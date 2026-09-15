use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn make_relaxes_bounded_development_flags_without_opening_external_targets() {
    let safe = WorkspaceSecurity::default();
    for target in [
        "ownership-check",
        "tokenizer-test",
        "deployment-plan-check",
        "pushdown-benchmark",
    ] {
        assert!(
            validate_command_policy("make", &args(&[target]), safe).is_ok(),
            "benign target name unexpectedly required authorization: {target}"
        );
    }
    for values in [
        vec!["--trace", "test"],
        vec!["--warn-undefined-variables", "check"],
        vec!["-rR", "build"],
        vec!["-Otarget", "test"],
        vec!["--output-sync=line", "test"],
    ] {
        assert!(
            validate_command_policy("make", &args(&values), safe).is_ok(),
            "bounded make flag unexpectedly required authorization: {values:?}"
        );
    }
    for target in [
        "publish-smoke",
        "deploy-smoke",
        "upload-smoke",
        "secret-smoke",
        "token-smoke",
        "login-smoke",
    ] {
        assert!(
            validate_command_policy("make", &args(&[target]), safe).is_err(),
            "externally consequential make target unexpectedly bypassed authorization: {target}"
        );
    }
    for values in [
        vec!["-j", "test"],
        vec!["--jobs=0", "test"],
        vec!["--output-sync=unknown", "test"],
        vec!["CC=sh", "test"],
        vec!["--eval", "value:=unsafe", "test"],
    ] {
        assert!(
            validate_command_policy("make", &args(&values), safe).is_err(),
            "unbounded make shape unexpectedly bypassed authorization: {values:?}"
        );
    }
}

#[test]
fn loopback_curl_is_autonomous_but_network_or_mutating_shapes_stay_gated() {
    let safe = WorkspaceSecurity::default();
    for values in [
        vec!["http://127.0.0.1:8765/healthz"],
        vec![
            "-fsS",
            "--max-time",
            "3",
            "http://localhost:8765/intelligence",
        ],
        vec!["-I", "--connect-timeout=2", "http://[::1]:8765/healthz"],
    ] {
        assert!(
            validate_command_policy("curl", &args(&values), safe).is_ok(),
            "loopback read probe unexpectedly required authorization: {values:?}"
        );
    }
    for values in [
        vec!["https://example.com/"],
        vec!["-L", "http://127.0.0.1:8765/healthz"],
        vec!["-X", "POST", "http://127.0.0.1:8765/healthz"],
        vec!["-o", "health.txt", "http://127.0.0.1:8765/healthz"],
        vec![
            "http://127.0.0.1:8765/healthz",
            "http://localhost:8765/healthz",
        ],
    ] {
        assert!(
            validate_command_policy("curl", &args(&values), safe).is_err(),
            "network or mutating curl shape unexpectedly bypassed authorization: {values:?}"
        );
    }
}
