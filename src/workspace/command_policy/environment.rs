use super::*;

const MANAGED_COMMAND_ENV_KEYS: [&str; 5] = ["HOST", "LOG_LEVEL", "NODE_ENV", "PORT", "RUST_LOG"];
const MIN_MANAGED_PORT: u16 = 1024;

pub(in crate::workspace) fn validate_command_environment(
    environment: &[(String, String)],
) -> Result<Vec<(String, String)>> {
    if environment.len() > MANAGED_COMMAND_ENV_KEYS.len() {
        bail!(
            "run_command env accepts at most {} bounded overrides",
            MANAGED_COMMAND_ENV_KEYS.len()
        );
    }
    let mut normalized = environment.to_vec();
    normalized.sort_by(|left, right| left.0.cmp(&right.0));
    if normalized.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        bail!("run_command env contains a duplicate key");
    }
    for (key, value) in &normalized {
        if !MANAGED_COMMAND_ENV_KEYS.contains(&key.as_str()) {
            bail!(
                "run_command env override is not in the bounded non-secret allowlist: [REDACTED]"
            );
        }
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            bail!("run_command env value for a supported key is empty, too long, or contains control characters");
        }
        let valid = match key.as_str() {
            "PORT" => value
                .parse::<u16>()
                .is_ok_and(|port| port >= MIN_MANAGED_PORT),
            "HOST" => matches!(value.as_str(), "127.0.0.1" | "::1"),
            "NODE_ENV" => matches!(value.as_str(), "development" | "test"),
            "RUST_LOG" | "LOG_LEVEL" => matches!(
                value.as_str(),
                "off" | "error" | "warn" | "info" | "debug" | "trace"
            ),
            _ => unreachable!("managed environment key was checked above"),
        };
        if !valid {
            bail!("run_command env value is invalid for supported key {key}");
        }
    }
    Ok(normalized)
}

pub(in crate::workspace) fn apply_command_environment(
    command: &mut Command,
    environment: &[(String, String)],
) {
    if environment.is_empty() {
        return;
    }
    // Supplying managed launch environment opts into a closed typed view for
    // these keys: inherited host values cannot silently bypass validation.
    for key in MANAGED_COMMAND_ENV_KEYS {
        command.env_remove(key);
    }
    for (key, value) in environment {
        command.env(key, value);
    }
}

pub(in crate::workspace) fn command_environment_injection_key(upper: &str) -> bool {
    upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || matches!(
            upper,
            "BASH_ENV"
                | "ENV"
                | "NODE_OPTIONS"
                | "NODE_PATH"
                | "PYTHONHOME"
                | "PYTHONPATH"
                | "PYTHONSTARTUP"
                | "RUBYOPT"
                | "RUBYLIB"
                | "PERL5OPT"
                | "PERL5LIB"
                | "LUA_INIT"
                | "LUA_PATH"
                | "LUA_CPATH"
                | "PHPRC"
                | "PHP_INI_SCAN_DIR"
                | "JAVA_TOOL_OPTIONS"
                | "_JAVA_OPTIONS"
                | "JDK_JAVA_OPTIONS"
                | "DOTNET_STARTUP_HOOKS"
                | "DOTNET_ADDITIONAL_DEPS"
                | "DOTNET_SHARED_STORE"
                | "RUSTC"
                | "RUSTDOC"
                | "RUSTC_WRAPPER"
                | "RUSTC_WORKSPACE_WRAPPER"
                | "RUSTFLAGS"
                | "CARGO_ENCODED_RUSTFLAGS"
        )
}
