# Sandboxing Cargo

Goal: Allow fearlessly running `cargo-sandbox build` within an arbitrary untrusted Rust project.

This includes running build scripts and procedural macros for arbitrary untrusted dependencies.

[Obligatory XKCD](https://xkcd.com/2044/).


## Platforms

| Platform | Support | Sandboxing mechanism |
| -------- | ------- | -------------------- |
| macOS    | ⚠️      | `sandbox_init`       |
| Linux    | ❌      | TODO                 |
| FreeBSD  | ❌      | TODO                 |
| Windows  | ❌      | TODO                 |


## Setup

Install using:

```console
$ cargo install cargo-sandbox
```

This will install the binary `cargo-sandbox` and the helper dylib `cargo-sandbox-interceptor`.

If you intend to use this in untrusted projects (as opposed to just for untrusted dependencies), you probably also want to also run:
```console
$ rustup set auto-install disable
```

To avoid issues with a malicious project-local `rust-toolchain.toml` automatically installing a vulnerable or local toolchain.

(As a bonus, this will also make your `cargo` invocations slightly faster, see [rust-lang/rustup#2626](https://github.com/rust-lang/rustup/issues/2626)).


## Usage

```console
$ cd your_project
$ cargo-sandbox check
$ cargo-sandbox build --whatever-flag
$ cargo-sandbox +nightly doc
$ # etc.
```

This invokes `cargo $YOUR_CMD`, and will sandbox all `rustc` and build script invocations that Cargo makes.

TODO: Document how to use with `rust-analyzer`.


## Configuration

By default, `cargo-sandbox` will allow read and execute access to files in to the crate's directory, system files, as well as anything in `$PATH` or `$PATH/../lib`. This should allow most projects to build out of the box, but if you find that this is too restrictive, you can configure the sandboxing using the configuration file in `$CARGO_HOME/cargo-sandbox.toml` (usually `~/.cargo/cargo-sandbox.toml`).

Configuration can be global, scoped to a specific build script or scoped to a specific proc-macro (TODO: Actually the `rustc` invocation of dependencies).

An example configuration file could be:

```toml
# Proc macros from the `sqlx` crate requires local network access to the
# database for the `query!` macro to work.
# TODO: Should specify the actual proc-macro crate `sqlx-macros`.
[proc-macros.sqlx.network]
local = "allow"

# Allow global network access to `mylib-sys`'s build script, e.g. if it pulled
# in its contents from the network.
[build-scripts.mylib-sys.network]
global = "allow"

# Allow read access to some global `sdkconfig.defaults` that is set in
# `[env] ESP_IDF_SDKCONFIG_DEFAULTS=...` for the `esp-idf-sys` build script.
[[build-scripts.esp-idf-sys.paths]]
path = "/Users/username/sdkconfig.defaults"
read = "allow"

# When using a temporarily downloaded `zig cc` as the C compiler or linker, e.g.
# with `CC=zig cc` or `[build] linker = "~/Downloads/zig-cc-wrapper"`, allow
# read and execute access to it.
[[paths]]
path = "/Users/username/Downloads/zig"
read = "allow"
exec = "allow"

# Allow read and execute access to packages in Homebrew.
[[paths]]
path = "/opt/homebrew/bin"
read = "allow"
exec = "allow"
[[paths]]
path = "/opt/homebrew/lib"
read = "allow"
exec = "allow"

# Allow read and execute access to everything in the Nix store.
[[paths]]
path = "/nix/store"
read = "allow"
exec = "allow"

# Disable sandboxing of the `foo` package altogether.
[build-scripts.foo]
default = "allow"

# Allow read, write, execute, delete etc. access to ~/foo.
[[paths]]
path = "/Users/username/foo"
default = "allow"
# But deny access to ~/foo/subdir.
[[paths]]
path = "/Users/username/foo/subdir"
default = "deny"
```

The full format of the configuration file looks as follows:

```toml
# Whether sandboxing is globally enabled.
#
# allow -> Disable sandboxing.
# warn -> Disable sandboxing and warn on accesses that would have been denied.
# deny -> Enable sandboxing.
global = "allow" | "warn" | "deny"

# The global network configuration.
[network]
all = "allow" | "warn" | "deny"
# Allow access to the local network.
# TODO: Or just localhost?
local = "allow" | "warn" | "deny"
# Allow access to the public / global network.
global = "allow" | "warn" | "deny"

# Additional paths to globally allow access to.
#
# These are parsed in order, with later entries overriding earlier ones.
[[paths]]
# TODO: Use `shellexpand` or similar on these?
# TODO: Allow `~/` and crate-relative dirs to work.
path = <path>
# Configure access to the path in general.
default = "allow" | "warn" | "deny"
# Configure read access to files in the path.
read = "allow" | "warn" | "deny"
# Configure write access to files in the path.
write = "allow" | "warn" | "deny"
# Configure execute access to files in the path.
exec = "allow" | "warn" | "deny"


# Configure individual build-scripts.
#
# NOTE: This uses just the package name; we don't actually care about which
# version or the source of it (TODO: We do care about source), if the user has
# allowed network access for this package, they'll probably want to keep
# allowing that even across versions.
[build-scripts.<package-name>]
default = "allow" | "warn" | "deny"

[build-scripts.<package-name>.network]
# ... overrides [network] above ...

[[build-scripts.<package-name>.paths]]
# ... extends [[paths]] above ...


# Configure individual proc-macros.
#
# This is a bit weird, because the sandbox doesn't actually apply to the crate,
# **it applies to all dependent crates** of the crate.
#
# But maybe we can change this in the future? Maybe compile `proc-macros`
# differently, or have a shim helper that is sandboxed instead, and which
# dlopens the proc macro (instead of the compiler doing so), and then copies
# data to/from it? Something similar to `rust-analyzer`'s `proc-macro-srv`.
#
# That would probably be an okay trade-off between raw speed when unsandboxed,
# and still reasonable performance when sandboxed. And we wouldn't need a
# recompile / massively change how proc-macros work (at least on the surface).
[proc-macros.<package-name>]
default = "allow" | "warn" | "deny"

[proc-macros.<package-name>.network]
# ... overrides [network] above ...

[[proc-macros.<package-name>.paths]]
# ... extends [[paths]] above ...
```

Project-local configs (e.g. `$PROJECT_DIR/.cargo/cargo-sandbox.toml`) are not supported, as that'd allow recently cloned projects to just configure away the sandbox. We might allow this in the future if [RFC 3279](https://github.com/rust-lang/rfcs/pull/3279) progresses further.


## How it works

`cargo-sandbox` forwards its arguments to a new `cargo` subprocess, but with the `DYLD_INSERT_LIBRARIES` (macOS) or `LD_PRELOAD` (other unixes) environment variable set to the [`cargo-sandbox-interceptor`](./src/bin/cargo-sandbox-interceptor) dylib, which causes it to be injected into the `cargo` subprocess.

The interceptor then loads the configuration file, and intercepts all further process spawning that the `cargo` subprocess does (such as `std::process::Command::spawn` or `fork + execve`).

When the interceptor detects that the `cargo` subprocess is invoking `rustc` or a build script, the interceptor wraps this invocation in the system's sandboxing mechanism, according to the configuration loaded.

TODO: How can we allow the interceptor to communicate back to the main `cargo-sandbox` process (e.g. for tracing / debugging)? Maybe use `proc_pidinfo` to find the parent process' PID (the PID of `cargo-sandbox`), and match that against some socket-like thing? `socketpair`?


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

TODO: Learn Scheme

TODO: How does sandboxing work when dlopening stuff?

What's the difference between sandboxing and entitlements?
- At least that entitlements require provisioning profile.
- Entitlements are a way to enable some parts of sandboxing
  - And then selectively disable sandboxing for the areas you want.
  - The Hardened Runtime is probably just harsher resitrictions here?
    - `com.apple.security.hardened-process`?
  - Basically confirmed by: <https://book.hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-sandbox/index.html#custom-sbpl-in-app-store-apps>

Alternative:
- Create app with specific entitlements that launches the process we want to sandbox?
- Or maybe enable sandboxing `com.apple.security.app-sandbox` with entitlements for build scripts in Cargo.
  - Does the build script need to be bundled for that to work?

TODO: What about access to CPU hardware such as the [AMX](https://github.com/corsix/amx), the GPU, the [ANE](https://github.com/hollance/neural-engine) etc.? Is this restricted?

NOTE: Writing sandbox profile to disk first should result in a small speed-up if we can avoid re-writing to disk every time, since the `sandbox_init` function can then internally cache the compiled profile.

An alternative would be to just sandbox the entire `cargo` process itself (with a given configuration). This is a fair bit simpler, but Cargo would still have to:
1. Read cfg tomls.
2. Have access to keychain/`~/.cargo/config.toml` (for `cargo publish` token)

It also doesn't help much, in larger projects (which is where sandboxing becomes really important) you'd definitely want each build script's sandbox to be configurable.

## Limitations

With the current design, we cannot protect against a build script doing:
```rust
println!("cargo::rustc-env=DYLD_INSERT_LIBRARIES=");
```

And then later invoking a malicious proc-macro with such a `rustc` configuration (Cargo gives build scripts higher env var precedence than the user).

Similarly broken Cargo features:
- `cargo::rerun-if-changed=` - the path is not sandboxed.
- `cargo::rerun-if-env-changed=` - the env var is not sandboxed.
- `cargo::rustc-link-arg*` - the env var is not sandboxed.
- `cargo::rerun-if-env-changed=` - the env var is not sandboxed.
- `cargo::rerun-if-env-changed=` - the env var is not sandboxed.

CORRECTION: Actually, we only need to overwrite `DYLD_INSERT_LIBRARIES` _in the Cargo process_, so this is actually fine.

## Alternative designs

TODO.

## Resources

- Somebody already did this: <https://github.com/trailofbits/build-wrap>!!!
- Official docs: <https://developer.apple.com/documentation/xcode/configuring-the-macos-app-sandbox>
- Overview of sandboxing on macOS: <https://bdash.net.nz/posts/sandboxing-on-macos/>
- The format itself: <https://reverse.put.as/2011/09/14/apple-sandbox-guide-v1-0/>
- Documentation for TinyScheme: <https://sourceforge.net/p/tinyscheme/code/HEAD/tree/trunk/Manual.txt>
- Overview of `sandbox-exec`: <https://book.hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-sandbox/index.html>
- Sandboxing discussions:
  - Cargo: <https://github.com/rust-lang/cargo/issues/5720>
  - RFC repo: <https://github.com/rust-lang/rfcs/issues/1515>
  - Compiler: <https://github.com/rust-lang/compiler-team/issues/475>
  - Build.rs sandbox Project Goal: <https://github.com/rust-lang/rust-project-goals/issues/108>
- Background: <https://repo.zenk-security.com/Magazine%20E-book/The%20Mac%20Hacker's%20Handbook.pdf>
- Background: iOS Hackers Handbook
- Background: <https://docs.darlinghq.org/internals/macos-specifics/index.html>
- Other project: <https://old.reddit.com/r/rust/comments/101qx84/im_releasing_cargosandbox/>
- Other use of it in Rust: <https://github.com/phylum-dev/birdcage/blob/main/src/macos.rs>
- Sandboxing on Linux: <https://lib.rs/crates/seccompiler>
  - See also SELinux
  - Maybe <https://github.com/containers/bubblewrap> is the better option.
  - See also <https://github.com/AsahiLinux/muvm>
  - <https://lib.rs/crates/landlock>
- Background: <https://media.blackhat.com/bh-dc-11/Blazakis/BlackHat_DC_2011_Blazakis_Apple_Sandbox-wp.pdf>
- Background: <https://theapplewiki.com/wiki/Dev:Seatbelt>
- Examples: <https://github.com/s7ephen/OSX-Sandbox--Seatbelt--Profiles>
- Examples: <https://github.com/hellais/Buckle-Up>
- Examples: <https://www.mybyways.com/blog/run-code-in-a-macos-sandbox>
- Examples: <https://github.com/chromium/chromium/tree/main/sandbox/policy/mac>
- Examples: <https://hg-edge.mozilla.org/mozilla-central/file/tip/security/sandbox/mac>
  - <https://hg-edge.mozilla.org/mozilla-central/file/tip/security/mac/hardenedruntime>
- GUI: <https://github.com/maruchinu/BuckleUp>
- `man sandbox`
- `man sandbox_init`
- `man sandbox-exec`
- Examples: `ls /usr/share/sandbox`
- Examples: `ls /System/Library/Sandbox/Profiles`
- View logs: `/usr/bin/log stream --style compact --predicate 'process=="kernel" AND sender=="Sandbox"'`
- Further:   `/usr/bin/log stream --style compact --predicate 'process=="kernel" AND sender=="Sandbox" AND NOT eventMessage contains "searchpartyuseragent" AND NOT eventMessage contains "imagent"'`
  - You might wanna close other programs, it can be spammy.
- Debugging: <https://chromium.googlesource.com/chromium/src/+/main/docs/mac/sandbox_debugging.md>
- Bazel: <https://blog.bazel.build/2017/08/25/introducing-sandboxfs.html>
  - Also: <https://bazel.build/docs/sandboxing>
  - Source: <https://github.com/bazelbuild/bazel/blob/0972528f8d6b236afffa960da6b7c92d023b35df/src/main/java/com/google/devtools/build/lib/sandbox/DarwinSandboxedSpawnRunner.java#L274-L320>
- Nix: <https://nix.dev/manual/nix/2.32/command-ref/conf-file.html#conf-sandbox>
- OpenAI's Codex: <https://github.com/openai/codex/blob/main/docs/sandbox.md>
