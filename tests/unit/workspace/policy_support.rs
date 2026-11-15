use super::*;

pub(super) fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(super) fn hardened() -> WorkspaceSecurity {
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
