use config::SandboxConfig;
use core::ffi::{CStr, c_char, c_int, c_void};
use core::fmt::Display;
use core::ptr;
use init::{get_policy, run_sandbox_init};
use std::ffi::{CString, OsStr, OsString};
use std::fs::{File, create_dir_all};
use std::io::Write;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

mod config;
mod ffi;
mod init;
mod proc_pidinfo;

static CARGO_HOME: OnceLock<PathBuf> = OnceLock::new();
static RUSTUP_HOME: OnceLock<PathBuf> = OnceLock::new();
static CONFIG: OnceLock<SandboxConfig> = OnceLock::new();
static DEVELOPER_DIR: OnceLock<PathBuf> = OnceLock::new();

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
    CARGO_HOME.set(cargo_home).unwrap();
    RUSTUP_HOME.set(rustup_home).unwrap();
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
    DEVELOPER_DIR.set(PathBuf::from(developer_dir)).unwrap();
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
    let policy = get_policy(config, &project_local_tmpdir, &kind);

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
    let policy = get_policy(config, &project_local_tmpdir, &kind);

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

/// How we should sandbox the process that is about to be spawned.
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
enum ProcessKind<'a> {
    BuildScript {
        package: &'a str,
        target_dir: &'a Path,
        // TODO: Add source (git or registry).
        manifest_dir: &'a Path,
    },
    Rustc {
        package: &'a str,
        externs: Vec<&'a str>,
        target_dir: &'a Path,
        manifest_dir: &'a Path,
    },
    Other,
}

impl<'a> ProcessKind<'a> {
    fn parse(
        bin: &'a CStr,
        args: impl Iterator<Item = &'a CStr> + Clone,
        env: impl Iterator<Item = &'a CStr> + Clone,
    ) -> Self {
        // TODO: Add test that the user can't overwrite these weirdly.
        let bin = Path::new(OsStr::from_bytes(bin.to_bytes()));

        if bin.file_name().unwrap() == "build-script-build" {
            // TODO: Or maybe use `CARGO_PKG_NAME`? Or can that be overwritten by
            // build scripts / `.cargo/config.toml` too? If so, then we probably
            // cannot use that.
            //
            // TODO: Read from `CARGO_MANIFEST_PATH` to determine what source the
            // build script is from (git or registry).
            let build_dir = bin.parent().unwrap().file_name().unwrap();
            let build_dir = build_dir.to_str().unwrap();
            let (package, _unique_suffix) = build_dir.rsplit_once("-").unwrap();
            let target_dir = bin
                .ancestors()
                .find(|dir| dir.file_name() == Some(OsStr::new("target")))
                .unwrap();

            let manifest_dir = env
                .clone()
                .find_map(|env| env.to_bytes().strip_prefix(b"CARGO_MANIFEST_DIR="))
                .map(|bin| Path::new(OsStr::from_bytes(bin)))
                .unwrap_or_else(|| {
                    panic!(
                        "failed finding CARGO_MANIFEST_DIR in {:#?}",
                        args.clone().collect::<Vec<_>>()
                    )
                });

            ProcessKind::BuildScript {
                package,
                target_dir,
                manifest_dir,
            }
        } else if bin.file_name().unwrap() == "rustc" {
            let first_arg = args.clone().skip(1).next();
            if first_arg == Some(c"-vV")
                || first_arg == Some(c"-")
                || first_arg == Some(c"--print=target-spec-json")
            {
                return ProcessKind::Other;
            }

            // let arg = args
            //     .skip_while(|arg| !arg.to_bytes().starts_with(b"--edition"))
            //     .skip(1)
            //     .next()
            //     .expect("must have edition arg and the `lib.rs` right after");
            //
            // TODO: This is not safe, build scripts could have modified this!
            // But I don't currently see a better way of doing it.
            let package = env
                .clone()
                .find_map(|env| env.to_bytes().strip_prefix(b"CARGO_PKG_NAME="))
                .unwrap_or_else(|| {
                    panic!(
                        "failed finding CARGO_PKG_NAME in {:#?}",
                        args.clone().collect::<Vec<_>>()
                    )
                });
            let package = str::from_utf8(package).unwrap();

            let mut externs = vec![];
            let mut out_dir = None;
            let mut args = args.clone();
            while let Some(arg) = args.next() {
                if arg == c"--extern" {
                    let extern_ = args.next().unwrap();
                    let extern_ = extern_.to_str().unwrap();
                    // NOTE: We're blindly trusting the name here!
                    let extern_ = if let Some((package, _path)) = extern_.split_once("=") {
                        package
                    } else {
                        extern_
                    };
                    externs.push(extern_);
                }
                if arg == c"--out-dir" {
                    out_dir = Some(Path::new(OsStr::from_bytes(
                        args.next().unwrap().to_bytes(),
                    )));
                }
            }
            let target_dir = out_dir
                .unwrap_or_else(|| {
                    Path::new(OsStr::from_bytes(
                        env.clone()
                            .find_map(|env| env.to_bytes().strip_prefix(b"OUT_DIR="))
                            .unwrap(),
                    ))
                })
                .ancestors()
                .find(|dir| dir.file_name() == Some(OsStr::new("target")))
                .unwrap();

            let manifest_dir = env
                .clone()
                .find_map(|env| env.to_bytes().strip_prefix(b"CARGO_MANIFEST_DIR="))
                .map(|bin| Path::new(OsStr::from_bytes(bin)))
                .unwrap_or_else(|| {
                    panic!(
                        "failed finding CARGO_MANIFEST_DIR in {:#?}",
                        args.clone().collect::<Vec<_>>()
                    )
                });

            ProcessKind::Rustc {
                package,
                externs,
                target_dir,
                manifest_dir,
            }
        } else {
            // TODO: Maybe enforce either Cargo or rustup here?
            ProcessKind::Other
        }
    }

    fn package(&self) -> &'a str {
        match self {
            Self::BuildScript { package, .. } => package,
            Self::Rustc { package, .. } => package,
            Self::Other => unreachable!(),
        }
    }

    fn target_dir(&self) -> &'a Path {
        match self {
            Self::BuildScript { target_dir, .. } => target_dir,
            Self::Rustc { target_dir, .. } => target_dir,
            Self::Other => unreachable!(),
        }
    }

    fn manifest_dir(&self) -> &'a Path {
        match self {
            Self::BuildScript { manifest_dir, .. } => manifest_dir,
            Self::Rustc { manifest_dir, .. } => manifest_dir,
            Self::Other => unreachable!(),
        }
    }
}

#[track_caller]
fn log_msg(msg: impl Display) {
    // NOTE: We cannot log to stdout/stderr in here, that interferes with
    // Cargo's `exec_with_streaming`.
    let mut file = File::options()
        .create(true)
        .append(true)
        .write(true)
        .open(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join("target/log.txt"),
        )
        .unwrap();

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

    // Remove `libinterpose_sandbox.dylib` from `DYLD_INSERT_LIBRARIES`, to
    // avoid trying to apply the sandbox on process invocations that `rustc`
    // or build scripts perform.
    let mut env: Vec<*const c_char> = unsafe { iter_null_terminated_lst(envp) }
        .map(|cstr| {
            if let Some(libraries) = cstr.to_bytes().strip_prefix(b"DYLD_INSERT_LIBRARIES=") {
                let mut new = Vec::from(b"DYLD_INSERT_LIBRARIES=");
                for (i, lib) in libraries
                    .split(|c| *c == b':')
                    .filter(|lib| !lib.ends_with(b"libinterpose_sandbox.dylib"))
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
