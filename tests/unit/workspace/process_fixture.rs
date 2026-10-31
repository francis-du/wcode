//! Exact child tests own their state roots and leave the parent environment intact.
use std::time::Duration;

const FIXTURE_ROOT: &str = "WCODE_SANDBOX_TEST_ROOT";

pub(in crate::workspace) async fn run_in_private_fixture(module: &str, name: &str) -> bool {
    if let Some(root) = std::env::var_os(FIXTURE_ROOT) {
        let root = std::path::PathBuf::from(root);
        assert!(root.is_absolute() && root.is_dir());
        let authority = crate::core_types::authority_state_root().unwrap();
        let intelligence = crate::core_types::intelligence_state_root().unwrap();
        assert_eq!(authority, root.join("authority"));
        assert!(intelligence.starts_with(root.join("tmp")));
        // Materialize only the child-private resolver roots, before any sandbox
        // preflight; never rely on another parallel test having created stores.
        std::fs::create_dir_all(&authority).unwrap();
        std::fs::create_dir_all(&intelligence).unwrap();
        return false;
    }
    let original_state = std::env::var_os("WCODE_STATE_DIR");
    let original_temp = std::env::temp_dir();
    let root = tempfile::Builder::new()
        .prefix("wcode-sandbox-fixture-")
        .tempdir()
        .unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let temporary = root_path.join("tmp");
    std::fs::create_dir(&temporary).unwrap();
    let module = module.split_once("::").unwrap().1;
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            format!("{module}::{name}"),
            "--exact".into(),
            "--nocapture".into(),
        ])
        .env(FIXTURE_ROOT, &root_path)
        .env("WCODE_STATE_DIR", root_path.join("authority"))
        .env("TMPDIR", &temporary)
        .env("TMP", &temporary)
        .env("TEMP", &temporary)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(60), command.output())
        .await
        .expect("private sandbox test must terminate")
        .unwrap();
    assert_eq!(std::env::var_os("WCODE_STATE_DIR"), original_state);
    assert_eq!(std::env::temp_dir(), original_temp);
    assert!(
        output.status.success(),
        "isolated sandbox test failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("running 1 test") && stdout.contains("1 passed; 0 failed"),
        "the exact child test must execute, not an empty filter: {stdout}"
    );
    true
}
