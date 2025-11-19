//! Intercept Cargo's process spawning.
//!
//! TODO: Explain the purpose of this.
//!
//! NOTE: Interposing doesn't work with system binaries like `/bin/sh` and
//! `/usr/bin/cc` (the latter of which `rustc` calls to link) or binaries
//! using a hardened runtime. But that's fine, we only intend on using this on
//! the `cargo` (and `rustup-init`) binary.
//!
//! ## Raw usage
//!
//! If you're testing things, you might want to use this without going through
//! `cargo-sandbox`. This can be done as follows:
//!
//! ```sh
//! cargo build --bin cargo-sandbox-interceptor
//! env DYLD_INSERT_LIBRARIES=$(pwd)/target/debug/cargo-sandbox-interceptor $(rustup which cargo) check
//! ```

// HACK: `cargo install` has no facility for installing libraries (which this
// really is), so we work around it by telling Cargo that it's a binary, while
// using `#![no_main]` and passing flags like `-dylib` and `-shared` to the
// linker via `build.rs`.
#![no_main]

use cargo_sandbox::env::Env;
use cargo_sandbox::proc_pidinfo::parent_pid;
use core::ffi::{CStr, c_char, c_int, c_void};
use core::fmt::Display;
use core::ptr;
use init::{get_policy, run_sandbox_init};
use std::ffi::{CString, OsString};
use std::fs::{File, create_dir_all};
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use cargo_sandbox::config::SandboxConfig;
use cargo_sandbox::init;
use cargo_sandbox::kind::*;

static ENV: OnceLock<Env> = OnceLock::new();
static CONFIG: OnceLock<SandboxConfig> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Load the sandbox configuration.
///
/// This happens inside Cargo's process space, before anything else, which
/// means in particular that things like the environment variables that we
/// load here cannot be overwritten by `cargo::rustc-env=` in build scripts or
/// the `[env]` table in configs.
#[ctor::ctor]
fn load_env() {
    let cargo_home = home::cargo_home().unwrap();
    let rustup_home = home::rustup_home().unwrap();
    let config = SandboxConfig::load(&cargo_home).expect("failed loading config");
    CONFIG.set(config).unwrap();

    // Look up the current Xcode path.
    let developer_dir = std::env::var_os("DEVELOPER_DIR").unwrap_or_else(|| {
        let mut output = Command::new("xcode-select")
            .arg("--print-path")
            .env("DYLD_INSERT_LIBRARIES", "")
            .output()
            .ok()
            .unwrap();
        assert!(output.status.success(), "failed finding developer dir");
        // Remove trailing newline.
        if let Some(b'\n') = output.stdout.last() {
            let _ = output.stdout.pop().unwrap();
        }
        OsString::from_vec(output.stdout)
    });
    let developer_dir = PathBuf::from(developer_dir);

    let path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();

    let parent_cargo_sandbox_pid = parent_pid(c"cargo-sandbox");

    ENV.set(Env {
        cargo_home,
        rustup_home,
        developer_dir,
        path,
        parent_cargo_sandbox_pid,
    })
    .unwrap();

    let log_path =
        std::env::temp_dir().join(format!("cargo-sandbox-{parent_cargo_sandbox_pid}.txt"));
    File::create(&log_path).unwrap();
    LOG_PATH.set(log_path).unwrap();
}

/// Interpose `posix_spawn`.
///
/// `posix_spawnp` calls this, see:
/// <https://github.com/apple-oss-distributions/Libc/blob/Libc-1698.140.3/sys/posix_spawn.c>
unsafe extern "C" fn posix_spawn(
    pid: *mut libc::pid_t,
    path: *const c_char,
    file_actions: *const libc::posix_spawn_file_actions_t,
    attrp: *const libc::posix_spawnattr_t,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    let path_cstr = unsafe { cstr(path) }.unwrap_or(&*c"<null>");
    log_msg(format!("execve called: path={path_cstr:?}"));

    let kind = ProcessKind::parse(
        path_cstr,
        unsafe { iter_null_terminated_lst(argv) },
        unsafe { iter_null_terminated_lst(envp) },
    );
    if kind == ProcessKind::Other {
        log_msg("skipping sandbox!\n");
        return unsafe {
            libc::posix_spawn(pid, path, file_actions, attrp, argv.cast(), envp.cast())
        };
    }

    log_msg("decided to sandbox!");

    let given_args = unsafe { iter_null_terminated_lst(argv) }.collect::<Vec<_>>();
    let given_env = unsafe { iter_null_terminated_lst(envp) }.collect::<Vec<_>>();
    log_msg(format!("argv={given_args:#?}"));
    log_msg(format!("env={given_env:#?}"));

    log_msg(format!("{kind:?}"));

    // If the path is relative and starts with `-`, we can't be sure that it
    // isn't some parameter that `sandbox-exec` will parse, like `-D`. So
    // error in that case.
    if path_cstr.to_bytes().starts_with(b"-") {
        return libc::EACCES;
    }

    let config = CONFIG.get().unwrap().config_for(&kind);

    let project_local_tmpdir = kind.target_dir().join("sandbox-tmp");
    create_dir_all(&project_local_tmpdir).unwrap();
    let policy = get_policy(
        config,
        &project_local_tmpdir,
        &kind,
        ENV.get().unwrap(),
        &project_local_tmpdir, // TODO
    );

    // Wrap and spawn inside `sandbox-exec`. This basically ends up calling
    // `sandbox_init`, but allows us to avoid manually fork+exec-ing.
    let sandbox_exec = c"/usr/bin/sandbox-exec";

    let mut args: Vec<*const c_char> = Vec::new();
    args.push(sandbox_exec.as_ptr());
    args.push(c"-p".as_ptr());
    args.push(policy.as_ptr());
    args.push(path_cstr.as_ptr());
    args.extend(
        // Skip arg0
        unsafe { read_null_terminated_lst(argv.cast()) }
            .iter()
            .skip(1),
    );
    args.push(ptr::null());

    let (_storage, env) = unsafe { override_env(&project_local_tmpdir, envp) };

    log_msg(format!("storage: {_storage:?}"));

    log_msg(format!("policy:\n{}", policy.to_string_lossy()));

    log_msg("");

    unsafe {
        libc::posix_spawn(
            pid,
            sandbox_exec.as_ptr(),
            file_actions,
            attrp,
            args.as_ptr().cast(),
            env.as_ptr().cast(),
        )
    }
}

/// Interpose `execve`.
///
/// Both `execv` and `execvp` bottom out into this, as documented in the man
/// page `exec(3)`:
/// > The functions described in this manual page are front-ends for the
/// > function execve(2).
///
/// NOTE: Doing anything complex here is generally a bad idea, since there
/// might be process-level locks. But this is probably the best we can do
/// without integrating into Cargo.
///
/// NOTE: The Rust standard library overwrites `environ` before calling
/// `execvp`, so we cannot rely on querying that for anything useful.
unsafe extern "C" fn execve(
    file: *const c_char,
    argv: *const *const c_char,
    envp: *const *const c_char,
) -> c_int {
    let file_cstr = unsafe { cstr(file) }.unwrap_or(&*c"<null>");
    log_msg(format!("execve called: path={file_cstr:?}"));

    let kind = ProcessKind::parse(
        file_cstr,
        unsafe { iter_null_terminated_lst(argv) },
        unsafe { iter_null_terminated_lst(envp) },
    );
    if kind == ProcessKind::Other {
        log_msg("skipping sandbox!\n");
        return unsafe { libc::execve(file, argv, envp) };
    }

    log_msg("decided to sandbox!");

    let given_args = unsafe { iter_null_terminated_lst(argv) }.collect::<Vec<_>>();
    let given_env = unsafe { iter_null_terminated_lst(envp) }.collect::<Vec<_>>();
    log_msg(format!("argv={given_args:#?}"));
    log_msg(format!("env={given_env:#?}"));

    // TODO: Modify attributes such as that set by `posix_spawnattr_set_csm_np`?

    log_msg(format!("{kind:?}"));

    let config = CONFIG.get().unwrap().config_for(&kind);

    let project_local_tmpdir = kind.target_dir().join("sandbox-tmp");
    create_dir_all(&project_local_tmpdir).unwrap(); // Maybe even mount_tmpfs?
    let policy = get_policy(
        config,
        &project_local_tmpdir,
        &kind,
        ENV.get().unwrap(),
        &std::env::current_dir().unwrap(),
    );

    let (_storage, env) = unsafe { override_env(&project_local_tmpdir, envp) };

    log_msg(format!("storage: {_storage:?}"));

    log_msg(&format!("policy:\n{}\n", policy.to_string_lossy()));

    log_msg("");

    if let Err(err) = run_sandbox_init(&policy) {
        log_msg(err);
        errno::set_errno(errno::Errno(-10000)); // TODO
        return -1;
    }

    unsafe { libc::execve(file, argv, env.as_ptr()) }
}

/// The interposition table, which allows us to replace the functions in the
/// process which this dylib is inserted into.
///
/// See: <http://toves.freeshell.org/interpose/>
#[used] // Declare this as an entry-point in the dylib.
#[unsafe(link_section = "__DATA,__interpose")]
static INTERPOSE_TABLE: [Interpose; 2] = [
    Interpose {
        replacement: posix_spawn as *const c_void,
        original: libc::posix_spawn as *const c_void,
    },
    Interpose {
        replacement: execve as *const c_void,
        original: libc::execve as *const c_void,
    },
];

#[repr(C)]
#[derive(Debug)]
struct Interpose {
    replacement: *const c_void,
    original: *const c_void,
}

unsafe impl Send for Interpose {}
unsafe impl Sync for Interpose {}

unsafe fn cstr<'a>(p: *const c_char) -> Option<&'a CStr> {
    if !p.is_null() {
        Some(unsafe { CStr::from_ptr(p) })
    } else {
        None
    }
}

#[track_caller]
fn log_msg(msg: impl Display) {
    let Some(log_path) = LOG_PATH.get() else {
        // Fail silently for now, the ctor doesn't seem to be run in certain cases?
        return;
    };
    // NOTE: We cannot log to stdout/stderr in here, that interferes with
    // Cargo's `exec_with_streaming`.
    // TODO: Work on a way to pass data between `cargo-sandbox` and `cargo-sandbox-interceptor`, integrate into `tracing!`.
    let mut file = File::options().append(true).open(log_path).unwrap();

    writeln!(&mut file, "{msg}").unwrap();
    file.flush().unwrap();
}

unsafe fn read_null_terminated_lst<'a>(lst: *const *const c_char) -> &'a [*const c_char] {
    if lst.is_null() {
        // Empty list.
        return &[];
    }

    let mut i = 0;
    while !unsafe { lst.add(i).read() }.is_null() {
        i += 1;
    }
    unsafe { core::slice::from_raw_parts(lst, i) }
}

unsafe fn iter_null_terminated_lst<'a>(
    argv: *const *const c_char,
) -> impl Iterator<Item = &'a CStr> + Clone {
    unsafe { read_null_terminated_lst(argv) }
        .iter()
        .map(|&ptr| unsafe { CStr::from_ptr(ptr) })
}

/// The returned `*const c_char` string is valid for as long as the `envp` as
/// well as the returned storage is kept alive.
pub unsafe fn override_env(
    project_local_tmpdir: &Path,
    envp: *const *const c_char,
) -> (Vec<CString>, Vec<*const c_char>) {
    // Storage location for new strings.
    let mut storage: Vec<CString> = Vec::new();

    // Remove `cargo-sandbox-interceptor` from `DYLD_INSERT_LIBRARIES`, to
    // avoid trying to apply the sandbox on process invocations that `rustc`
    // or build scripts perform.
    let mut env: Vec<*const c_char> = unsafe { iter_null_terminated_lst(envp) }
        .map(|cstr| {
            if let Some(libraries) = cstr.to_bytes().strip_prefix(b"DYLD_INSERT_LIBRARIES=") {
                let mut new = Vec::from(b"DYLD_INSERT_LIBRARIES=");
                for (i, lib) in libraries
                    .split(|c| *c == b':')
                    .filter(|lib| !lib.ends_with(b"cargo-sandbox-interceptor"))
                    .enumerate()
                {
                    if i != 0 {
                        new.push(b':');
                    }
                    new.extend(lib);
                }
                let new = CString::new(new).unwrap();
                let ptr = new.as_ptr();
                storage.push(new);
                ptr
            } else {
                cstr.as_ptr()
            }
        })
        .collect();

    // Insert or replace environment variable.
    let mut replace_env = |name, with: CString| {
        if let Some((i, _)) = unsafe { iter_null_terminated_lst(env.as_ptr()) }
            .enumerate()
            .find(|(_, cstr)| cstr.to_bytes().starts_with(name))
        {
            env[i] = with.as_ptr();
        } else {
            env.push(with.as_ptr());
        }
        storage.push(with);
    };

    // WARNING: It is not safe to allow arbitrary read+write to the user's
    // temporary directory returned by `std::env::temp_dir()`, as this might
    // contain sensitive in-flight data from other applications that have a
    // more relaxed threat model, and we really don't want malicious code to
    // have access to that.
    //
    // Instead, we set the `TMPDIR` env var this to a project-local directory,
    // which should make programs use this directory for their temporary file
    // operations instead. Programs that do not check this environment
    // variable and instead try to read/write directly to `/tmp` or
    // `confstr(_CS_DARWIN_USER_TEMP_DIR)` will fail, and will have to be
    // trusted manually by the user.
    if cfg!(windows) {
        // NOTE: On Windows, there's two environment variables that control
        // the temporary directory, `TMP` and `TEMP`, see `GetTempPath2`. We
        // set both of them for consistency with programs that only read one.
        replace_env(
            b"TMP",
            CString::new(format!("TMP={}", project_local_tmpdir.display())).unwrap(),
        );
        replace_env(
            b"TEMP",
            CString::new(format!("TEMP={}", project_local_tmpdir.display())).unwrap(),
        );
    } else {
        replace_env(
            b"TMPDIR",
            CString::new(format!("TMPDIR={}", project_local_tmpdir.display())).unwrap(),
        );
    }

    // End the list.
    env.push(ptr::null());

    (storage, env)
}
