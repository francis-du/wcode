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

fn python_cache_isolation_prefix() -> &'static PathBuf {
    static PREFIX: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PREFIX.get_or_init(|| {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("wcode-python-cache-{}-{nonce}", std::process::id()))
    })
}

pub(in crate::workspace) fn scrub_sensitive_environment(
    command: &mut Command,
    program: &str,
    args: &[String],
    allow_git_push_credentials: bool,
) {
    for (key, _) in std::env::vars() {
        let upper = key.to_ascii_uppercase();
        let gh_auth = program == "gh"
            && matches!(
                upper.as_str(),
                "GH_TOKEN" | "GITHUB_TOKEN" | "GH_ENTERPRISE_TOKEN"
            );
        let git_ssh_agent = program == "git"
            && allow_git_push_credentials
            && is_git_push_command(args)
            && upper == "SSH_AUTH_SOCK";
        let generic_secret = (program == "git" && upper.starts_with("GIT_"))
            || upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("PASSWORD")
            || upper.contains("CREDENTIAL")
            || upper.contains("PRIVATE_KEY")
            || upper == "WCODE_GITHUB_APP_KEY_FILE"
            || upper.ends_with("_KEY")
            || upper.starts_with("AWS_")
            || upper.starts_with("AZURE_")
            || upper.starts_with("GOOGLE_")
            || upper.starts_with("GITHUB_")
            || upper.starts_with("GITLAB_")
            || matches!(
                upper.as_str(),
                "SSH_AUTH_SOCK" | "KUBECONFIG" | "DOCKER_CONFIG" | "NETRC" | "GIT_ASKPASS"
            );
        let tool_redirect = (program == "gh"
            && matches!(
                upper.as_str(),
                "GH_REPO" | "GH_HOST" | "GH_CONFIG_DIR" | "GH_EDITOR" | "GH_BROWSER"
            ))
            || (program == "docker"
                && (upper.starts_with("COMPOSE_")
                    || matches!(
                        upper.as_str(),
                        "DOCKER_HOST" | "DOCKER_CONTEXT" | "DOCKER_CERT_PATH" | "DOCKER_TLS_VERIFY"
                    )))
            || (program == "kubectl" && upper == "KUBECTL_EXTERNAL_DIFF")
            || (program == "terraform"
                && (upper.starts_with("TF_CLI_ARGS")
                    || upper.starts_with("TF_VAR_")
                    || matches!(
                        upper.as_str(),
                        "TF_CLI_CONFIG_FILE" | "TF_DATA_DIR" | "TF_WORKSPACE"
                    )))
            || (program == "uv"
                && matches!(
                    upper.as_str(),
                    "UV_PROJECT"
                        | "UV_WORKING_DIR"
                        | "UV_CONFIG_FILE"
                        | "UV_DEFAULT_INDEX"
                        | "UV_INDEX"
                        | "UV_INSECURE_HOST"
                        | "UV_KEYRING_PROVIDER"
                        | "UV_CACHE_DIR"
                ));
        if (!gh_auth && !git_ssh_agent && generic_secret)
            || tool_redirect
            || command_environment_injection_key(&upper)
        {
            command.env_remove(key);
        }
    }
    command.env("NO_COLOR", "1");
    if matches!(program, "python" | "python3" | "pytest") {
        command
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONPYCACHEPREFIX", python_cache_isolation_prefix());
    }
    if program == "gh" {
        command
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_PAGER", "cat")
            .env("PAGER", "cat");
    }
    if program == "terraform" {
        command.env("TF_IN_AUTOMATION", "1");
    }
    if program == "git" {
        let null_config = if cfg!(windows) { "NUL" } else { "/dev/null" };
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", null_config)
            .env("GIT_CONFIG_GLOBAL", null_config)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_PROTOCOL_FROM_USER", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("GIT_EDITOR", "false")
            .env("GIT_SEQUENCE_EDITOR", "false")
            .env("GIT_EXTERNAL_DIFF", "");
    }
}
