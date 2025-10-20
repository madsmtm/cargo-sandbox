//! Copied from `include/sandbox.h`.

use std::ffi::{c_char, c_int};

#[link(name = "System")]
unsafe extern "C" {
    /*
     * @function sandbox_init
     * Places the current process in a sandbox with a profile as
     * specified.  If the process is already in a sandbox, the new profile
     * is ignored and sandbox_init() returns an error.
     *
     * @param profile (input)   The Sandbox profile to be used.  The format
     * and meaning of this parameter is modified by the `flags' parameter.
     *
     * @param flags (input)   Must be SANDBOX_NAMED.  All other
     * values are reserved.
     *
     * @param errorbuf (output)   In the event of an error, sandbox_init
     * will set `*errorbuf' to a pointer to a NUL-terminated string
     * describing the error. This string may contain embedded newlines.
     * This error information is suitable for developers and is not
     * intended for end users.
     *
     * If there are no errors, `*errorbuf' will be set to NULL.  The
     * buffer `*errorbuf' should be deallocated with `sandbox_free_error'.
     *
     * @result 0 on success, -1 otherwise.
     */
    #[deprecated = "No longer supported"]
    pub unsafe fn sandbox_init(
        profile: *const c_char,
        flags: u64,
        errorbuf: *mut *mut c_char,
    ) -> c_int;

    /*
     * @function sandbox_free_error
     * Deallocates an error string previously allocated by sandbox_init.
     *
     * @param errorbuf (input)   The buffer to be freed.  Must be a pointer
     * previously returned by sandbox_init in the `errorbuf' argument, or NULL.
     *
     * @result void
     */
    #[deprecated = "No longer supported"]
    pub unsafe fn sandbox_free_error(errorbuf: *mut c_char);
}

/// Undocumented constant, this makes the `profile` argument be a string to
/// the policy to apply.
pub const SANDBOX_STRING: u64 = 0x0000;
