use super::*;

pub(super) fn require_risky_exec(label: &str, enabled: bool) -> Result<()> {
    if enabled {
        Ok(())
    } else {
        bail!(
            "{label} requires exact risky-operation authorization; approve the specific operation in the TUI/Web UI, or restart with --allow-risky-exec only for a trusted repository"
        )
    }
}

/// Only exact, bounded inspection shapes use the separate process queue.
/// This is consulted after policy checks, never as a grant or validation bypass.
pub(in crate::workspace) fn is_inspection_probe(program: &str, args: &[String]) -> bool {
    if program == "cargo" {
        return args_equal(args, &["fmt", "--check"])
            || args_equal(args, &["metadata", "--no-deps"])
            || args_equal(args, &["metadata", "--no-deps", "--format-version", "1"])
            || args_equal(args, &["metadata", "--format-version", "1", "--no-deps"]);
    }
    if program != "git" {
        return false;
    }
    match args {
        [one] => matches!(one.as_str(), "--version" | "status"),
        [first, second] => matches!(
            (first.as_str(), second.as_str()),
            ("status", "--short")
                | ("diff", "--check" | "--numstat")
                | ("branch", "--show-current")
                | (
                    "rev-parse",
                    "--show-toplevel" | "--show-prefix" | "--is-inside-work-tree"
                )
        ),
        [first, second, third] => matches!(
            (first.as_str(), second.as_str(), third.as_str()),
            ("status", "--short", "--branch" | "--untracked-files=all")
                | ("diff", "--cached", "--check" | "--numstat")
        ),
        _ => false,
    }
}

pub(in crate::workspace) fn hardened_command_args(program: &str, args: &[String]) -> Vec<String> {
    if program != "git" {
        return args.to_vec();
    }
    let Some(subcommand_index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return args.to_vec();
    };

    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut overrides = vec![
        "core.fsmonitor=false".to_owned(),
        "core.untrackedCache=false".to_owned(),
        format!("core.hooksPath={null_path}"),
        format!("core.attributesFile={null_path}"),
        format!("core.excludesFile={null_path}"),
        "diff.external=".to_owned(),
        "commit.gpgSign=false".to_owned(),
        "tag.gpgSign=false".to_owned(),
        "credential.helper=".to_owned(),
        "core.askPass=".to_owned(),
        "core.gitProxy=".to_owned(),
        "http.extraHeader=".to_owned(),
        "protocol.ext.allow=never".to_owned(),
        "protocol.file.allow=never".to_owned(),
        "maintenance.auto=false".to_owned(),
        "gc.auto=0".to_owned(),
    ];
    overrides.push(if is_git_ssh_command(args) {
        "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new".to_owned()
    } else {
        "core.sshCommand=false".to_owned()
    });
    let extra_diff_args = if matches!(args[subcommand_index].as_str(), "diff" | "log" | "show") {
        2
    } else {
        0
    };
    let mut hardened = Vec::with_capacity(args.len() + overrides.len() * 2 + extra_diff_args);
    for override_value in overrides {
        hardened.push("-c".to_owned());
        hardened.push(override_value);
    }
    hardened.extend_from_slice(&args[..=subcommand_index]);
    if matches!(args[subcommand_index].as_str(), "diff" | "log" | "show") {
        hardened.push("--no-ext-diff".to_owned());
        hardened.push("--no-textconv".to_owned());
    }
    hardened.extend_from_slice(&args[subcommand_index + 1..]);
    hardened
}

pub(in crate::workspace) fn is_git_ssh_command(args: &[String]) -> bool {
    if is_git_push_command(args) {
        return true;
    }
    let Some(index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return false;
    };
    args[index] == "fetch" && bounded_full_access_command("git", args)
}

pub(super) fn is_git_push_command(args: &[String]) -> bool {
    let Some(index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return false;
    };
    args[index] == "push"
        || (args[index] == "lfs" && args.get(index + 1).is_some_and(|arg| arg == "push"))
}

pub(in crate::workspace) async fn read_bounded_stream<R>(
    mut reader: R,
    progress: Option<tokio::sync::mpsc::Sender<CommandOutputChunk>>,
    stderr: bool,
) -> std::io::Result<(String, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::with_capacity(MAX_OUTPUT_BYTES.min(16 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
        if let Some(progress) = &progress {
            let _ = progress
                .send(CommandOutputChunk {
                    stderr,
                    bytes: buffer[..read].to_vec(),
                    truncated,
                })
                .await;
        }
    }
    Ok((String::from_utf8_lossy(&stored).to_string(), truncated))
}
