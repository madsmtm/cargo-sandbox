# Interposition helper

Note: Interposing don't work with system binaries like `/bin/sh` and `/usr/bin/cc` (the latter of which `rustc` calls to link) or binaries using a hardened runtime. But that's fine, we only intend on using this on the `cargo` (and `rustup-init`) binary.

## Building

```sh
# Build for Aarch64 and x86_64
cargo +nightly build --package interpose-sandbox --profile interpose-sandbox -Zbuild-std=std,panic_abort -Zbuild-std-features= --target aarch64-apple-darwin --target x86_64-apple-darwin

# Combine the two into one binary.
lipo -create target/aarch64-apple-darwin/interpose-sandbox/libinterpose_sandbox.dylib target/x86_64-apple-darwin/interpose-sandbox/libinterpose_sandbox.dylib -output target/libinterpose_sandbox.dylib

# Use as follows:
env DYLD_INSERT_LIBRARIES=$(pwd)/target/libinterpose_sandbox.dylib cargo check

# Or when just testing:
cargo build -p interpose-sandbox && install_name_tool -id /opt/lib/lib$(uuidgen).dylib ./target/debug/libinterpose_sandbox.dylib
env DYLD_INSERT_LIBRARIES=$(pwd)/target/debug/libinterpose_sandbox.dylib cargo check

cargo build -p interpose-sandbox && cp ./target/debug/libinterpose_sandbox.dylib ./libinterpose_sandbox.dylib && cargo clean && env DYLD_INSERT_LIBRARIES=$(pwd)/libinterpose_sandbox.dylib cargo build -pcargo-sandbox
```

This uses a few tricks from [`min-sized-rust`](https://github.com/johnthagen/min-sized-rust) to make the produced `libinterpose_sandbox.dylib` much smaller, which is important because we ship it pre-compiled in `cargo-sandbox` on crates.io TODO.

## HACK

`cargo install` has no facility for installing libraries (which this really is), so we work around it by installing this as `~/.cargo/bin/interpose-sandbox`, and trick Cargo into thinking it's a binary (while we really pass `-dylib` to the linker via `build.rs`).

This allows `cargo install interpose-sandbox` to "work".

TODO: Versioning?
`CARGO_INSTALL_ROOT`

Alternative:
- Ship `src/lib.rs` inside `cargo-sandbox` (+inline dependencies)
- Unpack it into temporary directory on run.
- Build using `rustc`, and re-use that built dylib.
  - TODO: Versioning? Maybe explicit `dylib` versioning
  - `vtool -set-source 10 -replace -output $file $file` + later `vtool -show-source $file`?
  - Alterantive: `-compatibility_version`/`-current_version` linker flags.
