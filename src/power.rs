//! Process-lifetime idle-sleep inhibition.
//!
//! Display sleep and screen locking remain available. The guard only keeps the
//! machine awake so the local MCP server and its tunnel can continue to run.

use anyhow::Result;

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::{bail, Context, Result};
    use std::ffi::{c_char, c_void, CString};
    use std::ptr;

    type CfStringRef = *const c_void;
    type IopmAssertionId = u32;

    const UTF8_ENCODING: u32 = 0x0800_0100;
    const ASSERTION_LEVEL_ON: u32 = 255;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            value: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFRelease(value: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: CfStringRef,
            assertion_level: u32,
            assertion_name: CfStringRef,
            assertion_id: *mut IopmAssertionId,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: IopmAssertionId) -> i32;
    }

    pub(crate) struct Guard {
        assertion_id: IopmAssertionId,
    }

    impl Guard {
        pub(crate) fn acquire() -> Result<Self> {
            let assertion_type = cf_string("PreventUserIdleSystemSleep")?;
            let assertion_name = cf_string("wcode Remote MCP bridge")?;
            let mut assertion_id = 0;
            // SAFETY: both CF strings are valid for the duration of the call and the
            // assertion ID points to initialized writable storage.
            let result = unsafe {
                IOPMAssertionCreateWithName(
                    assertion_type,
                    ASSERTION_LEVEL_ON,
                    assertion_name,
                    &mut assertion_id,
                )
            };
            // SAFETY: both values were returned by CFStringCreateWithCString and are
            // released exactly once after IOKit has retained what it needs.
            unsafe {
                CFRelease(assertion_type);
                CFRelease(assertion_name);
            }
            if result != 0 {
                bail!("IOKit rejected the idle-sleep assertion (code {result})");
            }
            Ok(Self { assertion_id })
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: the ID belongs to this guard and is released exactly once.
            let _ = unsafe { IOPMAssertionRelease(self.assertion_id) };
        }
    }

    fn cf_string(value: &str) -> Result<CfStringRef> {
        let value = CString::new(value).context("keep-awake label contains an interior NUL")?;
        // SAFETY: CString guarantees a terminating NUL and the UTF-8 encoding is correct.
        let string =
            unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr().cast(), UTF8_ENCODING) };
        if string.is_null() {
            bail!("CoreFoundation could not allocate a keep-awake label");
        }
        Ok(string)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use anyhow::{bail, Result};
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED,
    };

    pub(crate) struct Guard;

    impl Guard {
        pub(crate) fn acquire() -> Result<Self> {
            // SAFETY: this process-wide API receives only a documented bitmask.
            if unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) } == 0 {
                bail!("SetThreadExecutionState rejected the idle-sleep request");
            }
            Ok(Self)
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: ES_CONTINUOUS without requirement flags clears this process's request.
            let _ = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use anyhow::{bail, Context, Result};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::thread;
    use std::time::Duration;

    const SIGTERM: i32 = 15;
    const PR_SET_PDEATHSIG: i32 = 1;

    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
        fn prctl(option: i32, argument: usize, ...) -> i32;
    }

    pub(crate) struct Guard {
        inhibitor: Child,
    }

    impl Guard {
        pub(crate) fn acquire() -> Result<Self> {
            let mut command = Command::new("systemd-inhibit");
            command
                .args([
                    "--what=idle:sleep",
                    "--who=wcode",
                    "--why=Keep the Remote MCP bridge online",
                    "--mode=block",
                    "/bin/sleep",
                    "infinity",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            // SAFETY: this runs after fork and before exec, invokes only the async-signal-safe
            // prctl syscall, and makes the inhibitor terminate if wcode dies unexpectedly.
            unsafe {
                command.pre_exec(|| {
                    if prctl(PR_SET_PDEATHSIG, SIGTERM as usize) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut inhibitor = command
                .spawn()
                .context("cannot start systemd-inhibit (systemd is required)")?;
            thread::sleep(Duration::from_millis(50));
            if let Some(status) = inhibitor
                .try_wait()
                .context("cannot inspect systemd-inhibit")?
            {
                bail!("systemd-inhibit exited during startup with {status}");
            }
            Ok(Self { inhibitor })
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let process_group = -(self.inhibitor.id() as i32);
            // SAFETY: the child was created as leader of this dedicated process group.
            let _ = unsafe { kill(process_group, SIGTERM) };
            let _ = self.inhibitor.wait();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod platform {
    use anyhow::{bail, Result};

    pub(crate) struct Guard;

    impl Guard {
        pub(crate) fn acquire() -> Result<Self> {
            bail!("idle-sleep prevention is supported on macOS, Linux, and Windows")
        }
    }
}

pub(crate) struct AwakeGuard {
    _guard: platform::Guard,
}

pub(crate) fn prevent_idle_sleep() -> Result<AwakeGuard> {
    platform::Guard::acquire().map(|guard| AwakeGuard { _guard: guard })
}
