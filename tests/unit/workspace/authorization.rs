use super::*;

#[test]
fn session_grants_require_explicit_human_approval() {
    let manager = AuthorizationManager::default();
    let request = manager.request(
        "demo",
        AuthorizationKind::RiskyExecution,
        "run cargo test",
        "sha256:demo",
    );
    assert!(!manager.is_granted("sha256:demo"));
    assert_eq!(manager.latest_pending().unwrap().id, request.id);
    assert!(manager.approve_session(&request.id));
    assert!(manager.is_granted("sha256:demo"));
    assert!(manager.latest_pending().is_none());
}

#[test]
fn denial_never_creates_a_grant() {
    let manager = AuthorizationManager::default();
    let request = manager.request(
        "demo",
        AuthorizationKind::RuntimeExecutor,
        "run canary",
        "sha256:deny",
    );
    assert_eq!(manager.request_by_id(&request.id).unwrap().id, request.id);
    assert!(manager.deny(&request.id));
    assert!(!manager.is_granted("sha256:deny"));
}

#[test]
fn command_access_requests_retain_the_requested_program() {
    let manager = AuthorizationManager::default();
    let request = manager.request_command("demo", "git", "sha256:command");
    assert_eq!(request.kind, AuthorizationKind::CommandAccess);
    assert_eq!(request.program.as_deref(), Some("git"));
    assert_eq!(request.summary, "authorize command: git");
}

#[test]
fn destructive_approval_is_exact_and_consumed_once() {
    let manager = AuthorizationManager::default();
    let request = manager.request(
        "demo",
        AuthorizationKind::DestructiveDelete,
        "delete src/obsolete.rs",
        "sha256:delete-once",
    );
    assert!(manager.approve_session(&request.id));
    assert_eq!(
        manager.requests(1)[0].status,
        AuthorizationStatus::ApprovedOnce
    );
    assert!(!manager.is_granted("sha256:delete-once"));
    assert!(manager.consume_one_shot_grant("sha256:delete-once"));
    assert!(!manager.consume_one_shot_grant("sha256:delete-once"));
}
