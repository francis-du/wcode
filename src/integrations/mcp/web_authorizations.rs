//! Grouped operator approval over the pending authorization queue.

use super::{
    intelligence_bad_request, intelligence_pending_authorizations, intelligence_ui_authorized,
    intelligence_workspace_id, MAX_GROUPED_AUTHORIZATIONS,
};
use crate::mcp::AppState;
use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::sync::Arc;

/// Approve a bounded set of pending requests as one operator decision.
pub(crate) async fn intelligence_web_approve_authorization_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    if let Err(response) = intelligence_ui_authorized(&state, &headers) {
        return *response;
    }
    let ids = payload.get("ids").and_then(Value::as_array).map(|ids| {
        ids.iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    let ids = match ids {
        Some(ids) if !ids.is_empty() && ids.len() <= MAX_GROUPED_AUTHORIZATIONS => ids,
        _ => return intelligence_bad_request("grouped authorization size is out of bounds"),
    };
    let workspace_id = match intelligence_workspace_id(&state, &headers) {
        Ok(id) => id,
        Err(error) => return intelligence_bad_request(error),
    };
    if ids.iter().any(|id| {
        state
            .workspaces
            .authorization_request(id)
            .is_none_or(|request| request.workspace != workspace_id)
    }) {
        return intelligence_bad_request(
            "authorization request does not belong to the selected workspace",
        );
    }
    let approved = state.workspaces.approve_authorization_group(&ids);
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "approved": approved,
            "pending": intelligence_pending_authorizations(&state, &workspace_id)
        })),
    )
        .into_response()
}
