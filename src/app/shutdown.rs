use anyhow::{anyhow, Result};
use std::future::Future;

/// Install handlers before publishing readiness; polling later must not lose a signal.
pub(super) fn signal() -> Result<impl Future<Output = Result<()>>> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        Ok(async move {
            let event = tokio::select! {
                value = interrupt.recv() => value,
                value = terminate.recv() => value,
            };
            event.ok_or_else(|| anyhow!("shutdown signal stream closed"))
        })
    }
    #[cfg(windows)]
    {
        let mut interrupt = tokio::signal::windows::ctrl_c()?;
        Ok(async move {
            interrupt
                .recv()
                .await
                .ok_or_else(|| anyhow!("shutdown signal stream closed"))
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(async { tokio::signal::ctrl_c().await.map_err(Into::into) })
    }
}
