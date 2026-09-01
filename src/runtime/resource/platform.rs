use super::ProcessSample;
use anyhow::Result;
use std::time::Duration;

#[cfg(any(unix, windows))]
use anyhow::anyhow;
#[cfg(windows)]
use std::mem::size_of;
#[cfg(any(unix, windows))]
use std::mem::MaybeUninit;

#[cfg(unix)]
pub(super) fn configure_process_group(command: &mut tokio::process::Command) {
    use std::os::unix::process::CommandExt;

    // A private process group lets timeout and drop paths terminate compiler or
    // language-server descendants instead of leaving them hot in the background.
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
pub(super) fn configure_process_group(_command: &mut tokio::process::Command) {}

#[cfg(unix)]
pub(super) fn terminate_process_group(child: &mut tokio::process::Child) {
    terminate_process_group_id(child.id());
}

#[cfg(not(unix))]
pub(super) fn terminate_process_group(_child: &mut tokio::process::Child) {}

#[cfg(unix)]
pub(super) fn terminate_process_group_id(process_group: Option<u32>) {
    let Some(process_group) = process_group.and_then(|id| i32::try_from(id).ok()) else {
        return;
    };
    // SAFETY: every governed child is launched in a process group whose ID is
    // its PID. A negative PID targets only that child-owned process group.
    unsafe {
        libc::kill(-process_group, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
pub(super) fn terminate_process_group_id(_process_group: Option<u32>) {}

#[cfg(unix)]
pub(super) fn thread_cpu_time() -> Option<Duration> {
    let mut value = MaybeUninit::<libc::timespec>::zeroed();
    // SAFETY: value points to writable storage for the documented clock_gettime output.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, value.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    // SAFETY: clock_gettime returned success and initialized the timespec.
    let value = unsafe { value.assume_init() };
    Some(Duration::new(
        u64::try_from(value.tv_sec).ok()?,
        u32::try_from(value.tv_nsec).ok()?,
    ))
}

#[cfg(windows)]
pub(super) fn thread_cpu_time() -> Option<Duration> {
    file_times(|creation, exit, kernel, user| unsafe {
        windows_sys::Win32::System::Threading::GetThreadTimes(
            windows_sys::Win32::System::Threading::GetCurrentThread(),
            creation,
            exit,
            kernel,
            user,
        )
    })
}

#[cfg(not(any(unix, windows)))]
pub(super) fn thread_cpu_time() -> Option<Duration> {
    None
}

#[cfg(unix)]
pub(super) fn lower_process_priority(nice: i32) -> Result<()> {
    // SAFETY: setpriority is called for the current process with a bounded nice value.
    let result = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) };
    if result == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "cannot lower process priority: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(windows)]
pub(super) fn lower_process_priority(_nice: i32) -> Result<()> {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS,
    };

    // SAFETY: the pseudo handle refers to the current process and the class is documented.
    if unsafe { SetPriorityClass(GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS) } == 0 {
        Err(anyhow!(
            "cannot lower process priority: {}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) fn lower_process_priority(_nice: i32) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn process_sample() -> Option<ProcessSample> {
    Some(ProcessSample {
        cpu_time: process_cpu_time()?,
        resident_memory_bytes: resident_memory_bytes()?,
    })
}

#[cfg(windows)]
pub(super) fn process_sample() -> Option<ProcessSample> {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

    let cpu_time = file_times(|creation, exit, kernel, user| unsafe {
        GetProcessTimes(GetCurrentProcess(), creation, exit, kernel, user)
    })?;
    let mut counters = MaybeUninit::<PROCESS_MEMORY_COUNTERS>::zeroed();
    // SAFETY: counters points to writable storage and the size matches the structure.
    let result = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            counters.as_mut_ptr(),
            u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?,
        )
    };
    if result == 0 {
        return None;
    }
    // SAFETY: K32GetProcessMemoryInfo returned success.
    let counters = unsafe { counters.assume_init() };
    Some(ProcessSample {
        cpu_time,
        resident_memory_bytes: counters.WorkingSetSize as u64,
    })
}

#[cfg(not(any(unix, windows)))]
pub(super) fn process_sample() -> Option<ProcessSample> {
    None
}

#[cfg(target_os = "macos")]
pub(super) fn release_memory() {
    // SAFETY: a null zone asks the system allocator to relieve pressure globally.
    unsafe {
        malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub(super) fn release_memory() {
    // SAFETY: malloc_trim is a process-local allocator pressure hint.
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(windows)]
pub(super) fn release_memory() {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};

    // SAFETY: -1/-1 asks Windows to trim the current process working set.
    unsafe {
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

#[cfg(not(any(
    target_os = "macos",
    all(target_os = "linux", target_env = "gnu"),
    windows
)))]
pub(super) fn release_memory() {}

#[cfg(unix)]
fn process_cpu_time() -> Option<Duration> {
    let mut usage = MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: usage points to writable storage for getrusage.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: getrusage returned success and initialized the structure.
    let usage = unsafe { usage.assume_init() };
    Some(timeval_duration(usage.ru_utime) + timeval_duration(usage.ru_stime))
}

#[cfg(unix)]
fn timeval_duration(value: libc::timeval) -> Duration {
    let seconds = u64::try_from(value.tv_sec).unwrap_or_default();
    let micros = u32::try_from(value.tv_usec)
        .unwrap_or_default()
        .min(999_999);
    Duration::new(seconds, micros.saturating_mul(1_000))
}

#[cfg(target_os = "linux")]
fn resident_memory_bytes() -> Option<u64> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    // SAFETY: sysconf is queried with a documented constant and has no side effects.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    (page_size > 0).then(|| resident_pages.saturating_mul(page_size as u64))
}

#[cfg(target_os = "macos")]
fn resident_memory_bytes() -> Option<u64> {
    let mut info = MaybeUninit::<RusageInfoV2>::zeroed();
    // SAFETY: info points to a correctly sized rusage_info_v2-compatible buffer.
    let result = unsafe {
        proc_pid_rusage(
            std::process::id() as i32,
            RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    };
    if result != 0 {
        return None;
    }
    // SAFETY: proc_pid_rusage returned success and initialized the structure.
    let info = unsafe { info.assume_init() };
    Some(info.phys_footprint.max(info.resident_size))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn resident_memory_bytes() -> Option<u64> {
    let mut usage = MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: usage points to writable storage for getrusage.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: getrusage returned success and initialized the structure.
    let usage = unsafe { usage.assume_init() };
    u64::try_from(usage.ru_maxrss)
        .ok()
        .map(|kilobytes| kilobytes.saturating_mul(1024))
}

#[cfg(windows)]
fn file_times(
    call: impl FnOnce(
        *mut windows_sys::Win32::Foundation::FILETIME,
        *mut windows_sys::Win32::Foundation::FILETIME,
        *mut windows_sys::Win32::Foundation::FILETIME,
        *mut windows_sys::Win32::Foundation::FILETIME,
    ) -> i32,
) -> Option<Duration> {
    use windows_sys::Win32::Foundation::FILETIME;

    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    if call(&mut creation, &mut exit, &mut kernel, &mut user) == 0 {
        return None;
    }
    let ticks = file_time_ticks(kernel).saturating_add(file_time_ticks(user));
    Some(Duration::from_nanos(ticks.saturating_mul(100)))
}

#[cfg(windows)]
fn file_time_ticks(value: windows_sys::Win32::Foundation::FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(target_os = "macos")]
const RUSAGE_INFO_V2: i32 = 2;

#[cfg(target_os = "macos")]
#[repr(C)]
struct RusageInfoV2 {
    uuid: [u8; 16],
    user_time: u64,
    system_time: u64,
    package_idle_wakeups: u64,
    interrupt_wakeups: u64,
    pageins: u64,
    wired_size: u64,
    resident_size: u64,
    phys_footprint: u64,
    process_start_abstime: u64,
    process_exit_abstime: u64,
    child_user_time: u64,
    child_system_time: u64,
    child_package_idle_wakeups: u64,
    child_interrupt_wakeups: u64,
    child_pageins: u64,
    child_elapsed_abstime: u64,
    disk_bytes_read: u64,
    disk_bytes_written: u64,
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "macos")]
extern "C" {
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
extern "C" {
    fn malloc_trim(pad: usize) -> i32;
}
