// use std::process::Command;

fn main() {
    // The script doesn't depend on our code.
    println!("cargo:rerun-if-changed=build.rs");

    println!(
        "cargo::rustc-env=HOST_TARGET={}",
        std::env::var_os("TARGET").unwrap().display()
    );
}
