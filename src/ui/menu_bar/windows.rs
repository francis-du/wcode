use super::*;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex};

pub(super) struct InstanceLock {
    _handle: HANDLE,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `CreateMutexW` and is still open.
        unsafe {
            ReleaseMutex(self._handle);
            CloseHandle(self._handle);
        }
    }
}

impl InstanceLock {
    fn acquire() -> Result<Option<Self>> {
        let name: Vec<u16> = "Local\\wcode-menu-bar"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: `name` is a valid null-terminated UTF-16 string and no
        // mutex security attributes are required.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        // SAFETY: `GetLastError` has no preconditions.
        let error = unsafe { GetLastError() };
        if handle.is_null() {
            return Err(std::io::Error::from_raw_os_error(error as i32).into());
        }
        if error == ERROR_ALREADY_EXISTS {
            // SAFETY: `handle` is an open mutex handle.
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(Self { _handle: handle }))
    }
}

pub(super) fn graphical_session() -> bool {
    use windows_sys::Win32::System::StationsAndDesktops::{
        GetProcessWindowStation, GetUserObjectInformationW, UOI_FLAGS, USEROBJECTFLAGS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::WSF_VISIBLE;

    // SAFETY: `GetProcessWindowStation` returns the calling process's window
    // station, and `GetUserObjectInformationW` writes only a `USEROBJECTFLAGS`
    // value into the supplied buffer.
    unsafe {
        let station = GetProcessWindowStation();
        if station.is_null() {
            return false;
        }
        let mut flags = USEROBJECTFLAGS {
            fInherit: 0,
            fReserved: 0,
            dwFlags: 0,
        };
        let mut needed = 0_u32;
        let written = GetUserObjectInformationW(
            station,
            UOI_FLAGS,
            std::ptr::addr_of_mut!(flags).cast(),
            std::mem::size_of::<USEROBJECTFLAGS>() as u32,
            &mut needed,
        );
        written != 0 && flags.dwFlags & WSF_VISIBLE as u32 != 0
    }
}

pub(super) fn run() -> Result<()> {
    native::run(InstanceLock::acquire()?)
}
