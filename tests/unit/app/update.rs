use super::*;
use std::ffi::OsString;

#[test]
fn update_targets_the_running_binary_directory_unless_explicitly_overridden() {
    let executable = PathBuf::from("/opt/wcode/bin/wcode");
    assert_eq!(
        install_dir(&executable, None).unwrap(),
        PathBuf::from("/opt/wcode/bin")
    );
    assert_eq!(
        install_dir(&executable, Some(OsString::from("/tmp/wcode-bin"))).unwrap(),
        PathBuf::from("/tmp/wcode-bin")
    );
    assert!(install_dir(&executable, Some(OsString::new())).is_err());
}

#[test]
fn update_notice_requires_reconnecting_existing_mcp_children() {
    assert!(RECONNECT_NOTICE.contains("Reconnect your MCP Host"));
    assert!(RECONNECT_NOTICE.contains("existing stdio sessions"));
}

#[test]
fn update_stages_unique_helper_scripts_outside_the_installation() {
    let first = temporary_script("sh");
    let second = temporary_script("sh");
    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(env::temp_dir().as_path()));
    assert_eq!(
        first.extension().and_then(|value| value.to_str()),
        Some("sh")
    );
}

#[cfg(unix)]
#[test]
fn embedded_unix_installer_keeps_checksum_smoke_and_atomic_replace_contract() {
    for required in [
        "SHA256SUMS",
        "SHA-256 checksum mismatch",
        "\"$install_tmp\" --version >/dev/null",
        "\"$install_tmp\" --help >/dev/null",
        "mv -f \"$install_tmp\" \"$install_path\"",
    ] {
        assert!(
            UNIX_INSTALLER.contains(required),
            "missing installer guard: {required}"
        );
    }
    let smoke = UNIX_INSTALLER
        .find("\"$install_tmp\" --help >/dev/null")
        .unwrap();
    let replace = UNIX_INSTALLER
        .find("mv -f \"$install_tmp\" \"$install_path\"")
        .unwrap();
    assert!(smoke < replace);
}

#[cfg(windows)]
#[test]
fn embedded_windows_installer_keeps_checksum_smoke_and_staged_replace_contract() {
    for required in [
        "SHA-256 checksum mismatch",
        "& $InstallTemp --version",
        "& $InstallTemp --help",
        "Move-Item -Force $InstallTemp $InstallPath",
    ] {
        assert!(
            WINDOWS_INSTALLER.contains(required),
            "missing installer guard: {required}"
        );
    }
}
