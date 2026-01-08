fn main() {
    // The build script doesn't depend on our code.
    println!("cargo:rerun-if-changed=build.rs");

    let target_vendor = std::env::var_os("CARGO_CFG_TARGET_VENDOR").unwrap();

    // HACK: Make the interceptor binary actually be a shared dylib, instead of
    // an executable (Cargo only supports installing executables).
    if target_vendor == "apple" {
        println!("cargo:rustc-link-arg-bin=cargo-sandbox-interceptor=-Wl,-dylib");
    } else {
        println!("cargo:rustc-link-arg-bin=cargo-sandbox-interceptor=-shared");
        println!("cargo:rustc-link-arg-bin=cargo-sandbox-interceptor=-no-pie");
    }

    // Embed the current version in the interceptor.
    // TODO: Should we check this against `cargo-sandbox`'s current version?
    let version = env!("CARGO_PKG_VERSION");
    if target_vendor == "apple" {
        // Read with: `otool -l target/debug/cargo-sandbox-interceptor | sed -n '/cmd LC_ID_DYLIB/,/cmd /p'`
        println!(
            "cargo:rustc-link-arg-bin=cargo-sandbox-interceptor=-Wl,-current_version,{version}"
        );
    } else {
        // Read with: `readelf -d target/debug/cargo-sandbox-interceptor | grep SONAME`
        println!(
            "cargo:rustc-link-arg-bin=cargo-sandbox-interceptor=-Wl,-soname,libcargo_sandbox_interceptor.so.{version}"
        );
    }
}
