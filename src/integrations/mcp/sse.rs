use crate::mcp::{dispatch_mcp_payload, forbidden_origin_response, origin_allowed, AppState};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;

const MAX_SESSIONS: usize = 128;
const PROTOCOL_VERSION: &str = "2024-11-05";
const CHANNEL_CAPACITY: usize = 32;

#[derive(Clone)]
struct Session {
    owner: String,
    public_url: String,
    sender: mpsc::Sender<Event>,
}

#[derive(Debug, Deserialize)]
struct MessageQuery {
    #[serde(rename = "sessionId")]
    session_id: String,
}

struct SessionReceiver {
    session_id: String,
    receiver: mpsc::Receiver<Event>,
}

impl Drop for SessionReceiver {
    fn drop(&mut self) {
        sessions()
            .lock()
            .expect("legacy SSE session lock poisoned")
            .remove(&self.session_id);
    }
}

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sse", get(open_session))
        .route("/message", post(post_message))
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn open_session(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(public_url) = state.auth.request_public_url(&headers) else {
        return forbidden_origin_response();
    };
    if !origin_allowed(&public_url, &headers) {
        return forbidden_origin_response();
    }
    let Some(owner) = state
        .auth
        .authorized_client_fingerprint_for(&headers, &public_url)
    else {
        return state.auth.unauthorized_response_for(&public_url);
    };

    let session_id = format!("{}-{}", state.auth.instance_id(), uuid::Uuid::new_v4());
    let endpoint = format!("{public_url}/message?sessionId={session_id}");
    let (sender, receiver) = mpsc::channel(CHANNEL_CAPACITY);
    sender
        .try_send(Event::default().event("endpoint").data(endpoint))
        .expect("new legacy SSE channel accepts its endpoint event");

    {
        let mut active = sessions().lock().expect("legacy SSE session lock poisoned");
        active.retain(|_, session| !session.sender.is_closed());
        if active.len() >= MAX_SESSIONS {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "too many active legacy MCP SSE sessions",
            )
                .into_response();
        }
        active.insert(
            session_id.clone(),
            Session {
                owner,
                public_url,
                sender,
            },
        );
    }

    state.monitor.mark_mcp_seen();
    let events = stream::unfold(
        SessionReceiver {
            session_id,
            receiver,
        },
        |mut session| async move {
            session
                .receiver
                .recv()
                .await
                .map(|event| (Ok::<Event, Infallible>(event), session))
        },
    );
    Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response()
}

async fn post_message(
    State(state): State<Arc<AppState>>,
    Query(query): Query<MessageQuery>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let Some(public_url) = state.auth.request_public_url(&headers) else {
        return forbidden_origin_response();
    };
    if !origin_allowed(&public_url, &headers) {
        return forbidden_origin_response();
    }
    let Some(owner) = state
        .auth
        .authorized_client_fingerprint_for(&headers, &public_url)
    else {
        return state.auth.unauthorized_response_for(&public_url);
    };
    if query.session_id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "missing legacy MCP sessionId").into_response();
    }

    let sender = sessions()
        .lock()
        .expect("legacy SSE session lock poisoned")
        .get(&query.session_id)
        .filter(|session| session.owner == owner && session.public_url == public_url)
        .map(|session| session.sender.clone());
    let Some(sender) = sender else {
        return (StatusCode::NOT_FOUND, "unknown legacy MCP SSE session").into_response();
    };

    state.monitor.mark_mcp_seen();
    if let Some(value) = dispatch_mcp_payload(state, payload, PROTOCOL_VERSION, &owner).await {
        let event = Event::default().event("message").data(value.to_string());
        match sender.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    "legacy MCP SSE client is not consuming responses",
                )
                    .into_response()
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                sessions()
                    .lock()
                    .expect("legacy SSE session lock poisoned")
                    .remove(&query.session_id);
                return (StatusCode::GONE, "legacy MCP SSE session is closed").into_response();
            }
        }
    }
    StatusCode::ACCEPTED.into_response()
}

#[cfg(test)]
pub(crate) fn active_session_count() -> usize {
    sessions()
        .lock()
        .expect("legacy SSE session lock poisoned")
        .len()
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/sse.rs"]
mod tests;
