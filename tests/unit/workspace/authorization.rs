use super::*;

#[test]
fn session_grants_require_explicit_human_approval() {
    let manager = AuthorizationManager::default();
    let request = manager
        .request(
            "demo",
            AuthorizationKind::RiskyExecution,
            "run cargo test",
            "sha256:demo",
        )
        .unwrap();
    assert!(!manager.is_granted("sha256:demo"));
    assert_eq!(manager.latest_pending().unwrap().id, request.id);
    assert!(manager.approve_session(&request.id));
    assert!(manager.is_granted("sha256:demo"));
    assert!(manager.latest_pending().is_none());
}

#[test]
fn denial_never_creates_a_grant() {
    let manager = AuthorizationManager::default();
    let request = manager
        .request(
            "demo",
            AuthorizationKind::RiskyExecution,
            "run remote mutation",
            "sha256:deny",
        )
        .unwrap();
    assert_eq!(manager.request_by_id(&request.id).unwrap().id, request.id);
    assert!(manager.deny(&request.id));
    assert!(!manager.is_granted("sha256:deny"));
}

#[test]
fn pending_request_deduplication_is_workspace_and_kind_bound() {
    let manager = AuthorizationManager::default();
    let first = manager
        .request(
            "alpha",
            AuthorizationKind::RiskyExecution,
            "alpha risky",
            "sha256:shared",
        )
        .unwrap();
    let duplicate = manager
        .request(
            "alpha",
            AuthorizationKind::RiskyExecution,
            "alpha risky duplicate",
            "sha256:shared",
        )
        .unwrap();
    let other_workspace = manager
        .request(
            "beta",
            AuthorizationKind::RiskyExecution,
            "beta risky",
            "sha256:shared",
        )
        .unwrap();
    let other_kind = manager
        .request(
            "alpha",
            AuthorizationKind::DestructiveDelete,
            "alpha delete",
            "sha256:shared",
        )
        .unwrap();

    assert_eq!(duplicate.id, first.id);
    assert_ne!(other_workspace.id, first.id);
    assert_ne!(other_kind.id, first.id);
    assert_eq!(manager.requests(16).len(), 3);
}

#[test]
fn command_access_requests_retain_the_requested_program() {
    let manager = AuthorizationManager::default();
    let request = manager
        .request_command("demo", "git", "sha256:command")
        .unwrap();
    assert_eq!(request.kind, AuthorizationKind::CommandAccess);
    assert_eq!(request.program.as_deref(), Some("git"));
    assert_eq!(request.summary, "authorize command: git");
}

#[test]
fn destructive_approval_is_exact_and_consumed_once() {
    let manager = AuthorizationManager::default();
    let request = manager
        .request(
            "demo",
            AuthorizationKind::DestructiveDelete,
            "delete src/obsolete.rs",
            "sha256:delete-once",
        )
        .unwrap();
    assert!(manager.approve_session(&request.id));
    assert_eq!(
        manager.requests(1)[0].status,
        AuthorizationStatus::ApprovedOnce
    );
    assert!(!manager.is_granted("sha256:delete-once"));
    assert!(manager.consume_one_shot_grant("sha256:delete-once"));
    assert!(!manager.consume_one_shot_grant("sha256:delete-once"));
}

#[test]
fn workspace_command_grant_resolves_command_requests_but_not_delete() {
    let manager = AuthorizationManager::default();
    let command = manager
        .request_command("demo", "cargo", "sha256:command")
        .unwrap();
    let risky = manager
        .request(
            "demo",
            AuthorizationKind::RiskyExecution,
            "run cargo publish",
            "sha256:risky",
        )
        .unwrap();
    let delete = manager
        .request(
            "demo",
            AuthorizationKind::DestructiveDelete,
            "delete src/obsolete.rs",
            "sha256:delete",
        )
        .unwrap();

    assert!(manager.set_workspace_commands_granted("demo", true));
    assert!(manager.workspace_commands_granted("demo"));
    assert_eq!(
        manager.request_by_id(&command.id).unwrap().status,
        AuthorizationStatus::ApprovedSession
    );
    assert_eq!(
        manager.request_by_id(&risky.id).unwrap().status,
        AuthorizationStatus::ApprovedSession
    );
    assert_eq!(
        manager.request_by_id(&delete.id).unwrap().status,
        AuthorizationStatus::Pending
    );

    assert!(manager.set_workspace_commands_granted("demo", false));
    assert!(!manager.workspace_commands_granted("demo"));
}

fn expire_request(manager: &AuthorizationManager, id: &str) {
    manager
        .state
        .lock()
        .unwrap()
        .pending_deadlines
        .insert(id.to_owned(), Instant::now() - Duration::from_secs(1));
}

#[test]
fn authorization_pending_capacity_fails_closed_without_evicting_requests() {
    let manager = AuthorizationManager::default();
    let requests = (0..MAX_AUTHORIZATION_REQUESTS)
        .map(|index| {
            manager
                .request_command("demo", "git", format!("fingerprint:{index}"))
                .unwrap()
        })
        .collect::<Vec<_>>();
    let challenge = manager.interactive_token(&requests[0].id).unwrap();
    for index in MAX_AUTHORIZATION_REQUESTS..MAX_AUTHORIZATION_REQUESTS + 300 {
        assert!(manager
            .request_command("other", "cargo", format!("fingerprint:{index}"))
            .unwrap_err()
            .to_string()
            .contains("queue is full"));
    }
    let duplicate = manager
        .request_command("demo", "git", "fingerprint:0")
        .unwrap();
    assert_eq!(duplicate.id, requests[0].id);
    assert_eq!(manager.interactive_token(&duplicate.id), Some(challenge));
    assert_eq!(
        manager.requests(usize::MAX).len(),
        MAX_AUTHORIZATION_REQUESTS
    );
    for request in &requests {
        assert_eq!(
            manager.request_by_id(&request.id).unwrap().status,
            AuthorizationStatus::Pending
        );
    }
    let state = manager.state.lock().unwrap();
    assert_eq!(state.requests.len(), MAX_AUTHORIZATION_REQUESTS);
    assert_eq!(state.interactive_tokens.len(), MAX_AUTHORIZATION_REQUESTS);
    assert_eq!(state.pending_deadlines.len(), MAX_AUTHORIZATION_REQUESTS);
}

#[test]
fn authorization_expiry_rejects_approval_and_invalidates_challenges() {
    for kind in [
        AuthorizationKind::CommandAccess,
        AuthorizationKind::RiskyExecution,
        AuthorizationKind::DestructiveDelete,
        AuthorizationKind::HumanDecision,
    ] {
        let manager = AuthorizationManager::default();
        let request = manager
            .request("demo", kind, "operation", "expiry")
            .unwrap();
        assert!(manager.interactive_token(&request.id).is_some());
        expire_request(&manager, &request.id);
        assert!(!manager.approve_session(&request.id));
        assert!(!manager.is_granted("expiry"));
        assert!(!manager.consume_one_shot_grant("expiry"));
        assert!(manager.interactive_token(&request.id).is_none());
        assert_eq!(
            manager.request_by_id(&request.id).unwrap().status,
            AuthorizationStatus::Expired
        );
        assert!(manager.latest_pending().is_none());
        assert!(!manager.deny(&request.id));
        assert!(manager.state.lock().unwrap().pending_deadlines.is_empty());
    }
}

#[test]
fn authorization_deduplication_does_not_extend_pending_lease() {
    let manager = AuthorizationManager::default();
    let first = manager.request_command("demo", "git", "dedup").unwrap();
    let original_deadline = Instant::now() + Duration::from_secs(1);
    manager
        .state
        .lock()
        .unwrap()
        .pending_deadlines
        .insert(first.id.clone(), original_deadline);
    let duplicate = manager.request_command("demo", "git", "dedup").unwrap();
    assert_eq!(duplicate.id, first.id);
    assert_eq!(duplicate.created_at_ms, first.created_at_ms);
    assert_eq!(
        manager.state.lock().unwrap().pending_deadlines[&first.id],
        original_deadline
    );
    expire_request(&manager, &first.id);
    let renewed = manager.request_command("demo", "git", "dedup").unwrap();
    assert_ne!(renewed.id, first.id);
    assert_eq!(
        manager.request_by_id(&first.id).unwrap().status,
        AuthorizationStatus::Expired
    );
    assert!(manager.interactive_token(&first.id).is_none());
    assert!(!manager.approve_session(&first.id));
    assert!(manager.interactive_token(&renewed.id).is_some());
}

#[test]
fn authorization_bulk_command_approval_does_not_revive_expired_requests() {
    let manager = AuthorizationManager::default();
    let command = manager
        .request_command("demo", "git", "expired-command")
        .unwrap();
    let risky = manager
        .request(
            "demo",
            AuthorizationKind::RiskyExecution,
            "operation",
            "expired-risky",
        )
        .unwrap();
    expire_request(&manager, &command.id);
    expire_request(&manager, &risky.id);
    assert!(manager.set_workspace_commands_granted("demo", true));
    for request in [command, risky] {
        assert_eq!(
            manager.request_by_id(&request.id).unwrap().status,
            AuthorizationStatus::Expired
        );
        assert!(manager.interactive_token(&request.id).is_none());
        assert!(!manager.is_granted(&request.fingerprint));
    }
}

#[test]
fn authorization_capacity_reclaims_only_terminal_requests() {
    let manager = AuthorizationManager::default();
    let requests = (0..MAX_AUTHORIZATION_REQUESTS)
        .map(|index| {
            manager
                .request_command("demo", "git", format!("slot:{index}"))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(manager.deny(&requests[0].id));
    let replacement = manager
        .request_command("other", "cargo", "replacement")
        .unwrap();
    assert!(manager.request_by_id(&requests[0].id).is_none());
    assert!(manager.interactive_token(&requests[0].id).is_none());
    assert_eq!(
        manager.requests(usize::MAX).len(),
        MAX_AUTHORIZATION_REQUESTS
    );
    for request in requests.iter().skip(1) {
        assert_eq!(
            manager.request_by_id(&request.id).unwrap().status,
            AuthorizationStatus::Pending
        );
    }
    assert_eq!(
        manager.request_by_id(&replacement.id).unwrap().status,
        AuthorizationStatus::Pending
    );
    assert_eq!(
        manager.state.lock().unwrap().interactive_tokens.len(),
        MAX_AUTHORIZATION_REQUESTS
    );
}

#[test]
fn authorization_concurrent_admission_keeps_requests_and_challenges_bounded() {
    let manager = AuthorizationManager::default();
    let workers = (0..8)
        .map(|worker| {
            let manager = manager.clone();
            std::thread::spawn(move || {
                (0..64)
                    .filter(|index| {
                        manager
                            .request_command("demo", "git", format!("parallel:{worker}:{index}"))
                            .is_ok()
                    })
                    .count()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum::<usize>(),
        MAX_AUTHORIZATION_REQUESTS
    );
    let state = manager.state.lock().unwrap();
    assert_eq!(state.requests.len(), MAX_AUTHORIZATION_REQUESTS);
    assert_eq!(state.interactive_tokens.len(), MAX_AUTHORIZATION_REQUESTS);
    assert_eq!(state.pending_deadlines.len(), MAX_AUTHORIZATION_REQUESTS);
}
#[test]
fn human_decision_is_one_shot_and_never_covered_by_command_trust() {
    let manager = AuthorizationManager::default();
    manager.set_workspace_commands_granted("demo", true);
    let request = manager
        .request(
            "demo",
            AuthorizationKind::HumanDecision,
            "approve exact plan",
            "human-bound",
        )
        .unwrap();
    manager.set_workspace_commands_granted("demo", true);
    assert_eq!(
        manager.request_by_id(&request.id).unwrap().status,
        AuthorizationStatus::Pending
    );
    assert!(!manager.is_granted("human-bound"));
    assert!(manager.approve_session(&request.id));
    assert!(!manager.consume_one_shot_grant("human-bound"));
    assert!(manager
        .consume_human_decision("other", "human-bound")
        .is_none());
    assert!(manager
        .consume_human_decision("demo", "different")
        .is_none());
    let receipt = manager
        .consume_human_decision("demo", "human-bound")
        .unwrap();
    assert_eq!(receipt.id, request.id);
    assert_eq!(receipt.status, AuthorizationStatus::ApprovedOnce);
    assert!(receipt.decided_at_ms.is_some());
    assert_eq!(
        manager.request_by_id(&request.id).unwrap().status,
        AuthorizationStatus::Consumed
    );
    assert!(manager
        .consume_human_decision("demo", "human-bound")
        .is_none());
    assert!(!manager.approve_session(&request.id));
    assert!(AuthorizationManager::default()
        .consume_human_decision("demo", "human-bound")
        .is_none());
}

#[test]
fn approved_one_shot_grants_expire_without_renewal_or_replay() {
    for kind in [
        AuthorizationKind::DestructiveDelete,
        AuthorizationKind::HumanDecision,
    ] {
        let manager = AuthorizationManager::default();
        let request = manager
            .request("demo", kind, "decision", "once-expiry")
            .unwrap();
        assert!(manager.approve_session(&request.id));
        let deadline = manager.state.lock().unwrap().one_shot_grants["once-expiry"].1;
        let duplicate = manager
            .request("demo", kind, "duplicate", "once-expiry")
            .unwrap();
        assert_eq!(duplicate.id, request.id);
        assert_eq!(
            manager.state.lock().unwrap().one_shot_grants["once-expiry"].1,
            deadline
        );
        manager
            .state
            .lock()
            .unwrap()
            .one_shot_grants
            .get_mut("once-expiry")
            .unwrap()
            .1 = Instant::now() - Duration::from_secs(1);
        assert!(!manager.consume_one_shot_grant("once-expiry"));
        assert!(manager
            .consume_human_decision("demo", "once-expiry")
            .is_none());
        assert_eq!(
            manager.request_by_id(&request.id).unwrap().status,
            AuthorizationStatus::Expired
        );
        assert!(!manager.approve_session(&request.id));
    }
}

#[test]
fn human_decision_pending_lease_is_short_and_concurrent_consume_is_atomic() {
    let manager = AuthorizationManager::default();
    let request = manager
        .request(
            "demo",
            AuthorizationKind::HumanDecision,
            "decision",
            "atomic",
        )
        .unwrap();
    let deadline = manager.state.lock().unwrap().pending_deadlines[&request.id];
    assert!(deadline.duration_since(Instant::now()) <= ONE_SHOT_AUTHORIZATION_TTL);
    assert!(manager.approve_session(&request.id));
    let workers = (0..8)
        .map(|_| {
            let manager = manager.clone();
            std::thread::spawn(move || {
                usize::from(manager.consume_human_decision("demo", "atomic").is_some())
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum::<usize>(),
        1
    );
}

#[test]
fn authorization_capacity_preserves_unconsumed_operator_grants() {
    let manager = AuthorizationManager::default();
    let requests = (0..MAX_AUTHORIZATION_REQUESTS)
        .map(|index| {
            let request = manager
                .request(
                    "demo",
                    AuthorizationKind::HumanDecision,
                    "decision",
                    format!("grant:{index}"),
                )
                .unwrap();
            assert!(manager.approve_session(&request.id));
            request
        })
        .collect::<Vec<_>>();
    assert!(manager.request_command("demo", "git", "overflow").is_err());
    for request in &requests {
        assert_eq!(
            manager.request_by_id(&request.id).unwrap().status,
            AuthorizationStatus::ApprovedOnce
        );
    }
    assert!(manager
        .consume_human_decision("demo", &requests[0].fingerprint)
        .is_some());
    assert!(manager
        .request_command("demo", "git", "after-consume")
        .is_ok());
}
