#[cfg(target_os = "linux")]
pub(crate) fn check_pid_alive(pid: i32) -> Option<bool> {
    use std::path::Path;

    Some(Path::new("/proc").join(pid.to_string()).exists())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn check_pid_alive(pid: i32) -> Option<bool> {
    use libc::{kill, c_int};
    const SIGZERO: c_int = 0;
    let result = unsafe { kill(pid, SIGZERO) };
    if result == 0 {
        return Some(true);
    }
    let errno = std::io::Error::last_os_error().raw_os_error()?;
    Some(errno != libc::ESRCH)
}

#[cfg(target_os = "windows")]
pub(crate) fn check_pid_alive(pid: i32) -> Option<bool> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32);
        if handle.is_null() {
            return Some(false);
        }

        let mut status: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut status as *mut u32);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(status == STILL_ACTIVE as u32)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios", target_os = "windows")))]
pub(crate) fn check_pid_alive(_pid: i32) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::check_pid_alive;

    #[test]
    fn current_process_is_alive() {
        let pid = std::process::id() as i32;
        assert_eq!(check_pid_alive(pid), Some(true));
    }

    #[test]
    fn implausible_process_is_not_alive() {
        let pid = i32::MAX;
        assert_eq!(check_pid_alive(pid), Some(false));
    }
}
