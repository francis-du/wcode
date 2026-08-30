use super::{valid_redirect_uri, AccessToken, Client, RefreshToken};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use url::Url;
use uuid::Uuid;

const FORMAT_VERSION: u8 = 1;
const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct AuthStore {
    path: PathBuf,
}

#[derive(Default)]
pub(super) struct StoredAuth {
    pub(super) clients: HashMap<String, Client>,
    pub(super) access_tokens: HashMap<String, AccessToken>,
    pub(super) refresh_tokens: HashMap<String, RefreshToken>,
}

#[derive(Deserialize, Serialize)]
struct StateFile {
    version: u8,
    clients: BTreeMap<String, Client>,
    access_tokens: BTreeMap<String, AccessToken>,
    refresh_tokens: BTreeMap<String, RefreshToken>,
}

impl AuthStore {
    pub(super) fn for_workspaces(roots: &[PathBuf]) -> Result<Self> {
        let mut roots = roots
            .iter()
            .map(|root| root.to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();
        roots.sort();
        let mut hasher = Sha256::new();
        hasher.update(b"wcode-oauth-workspaces-v1\0");
        for root in roots {
            hasher.update(root.as_bytes());
            hasher.update(b"\0");
        }
        let scope = format!("{:x}", hasher.finalize());
        Ok(Self {
            path: state_root()?.join("oauth").join(format!("{scope}.json")),
        })
    }

    #[cfg(test)]
    pub(super) fn at_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub(super) fn load(&self) -> Result<StoredAuth> {
        if !self.path.exists() {
            return Ok(StoredAuth::default());
        }
        let metadata = secure_file_metadata(&self.path)?;
        if metadata.len() > MAX_STATE_BYTES {
            bail!("OAuth state exceeds the {} byte limit", MAX_STATE_BYTES);
        }
        secure_existing_permissions(&self.path, &metadata)?;
        let bytes = fs::read(&self.path)
            .with_context(|| format!("cannot read OAuth state at {}", self.path.display()))?;
        let saved: StateFile = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid OAuth state at {}", self.path.display()))?;
        validate(&saved)?;
        Ok(StoredAuth {
            clients: saved.clients.into_iter().collect(),
            access_tokens: saved.access_tokens.into_iter().collect(),
            refresh_tokens: saved.refresh_tokens.into_iter().collect(),
        })
    }

    pub(super) fn save(
        &self,
        clients: &HashMap<String, Client>,
        access_tokens: &HashMap<String, AccessToken>,
        refresh_tokens: &HashMap<String, RefreshToken>,
    ) -> Result<()> {
        let state = StateFile {
            version: FORMAT_VERSION,
            clients: clients
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            access_tokens: access_tokens
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            refresh_tokens: refresh_tokens
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        };
        validate(&state)?;
        let bytes = serde_json::to_vec(&state).context("cannot encode OAuth state")?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            bail!("OAuth state exceeds the {} byte limit", MAX_STATE_BYTES);
        }
        atomic_write(&self.path, &bytes)
    }
}

fn validate(state: &StateFile) -> Result<()> {
    if state.version != FORMAT_VERSION {
        bail!("unsupported OAuth state version {}", state.version);
    }
    if state.clients.len() > super::MAX_REGISTERED_CLIENTS
        || state.access_tokens.len() > super::MAX_ACCESS_TOKENS
        || state.refresh_tokens.len() > super::MAX_REFRESH_TOKENS
    {
        bail!("OAuth state exceeds configured entry limits");
    }
    for (client_id, client) in &state.clients {
        if !valid_client_id(client_id)
            || client.redirect_uris.is_empty()
            || client.redirect_uris.len() > super::MAX_REDIRECT_URIS_PER_CLIENT
            || client
                .redirect_uris
                .iter()
                .any(|uri| !valid_redirect_uri(uri))
        {
            bail!("OAuth state contains invalid client metadata");
        }
    }
    validate_tokens(&state.access_tokens, "access_", &state.clients)?;
    validate_tokens(&state.refresh_tokens, "refresh_", &state.clients)?;
    Ok(())
}

fn validate_tokens<T>(
    tokens: &BTreeMap<String, T>,
    prefix: &str,
    clients: &BTreeMap<String, Client>,
) -> Result<()>
where
    T: TokenRecord,
{
    for (token, saved) in tokens {
        if token.len() > 128
            || !token.starts_with(prefix)
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || !clients.contains_key(saved.client_id())
            || saved
                .resource()
                .is_some_and(|resource| !valid_resource(resource))
        {
            bail!("OAuth state contains invalid token metadata");
        }
    }
    Ok(())
}

trait TokenRecord {
    fn client_id(&self) -> &str;
    fn resource(&self) -> Option<&str>;
}

impl TokenRecord for AccessToken {
    fn client_id(&self) -> &str {
        &self.client_id
    }

    fn resource(&self) -> Option<&str> {
        self.resource.as_deref()
    }
}

impl TokenRecord for RefreshToken {
    fn client_id(&self) -> &str {
        &self.client_id
    }

    fn resource(&self) -> Option<&str> {
        self.resource.as_deref()
    }
}

fn valid_client_id(value: &str) -> bool {
    value
        .strip_prefix("wcode-")
        .and_then(|value| Uuid::parse_str(value).ok())
        .is_some_and(|uuid| uuid.get_version_num() == 4)
}

fn valid_resource(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    let secure = url.scheme() == "https" && url.host_str().is_some();
    let loopback =
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    (secure || loopback)
        && url.path() == "/mcp"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn state_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os("WCODE_STATE_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    if cfg!(target_os = "windows") {
        let base = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("USERPROFILE"))
            .context("LOCALAPPDATA and USERPROFILE are not set")?;
        return Ok(PathBuf::from(base).join("wcode"));
    }
    if let Some(base) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("wcode"));
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/state/wcode"))
}

fn secure_file_metadata(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect OAuth state at {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("OAuth state path is not a regular file");
    }
    Ok(metadata)
}

#[cfg(unix)]
fn secure_existing_permissions(path: &Path, metadata: &fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("cannot secure OAuth state at {}", path.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn secure_existing_permissions(_path: &Path, _metadata: &fs::Metadata) -> Result<()> {
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("OAuth state path has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("cannot create OAuth state directory {}", parent.display()))?;
    let metadata = fs::symlink_metadata(parent)
        .with_context(|| format!("cannot inspect OAuth state directory {}", parent.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("OAuth state directory is not a regular directory");
    }
    if path.exists() {
        secure_file_metadata(path)?;
    }

    let temporary = parent.join(format!(".oauth-{}.tmp", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("cannot create {}", temporary.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("cannot write {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("cannot sync {}", temporary.display()))?;
        replace_path(&temporary, path)
            .with_context(|| format!("cannot publish OAuth state at {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/auth/store.rs"]
mod tests;
