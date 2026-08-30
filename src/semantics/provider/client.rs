use super::*;

pub(super) struct LspClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    workspace_uri: Option<String>,
}

impl LspClient {
    pub(super) async fn start(
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
    ) -> Result<Self> {
        let mut command = Command::new(executable);
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
                        "workspace":{"workspaceFolders":true},
                        "textDocument":{
                            "documentSymbol":{"hierarchicalDocumentSymbolSupport":true},
                            "callHierarchy":{"dynamicRegistration":false}
                        }
                    },
                    "clientInfo":{"name":"wcode","version":env!("CARGO_PKG_VERSION")}
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        Ok(result.get("capabilities").cloned().unwrap_or(Value::Null))
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

    pub(super) async fn shutdown(&mut self) -> Result<()> {
        let _ = self.request("shutdown", Value::Null).await;
        let _ = self.notify("exit", Value::Null).await;
        let _ = timeout(Duration::from_secs(2), self.child.wait()).await;
        Ok(())
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
                "SSH_AUTH_SOCK" | "KUBECONFIG" | "DOCKER_CONFIG" | "NETRC" | "GIT_ASKPASS"
            )
        {
            command.env_remove(key);
        }
    }
}
