use super::*;

pub(super) struct LspClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    provider_id: &'static str,
    workspace_uri: Option<String>,
}

impl LspClient {
    pub(super) async fn start(
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
    ) -> Result<Self> {
        let executable = canonical_provider_executable(workspace, executable)?;
        let mut command = Command::new(&executable);
        command
            .args(provider.args)
            .current_dir(workspace.root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .env("NO_COLOR", "1");
        scrub_environment(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start semantic provider {}", provider.id))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("language server stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("language server stdout unavailable"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            provider_id: provider.id,
            workspace_uri: None,
        })
    }

    pub(super) async fn initialize(&mut self, root_uri: &str) -> Result<Value> {
        self.workspace_uri = Some(root_uri.to_owned());
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": Value::Null,
                    "rootUri": root_uri,
                    "workspaceFolders":[{"uri":root_uri,"name":"wcode-workspace"}],
                    "capabilities":{
                        "general":{"positionEncodings":["utf-8","utf-16"]},
                        "workspace":{"workspaceFolders":true},
                        "textDocument":{
                            "documentSymbol":{"hierarchicalDocumentSymbolSupport":true},
                            "definition":{"dynamicRegistration":false,"linkSupport":true},
                            "references":{"dynamicRegistration":false},
                            "implementation":{"dynamicRegistration":false,"linkSupport":true},
                            "hover":{"dynamicRegistration":false},
                            "callHierarchy":{"dynamicRegistration":false}
                        }
                    },
                    "clientInfo":{"name":"wcode","version":env!("CARGO_PKG_VERSION")},
                    "initializationOptions": initialization_options(self.provider_id)
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        Ok(result.get("capabilities").cloned().unwrap_or(Value::Null))
    }

    pub(super) fn is_alive(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|status| status.is_none())
    }

    pub(super) async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.write_message(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        timeout(LSP_REQUEST_TIMEOUT, async {
            loop {
                let message = self.read_message().await?;
                if message.get("id").and_then(Value::as_u64) == Some(id) {
                    if let Some(error) = message.get("error") {
                        bail!("LSP {method} failed: {error}");
                    }
                    return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                }
                if message.get("method").is_some() && message.get("id").is_some() {
                    self.answer_server_request(&message).await?;
                }
            }
        })
        .await
        .map_err(|_| anyhow!("LSP request {method} timed out"))?
    }

    pub(super) async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write_message(&json!({"jsonrpc":"2.0","method":method,"params":params}))
            .await
    }

    async fn answer_server_request(&mut self, message: &Value) -> Result<()> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let result = match method {
            "workspace/configuration" => {
                let count = message
                    .pointer("/params/items")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                Value::Array(std::iter::repeat_n(Value::Null, count).collect())
            }
            "workspace/workspaceFolders" => self
                .workspace_uri
                .as_ref()
                .map(|uri| json!([{"uri":uri,"name":"wcode-workspace"}]))
                .unwrap_or_else(|| Value::Array(Vec::new())),
            _ => Value::Null,
        };
        self.write_message(&json!({"jsonrpc":"2.0","id":id,"result":result}))
            .await
    }

    async fn write_message(&mut self, value: &Value) -> Result<()> {
        let body = serde_json::to_vec(value)?;
        self.stdin
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await?;
        self.stdin.write_all(&body).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Value> {
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).await?;
            if read == 0 {
                bail!("language server closed stdout");
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if content_length.is_some() {
                    break;
                }
                continue;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .context("invalid LSP Content-Length")?,
                );
            }
        }
        let length = content_length.ok_or_else(|| anyhow!("LSP message missing Content-Length"))?;
        if length > 8 * 1024 * 1024 {
            bail!("LSP message exceeds 8 MiB bound");
        }
        let mut body = vec![0u8; length];
        self.stdout.read_exact(&mut body).await?;
        Ok(serde_json::from_slice(&body)?)
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn canonical_provider_executable(workspace: &Workspace, executable: &Path) -> Result<PathBuf> {
    let executable = if executable.is_absolute() {
        executable.to_path_buf()
    } else {
        env::current_dir()
            .context("cannot resolve current directory for semantic provider")?
            .join(executable)
    };
    let canonical = executable
        .canonicalize()
        .with_context(|| format!("cannot resolve semantic provider {}", executable.display()))?;
    if canonical.starts_with(workspace.root()) {
        bail!("semantic provider executable resolves inside the workspace and is not trusted");
    }
    // Validate the canonical target, but execute the original absolute path so
    // argv[0]-sensitive tool proxies such as rustup's rust-analyzer keep their identity.
    Ok(executable)
}

pub(super) fn initialization_options(provider: &str) -> Value {
    match provider {
        "rust-analyzer" => json!({
            "cargo": {
                "autoreload": false,
                "buildScripts": {"enable": false}
            },
            "procMacro": {"enable": false},
            "checkOnSave": false
        }),
        _ => json!({}),
    }
}

fn scrub_environment(command: &mut Command) {
    for (key, _) in env::vars() {
        let upper = key.to_ascii_uppercase();
        if upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("PASSWORD")
            || upper.ends_with("_KEY")
            || upper.starts_with("AWS_")
            || upper.starts_with("AZURE_")
            || upper.starts_with("GOOGLE_")
            || upper.starts_with("GITHUB_")
            || upper.starts_with("GITLAB_")
            || matches!(
                upper.as_str(),
                "SSH_AUTH_SOCK"
                    | "KUBECONFIG"
                    | "DOCKER_CONFIG"
                    | "NETRC"
                    | "GIT_ASKPASS"
                    | "NODE_OPTIONS"
                    | "NODE_PATH"
                    | "RUSTC_WRAPPER"
                    | "RUSTC_WORKSPACE_WRAPPER"
                    | "RUSTFLAGS"
                    | "RUSTDOCFLAGS"
                    | "CARGO_ENCODED_RUSTFLAGS"
                    | "PYTHONPATH"
                    | "PYTHONSTARTUP"
                    | "RUBYOPT"
                    | "RUBYLIB"
                    | "LD_PRELOAD"
                    | "DYLD_INSERT_LIBRARIES"
                    | "DYLD_LIBRARY_PATH"
                    | "JAVA_TOOL_OPTIONS"
                    | "_JAVA_OPTIONS"
                    | "JDK_JAVA_OPTIONS"
            )
        {
            command.env_remove(key);
        }
    }
}
