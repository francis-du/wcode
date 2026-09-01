use super::*;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard};

const MAX_SESSIONS: usize = 16;
const SESSION_IDLE_MS: u64 = 10 * 60 * 1_000;

fn session_limit() -> usize {
    crate::resource::limits()
        .semantic_session_limit()
        .min(MAX_SESSIONS)
}

#[derive(Clone, Default)]
pub(crate) struct SemanticSessionPool {
    state: Arc<Mutex<SessionPoolState>>,
}

#[derive(Default)]
struct SessionPoolState {
    slots: BTreeMap<String, Arc<SessionSlot>>,
}

struct SessionSlot {
    workspace: PathBuf,
    provider: &'static str,
    executable: PathBuf,
    metrics: Arc<SessionMetrics>,
    session: AsyncMutex<Option<SemanticSession>>,
}

#[derive(Default)]
struct SessionMetrics {
    last_used_ms: AtomicU64,
    starts: AtomicU64,
    requests: AtomicU64,
    documents: AtomicUsize,
}

pub(super) struct SessionHandle {
    slot: Arc<SessionSlot>,
}

pub(super) struct SemanticSession {
    pub(super) client: LspClient,
    pub(super) capabilities: Value,
    pub(super) position_encoding: String,
    document_sync: TextDocumentSyncProfile,
    documents: BTreeMap<String, SyncedDocument>,
    metrics: Arc<SessionMetrics>,
}

struct SyncedDocument {
    sha256: String,
    version: i64,
    // Full source text already lives in the provider. A full-document
    // incremental replacement only needs the previous encoded end position.
    end_position: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextDocumentSyncMode {
    None,
    Full,
    Incremental,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TextDocumentSyncProfile {
    open_close: bool,
    change: TextDocumentSyncMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocumentSyncState {
    Opened,
    Changed,
    Current,
    ServerReadsDisk,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticSessionPoolStatus {
    pub sessions: usize,
    pub documents: usize,
    pub starts: u64,
    pub requests: u64,
    pub max_sessions: usize,
    pub idle_timeout_seconds: u64,
}

impl SemanticSessionPool {
    pub(super) fn handle(
        &self,
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
    ) -> Result<SessionHandle> {
        let key = session_key(workspace, provider, executable)?;
        let now = now_ms();
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("semantic session pool lock poisoned"))?;
        prune_slots(&mut state, now);
        if let Some(existing) = state.slots.get(&key) {
            existing.metrics.last_used_ms.store(now, Ordering::Relaxed);
            return Ok(SessionHandle {
                slot: existing.clone(),
            });
        }
        if state.slots.values().any(|slot| {
            slot.workspace == workspace.root()
                && slot.provider == provider.id
                && Arc::strong_count(slot) > 1
        }) {
            bail!(
                "semantic provider session is busy while provider identity changed; retry after the active request completes"
            );
        }
        state.slots.retain(|_, slot| {
            !(slot.workspace == workspace.root() && slot.provider == provider.id)
        });
        while state.slots.len() >= session_limit() {
            let Some(oldest) = state
                .slots
                .iter()
                .filter(|(_, slot)| Arc::strong_count(slot) == 1)
                .min_by_key(|(_, slot)| slot.metrics.last_used_ms.load(Ordering::Relaxed))
                .map(|(key, _)| key.clone())
            else {
                bail!(
                    "semantic session pool is at capacity and every slot is currently leased; retry after an active request completes"
                );
            };
            state.slots.remove(&oldest);
        }
        let metrics = Arc::new(SessionMetrics::default());
        metrics.last_used_ms.store(now, Ordering::Relaxed);
        let slot = Arc::new(SessionSlot {
            workspace: workspace.root().to_path_buf(),
            provider: provider.id,
            executable: executable.to_path_buf(),
            metrics,
            session: AsyncMutex::new(None),
        });
        state.slots.insert(key, slot.clone());
        Ok(SessionHandle { slot })
    }

    pub(crate) fn prune_idle(&self) {
        if let Ok(mut state) = self.state.lock() {
            prune_slots(&mut state, now_ms());
            prune_to_limit(&mut state, session_limit());
        }
    }

    pub(crate) fn trim_memory(&self, aggressive: bool) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let target = if aggressive {
            0
        } else {
            (session_limit() / 2).max(1)
        };
        prune_to_limit(&mut state, target);
    }

    pub(super) fn validated(
        &self,
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
    ) -> bool {
        let Ok(key) = session_key(workspace, provider, executable) else {
            return false;
        };
        let now = now_ms();
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        prune_slots(&mut state, now);
        state
            .slots
            .get(&key)
            .is_some_and(|slot| slot.metrics.starts.load(Ordering::Relaxed) > 0)
    }

    pub(crate) fn status(&self) -> SemanticSessionPoolStatus {
        self.status_for_root(None)
    }

    pub(crate) fn status_for(&self, workspace: &Workspace) -> SemanticSessionPoolStatus {
        self.status_for_root(Some(workspace.root()))
    }

    fn status_for_root(&self, root: Option<&Path>) -> SemanticSessionPoolStatus {
        let now = now_ms();
        let mut state = self
            .state
            .lock()
            .expect("semantic session pool lock poisoned");
        prune_slots(&mut state, now);
        let slots = state
            .slots
            .values()
            .filter(|slot| root.is_none_or(|root| slot.workspace == root))
            .collect::<Vec<_>>();
        SemanticSessionPoolStatus {
            sessions: slots.len(),
            documents: slots
                .iter()
                .map(|slot| slot.metrics.documents.load(Ordering::Relaxed))
                .sum(),
            starts: slots
                .iter()
                .map(|slot| slot.metrics.starts.load(Ordering::Relaxed))
                .sum(),
            requests: slots
                .iter()
                .map(|slot| slot.metrics.requests.load(Ordering::Relaxed))
                .sum(),
            max_sessions: session_limit(),
            idle_timeout_seconds: SESSION_IDLE_MS / 1_000,
        }
    }

    pub(super) fn invalidate(
        &self,
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
    ) {
        let Ok(key) = session_key(workspace, provider, executable) else {
            return;
        };
        if let Ok(mut state) = self.state.lock() {
            state.slots.remove(&key);
        }
    }
}

impl SessionHandle {
    pub(super) async fn lock(&self) -> AsyncMutexGuard<'_, Option<SemanticSession>> {
        self.slot
            .metrics
            .last_used_ms
            .store(now_ms(), Ordering::Relaxed);
        self.slot.session.lock().await
    }

    pub(super) async fn ensure_started(
        &self,
        workspace: &Workspace,
        provider: ProviderCandidate,
    ) -> Result<bool> {
        let mut session = self.lock().await;
        let mut reused = session.is_some();
        if session
            .as_mut()
            .is_some_and(|session| !session.client.is_alive())
        {
            *session = None;
            reused = false;
        }
        if session.is_none() {
            *session = Some(
                SemanticSession::start(
                    workspace,
                    provider,
                    &self.slot.executable,
                    self.slot.metrics.clone(),
                )
                .await?,
            );
        }
        Ok(reused)
    }
}

impl SemanticSession {
    async fn start(
        workspace: &Workspace,
        provider: ProviderCandidate,
        executable: &Path,
        metrics: Arc<SessionMetrics>,
    ) -> Result<Self> {
        let mut client = LspClient::start(workspace, provider, executable).await?;
        let root_uri = Url::from_directory_path(workspace.root())
            .map_err(|_| anyhow!("workspace root could not be converted to a file URI"))?
            .to_string();
        let capabilities = client.initialize(&root_uri).await?;
        let position_encoding = capabilities
            .get("positionEncoding")
            .and_then(Value::as_str)
            .unwrap_or("utf-16")
            .to_owned();
        let document_sync = text_document_sync_profile(&capabilities);
        metrics.starts.fetch_add(1, Ordering::Relaxed);
        metrics.last_used_ms.store(now_ms(), Ordering::Relaxed);
        Ok(Self {
            client,
            capabilities,
            position_encoding,
            document_sync,
            documents: BTreeMap::new(),
            metrics,
        })
    }

    pub(super) async fn sync_document(
        &mut self,
        workspace: &Workspace,
        source: &SourceDocument,
        language: SemanticLanguage,
    ) -> Result<(String, DocumentSyncState)> {
        let uri = Url::from_file_path(workspace.root().join(&source.path))
            .map_err(|_| anyhow!("source path could not be converted to a file URI"))?
            .to_string();
        let state = match self.documents.get(&source.path) {
            Some(document) if document.sha256 == source.sha256 => DocumentSyncState::Current,
            Some(document) => {
                let version = document.version.saturating_add(1);
                let sync_state = match self.document_sync.change {
                    TextDocumentSyncMode::None => DocumentSyncState::ServerReadsDisk,
                    TextDocumentSyncMode::Full => {
                        self.notify(
                            "textDocument/didChange",
                            json!({
                                "textDocument": {"uri": uri, "version": version},
                                "contentChanges": [{"text": source.content}]
                            }),
                        )
                        .await?;
                        DocumentSyncState::Changed
                    }
                    TextDocumentSyncMode::Incremental => {
                        let end = document.end_position.clone();
                        self.notify(
                            "textDocument/didChange",
                            json!({
                                "textDocument": {"uri": uri, "version": version},
                                "contentChanges": [{
                                    "range": {"start":{"line":0,"character":0},"end":end},
                                    "text": source.content
                                }]
                            }),
                        )
                        .await?;
                        DocumentSyncState::Changed
                    }
                };
                if let Some(document) = self.documents.get_mut(&source.path) {
                    document.version = version;
                    document.sha256 = source.sha256.clone();
                    document.end_position =
                        document_end_position(&source.content, &self.position_encoding);
                }
                sync_state
            }
            None => {
                let sync_state = if self.document_sync.open_close {
                    self.notify(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": uri,
                                "languageId": language.lsp_language_id(),
                                "version": 1,
                                "text": source.content,
                            }
                        }),
                    )
                    .await?;
                    DocumentSyncState::Opened
                } else {
                    DocumentSyncState::ServerReadsDisk
                };
                self.documents.insert(
                    source.path.clone(),
                    SyncedDocument {
                        sha256: source.sha256.clone(),
                        version: 1,
                        end_position: document_end_position(
                            &source.content,
                            &self.position_encoding,
                        ),
                    },
                );
                self.metrics
                    .documents
                    .store(self.documents.len(), Ordering::Relaxed);
                sync_state
            }
        };
        self.metrics.last_used_ms.store(now_ms(), Ordering::Relaxed);
        Ok((uri, state))
    }

    pub(super) async fn retain_documents(
        &mut self,
        workspace: &Workspace,
        keep: &BTreeSet<String>,
    ) -> Result<()> {
        let stale = self
            .documents
            .keys()
            .filter(|path| !keep.contains(*path))
            .cloned()
            .collect::<Vec<_>>();
        for path in stale {
            if self.document_sync.open_close {
                let uri = Url::from_file_path(workspace.root().join(&path))
                    .map_err(|_| anyhow!("source path could not be converted to a file URI"))?
                    .to_string();
                self.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}))
                    .await?;
            }
            self.documents.remove(&path);
        }
        self.metrics
            .documents
            .store(self.documents.len(), Ordering::Relaxed);
        Ok(())
    }

    pub(super) async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.metrics.requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.last_used_ms.store(now_ms(), Ordering::Relaxed);
        self.client.request(method, params).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.metrics.last_used_ms.store(now_ms(), Ordering::Relaxed);
        self.client.notify(method, params).await
    }
}

fn text_document_sync_profile(capabilities: &Value) -> TextDocumentSyncProfile {
    let Some(sync) = capabilities.get("textDocumentSync") else {
        return TextDocumentSyncProfile {
            open_close: false,
            change: TextDocumentSyncMode::None,
        };
    };
    if let Some(kind) = sync.as_u64() {
        let change = text_document_sync_mode(kind);
        return TextDocumentSyncProfile {
            open_close: change != TextDocumentSyncMode::None,
            change,
        };
    }
    TextDocumentSyncProfile {
        open_close: sync
            .get("openClose")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        change: sync
            .get("change")
            .and_then(Value::as_u64)
            .map(text_document_sync_mode)
            .unwrap_or(TextDocumentSyncMode::None),
    }
}

fn text_document_sync_mode(kind: u64) -> TextDocumentSyncMode {
    match kind {
        1 => TextDocumentSyncMode::Full,
        2 => TextDocumentSyncMode::Incremental,
        _ => TextDocumentSyncMode::None,
    }
}

fn document_end_position(content: &str, encoding: &str) -> Value {
    let line = content.bytes().filter(|byte| *byte == b'\n').count() as u64;
    let tail = content.rsplit('\n').next().unwrap_or_default();
    let character = match encoding {
        "utf-8" => tail.len() as u64,
        "utf-32" => tail.chars().count() as u64,
        _ => tail.encode_utf16().count() as u64,
    };
    json!({"line":line,"character":character})
}

pub(super) fn session_key(
    workspace: &Workspace,
    provider: ProviderCandidate,
    executable: &Path,
) -> Result<String> {
    let canonical = executable
        .canonicalize()
        .with_context(|| format!("cannot resolve semantic provider {}", executable.display()))?;
    if canonical.starts_with(workspace.root()) {
        bail!("semantic provider executable resolves inside the workspace and is not trusted");
    }
    let metadata = std::fs::metadata(&canonical)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    let material = format!(
        "semantic-session-v1\n{}\n{}\n{}\n{}\n{}",
        workspace.root().display(),
        provider.id,
        canonical.display(),
        metadata.len(),
        modified
    );
    Ok(format!("sha256:{:x}", Sha256::digest(material.as_bytes())))
}

fn prune_slots(state: &mut SessionPoolState, now: u64) {
    state.slots.retain(|_, slot| {
        let idle =
            now.saturating_sub(slot.metrics.last_used_ms.load(Ordering::Relaxed)) > SESSION_IDLE_MS;
        !idle || Arc::strong_count(slot) > 1
    });
}

fn prune_to_limit(state: &mut SessionPoolState, target: usize) {
    while state.slots.len() > target {
        let Some(oldest) = state
            .slots
            .iter()
            .filter(|(_, slot)| Arc::strong_count(slot) == 1)
            .min_by_key(|(_, slot)| slot.metrics.last_used_ms.load(Ordering::Relaxed))
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        state.slots.remove(&oldest);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../../tests/unit/semantics/session.rs"]
mod tests;
