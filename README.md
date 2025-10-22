Obligatory XKCD: <https://xkcd.com/2044/>

TODO: Learn Scheme

TODO: How does sandboxing work when dlopening stuff?

What's the difference between sandboxing and entitlements?
- At least that entitlements require provisioning profile.
- Entitlements are a way to enable some parts of sandboxing
  - And then selectively disable sandboxing for the areas you want.
  - The Hardened Runtime is probably just harsher resitrictions here?
    - `com.apple.security.hardened-process`?
  - Basically cofirmed by: <https://book.hacktricks.wiki/en/macos-hardening/macos-security-and-privilege-escalation/macos-security-protections/macos-sandbox/index.html#custom-sbpl-in-app-store-apps>

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

## Design

User configuration is read from ``.

Compile [`interpose-sandbox`](./interpose-sandbox) to a `.dylib`, and spawn the normal `cargo` process with it inserted as a `DYLD_INSERT_LIBRARIES`.

TODO: Place the sandboxing stuff in non-writable directories.

TODO: How do we pass configuration? Cannot be via environment variables, since the user can control that using `println!("rustc-env:...");` in a build script. Or maybe we can, does environment variables from the top-level override those?
- Happens to do so because Rust's `std` annoyingly overwrites `environ` instead of calling `execve`.
- Maybe use macOS specific things like mach bootstrap stuff instead?
  - `(allow file-read* (extension "com.apple.app-sandbox.read"))`
  - `(allow file-read* file-write* (extension "com.apple.app-sandbox.read-write"))`
  - To allow Cargo to "send" files.
- Maybe use `proc_pidinfo` to find parent process' PID (the PID of `cargo-sandbox`), and match that against some socket-like thing?
  - `socketpair`?
- Maybe interpose some other part of Cargo and do static initialization then. Would work weirdly, `interpose-sandbox` may be re-initialized several times in the process of finding the actual `cargo` executable. But maybe we can get around that by looking the exe up in `cargo-sandbox`? That would also simplify the question of what process spawns to sandbox (the answer would be "all" (maybe except the thing opened with `cargo doc --open`)).


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


## Blessing UI tests

```sh
cargo test --test ui -- --bless
```


## Testing

```sh
cargo install --path .
# In some other project:
cargo-sandbox build
```

```sh
cargo build -pcargo-sandbox
cp target/debug/cargo-sandbox ./cargo-sandbox
cargo clean
./cargo-sandbox build -pcargo-sandbox
```

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
