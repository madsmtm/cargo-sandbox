fn main() {
    // The script doesn't depend on our code.
    println!("cargo:rerun-if-changed=build.rs");

    // Try to set an environment variable that we use for passing sandbox data.
    // TODO: Change this to the actual variable.
    // println!("cargo::rustc-env=TMPDIR=/foo");

    // println!("cargo::rustc-env=DYLD_INSERT_LIBRARIES=123");
}
