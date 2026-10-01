use anyhow::{bail, Result};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MAX_AUTHORIZATION_REQUESTS: usize = 256;
const PENDING_AUTHORIZATION_TTL: Duration = Duration::from_secs(10 * 60);
const ONE_SHOT_AUTHORIZATION_TTL: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationKind {
    CommandAccess,
    RiskyExecution,
    DestructiveDelete,
    HumanDecision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    Pending,
    ApprovedSession,
    ApprovedOnce,
    Consumed,
    Denied,
    Expired,
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

#[derive(Clone, Debug)]
pub struct AuthorizationRequired {
    pub request: AuthorizationRequest,
}

impl AuthorizationRequired {
    pub fn new(request: AuthorizationRequest) -> Self {
        Self { request }
    }
}

impl std::fmt::Display for AuthorizationRequired {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "authorization required: {} · {}. Explicit human approval is required before retrying the operation",
            self.request.id, self.request.summary
        )
    }
}

impl std::error::Error for AuthorizationRequired {}

#[derive(Default)]
struct AuthorizationState {
    next_id: u64,
    requests: BTreeMap<String, AuthorizationRequest>,
    session_grants: BTreeMap<String, String>,
    one_shot_grants: BTreeMap<String, (String, Instant)>,
    workspace_command_grants: HashSet<String>,
    interactive_tokens: BTreeMap<String, String>,
    pending_deadlines: BTreeMap<String, Instant>,
}

impl AuthorizationState {
    fn expire_pending(&mut self) {
        let now = Instant::now();
        let expired = self
            .pending_deadlines
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in expired {
            if let Some(request) = self.requests.get_mut(&id) {
                if request.status == AuthorizationStatus::Pending {
                    request.status = AuthorizationStatus::Expired;
                    request.decided_at_ms = Some(now_ms());
                }
            }
            self.interactive_tokens.remove(&id);
            self.pending_deadlines.remove(&id);
        }
        let expired_grants = self
            .one_shot_grants
            .iter()
            .filter(|(_, (_, deadline))| *deadline <= now)
            .map(|(fingerprint, (id, _))| (fingerprint.clone(), id.clone()))
            .collect::<Vec<_>>();
        for (fingerprint, id) in expired_grants {
            self.one_shot_grants.remove(&fingerprint);
            if let Some(request) = self.requests.get_mut(&id) {
                if request.status == AuthorizationStatus::ApprovedOnce {
                    request.status = AuthorizationStatus::Expired;
                }
            }
        }
    }

    fn admit_request(&mut self) -> Result<()> {
        while self.requests.len() >= MAX_AUTHORIZATION_REQUESTS {
            let removable = self
                .requests
                .iter()
                .find(|(_, request)| {
                    !matches!(
                        request.status,
                        AuthorizationStatus::Pending | AuthorizationStatus::ApprovedOnce
                    )
                })
                .map(|(id, _)| id.clone());
            let Some(id) = removable else {
                bail!(
                    "authorization queue is full; resolve or wait for pending requests to expire"
                );
            };
            self.requests.remove(&id);
            self.interactive_tokens.remove(&id);
            self.pending_deadlines.remove(&id);
        }
        Ok(())
    }
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
            .contains_key(fingerprint)
    }

    pub fn workspace_commands_granted(&self, workspace: &str) -> bool {
        self.state
            .lock()
            .expect("authorization state lock poisoned")
            .workspace_command_grants
            .contains(workspace)
    }

    pub(crate) fn revoke_workspace(&self, workspace: &str) {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        state.workspace_command_grants.remove(workspace);
        state
            .session_grants
            .retain(|_, grant_workspace| grant_workspace != workspace);
        let request_ids = state
            .requests
            .iter()
            .filter(|(_, request)| request.workspace == workspace)
            .map(|(id, request)| (id.clone(), request.fingerprint.clone()))
            .collect::<Vec<_>>();
        let decided_at_ms = now_ms();
        for (id, fingerprint) in request_ids {
            state.one_shot_grants.remove(&fingerprint);
            state.interactive_tokens.remove(&id);
            state.pending_deadlines.remove(&id);
            if let Some(request) = state.requests.get_mut(&id) {
                if matches!(
                    request.status,
                    AuthorizationStatus::Pending
                        | AuthorizationStatus::ApprovedOnce
                        | AuthorizationStatus::ApprovedSession
                ) {
                    request.status = AuthorizationStatus::Denied;
                    request.decided_at_ms = Some(decided_at_ms);
                }
            }
        }
    }

    pub fn set_workspace_commands_granted(&self, workspace: &str, enabled: bool) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        let changed = if enabled {
            state.workspace_command_grants.insert(workspace.to_owned())
        } else {
            state.workspace_command_grants.remove(workspace)
        };
        if enabled {
            let decided_at_ms = now_ms();
            let resolved = state
                .requests
                .iter_mut()
                .filter_map(|(id, request)| {
                    (request.workspace == workspace
                        && request.status == AuthorizationStatus::Pending
                        && matches!(
                            request.kind,
                            AuthorizationKind::CommandAccess | AuthorizationKind::RiskyExecution
                        ))
                    .then(|| {
                        request.status = AuthorizationStatus::ApprovedSession;
                        request.decided_at_ms = Some(decided_at_ms);
                        id.clone()
                    })
                })
                .collect::<Vec<_>>();
            for id in resolved {
                state.interactive_tokens.remove(&id);
                state.pending_deadlines.remove(&id);
            }
        }
        changed
    }

    pub fn request(
        &self,
        workspace: impl Into<String>,
        kind: AuthorizationKind,
        summary: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Result<AuthorizationRequest> {
        self.request_with_program(workspace, kind, summary, None, fingerprint)
    }

    pub fn request_command(
        &self,
        workspace: impl Into<String>,
        program: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Result<AuthorizationRequest> {
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
    ) -> Result<AuthorizationRequest> {
        let workspace = workspace.into();
        let summary = summary.into();
        let fingerprint = fingerprint.into();
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        if let Some(existing) = state
            .requests
            .values()
            .rev()
            .find(|request| {
                request.workspace == workspace
                    && request.kind == kind
                    && request.fingerprint == fingerprint
                    && matches!(
                        request.status,
                        AuthorizationStatus::Pending | AuthorizationStatus::ApprovedOnce
                    )
            })
            .cloned()
        {
            return Ok(existing);
        }
        state.admit_request()?;
        let created_at_ms = now_ms();
        state.next_id = state.next_id.saturating_add(1).max(1);
        let request = AuthorizationRequest {
            id: format!("AUTH-{:08}", state.next_id),
            workspace,
            kind,
            summary,
            program,
            fingerprint,
            status: AuthorizationStatus::Pending,
            created_at_ms,
            decided_at_ms: None,
        };
        state
            .interactive_tokens
            .insert(request.id.clone(), Uuid::new_v4().simple().to_string());
        state.pending_deadlines.insert(
            request.id.clone(),
            Instant::now()
                + if kind == AuthorizationKind::HumanDecision {
                    ONE_SHOT_AUTHORIZATION_TTL
                } else {
                    PENDING_AUTHORIZATION_TTL
                },
        );
        state.requests.insert(request.id.clone(), request.clone());
        Ok(request)
    }

    pub fn request_by_id(&self, id: &str) -> Option<AuthorizationRequest> {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        state.requests.get(id).cloned()
    }

    pub fn interactive_token(&self, id: &str) -> Option<String> {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        state
            .requests
            .get(id)
            .filter(|request| request.status == AuthorizationStatus::Pending)
            .and_then(|_| state.interactive_tokens.get(id).cloned())
    }

    pub fn approve_session(&self, id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        let (fingerprint, workspace, one_shot) = {
            let Some(request) = state.requests.get_mut(id) else {
                return false;
            };
            if request.status != AuthorizationStatus::Pending {
                return false;
            }
            let one_shot = matches!(
                request.kind,
                AuthorizationKind::DestructiveDelete | AuthorizationKind::HumanDecision
            );
            request.status = if one_shot {
                AuthorizationStatus::ApprovedOnce
            } else {
                AuthorizationStatus::ApprovedSession
            };
            request.decided_at_ms = Some(now_ms());
            (
                request.fingerprint.clone(),
                request.workspace.clone(),
                one_shot,
            )
        };
        state.interactive_tokens.remove(id);
        state.pending_deadlines.remove(id);
        if one_shot {
            state.one_shot_grants.insert(
                fingerprint,
                (id.to_owned(), Instant::now() + ONE_SHOT_AUTHORIZATION_TTL),
            );
        } else {
            state.session_grants.insert(fingerprint, workspace);
        }
        true
    }

    pub fn consume_one_shot_grant(&self, fingerprint: &str) -> bool {
        self.consume_one_shot_receipt(None, AuthorizationKind::DestructiveDelete, fingerprint)
            .is_some()
    }

    pub(crate) fn consume_human_decision(
        &self,
        workspace: &str,
        fingerprint: &str,
    ) -> Option<AuthorizationRequest> {
        self.consume_one_shot_receipt(
            Some(workspace),
            AuthorizationKind::HumanDecision,
            fingerprint,
        )
    }

    fn consume_one_shot_receipt(
        &self,
        workspace: Option<&str>,
        kind: AuthorizationKind,
        fingerprint: &str,
    ) -> Option<AuthorizationRequest> {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        let (id, _) = state.one_shot_grants.get(fingerprint)?.clone();
        let request = state.requests.get(&id)?;
        if request.kind != kind
            || request.status != AuthorizationStatus::ApprovedOnce
            || workspace.is_some_and(|workspace| request.workspace != workspace)
        {
            return None;
        }
        let receipt = request.clone();
        state.one_shot_grants.remove(fingerprint);
        state.requests.get_mut(&id)?.status = AuthorizationStatus::Consumed;
        Some(receipt)
    }

    pub fn deny(&self, id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        let Some(request) = state.requests.get_mut(id) else {
            return false;
        };
        if request.status != AuthorizationStatus::Pending {
            return false;
        }
        request.status = AuthorizationStatus::Denied;
        request.decided_at_ms = Some(now_ms());
        state.interactive_tokens.remove(id);
        state.pending_deadlines.remove(id);
        true
    }

    pub fn requests(&self, limit: usize) -> Vec<AuthorizationRequest> {
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
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
        let mut state = self
            .state
            .lock()
            .expect("authorization state lock poisoned");
        state.expire_pending();
        state
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
