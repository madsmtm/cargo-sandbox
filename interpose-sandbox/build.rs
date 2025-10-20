fn main() {
    // The script doesn't depend on our code.
    println!("cargo:rerun-if-changed=build.rs");

    // Make the produced binary actually be a dylib.
    println!("cargo:rustc-link-arg-bins=-Wl,-dylib");
}
