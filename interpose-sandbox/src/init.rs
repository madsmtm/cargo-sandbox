use std::{
    ffi::{CStr, CString, c_char},
    fmt::{self, Write},
    path::Path,
    ptr,
};

use crate::{
    CARGO_HOME, DEVELOPER_DIR, ProcessKind, RUSTUP_HOME,
    config::{SandboxOption, SandboxPackageConfig},
    proc_pidinfo::parent_cargo_sandbox_pid,
};

use super::ffi;

/// Apply the policy with `sandbox_init` after `fork` but before `exec`.
///
/// This is similar to what `/usr/bin/sandbox-exec` does.
///
/// # Deprecation
///
/// `sandbox_init` and `sandbox-exec` are officially deprecated in favour
/// of the app sandbox in entitlements. This deprecation has been the
/// status quo at least since macOS 10.8 (more than 10 years ago).
///
/// In theory, we might be able to use the app sandbox instead, but in
/// practice, it is too limited for us to make use of, as they require
/// codesigning and don't allow granting the fine-grained file permissions
/// that we need.
///
/// Besides, as far as I can tell, the app sandbox is intended for exploit
/// _mitigation_, while for our use-case we need to allow untrusted and
/// potentially malicious scripts to run safely, and while these may seem
/// similar, the security model is fairly different; our use-case needs a
/// lot more restrictions.
///
/// TODO: `man sandbox` says it must not be used for the purpose we're using
/// it for.
///
/// Furthermore, Apple actually internally implements the app sandbox by
/// looking at the presence of the `com.apple.security.app-sandbox`
/// entitlement, or the stricter `com.apple.security.hardened-process`, and
/// then uses the exact same mechanism as we use here to actually apply the
/// sandboxing.
///
/// As another data point, the sandboxing mechanism is widely used to
/// sandbox system binaries, just have a look inside `/usr/share/sandbox`
/// and `/System/Library/Sandbox/Profiles`. Both Chromium and Firefox also
/// use this "deprecated" functionality:
/// - Firefox: <https://wiki.mozilla.org/Sandbox/OS_X_Rule_Set>
/// - Chromium: <https://www.chromium.org/developers/design-documents/sandbox/osx-sandboxing-design/>
///
/// In conclusion, I think it's safe to say that the deprecation is mostly
/// there to nudge people towards the hardened runtime and entitlements
/// instead, and that Apple will continue supporting this API for the
/// forseeable future (and at the very least replace it with something
/// equivalent if they do decide to completely remove it).
pub fn run_sandbox_init(policy: &CStr) -> Result<(), SandboxError> {
    let mut error = ptr::null_mut();
    // SAFETY: The policy is a valid NUL-terminated string and the given
    // error reference is a valid writable place.
    #[allow(deprecated)]
    let result = unsafe { ffi::sandbox_init(policy.as_ptr(), ffi::SANDBOX_STRING, &mut error) };

    if result != 0 {
        // SAFETY: The error came from `sandbox_init`.
        let error = unsafe { SandboxError::new(error) };

        // TODO: Handle differently between failing to init because the
        // policy is invalid, and failing because we're already sandboxed.
        Err(error)
    } else {
        Ok(())
    }
}

#[derive(Debug)]
pub struct SandboxError(*mut c_char);

impl SandboxError {
    /// # Safety
    ///
    /// The error must have been set by `sandbox_init`.
    unsafe fn new(ptr: *mut c_char) -> Self {
        Self(ptr)
    }

    fn as_cstr(&self) -> &CStr {
        // We _should_ have gotten an error message, and it _should_ be safe
        // to read that directly as a `CStr`.
        //
        // It might theoretically fail e.g. due to out of memory conditions
        // though, so to be absolutely safe, we validate that the error was
        // actually set.
        if self.0.is_null() {
            c"<null>"
        } else {
            // SAFETY: The result code was non-zero, so the error is now
            // guaranteed to be a valid NULL-terminated string, at least
            // until `sandbox_free_error` is called (which we only do when
            // this type is dropped).
            unsafe { CStr::from_ptr(self.0) }
        }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed running sandbox_init: {}",
            self.as_cstr().to_string_lossy()
        )
    }
}

impl std::error::Error for SandboxError {}

impl Drop for SandboxError {
    #[allow(deprecated)]
    fn drop(&mut self) {
        // SAFETY: The error came from `sandbox_init`, and is guaranteed to be
        // valid until freed here.
        unsafe { ffi::sandbox_free_error(self.0) };
    }
}

pub fn get_policy(
    config: SandboxPackageConfig,
    project_local_tmpdir: &Path,
    kind: &ProcessKind,
) -> CString {
    // (with send-signal SIGFPE)
    // (with no-sandbox) can be placed on `process-exec`, disables sandbox
    //     when spawning those processes. Useful e.g. on privileged binaries
    //     like `/usr/sbin/bless` or `/sbin/mount`.
    // (with telemetry) ?
    // (with no-report) can be placed on `deny`
    // (with report) can be placed on `allow`
    // (with no-callout) ?
    // (with telemetry-backtrace) ?
    // (with partial-symbolication) ?
    // (with errno EACCES) ?
    let deny_with = "";

    let message = format!(
        "interpose-sandbox({}, {}, {})",
        parent_cargo_sandbox_pid(),
        if matches!(kind, ProcessKind::BuildScript { .. }) {
            "build-script"
        } else {
            "rustc"
        },
        kind.package(),
    );
    let user_temp_dir = std::env::temp_dir();
    let developer_dir = DEVELOPER_DIR.get().unwrap();
    let xcode_dir = if developer_dir.ends_with("Contents/Developer") {
        // /Applications/Xcode.app/Contents/Developer
        developer_dir.parent().unwrap().parent().unwrap()
    } else if developer_dir.ends_with("SDKs") {
        // /Library/Developer/CommandLineTools/SDKs.
        developer_dir.parent().unwrap()
    } else {
        developer_dir
    };

    let mut extra = String::new();
    for path in config.paths {
        // TODO: Use configuration values properly.
        let p = quote_path(&path.path);
        if let Some(option) = &path.default_ {
            writeln!(&mut extra, "({option} process-exec file* (subpath {p}))").unwrap();
        }
        if let Some(option) = &path.read {
            writeln!(&mut extra, "({option} file-read* (subpath {p}))").unwrap();
        }
        if let Some(option) = &path.write {
            writeln!(&mut extra, "({option} file-write* (subpath {p}))").unwrap();
        }
    }

    // NOTE: We _could_ replace some of this string interpolation with the
    // `sandbox_init_with_parameters` function instead - this is used by
    // Firefox, so even though it doesn't appear in any headers, it should
    // be reasonably safe to use. But we need to support passing an
    // arbitary amount of user-specified paths, and then it's just much
    // easier to generate the entire policy dynamically than to muck
    // around with quoting and string splitting in Scheme.
    CString::from_vec_with_nul(
        format!(
            concat!(include_str!("policy.sb"), "\0"),
            message = quote_str(&message),
            deny_with = deny_with,
            user_temp_dir = quote_path(&user_temp_dir),
            project_local_tmpdir = quote_path(project_local_tmpdir),
            rustup_home = quote_path(&RUSTUP_HOME.get().unwrap()),
            cargo_home = quote_path(&CARGO_HOME.get().unwrap()),
            manifest_dir = quote_path(kind.manifest_dir()),
            target_dir = quote_path(kind.target_dir()),
            xcode_dir = quote_path(xcode_dir),
            // TODO: Pass further options.
            allow_network = boolean(config.network.all == Some(SandboxOption::Allow)),
            extra = extra,
        )
        .into(),
    )
    .expect("policy contained interior NUL byte")
}

/// Quote a path as a string.
fn quote_path(path: &Path) -> impl fmt::Display {
    // All paths used when specifying sandboxing must be canonical.
    let path = path.canonicalize().unwrap_or_else(|_e| path.to_owned());
    // TODO: Proper quoting.
    format!("{path:?}")
}

/// Quote a string.
fn quote_str(s: &str) -> impl fmt::Display {
    // TODO: Proper quoting.
    format!("{s:?}")
}

/// Convert a boolean to a scheme boolean.
fn boolean(value: bool) -> impl fmt::Display {
    if value { "#t" } else { "#f" }
}
