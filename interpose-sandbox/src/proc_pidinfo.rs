use core::ffi::CStr;
use core::mem;

/// Walk the process parents, looking for the pid of the nearest parent
/// `cargo-sandbox` process.
pub fn parent_cargo_sandbox_pid() -> libc::pid_t {
    let mut pid = unsafe { libc::getpid() };

    while pid != 0 {
        let mut info: libc::proc_bsdinfo = unsafe { mem::zeroed() };
        let r = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut _,
                size_of::<libc::proc_bsdinfo>() as i32,
            )
        };
        if r as usize != size_of::<libc::proc_bsdinfo>() {
            break;
        }

        let name = unsafe { CStr::from_ptr(info.pbi_name.as_ptr()) };
        if name == c"cargo-sandbox" {
            return pid;
        }

        pid = info.pbi_ppid as libc::pid_t;
    }

    pid
}
