use super::*;

/// Building and parsing the command tree costs ~1 MiB of stack in a debug
/// build, which overflows the 1 MiB stack Windows gives the process main thread.
/// Only the parse needs that headroom, so run it on an explicitly sized thread
/// and keep the runtime (and the macOS menu bar) on the main thread.
const CLI_PARSE_STACK_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn parse_cli_args() -> Result<Args> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("wcode-cli".to_owned())
        .stack_size(CLI_PARSE_STACK_BYTES)
        .spawn(move || {
            let _ = sender.send(Args::parse());
        })
        .map_err(|error| anyhow::anyhow!("cannot start the CLI parsing thread: {error}"))?;
    match handle.join() {
        Ok(()) => receiver
            .recv()
            .map_err(|_| anyhow::anyhow!("the CLI parsing thread exited without a result")),
        Err(_) => Err(anyhow::anyhow!("the CLI parsing thread panicked")),
    }
}
