use super::*;

/// Building/parsing the command tree and rendering the complete CLI reference
/// each cost ~1 MiB of stack in a debug build. Windows gives the process main
/// thread a 1 MiB stack, so both run on an explicitly sized thread while the
/// runtime (and the macOS menu bar) stays on the main thread.
const EXPLICIT_STACK_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn run_on_explicit_stack<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("wcode-stack".to_owned())
        .stack_size(EXPLICIT_STACK_BYTES)
        .spawn(move || {
            let _ = sender.send(operation());
        })
        .expect("cannot start the explicitly sized stack thread");
    match handle.join() {
        Ok(()) => receiver
            .recv()
            .expect("the explicitly sized stack thread exited without a result"),
        Err(_) => panic!("the explicitly sized stack thread panicked"),
    }
}

pub(super) fn parse_cli_args() -> Args {
    run_on_explicit_stack(Args::parse)
}
