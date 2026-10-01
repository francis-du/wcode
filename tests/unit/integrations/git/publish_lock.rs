use super::*;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    root
}
fn acquire(root: &Path) -> Result<PublicationGuard> {
    PublicationGuard::acquire_at(&root.canonicalize().unwrap(), "1".repeat(64))
}

#[test]
fn publication_lock_is_nonblocking_and_reuses_inode_after_explicit_unlock() {
    let directory = root();
    let first = acquire(directory.path()).unwrap();
    let alias = first.file.try_clone().unwrap();
    let path = first.path.clone();
    assert!(acquire(directory.path())
        .err()
        .unwrap()
        .is::<PublicationBusy>());
    drop(first);
    let next = acquire(directory.path()).unwrap();
    assert_eq!(next.path, path);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    drop(alias);
    assert!(acquire(directory.path())
        .err()
        .unwrap()
        .is::<PublicationBusy>());
    drop(next);
    assert!(acquire(directory.path()).is_ok());
}

#[test]
fn publication_lock_binding_ignores_pr_base_and_credentials_but_separates_heads() {
    let provider = GitHubProvider::new(
        GitHubConfig::new(
            ProviderRepository::new("owner", "repo").unwrap(),
            42,
            99,
            "check",
        )
        .unwrap(),
        "NOT-LIVE-KEY",
    )
    .unwrap();
    let mut target = ProviderTarget {
        repository: provider.config.repository.clone(),
        change: 7,
        base_sha: "a".repeat(40),
        head_sha: "b".repeat(40),
    };
    let original = binding_key(&provider, &target).unwrap();
    let directory = root();
    let guard =
        PublicationGuard::acquire_at(&directory.path().canonicalize().unwrap(), original.clone())
            .unwrap();
    assert!(guard.check(&provider, &target).is_ok());
    target.change = 8;
    target.base_sha = "c".repeat(40);
    target.head_sha = "B".repeat(40);
    assert_eq!(binding_key(&provider, &target).unwrap(), original);
    assert!(guard.check(&provider, &target).is_ok());
    let rotated = GitHubProvider::new(provider.config.clone(), "ROTATED-NOT-LIVE-KEY").unwrap();
    assert!(guard.check(&rotated, &target).is_ok());
    target.head_sha = "d".repeat(40);
    assert_ne!(binding_key(&provider, &target).unwrap(), original);
    assert!(guard.check(&provider, &target).is_err());
    let independent = PublicationGuard::acquire_at(
        &directory.path().canonicalize().unwrap(),
        binding_key(&provider, &target).unwrap(),
    )
    .unwrap();
    assert!(independent.check(&provider, &target).is_ok());
    target.repository = ProviderRepository::new("other", "repo").unwrap();
    assert!(binding_key(&provider, &target).is_err());
    assert!(!original.contains("NOT-LIVE"));
}

#[test]
fn publication_lock_rejects_nonempty_or_replaced_lock_without_truncating() {
    let directory = root();
    let guard = acquire(directory.path()).unwrap();
    let path = guard.path.clone();
    // Windows locks may deny I/O through another handle. Corrupt the inactive
    // file here; the Unix test below separately challenges a live inode.
    drop(guard);
    fs::write(&path, b"untrusted-state").unwrap();
    assert!(acquire(directory.path()).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"untrusted-state");
}

#[cfg(unix)]
#[test]
fn publication_lock_rejects_aliases_permissions_and_inode_replacement() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let directory = root();
    let canonical = directory.path().canonicalize().unwrap();
    let real = canonical.join("real");
    fs::create_dir(&real).unwrap();
    let alias = canonical.join("alias");
    symlink(&real, &alias).unwrap();
    assert!(PublicationGuard::acquire_at(&alias.join("new"), "1".repeat(64)).is_err());
    assert!(!real.join("new").exists());
    let guard = acquire(directory.path()).unwrap();
    let link = canonical.join("duplicate.lock");
    fs::hard_link(&guard.path, &link).unwrap();
    assert!(guard.check_path().is_err());
    assert!(acquire(directory.path()).is_err());
    fs::remove_file(link).unwrap();
    fs::set_permissions(&guard.path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(guard.check_path().is_err());
    fs::set_permissions(&guard.path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::rename(&guard.path, canonical.join("old.lock")).unwrap();
    inbox::create_file(&guard.path).unwrap();
    assert!(guard.check_path().is_err());
}

// A real second OS process; only this test-only path accepts a supplied lock root.
#[test]
fn publication_lock_process_probe() {
    let Some(root) = std::env::var_os("WCODE_TEST_PUBLICATION_LOCK_ROOT") else {
        return;
    };
    match acquire(Path::new(&root)) {
        Ok(guard) => {
            println!("publication-lock-held");
            std::io::stdout().flush().unwrap();
            if std::env::var("WCODE_TEST_PUBLICATION_HOLD").as_deref() == Ok("1") {
                let mut byte = [0];
                std::io::Read::read(&mut std::io::stdin(), &mut byte).unwrap();
            }
            drop(guard);
        }
        Err(error) => {
            assert!(error.is::<PublicationBusy>(), "{error}");
            println!("publication-lock-busy");
        }
    }
}

fn child(directory: &Path, hold: bool) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "git_provider::github::publish_lock::tests::publication_lock_process_probe",
            "--nocapture",
        ])
        .env("WCODE_TEST_PUBLICATION_LOCK_ROOT", directory)
        .env("WCODE_TEST_PUBLICATION_HOLD", if hold { "1" } else { "0" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn publication_lock_blocks_another_process_and_recovers_after_process_exit() {
    let directory = root();
    let guard = acquire(directory.path()).unwrap();
    let denied = child(directory.path(), false).output().unwrap();
    assert!(
        denied.status.success(),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    assert!(String::from_utf8_lossy(&denied.stdout).contains("publication-lock-busy"));
    drop(guard);
    let allowed = child(directory.path(), false).output().unwrap();
    assert!(allowed.status.success());
    assert!(String::from_utf8_lossy(&allowed.stdout).contains("publication-lock-held"));
}

#[test]
fn publication_lock_process_death_releases_only_local_ownership() {
    struct OwnedChild(std::process::Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = root();
    let mut process = OwnedChild(child(directory.path(), true).spawn().unwrap());
    let output = process.0.stdout.take().unwrap();
    let mut reader = std::io::BufReader::new(output);
    let mut line = String::new();
    loop {
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "child exited before holding lock"
        );
        if line.contains("publication-lock-held") {
            break;
        }
        line.clear();
    }
    assert!(acquire(directory.path())
        .err()
        .unwrap()
        .is::<PublicationBusy>());
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    assert!(acquire(directory.path()).is_ok());
    // Local admission recovery asserts no GitHub revocation or remote rollback.
}
