//! Bounded webhook intake only. Deploy behind a trusted HTTPS proxy; this router
//! never reads a publisher API token, executes a candidate or approves a change.
use super::*;
use axum::body::to_bytes;
use axum::extract::{Request, State as ExtractState};
use axum::http::StatusCode as HttpStatus;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::{timeout_at, Instant};

const RECEIVE_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(8);
const INTAKE_SLOTS: usize = 2;

#[derive(Clone)]
struct ReceiverState {
    inbox: GitHubInbox,
    key: Arc<Vec<u8>>,
    slots: Arc<Semaphore>,
    receive_timeout: Duration,
    response_timeout: Duration,
}

impl GitHubInbox {
    /// Only POST /github/webhook is exposed. No history, payload or administrative
    /// endpoints. Enqueue acknowledges only confirmed local persistence.
    pub fn receiver_router(&self, key: &[u8]) -> Result<Router> {
        ensure!(
            (16..=4096).contains(&key.len()),
            "webhook receiver key is invalid"
        );
        self.read(now_ms()?)?;
        Ok(router(ReceiverState {
            inbox: self.clone(),
            key: Arc::new(key.to_vec()),
            slots: Arc::new(Semaphore::new(INTAKE_SLOTS)),
            receive_timeout: RECEIVE_TIMEOUT,
            response_timeout: RESPONSE_TIMEOUT,
        }))
    }
}

fn router(state: ReceiverState) -> Router {
    Router::new()
        .route("/github/webhook", post(receive))
        .with_state(state)
}

fn failure(status: HttpStatus, message: &'static str) -> Response {
    (
        status,
        Json(json!({"accepted":false,"current_acceptance":false,"error":message})),
    )
        .into_response()
}

async fn receive(ExtractState(state): ExtractState<ReceiverState>, request: Request) -> Response {
    let started = Instant::now();
    let Ok(permit) = state.slots.clone().try_acquire_owned() else {
        return failure(
            HttpStatus::SERVICE_UNAVAILABLE,
            "webhook intake busy; redeliver",
        );
    };
    if request
        .headers()
        .get_all("content-encoding")
        .iter()
        .next()
        .is_some()
    {
        return failure(
            HttpStatus::UNSUPPORTED_MEDIA_TYPE,
            "encoded webhook bodies are not supported",
        );
    }
    let headers = request.headers().clone();
    let bytes = match timeout_at(
        started + state.receive_timeout,
        to_bytes(request.into_body(), MAX_WEBHOOK_BYTES),
    )
    .await
    {
        Err(_) => {
            return failure(
                HttpStatus::REQUEST_TIMEOUT,
                "webhook body deadline exceeded",
            )
        }
        Ok(Err(_)) => {
            return failure(
                HttpStatus::PAYLOAD_TOO_LARGE,
                "webhook body is invalid or exceeds its bound",
            )
        }
        Ok(Ok(bytes)) => bytes,
    };
    // File I/O remains off the async executor. The permit moves into the actual
    // work, so timing out the response cannot admit unbounded blocking workers.
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        VerifiedPullRequestDelivery::verify(
            &state.inbox.config,
            state.inbox.binding.installation_id,
            &headers,
            &bytes,
            &state.key,
        )
        .map_err(|_| HttpStatus::UNAUTHORIZED)?;
        state
            .inbox
            .enqueue(&headers, &bytes, &state.key)
            .map_err(|_| HttpStatus::SERVICE_UNAVAILABLE)
    });
    persistence_response(started + state.response_timeout, work).await
}

async fn persistence_response(
    deadline: Instant,
    work: tokio::task::JoinHandle<Result<InboxEnqueueResult, HttpStatus>>,
) -> Response {
    match timeout_at(deadline, work).await {
        Ok(Ok(Ok(result))) => (
            HttpStatus::ACCEPTED,
            Json(json!({
                "accepted":true,"duplicate":result.duplicate,"body_digest":result.body_digest,
                "reauthenticated":result.reauthenticated,
                "generation":result.generation,"current_acceptance":false,
            })),
        )
            .into_response(),
        Ok(Ok(Err(HttpStatus::UNAUTHORIZED))) => failure(
            HttpStatus::UNAUTHORIZED,
            "webhook authentication or routing rejected",
        ),
        // The OS might finish a write after a response deadline. Never return
        // accepted on uncertainty; a signed redelivery safely uses the body digest.
        _ => failure(
            HttpStatus::SERVICE_UNAVAILABLE,
            "inbox persistence unconfirmed; redeliver",
        ),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/receiver.rs"]
mod tests;
