use anyhow::{bail, Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const CONTROL_PATH: &str = "/_wcode/control";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ControlAction {
    Restart,
    Stop,
}

impl ControlAction {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::Stop => "stop",
        }
    }
}

#[derive(Deserialize, Serialize)]
struct RuntimeState {
    pid: u32,
    address: SocketAddr,
    token: String,
}

#[derive(Clone)]
struct ControlState {
    token: Arc<str>,
    sender: mpsc::UnboundedSender<ControlAction>,
}

pub(crate) struct RuntimeRegistration {
    path: PathBuf,
    token: String,
}

pub(crate) fn register(
    listener_address: SocketAddr,
) -> Result<(
    Router,
    mpsc::UnboundedReceiver<ControlAction>,
    RuntimeRegistration,
)> {
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let address = match listener_address.ip() {
        IpAddr::V4(address) if address.is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), listener_address.port())
        }
        IpAddr::V6(address) if address.is_unspecified() => SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            listener_address.port(),
        ),
        _ => listener_address,
    };
    let state = RuntimeState {
        pid: std::process::id(),
        address,
        token: token.clone(),
    };
    let path = state_path()?;
    write_state(&path, &state)?;
    let (sender, receiver) = mpsc::unbounded_channel();
    let router = Router::new()
        .route(CONTROL_PATH, post(handle_control))
        .with_state(ControlState {
            token: Arc::from(token.clone()),
            sender,
        });
    Ok((router, receiver, RuntimeRegistration { path, token }))
}

pub(crate) fn send(action: ControlAction) -> Result<()> {
    let path = state_path()?;
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "no running wcode instance was found at {}; start wcode first",
            path.display()
        )
    })?;
    let state: RuntimeState =
        serde_json::from_slice(&bytes).context("the wcode runtime state file is invalid")?;
    let body = serde_json::to_string(&action).context("cannot encode control request")?;
    let mut stream = TcpStream::connect_timeout(&state.address, Duration::from_secs(3))
        .with_context(|| {
            format!(
                "cannot connect to the running wcode process (pid {})",
                state.pid
            )
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .context("cannot configure control response timeout")?;
    write!(
        stream,
        "POST {CONTROL_PATH} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        state.address,
        state.token,
        body.len(),
        body
    )
    .context("cannot send control request")?;
    stream.flush().context("cannot flush control request")?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .context("cannot read control response")?;
    let status_line = response.lines().next().unwrap_or_default();
    if !status_line.contains(" 202 ") {
        bail!("wcode rejected the control request: {status_line}");
    }
    Ok(())
}

async fn handle_control(
    State(state): State<ControlState>,
    headers: HeaderMap,
    Json(action): Json<ControlAction>,
) -> StatusCode {
    let authorized = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| constant_time_eq(value.as_bytes(), state.token.as_bytes()));
    if !authorized {
        return StatusCode::UNAUTHORIZED;
    }
    if state.sender.send(action).is_err() {
        return StatusCode::SERVICE_UNAVAILABLE;
    }
    StatusCode::ACCEPTED
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn state_path() -> Result<PathBuf> {
    if let Some(runtime) = env::var_os("WCODE_RUNTIME_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(runtime).join("runtime.json"));
    }
    if cfg!(target_os = "windows") {
        let base = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("USERPROFILE"))
            .context("LOCALAPPDATA and USERPROFILE are not set")?;
        return Ok(PathBuf::from(base).join("wcode/runtime.json"));
    }
    if cfg!(target_os = "linux") {
        if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) {
            return Ok(PathBuf::from(runtime).join("wcode/runtime.json"));
        }
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache/wcode/runtime.json"))
}

fn write_state(path: &Path, state: &RuntimeState) -> Result<()> {
    let parent = path.parent().context("runtime state path has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("cannot create runtime directory {}", parent.display()))?;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("cannot create {}", temporary.display()))?;
    let bytes = serde_json::to_vec(state).context("cannot encode runtime state")?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write {}", temporary.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("cannot publish runtime state at {}", path.display()))?;
    Ok(())
}

impl Drop for RuntimeRegistration {
    fn drop(&mut self) {
        let owned = fs::read(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RuntimeState>(&bytes).ok())
            .is_some_and(|state| state.token == self.token);
        if owned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_rejects_prefixes_and_differences() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"sam"));
        assert!(!constant_time_eq(b"same", b"sane"));
    }

    #[test]
    fn control_actions_have_stable_wire_names() {
        assert_eq!(
            serde_json::to_string(&ControlAction::Stop).unwrap(),
            "\"stop\""
        );
        assert_eq!(
            serde_json::to_string(&ControlAction::Restart).unwrap(),
            "\"restart\""
        );
    }
}
