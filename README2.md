# Sandboxing Cargo

Goal: Allow fearlessly running `cargo build` within an arbitrary untrusted Rust project.

Attacks:
- Data exfiltration
  - General user data (username, system version, installed packages, current terminal/editor, usage history etc.)
    - If network access is enabled, this cannot be prevented.
  - Private user data (personal data, source code for other projects, keys etc.)
- Persistence
  - TODO: Prevent malware persistence.

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

Out of scope:
- Tools installed with `cargo install`.
- Security of metadata fields used by other tools.
- `cargo test`/`cargo run`/`cargo bench` (for now at least). Running these are always the result of an explicit user action.
  - A large attack vector here would be `ctor`s.
- `rust-toolchain.toml` overrides. These are problematic, see e.g. [`mallory`](https://github.com/jonas-schievink/mallory), but this problem cannot be handled by Cargo (since it happens before Cargo is even invoked).
- `cargo publish`, this copies symlinked things, e.g. file a symlinked to `~/.ssh`.
- Other configuration that makes Cargo's read/write files inside its process space
  - Might be problematic: `package.workspace`, `[patch]`, `dependencies.*.path`, `workspace.members`?
  - Keys that are probably safe: `package.readme`, `package.license-file`, `package.build`, `package.exclude/include`.
- Make build scripts and proc-macros more reproducible by having them depend less on the environment.

## TODO

Trust roots; we'd probably trust packages from crates.io, but probably not git sources, or at least not by default.

`[hints]` keys to allow library authors to request certain sandboxing opt-outs?
- E.g. `hints.sandbox.build-script.allow-network = "message"` or `hints.sandbox.proc-macro.allow-network = "message"`.

Deterministic builds:
- Disallow Xcode?

How do we ensure that this stays secure?
- Feature additions to Cargo must have a `# Security` section.
- Help ensure process spawning and file system access is wrapped with `clippy.toml` deny methods.

Add an option to run as a different less-privileged user / w. ACLs?
- That would mostly resolve the `TMPDIR` shenanigans.
- See <https://developer.apple.com/library/archive/documentation/Security/Conceptual/AuthenticationAndAuthorizationGuide/Introduction/Introduction.html>

Networking:
- Socket vs. TCP/UDP, local vs. external.


## Platform details

### macOS

macOS uses `sandbox-exec` / `sandbox_init`, TODO.
