use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_AUTHORIZATION_REQUESTS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationKind {
    CommandAccess,
    RiskyExecution,
    RuntimeExecutor,
    DestructiveDelete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    Pending,
    ApprovedSession,
    ApprovedOnce,
    Denied,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorizationRequest {
    pub id: String,
    pub workspace: String,
    pub kind: AuthorizationKind,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub fingerprint: String,
    pub status: AuthorizationStatus,
    pub created_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at_ms: Option<u64>,
}

#[derive(Default)]
struct AuthorizationState {
    next_id: u64,
    requests: BTreeMap<String, AuthorizationRequest>,
    session_grants: HashSet<String>,
    one_shot_grants: HashSet<String>,
}

#[derive(Clone, Default)]
pub struct AuthorizationManager {
    state: Arc<Mutex<AuthorizationState>>,
}

impl AuthorizationManager {
    pub fn is_granted(&self, fingerprint: &str) -> bool {
        self.state
            .lock()
            .expect("authorization state lock poisoned")
            .session_grants
            .contains(fingerprint)
    }

    pub fn request(
        &self,
        workspace: impl Into<String>,
        kind: AuthorizationKind,
        summary: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> AuthorizationRequest {
        self.request_with_program(workspace, kind, summary, None, fingerprint)
    }

    pub fn request_command(
        &self,
        workspace: impl Into<String>,
        program: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> AuthorizationRequest {
        let program = program.into();
        self.request_with_program(
            workspace,
            AuthorizationKind::CommandAccess,
            format!("authorize command: {program}"),
            Some(program),
            fingerprint,
        )
    }

    fn request_with_program(
        &self,
        workspace: impl Into<String>,
        kind: AuthorizationKind,
        summary: impl Into<String>,
        program: Option<String>,
        fingerprint: impl Into<String>,
    ) -> AuthorizationRequest {
        let workspace = workspace.into();
        let summary = summary.into();
        let fingerprint = fingerprint.into();
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        if let Some(existing) = state
            .requests
            .values()
            .rev()
            .find(|request| {
                request.fingerprint == fingerprint && request.status == AuthorizationStatus::Pending
            })
            .cloned()
        {
            return existing;
        }
        state.next_id = state.next_id.saturating_add(1).max(1);
        let request = AuthorizationRequest {
            id: format!("AUTH-{:08}", state.next_id),
            workspace,
            kind,
            summary,
            program,
            fingerprint,
            status: AuthorizationStatus::Pending,
            created_at_ms: now_ms(),
            decided_at_ms: None,
        };
        state.requests.insert(request.id.clone(), request.clone());
        while state.requests.len() > MAX_AUTHORIZATION_REQUESTS {
            let removable = state
                .requests
                .iter()
                .find(|(_, request)| request.status != AuthorizationStatus::Pending)
                .map(|(id, _)| id.clone());
            let Some(id) = removable else {
                break;
            };
            state.requests.remove(&id);
        }
        request
    }

    pub fn request_by_id(&self, id: &str) -> Option<AuthorizationRequest> {
        self.state
            .lock()
            .expect("authorization state lock poisoned")
            .requests
            .get(id)
            .cloned()
    }

    pub fn approve_session(&self, id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        let (fingerprint, one_shot) = {
            let Some(request) = state.requests.get_mut(id) else {
                return false;
            };
            if request.status != AuthorizationStatus::Pending {
                return false;
            }
            let one_shot = request.kind == AuthorizationKind::DestructiveDelete;
            request.status = if one_shot {
                AuthorizationStatus::ApprovedOnce
            } else {
                AuthorizationStatus::ApprovedSession
            };
            request.decided_at_ms = Some(now_ms());
            (request.fingerprint.clone(), one_shot)
        };
        if one_shot {
            state.one_shot_grants.insert(fingerprint);
        } else {
            state.session_grants.insert(fingerprint);
        }
        true
    }

    pub fn consume_one_shot_grant(&self, fingerprint: &str) -> bool {
        self.state
            .lock()
            .expect("authorization state lock poisoned")
            .one_shot_grants
            .remove(fingerprint)
    }

    pub fn deny(&self, id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        let Some(request) = state.requests.get_mut(id) else {
            return false;
        };
        if request.status != AuthorizationStatus::Pending {
            return false;
        }
        request.status = AuthorizationStatus::Denied;
        request.decided_at_ms = Some(now_ms());
        true
    }

    pub fn requests(&self, limit: usize) -> Vec<AuthorizationRequest> {
        let state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state
            .requests
            .values()
            .rev()
            .take(limit.clamp(1, MAX_AUTHORIZATION_REQUESTS))
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub fn latest_pending(&self) -> Option<AuthorizationRequest> {
        self.state
            .lock()
            .expect("authorization state lock poisoned")
            .requests
            .values()
            .rev()
            .find(|request| request.status == AuthorizationStatus::Pending)
            .cloned()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/workspace/authorization.rs"]
mod tests;
