use super::*;
use std::collections::BTreeMap;
use std::env;
use std::sync::{Arc, Mutex, PoisonError};

const MAX_LSP_HEADER_BYTES: usize = 8 * 1024;
const MAX_LSP_STDERR_BYTES: usize = 16 * 1024;
const MAX_LSP_DIAGNOSTIC_DOCUMENTS: usize = 32;
const MAX_LSP_DIAGNOSTICS_PER_DOCUMENT: usize = 64;
const MAX_LSP_DIAGNOSTIC_BYTES_PER_DOCUMENT: usize = 64 * 1024;
const MAX_LSP_DIAGNOSTIC_MESSAGE_CHARS: usize = 2_000;
const MAX_LSP_DIAGNOSTIC_DATA_BYTES: usize = 4 * 1024;
const LSP_STDERR_SETTLE_DELAY: Duration = Duration::from_millis(100);

pub(super) fn render_error_chain(error: &anyhow::Error) -> String {
    // Render the whole chain: the stage context alone hides the root cause
    // (and captured server stderr) that the operator needs to act on.
    let mut rendered = error.to_string();
    for source in error.chain().skip(1) {
        rendered.push_str(": ");
        rendered.push_str(&source.to_string());
    }
    rendered
}

#[derive(Clone, Debug)]
struct PublishedDiagnostics {
    version: Option<i64>,
    diagnostics: Vec<Value>,
    truncated: bool,
}

pub(super) struct LspClient {
    child: Child,
    child_group: crate::resource::ChildProcessGuard,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    // Continuously drained by a background task so chatty servers can never
    // block on a full stderr pipe; only the bounded tail is kept for failures.
    stderr_tail: Arc<Mutex<Vec<u8>>>,
    next_id: u64,
    provider_id: &'static str,
    workspace_uri: Option<String>,
    published_diagnostics: BTreeMap<String, PublishedDiagnostics>,
}

impl LspClient {
    pub(super) async fn start(
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
    ) -> Result<Self> {
        let executable = provider_launch_executable(workspace, provider, executable)?;
        let args = provider_launch_args(workspace, provider)?;
        let mut command = Command::new(&executable);
        command
            .args(&args)
            .current_dir(workspace.root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .env("NO_COLOR", "1");
        scrub_environment(&mut command);
        crate::resource::apply_child_limits(&mut command);
        let mut child = command.spawn().with_context(|| {
            format!(
                "LSP stage=spawn server={} executable={} action=check_server_permissions_or_reinstall",
                provider.id,
                executable.display()
            )
        })?;
        let child_group = crate::resource::supervise_child(&child);
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("LSP server stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("LSP server stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("LSP server stderr unavailable"))?;
        let stderr_tail = Arc::new(Mutex::new(Vec::new()));
        {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut stderr = BufReader::new(stderr);
                let mut chunk = [0u8; 4096];
                loop {
                    let read = match stderr.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(read) => read,
                    };
                    let mut tail = tail.lock().unwrap_or_else(PoisonError::into_inner);
                    tail.extend_from_slice(&chunk[..read]);
                    let excess = tail.len().saturating_sub(MAX_LSP_STDERR_BYTES);
                    if excess > 0 {
                        tail.drain(..excess);
                    }
                }
            });
        }
        Ok(Self {
            child,
            child_group,
            stdin,
            stdout: BufReader::new(stdout),
            stderr_tail,
            next_id: 1,
            provider_id: provider.id,
            workspace_uri: None,
            published_diagnostics: BTreeMap::new(),
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
                        "workspace":{"workspaceFolders":true,"configuration":true},
                        "textDocument":{
                            "synchronization":{
                                "dynamicRegistration":false,
                                "willSave":false,
                                "willSaveWaitUntil":false,
                                "didSave":false
                            },
                            "documentSymbol":{"hierarchicalDocumentSymbolSupport":true},
                            "definition":{"dynamicRegistration":false,"linkSupport":true},
                            "references":{"dynamicRegistration":false},
                            "implementation":{"dynamicRegistration":false,"linkSupport":true},
                            "hover":{"dynamicRegistration":false},
                            "callHierarchy":{"dynamicRegistration":false},
                            "publishDiagnostics":{
                                "relatedInformation":true,
                                "versionSupport":true,
                                "codeDescriptionSupport":true,
                                "dataSupport":true
                            },
                            "codeAction":{
                                "dynamicRegistration":false,
                                "dataSupport":true,
                                "resolveSupport":{"properties":["edit"]},
                                "codeActionLiteralSupport":{
                                    "codeActionKind":{"valueSet":["quickfix","source.organizeImports"]}
                                }
                            }
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
                } else if message.get("method").is_some() {
                    self.capture_notification(&message);
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

    pub(super) async fn drain_notifications(&mut self, max_wait: Duration) -> Result<usize> {
        let deadline = tokio::time::Instant::now() + max_wait;
        let mut captured = 0usize;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline.saturating_duration_since(now);
            let message = match timeout(remaining, self.read_message()).await {
                Ok(message) => message?,
                Err(_) => break,
            };
            if message.get("method").is_some() && message.get("id").is_some() {
                self.answer_server_request(&message).await?;
            } else if message.get("method").is_some() && self.capture_notification(&message) {
                captured = captured.saturating_add(1);
            }
        }
        Ok(captured)
    }

    #[cfg(test)]
    pub(super) fn diagnostics_for_uri(&self, uri: &str, version: i64) -> Vec<Value> {
        self.diagnostics_snapshot_for_uri(uri, version)
            .map(|(diagnostics, _)| diagnostics)
            .unwrap_or_default()
    }

    pub(super) fn diagnostics_snapshot_for_uri(
        &self,
        uri: &str,
        version: i64,
    ) -> Option<(Vec<Value>, bool)> {
        self.published_diagnostics
            .get(uri)
            .filter(|published| published.version == Some(version))
            .map(|published| (published.diagnostics.clone(), published.truncated))
    }

    fn capture_notification(&mut self, message: &Value) -> bool {
        if message.get("method").and_then(Value::as_str) != Some("textDocument/publishDiagnostics")
        {
            return false;
        }
        let Some(uri) = message.pointer("/params/uri").and_then(Value::as_str) else {
            return false;
        };
        let version = message.pointer("/params/version").and_then(Value::as_i64);
        let Some(raw) = message
            .pointer("/params/diagnostics")
            .and_then(Value::as_array)
        else {
            return false;
        };
        let published = compact_published_diagnostics(version, raw);
        if !self.published_diagnostics.contains_key(uri)
            && self.published_diagnostics.len() >= MAX_LSP_DIAGNOSTIC_DOCUMENTS
        {
            if let Some(oldest) = self.published_diagnostics.keys().next().cloned() {
                self.published_diagnostics.remove(&oldest);
            }
        }
        self.published_diagnostics.insert(uri.to_owned(), published);
        true
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
        let protocol_context = || {
            format!(
                "LSP stage=protocol server={} action=restart_or_reinstall_lsp",
                self.provider_id
            )
        };
        self.stdin
            .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .with_context(protocol_context)?;
        self.stdin
            .write_all(&body)
            .await
            .with_context(protocol_context)?;
        self.stdin.flush().await.with_context(protocol_context)?;
        Ok(())
    }

    async fn read_message(&mut self) -> Result<Value> {
        let mut content_length = None;
        let mut header_bytes = 0usize;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).await.with_context(|| {
                format!(
                    "LSP stage=protocol server={} action=restart_or_reinstall_lsp",
                    self.provider_id
                )
            })?;
            if read == 0 {
                // Give the stderr drain task a moment to collect crash-time output
                // before snapshotting the tail for diagnostics.
                tokio::time::sleep(LSP_STDERR_SETTLE_DELAY).await;
                let diagnostics = self.failure_diagnostics();
                bail!(
                    "LSP stage=protocol server={} action=check_lsp_configuration_or_permissions: server closed stdout{}",
                    self.provider_id,
                    diagnostics
                );
            }
            header_bytes = header_bytes.saturating_add(read);
            if header_bytes > MAX_LSP_HEADER_BYTES {
                bail!("LSP headers exceed 8 KiB bound");
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
        self.stdout.read_exact(&mut body).await.with_context(|| {
            format!(
                "LSP stage=protocol server={} action=restart_or_reinstall_lsp",
                self.provider_id
            )
        })?;
        serde_json::from_slice(&body).with_context(|| {
            format!(
                "LSP stage=protocol server={} action=check_lsp_compatibility",
                self.provider_id
            )
        })
    }

    fn failure_diagnostics(&self) -> String {
        let bytes = self
            .stderr_tail
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if bytes.is_empty() {
            return String::new();
        }
        let text = String::from_utf8_lossy(&bytes);
        let (safe, _) = redact_sensitive_text(text.trim());
        if safe.is_empty() {
            String::new()
        } else {
            format!("; stderr={safe}")
        }
    }
}

fn compact_published_diagnostics(version: Option<i64>, raw: &[Value]) -> PublishedDiagnostics {
    let mut diagnostics = Vec::new();
    let mut bytes = 0usize;
    let mut truncated = raw.len() > MAX_LSP_DIAGNOSTICS_PER_DOCUMENT;
    for diagnostic in raw.iter().take(MAX_LSP_DIAGNOSTICS_PER_DOCUMENT) {
        let Some(diagnostic) = compact_diagnostic(diagnostic) else {
            continue;
        };
        let size = serde_json::to_vec(&diagnostic)
            .map(|value| value.len())
            .unwrap_or(0);
        if bytes.saturating_add(size) > MAX_LSP_DIAGNOSTIC_BYTES_PER_DOCUMENT {
            truncated = true;
            break;
        }
        bytes = bytes.saturating_add(size);
        diagnostics.push(diagnostic);
    }
    PublishedDiagnostics {
        version,
        diagnostics,
        truncated,
    }
}

fn compact_diagnostic(value: &Value) -> Option<Value> {
    let range = value.get("range")?.clone();
    if !range.is_object() {
        return None;
    }
    let message = value.get("message")?.as_str()?;
    let mut object = serde_json::Map::new();
    object.insert("range".into(), range);
    object.insert(
        "message".into(),
        Value::String(
            message
                .chars()
                .take(MAX_LSP_DIAGNOSTIC_MESSAGE_CHARS)
                .collect(),
        ),
    );
    for key in ["severity", "code", "source", "tags"] {
        if let Some(field) = value.get(key) {
            object.insert(key.to_owned(), field.clone());
        }
    }
    if let Some(data) = value.get("data") {
        if serde_json::to_vec(data).is_ok_and(|bytes| bytes.len() <= MAX_LSP_DIAGNOSTIC_DATA_BYTES)
        {
            object.insert("data".into(), data.clone());
        }
    }
    Some(Value::Object(object))
}

#[cfg(test)]
#[path = "../../../tests/unit/semantics/client.rs"]
mod tests;

impl Drop for LspClient {
    fn drop(&mut self) {
        self.child_group.terminate();
        let _ = self.child.start_kill();
    }
}

pub(super) fn provider_launch_executable(
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
) -> Result<PathBuf> {
    let executable = if executable.is_absolute() {
        executable.to_path_buf()
    } else {
        env::current_dir()
            .context("cannot resolve current directory for LSP server")?
            .join(executable)
    };
    let canonical = executable.canonicalize().with_context(|| {
        format!(
            "LSP stage=discovery executable={} action=refresh_lsp_discovery_or_reinstall",
            executable.display()
        )
    })?;
    if canonical.starts_with(workspace.root()) {
        bail!("LSP executable resolves inside the Workspace and is not trusted");
    }
    // Validate the canonical target. Most providers execute the discovered path so
    // argv[0]-sensitive proxies such as rustup's rust-analyzer keep their identity.
    // LuaLS is the notable inverse: upstream warns that symlinking its binary can
    // break relative script discovery, so launch the canonical target when needed.
    if provider.id == "lua-language-server"
        && executable.symlink_metadata()?.file_type().is_symlink()
    {
        return Ok(canonical);
    }
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
