//! Local persistence fixtures only; these do not approve a Policy or mint Evidence.
use super::*;
use crate::design::{AcceptancePolicy, PolicyLevel, PolicyPathMappings, PolicyRequirements};
use crate::evidence::{RequiredVerificationCheck, Revision};
use crate::verification::policy::{FrozenPolicyCheck, PolicySourceDigest};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ALIAS: &str = "policy-fixture";
const CLOCK: u64 = 1_000_000;
const CHILD_INPUT: &str = "WCODE_POLICY_STORE_FIXTURE";

fn snapshot(root_digest: &str) -> PolicySnapshot {
    let args = vec!["diff".to_owned(), "--check".to_owned()];
    let value = PolicySnapshot {
        schema_version: 1,
        workspace: ALIAS.into(),
        root_digest: root_digest.into(),
        revision: Revision {
            code: format!("sha256:{}", "a".repeat(64)),
            design: Some(format!("sha256:{}", "b".repeat(64))),
        },
        policy: AcceptancePolicy {
            schema_version: 1,
            id: "baseline".into(),
            version: 1,
            requirements: PolicyRequirements {
                minimum_level: PolicyLevel::Full,
                checks: vec!["git-diff-check".into()],
                stages: Vec::new(),
                reviewers: Vec::new(),
                human_approval: false,
                human_approval_min_risk: None,
            },
            docs_only: None,
            rules: Vec::new(),
        },
        mappings: PolicyPathMappings {
            complete: true,
            components: Default::default(),
            requirements: Default::default(),
        },
        checks: vec![FrozenPolicyCheck {
            binding: RequiredVerificationCheck::from_command(
                "git-diff-check",
                "git",
                &args,
                ".",
                ".",
            ),
            level: "quick".into(),
            phase: 0,
            program: "git".into(),
            args,
            cwd: Some(".".into()),
            island: Some(".".into()),
        }],
        sources: vec![PolicySourceDigest {
            path: ".wcode/project.yaml".into(),
            sha256: Some("c".repeat(64)),
        }],
    };
    value.validate().unwrap();
    value
}

fn fixture_binding() -> Binding {
    Binding {
        workspace: ALIAS.into(),
        root_digest: format!("sha256:{}", "d".repeat(64)),
    }
}

fn intent(binding: &Binding, id: &str, action: PolicyAction, decided: u64) -> Intent {
    Intent {
        action,
        snapshot: (action == PolicyAction::Active).then(|| snapshot(&binding.root_digest)),
        operator_receipt: OperatorReceipt::new(id, decided).unwrap().into(),
        expires_at_ms: None,
    }
}

fn first(directory: &Path, binding: &Binding) -> NativePolicyRecord {
    persist_at(
        directory,
        binding,
        0,
        intent(binding, "activate-1", PolicyAction::Active, CLOCK),
        CLOCK,
    )
    .unwrap()
}

fn store_files(directory: &Path) -> Vec<String> {
    let mut names = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn native_policy_activation_expiry_and_revocation_survive_restart() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let now = now_ms().unwrap();
    let expiry = now + 100_000;
    let record = activate(
        &workspace,
        ALIAS,
        0,
        snapshot(&workspace_root_digest(&workspace).unwrap()),
        OperatorReceipt::new("operator-activate", now).unwrap(),
        Some(expiry),
    )
    .unwrap();
    assert_eq!(record.generation(), 1);
    assert!(record.is_active_at(record.created_at_ms()));
    assert!(record.is_active_at(expiry - 1));
    assert!(!record.is_active_at(expiry));
    assert!(!record.is_active_at(record.created_at_ms() - 1));
    let restarted = Workspace::new(root.path(), false, false).unwrap();
    assert_eq!(load(&restarted, ALIAS).unwrap(), Some(record.clone()));
    let revoked = revoke(
        &restarted,
        ALIAS,
        1,
        OperatorReceipt::new("operator-revoke", now_ms().unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(revoked.generation(), 2);
    assert_eq!(revoked.previous_digest, Some(record.digest().unwrap()));
    assert!(revoked.snapshot().is_none());
    assert!(!revoked.is_active_at(now_ms().unwrap()));
    assert_eq!(load(&workspace, ALIAS).unwrap(), Some(revoked));
    assert!(activate(
        &workspace,
        ALIAS,
        1,
        record.snapshot().unwrap().clone(),
        OperatorReceipt::new("stale-approval", now_ms().unwrap()).unwrap(),
        None,
    )
    .is_err());
}

#[test]
fn policy_cas_retry_is_idempotent_without_renewing_expiry_or_replaying_requests() {
    let root = tempfile::tempdir().unwrap();
    let binding = fixture_binding();
    let mut request = intent(&binding, "activate-1", PolicyAction::Active, CLOCK);
    request.expires_at_ms = Some(CLOCK + 2);
    let original = persist_at(root.path(), &binding, 0, request, CLOCK).unwrap();
    let mut retry = intent(&binding, "activate-1", PolicyAction::Active, CLOCK);
    retry.expires_at_ms = Some(CLOCK + 2);
    let expired_retry = persist_at(root.path(), &binding, 0, retry, CLOCK + 3).unwrap();
    assert_eq!(expired_retry, original);
    assert!(!expired_retry.is_active_at(CLOCK + 3));
    assert_eq!(store_files(root.path()).len(), 2);
    assert!(persist_at(
        root.path(),
        &binding,
        0,
        intent(&binding, "different-request", PolicyAction::Active, CLOCK),
        CLOCK + 3,
    )
    .is_err());
    assert!(persist_at(
        root.path(),
        &binding,
        1,
        intent(&binding, "activate-1", PolicyAction::Active, CLOCK),
        CLOCK + 3,
    )
    .is_err());
    assert_eq!(
        load_at(root.path(), &binding, CLOCK + 3).unwrap(),
        Some(original)
    );
}

#[test]
fn policy_authority_rejects_future_clock_invalid_expiry_and_exhausted_generation() {
    let root = tempfile::tempdir().unwrap();
    let binding = fixture_binding();
    assert!(OperatorReceipt::new("operator", 0).is_err());
    assert!(OperatorReceipt::new("operator/token", CLOCK).is_err());
    assert!(persist_at(
        root.path(),
        &binding,
        0,
        intent(&binding, "future", PolicyAction::Active, CLOCK + 1),
        CLOCK,
    )
    .is_err());
    for expiry in [CLOCK, CLOCK - 1, CLOCK + MAX_LIFETIME_MS + 1] {
        let mut request = intent(&binding, "invalid-expiry", PolicyAction::Active, CLOCK);
        request.expires_at_ms = Some(expiry);
        assert!(persist_at(root.path(), &binding, 0, request, CLOCK).is_err());
    }
    assert!(persist_at(
        root.path(),
        &binding,
        u64::MAX,
        intent(&binding, "overflow", PolicyAction::Active, CLOCK),
        CLOCK,
    )
    .is_err());
    let original = first(root.path(), &binding);
    assert!(load_at(root.path(), &binding, CLOCK - 1).is_err());
    assert!(load_at(root.path(), &binding, 0).is_err());
    assert_eq!(
        load_at(root.path(), &binding, CLOCK).unwrap(),
        Some(original)
    );
}

#[test]
fn policy_snapshot_and_tombstone_cannot_move_between_workspace_roots_or_aliases() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let foreign = Workspace::new(other.path(), false, false).unwrap();
    let record = activate(
        &workspace,
        ALIAS,
        0,
        snapshot(&workspace_root_digest(&workspace).unwrap()),
        OperatorReceipt::new("activate", now_ms().unwrap()).unwrap(),
        None,
    )
    .unwrap();
    assert!(load(&workspace, "renamed-alias").is_err());
    assert!(revoke(
        &workspace,
        "renamed-alias",
        1,
        OperatorReceipt::new("revoke", now_ms().unwrap()).unwrap(),
    )
    .is_err());
    assert!(activate(
        &foreign,
        ALIAS,
        0,
        record.snapshot().unwrap().clone(),
        OperatorReceipt::new("foreign", now_ms().unwrap()).unwrap(),
        None,
    )
    .is_err());
    let tombstone = revoke(
        &workspace,
        ALIAS,
        1,
        OperatorReceipt::new("revoke", now_ms().unwrap()).unwrap(),
    )
    .unwrap();
    let target = directory(&foreign).unwrap();
    fs::create_dir_all(&target).unwrap();
    for entry in fs::read_dir(directory(&workspace).unwrap()).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
    }
    assert!(load(&foreign, ALIAS).is_err());
    assert_eq!(load(&workspace, ALIAS).unwrap(), Some(tombstone));
}

#[test]
fn policy_corrupt_highest_generation_never_falls_back_or_accepts_higher_repair() {
    let root = tempfile::tempdir().unwrap();
    let binding = fixture_binding();
    first(root.path(), &binding);
    let revoked = persist_at(
        root.path(),
        &binding,
        1,
        intent(&binding, "revoke-2", PolicyAction::Revoke, CLOCK),
        CLOCK,
    )
    .unwrap();
    let path = record_path(root.path(), 2);
    let good = fs::read(&path).unwrap();
    let mut unknown = serde_json::to_value(&revoked).unwrap();
    unknown["trusted"] = serde_json::json!(true);
    let mut changed = serde_json::to_value(&revoked).unwrap();
    changed["action"] = serde_json::json!("active");
    for bad in [
        b"{".to_vec(),
        serde_json::to_vec(&unknown).unwrap(),
        serde_json::to_vec(&changed).unwrap(),
    ] {
        fs::write(&path, bad).unwrap();
        assert!(load_at(root.path(), &binding, CLOCK).is_err());
        assert!(persist_at(
            root.path(),
            &binding,
            2,
            intent(&binding, "repair", PolicyAction::Active, CLOCK),
            CLOCK,
        )
        .is_err());
        assert!(!record_path(root.path(), 3).exists());
    }
    fs::write(&path, &good).unwrap();
    let oversized = OpenOptions::new().write(true).open(&path).unwrap();
    oversized.set_len(MAX_RECORD_BYTES + 1).unwrap();
    drop(oversized);
    assert!(load_at(root.path(), &binding, CLOCK).is_err());
    fs::write(&path, &good).unwrap();
    let mut mismatch = revoked.clone();
    mismatch.generation = 3;
    mismatch.checksum = mismatch.digest().unwrap();
    fs::write(&path, serde_json::to_vec(&mismatch).unwrap()).unwrap();
    assert!(load_at(root.path(), &binding, CLOCK).is_err());
    fs::write(&path, good).unwrap();
    assert_eq!(
        load_at(root.path(), &binding, CLOCK).unwrap(),
        Some(revoked)
    );
}

#[test]
fn policy_uncommitted_head_and_bad_marker_block_old_active_state() {
    let root = tempfile::tempdir().unwrap();
    let binding = fixture_binding();
    first(root.path(), &binding);
    let revoked = persist_at(
        root.path(),
        &binding,
        1,
        intent(&binding, "revoke-2", PolicyAction::Revoke, CLOCK),
        CLOCK,
    )
    .unwrap();
    let marker = marker_path(root.path(), 2);
    let good = fs::read(&marker).unwrap();
    fs::remove_file(&marker).unwrap();
    assert!(load_at(root.path(), &binding, CLOCK).is_err());
    assert!(persist_at(
        root.path(),
        &binding,
        2,
        intent(&binding, "new-active", PolicyAction::Active, CLOCK),
        CLOCK,
    )
    .is_err());
    for bad in [
        b"{".to_vec(),
        serde_json::to_vec(&CommitMarker {
            generation: 2,
            checksum: format!("sha256:{}", "e".repeat(64)),
        })
        .unwrap(),
    ] {
        fs::write(&marker, bad).unwrap();
        assert!(load_at(root.path(), &binding, CLOCK).is_err());
    }
    fs::write(&marker, good).unwrap();
    assert_eq!(
        load_at(root.path(), &binding, CLOCK).unwrap(),
        Some(revoked)
    );
    let dangling = marker_path(root.path(), 3);
    fs::write(&dangling, b"{}").unwrap();
    assert!(load_at(root.path(), &binding, CLOCK).is_err());
}

#[test]
fn policy_history_capacity_preserves_current_revocation_and_rejects_append() {
    let root = tempfile::tempdir().unwrap();
    let binding = fixture_binding();
    first(root.path(), &binding);
    let mut last = None;
    for generation in 2..=MAX_RECORDS as u64 {
        last = Some(
            persist_at(
                root.path(),
                &binding,
                generation - 1,
                intent(
                    &binding,
                    &format!("revoke-{generation}"),
                    PolicyAction::Revoke,
                    CLOCK,
                ),
                CLOCK,
            )
            .unwrap(),
        );
    }
    let before = store_files(root.path());
    assert_eq!(before.len(), MAX_RECORDS * 2);
    assert!(persist_at(
        root.path(),
        &binding,
        MAX_RECORDS as u64,
        intent(&binding, "over-capacity", PolicyAction::Active, CLOCK),
        CLOCK,
    )
    .is_err());
    assert_eq!(store_files(root.path()), before);
    let restored = load_at(root.path(), &binding, CLOCK).unwrap().unwrap();
    assert_eq!(Some(restored.clone()), last);
    assert_eq!(restored.action, PolicyAction::Revoke);
    assert!(!restored.is_active_at(CLOCK));
    assert!(
        collect_generations(std::iter::once(Err(std::io::Error::other("fixture IO")))).is_err()
    );
    fs::write(root.path().join("g00000000000000000257.json"), b"{}").unwrap();
    assert!(load_at(root.path(), &binding, CLOCK).is_err());
}

#[cfg(unix)]
#[test]
fn policy_authority_rejects_linked_records_and_directories() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("store");
    let binding = fixture_binding();
    first(&store, &binding);
    let path = record_path(&store, 1);
    let original = fs::read(&path).unwrap();
    let external = root.path().join("external.json");
    fs::write(&external, &original).unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&external, &path).unwrap();
    assert!(load_at(&store, &binding, CLOCK).is_err());
    fs::remove_file(&path).unwrap();
    fs::hard_link(&external, &path).unwrap();
    assert!(load_at(&store, &binding, CLOCK).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, original).unwrap();
    let alias = root.path().join("alias");
    symlink(&store, &alias).unwrap();
    assert!(load_at(&alias, &binding, CLOCK).is_err());
    let missing = root.path().join("dangling");
    symlink(root.path().join("absent"), &missing).unwrap();
    assert!(load_at(&missing, &binding, CLOCK).is_err());
}

#[cfg(unix)]
#[test]
fn policy_authority_non_utf8_root_rejection_respects_filesystem_capability() {
    use std::os::unix::ffi::OsStringExt;
    let root = tempfile::tempdir().unwrap();
    let invalid = root
        .path()
        .join(std::ffi::OsString::from_vec(vec![b'p', 0xff]));
    match fs::create_dir(&invalid) {
        Ok(()) => {
            // Filesystems admitting these bytes exercise the native root binding.
            let workspace = Workspace::new(&invalid, false, false).unwrap();
            assert!(workspace_root_digest(&workspace).is_err());
        }
        Err(error) => {
            // APFS rejects the fixture before Workspace or Policy sees it. This
            // asserts only that filesystem prerequisite, not engine coverage.
            #[cfg(target_os = "macos")]
            {
                assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
                assert!(!invalid.exists());
            }
            #[cfg(not(target_os = "macos"))]
            panic!("cannot construct non-UTF-8 root fixture: {error}");
        }
    }
}

#[derive(Deserialize, Serialize)]
struct ChildInput {
    directory: PathBuf,
    control: PathBuf,
    name: String,
    binding_root: String,
    snapshot: Option<PolicySnapshot>,
    action: PolicyAction,
}

struct FixtureChild(Child);

impl Drop for FixtureChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "policy subprocess fixture timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn policy_store_subprocess_helper() {
    let Some(input_path) = std::env::var_os(CHILD_INPUT) else {
        return;
    };
    let bytes = fs::read(input_path).unwrap();
    assert!(bytes.len() < 64 * 1024);
    let input: ChildInput = serde_json::from_slice(&bytes).unwrap();
    let binding = Binding {
        workspace: ALIAS.into(),
        root_digest: input.binding_root,
    };
    assert_eq!(
        load_at(&input.directory, &binding, CLOCK)
            .unwrap()
            .unwrap()
            .generation(),
        1
    );
    fs::write(
        input.control.join(format!("{}.ready", input.name)),
        b"ready",
    )
    .unwrap();
    wait_until(|| input.control.join("go").exists());
    let outcome = persist_at(
        &input.directory,
        &binding,
        1,
        Intent {
            action: input.action,
            snapshot: input.snapshot,
            operator_receipt: OperatorReceipt::new(&input.name, CLOCK).unwrap().into(),
            expires_at_ms: None,
        },
        CLOCK,
    );
    std::process::exit(if outcome.is_ok() { 0 } else { 10 });
}

#[test]
fn policy_two_processes_compete_for_one_generation_without_losing_revocation() {
    let root = tempfile::tempdir().unwrap();
    let store = root.path().join("authority");
    let control = root.path().join("control");
    fs::create_dir(&control).unwrap();
    let binding = fixture_binding();
    first(&store, &binding);
    let test_path = format!(
        "{}::policy_store_subprocess_helper",
        module_path!().split_once("::").unwrap().1,
    );
    let mut children = Vec::new();
    for (name, action) in [
        ("operator-a", PolicyAction::Active),
        ("operator-b", PolicyAction::Revoke),
    ] {
        let mut input_snapshot =
            (action == PolicyAction::Active).then(|| snapshot(&binding.root_digest));
        if let Some(value) = &mut input_snapshot {
            value.policy.version = 2;
        }
        let input = ChildInput {
            directory: store.clone(),
            control: control.clone(),
            name: name.into(),
            binding_root: binding.root_digest.clone(),
            snapshot: input_snapshot,
            action,
        };
        let path = root.path().join(format!("{name}.json"));
        fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &test_path, "--nocapture"])
            .env(CHILD_INPUT, &path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        children.push(FixtureChild(child));
    }
    wait_until(|| {
        ["operator-a", "operator-b"]
            .iter()
            .all(|name| control.join(format!("{name}.ready")).exists())
    });
    fs::write(control.join("go"), b"go").unwrap();
    let mut statuses = [None, None];
    wait_until(|| {
        for (index, child) in children.iter_mut().enumerate() {
            if statuses[index].is_none() {
                statuses[index] = child.0.try_wait().unwrap();
            }
        }
        statuses.iter().all(Option::is_some)
    });
    let codes = statuses.map(|status| status.unwrap().code().unwrap());
    assert_eq!(codes.iter().filter(|code| **code == 0).count(), 1);
    assert_eq!(codes.iter().filter(|code| **code == 10).count(), 1);
    assert_eq!(store_files(&store).len(), 4);
    let current = load_at(&store, &binding, CLOCK).unwrap().unwrap();
    assert_eq!(current.generation(), 2);
    let winner = if codes[0] == 0 {
        "operator-a"
    } else {
        "operator-b"
    };
    assert_eq!(current.operator_receipt.request_id, winner);
    assert_eq!(
        current.action,
        if winner == "operator-a" {
            PolicyAction::Active
        } else {
            PolicyAction::Revoke
        }
    );
    let revoked = if current.action == PolicyAction::Revoke {
        current
    } else {
        persist_at(
            &store,
            &binding,
            2,
            intent(
                &binding,
                "operator-final-revoke",
                PolicyAction::Revoke,
                CLOCK,
            ),
            CLOCK,
        )
        .unwrap()
    };
    assert!(!revoked.is_active_at(CLOCK));
    assert_eq!(load_at(&store, &binding, CLOCK).unwrap(), Some(revoked));
    assert!(persist_at(
        &store,
        &binding,
        1,
        intent(&binding, "late-activation", PolicyAction::Active, CLOCK),
        CLOCK,
    )
    .is_err());
}
