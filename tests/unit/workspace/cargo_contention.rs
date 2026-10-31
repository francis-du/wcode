use super::*;

#[tokio::test]
async fn lockfile_writers_wait_for_readers_without_reserving_the_global_registry() {
    for command in ["fetch", "update"] {
        let root = tempfile::tempdir().unwrap();
        let gate = cargo_workspace_gate(root.path());
        let reader = gate.access.read().await;
        let args = arguments(&[command]);
        let mut writer = Box::pin(acquire_cargo_contention_gate(
            "cargo",
            &args,
            root.path(),
            Duration::from_secs(3),
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(25), writer.as_mut())
                .await
                .is_err(),
            "{command} can create or change Cargo.lock and must wait for readers"
        );
        let registry = tokio::time::timeout(
            Duration::from_secs(2),
            Arc::clone(cargo_registry_gate()).acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();
        drop(registry);
        drop(reader);
        let permit = writer.await.unwrap();
        assert!(permit.is_some());
        assert!(
            gate.access.try_read().is_err(),
            "lockfile writer must retain exclusive access"
        );
        assert!(
            cargo_registry_gate().try_acquire().is_err(),
            "registry access must also be retained"
        );
        drop(permit);
        assert!(gate.access.try_write().is_ok());
    }
}

#[tokio::test]
async fn cancelled_registry_wait_releases_its_lockfile_writer() {
    let root = tempfile::tempdir().unwrap();
    let gate = cargo_workspace_gate(root.path());
    let registry = tokio::time::timeout(
        Duration::from_secs(2),
        Arc::clone(cargo_registry_gate()).acquire_owned(),
    )
    .await
    .unwrap()
    .unwrap();
    let args = arguments(&["update"]);
    let mut operation = Box::pin(acquire_cargo_contention_gate(
        "cargo",
        &args,
        root.path(),
        Duration::from_secs(3),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), operation.as_mut())
            .await
            .is_err()
    );
    assert!(
        gate.access.try_read().is_err(),
        "the registry waiter owns the local writer"
    );
    drop(operation);
    assert!(
        gate.access.try_write().is_ok(),
        "cancellation must release local access"
    );
    drop(registry);
}

#[tokio::test]
async fn registry_timeout_releases_its_lockfile_writer_without_launching_work() {
    let root = tempfile::tempdir().unwrap();
    let gate = cargo_workspace_gate(root.path());
    let registry = tokio::time::timeout(
        Duration::from_secs(2),
        Arc::clone(cargo_registry_gate()).acquire_owned(),
    )
    .await
    .unwrap()
    .unwrap();
    let result = acquire_cargo_contention_gate(
        "cargo",
        &arguments(&["fetch"]),
        root.path(),
        Duration::from_millis(25),
    )
    .await;
    assert!(result.is_err());
    assert!(gate.access.try_write().is_ok());
    drop(registry);
}

fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn maintenance_and_unknown_cargo_commands_take_exclusive_workspace_access() {
    for args in [
        vec!["clean"],
        vec!["clean", "--release"],
        vec!["fmt"],
        vec!["fmt", "--all"],
        vec!["add", "serde"],
        vec!["remove", "serde"],
        vec!["generate-lockfile"],
        vec!["custom-command", "test"],
        vec!["custom-command", "search"],
        vec!["--config", "search", "check"],
        vec!["--unknown-option", "search"],
    ] {
        assert_eq!(
            cargo_contention_lane("cargo", &arguments(&args)),
            Some(CargoContentionLane::WorkspaceExclusive),
            "maintenance, aliases and option values must not bypass the workspace gate: {args:?}"
        );
    }
}

#[tokio::test]
async fn clean_waits_for_running_tests_and_cancelled_clean_releases_its_waiter() {
    let root = tempfile::tempdir().unwrap();
    let reader = acquire_cargo_contention_gate(
        "cargo",
        &arguments(&["test", "--locked"]),
        root.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap()
    .unwrap();
    let args = arguments(&["clean"]);
    let mut cleaner = Box::pin(acquire_cargo_contention_gate(
        "cargo",
        &args,
        root.path(),
        Duration::from_secs(1),
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), cleaner.as_mut())
            .await
            .is_err(),
        "clean must wait instead of removing artifacts under an active test"
    );
    drop(cleaner);
    // Also works on a host configured with just one shared test slot.
    drop(reader);
    let later_reader = acquire_cargo_contention_gate(
        "cargo",
        &arguments(&["test", "--locked"]),
        root.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(
        later_reader.is_some(),
        "cancelled cleanup must not strand readers"
    );
    drop(later_reader);
    assert!(
        acquire_cargo_contention_gate("cargo", &args, root.path(), Duration::from_secs(1),)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn maintenance_does_not_block_an_unrelated_workspace() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let cleaner = acquire_cargo_contention_gate(
        "cargo",
        &arguments(&["clean"]),
        left.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(cleaner.is_some());
    assert!(
        acquire_cargo_contention_gate(
            "cargo",
            &arguments(&["test", "--locked"]),
            left.path(),
            Duration::from_millis(25),
        )
        .await
        .is_err(),
        "active maintenance must block tests in the same workspace"
    );
    let reader = acquire_cargo_contention_gate(
        "cargo",
        &arguments(&["test", "--locked"]),
        right.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(reader.is_some());
}

#[test]
fn explicit_read_only_commands_keep_their_existing_lanes() {
    for args in [
        vec!["fmt", "--check"],
        vec!["fmt", "--all", "--", "--check"],
        vec!["--version"],
        vec!["--help"],
        vec!["help", "test"],
        vec!["--list"],
    ] {
        assert_eq!(
            cargo_contention_lane("cargo", &arguments(&args)),
            None,
            "{args:?}"
        );
    }
    assert_eq!(
        cargo_contention_lane("cargo", &arguments(&["+nightly", "search", "serde"])),
        Some(CargoContentionLane::Registry)
    );
    assert_eq!(
        cargo_contention_lane("cargo", &arguments(&["test", "--locked"])),
        Some(CargoContentionLane::WorkspaceTest)
    );
    assert_eq!(cargo_contention_lane("node", &arguments(&["clean"])), None);
}
