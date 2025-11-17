# Threat model for sandboxing in Cargo

Cargo has various attack vectors that a malicous acter can use to execute code or exfiltrate data at compile time on the user's host machine.

The most prominent of these is probably [build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html), which (by design) allow arbitrary code execution at build time, usually to invoke a C compiler or similar.

Additionally, the Rust compiler is in itself a huge piece of software that may have security issues. Examples include:
- [Procedural macros](https://doc.rust-lang.org/reference/procedural-macros.html) are `dlopen`ed in the compiler's process and allow arbitrary code execution.
- File reading functionality such as `include_str!("/etc/passwd")` or `#[path = "..."]`. See also [RFC 2794](https://github.com/rust-lang/rfcs/pull/2794).
- A buffer overflow in e.g. LLVM might allow malicous code to break out of any local sandbox that `rustc` were to implement.

Finally, local configuration files (`.cargo/config.toml`, `rust-toolchain.toml` and local `Cargo.toml`, though not dependencies' `Cargo.toml`) allow things such as overriding the linker (with e.g. a local script) or the Rust toolchain (potentially downgrading to a less secure version). See also [RFC 3279](https://github.com/rust-lang/rfcs/pull/3279).

All of these are within scope of this threat model.


## Anatomy of a security attack

### Data access

The primary thing we want to avoid is for a malicous party to gain access to sensitive data.

- General user data (username, system version, installed packages, current terminal/editor, usage history etc.)
- Private user data (private keys, personal data, source code for other projects etc.)

This includes things like accessing connected (host machine capabilities); while you can do a lot of _damage_,

### Exfiltration

Generally.

Without being able to exfiltrate the , the , which is a denial of service issue, but not necessarily a _security_ issue.

By limiting data access, we thereby limit the amount of damage one can do.

- Usually needs some form of network access.
- But also possible in more limited forms with e.g. embedding sensitive data in a binary, and then getting access to said binary.


### Persistence

TODO.

### Escalation

TODO.


Requirements:
- We want to give `cargo` the ability to do network access etc., since it needs that for e.g. `cargo update`.
- All spawned `rustc` (and later linker `cc`/`ld`) processes should have restricted permissions (to sandbox proc-macros).
  - Should allow target dir ofc.
  - Ideally some way to change the sandbox depending on passed proc-macro.
  - RUSTC_WRAPPER?
- Sandbox rustc (needed for proc-macros)
- Build scripts should be sandboxed slightly differently from `rustc`.
  - Cargo runner?
- Tests / the final binary with `cargo run` should again be handled differently.
- Should work with the `rustup` trampolines.
- Should work with Cargo things like `[target.x] runner = "..."` or `RUSTC_WRAPPER` (though less of a requirement).
- Should work with invocations like `cargo script`.
- Should protect against buffer overflows etc. in `rustc` or LLVM that the user can exploit.
- Make detection / allowing easy.

Assumptions:
- The user's current environment variables aren't malicious.
  - Pre-existing `LD_PRELOAD` and `DYLD_INSERT_LIBRARIES` env vars are intentional and don't need to be stripped.
- The user's current environment variables contain no sensitive data.
  - `PATH` in particular, read, stat and execute access is given to all files in this by default.
  - TODO: `SSH_AUTH_SOCK`?
- Project-local `./.cargo/config.toml` is safe.
  - Possibly problematic things include:
    - `paths = [...]`
    - `[alias] build = "build --config=sandbox.enable=false"`.
    - `[doc] browser = "malicious"`.
    - `[sandbox]`.
    - `[env]` table. Probably fine, these do not apply to Cargo itself, only to spawned subprocesses. At least it's fine as long as we sandbox the processes that get these env vars (it's not fine if you could add `DYLD_INSERT_LIBRARIES` and have e.g. `cargo help` execute that while spawning `man`/`less`).
    - `[credential-alias]`?
    - `[http]`.
    - `[install]`.
    - `[net.ssh]`.
    - `[registry]` / `[source.*]`.
  - `[profile]` and `[target]` settings are fine.
  - We could add a `sandbox.allow-projects = ["/path/to/project"]` that can be specified in user's home dir.
  - And maybe have a whitelist of allowed keys in project-local configs?
- Whatever the user specifies in `$CARGO_HOME/config.toml` is correct.
  - Things like `allow-paths = ["~/.ssh"]` is clearly not, but we'll allow it anyhow.
- The system's sandboxing mechanism is sound.
- Something sandboxed would still be bound by the sandbox.

Allow by default:
- Philosophy: Allowlist, not denylist, much more robust.
- Read + execute in `$CARGO_HOME/bin`, `~/.rustup/toolchains/*/bin` and `PATH`.
- Read in registries `$CARGO_HOME/registry/`.
  - And execute? This would allow prebuilt binaries from registries.
- Read/execute in `build.target-dir`.
- `rustc`: Write in `build.target-dir`.
- Build scripts: Allow write in `OUT_DIR`.
- Read project dir.
- TODO: Allow read in `$PATH/../lib`? This would allow linking Homebrew libraries (like maybe OpenSSL?) in build scripts and proc-macros.
- TODO: What's needed for `pkg-config`?
- `build.rustc/rustc-wrapper/rustc-workspace-wrapper/rustdoc`?
- Only in integration tests: Allow read/write in `CARGO_TARGET_TMPDIR`.
- macOS:
  - `xcode-select --print-path`/`DEVELOPER_DIR` and `SDKROOT`.

Definitely needs to be denied:
- Read `$CARGO_HOME/credentials.toml`, as this may contain secrets.

Supported operations:
- `cargo check`.
- `cargo build`.
- `cargo doc` (TODO).
- `cargo fmt` (TODO).
- `cargo clippy` (TODO).

Out of scope:
- Tools installed with `cargo install`.
- Security of metadata fields used by other tools.
- `cargo test`/`cargo run`/`cargo bench` (for now at least)
  - Running these are always the result of an explicit user action.
  - A large attack vector here would be `#[global_allocator]`, `ctor`s and similar global stuff and linker magic.
- `rust-toolchain.toml` overrides. These are problematic, see e.g. [`mallory`](https://github.com/jonas-schievink/mallory), but this problem cannot be handled by Cargo (since it happens before Cargo is even invoked).
- `cargo publish`, this copies symlinked things, e.g. file a symlinked to `~/.ssh`.
- Other configuration that makes Cargo's read/write files inside its process space
  - Might be problematic: `package.workspace`, `[patch]`, `dependencies.*.path`, `workspace.members`?
  - Keys that are probably safe: `package.readme`, `package.license-file`, `package.build`, `package.exclude/include`.
- Make build scripts and proc-macros more reproducible by having them depend less on the environment.
- Denial of service attacks like system OOM or exhausting available disk space.
- Embedded `#![debugger_visualizer(...)]` scripts (the security of these are the responsibility of the debugger).
