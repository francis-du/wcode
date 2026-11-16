use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn hardened() -> WorkspaceSecurity {
    WorkspaceSecurity {
        allow_risky_exec: false,
        allow_unrestricted_commands: false,
        allow_semantic_exec: false,
        allow_destructive_writes: false,
        allow_overlapping_workspaces: false,
        allow_user_home_workspace: false,
        allow_broad_workspace: false,
    }
}

#[test]
fn gh_shorthand_attached_values_cannot_redirect_the_repository() {
    // pflag accepts `-R=owner/repo` and `-Rowner/repo`, and `--web=true`, none
    // of which match the bare flag names.
    for command in [
        vec!["pr", "view", "1", "-R=octocat/hello"],
        vec!["pr", "view", "1", "-Roctocat/hello"],
        vec!["pr", "view", "1", "--web=true"],
        vec!["pr", "view", "1", "--editor=vim"],
    ] {
        assert!(
            validate_command_policy("gh", &args(&command), hardened()).is_err(),
            "gh redirection unexpectedly accepted: {command:?}"
        );
    }
    assert!(
        validate_command_policy("gh", &args(&["pr", "view", "1"]), hardened()).is_ok(),
        "plain read-only gh must stay allowed"
    );
}

#[test]
fn bazel_label_options_cannot_traverse_out_of_the_workspace() {
    // A `..` component is never a valid label path, so `--output_base=//../..`
    // is a filesystem path and must go through the containment check.
    for (program, command) in [
        ("bazel", vec!["--output_base=//../../tmp/out", "build"]),
        ("bazel", vec!["--output_base", "//../../tmp/out", "build"]),
        ("bazelisk", vec!["build", "@repo//../../etc"]),
    ] {
        assert!(
            validate_command_policy(program, &args(&command), hardened()).is_err(),
            "bazel traversal unexpectedly accepted: {command:?}"
        );
    }
    assert!(
        validate_command_policy("bazel", &args(&["build", "//src:main"]), hardened()).is_ok(),
        "ordinary label must stay allowed"
    );
}
