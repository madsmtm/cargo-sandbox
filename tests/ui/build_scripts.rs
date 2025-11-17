use crate::{SandboxCommandExt, prelude::*, sandbox_config_file};
use cargo_test_support::{Project, project};
use snapbox::str;

fn sandboxed_build_script(build_script: &str) -> Project {
    project()
        .file(
            sandbox_config_file(),
            r#"
                global = "deny"
            "#,
        )
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2024"

                [features]
                allow = []
            "#,
        )
        .file("src/lib.rs", "")
        .file("build.rs", build_script)
        .build()
}

#[cargo_test]
fn smoke() {
    let p = sandboxed_build_script("fn main() {}");
    p.cargo("check").run();

    p.change_file(
        sandbox_config_file(),
        r#"
            global = "allow"
        "#,
    );
    p.cargo("check").run();
}

/// Test with a build script that tries to read `/etc/passwd`.
#[cargo_test]
fn read_etc_passwd() {
    // Only run test if we'd normally be able to access `/etc/passwd`.
    if !std::fs::exists("/etc/passwd").unwrap_or(false) {
        return;
    }

    let p = sandboxed_build_script(
        r#"
            fn main() {
                let result = std::fs::read("/etc/passwd");
                if cfg!(feature = "allow") {
                    assert!(result.is_ok());
                } else {
                    let err = result.unwrap_err();
                    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
                }
            }
        "#,
    );

    // Allowed by default on macOS
    p.cargo("check --features allow").run();

    p.cargo("clean").run();
    p.sandbox_config(
        r#"
            [[build-scripts.foo.paths]]
            path = "/etc/passwd"
            read = "allow"
        "#,
    );
    p.cargo("check --features allow").run();

    p.cargo("clean").run();
    p.sandbox_config(
        r#"
            [[build-scripts.foo.paths]]
            path = "/etc/passwd"
            read = "deny"
        "#,
    );
    p.cargo("check").run();

    // Test config propagation.
    p.cargo("clean").run();
    p.sandbox_config(
        r#"
            [[paths]]
            path = "/etc/passwd"
            read = "allow"

            [[build-scripts.bar.paths]] # Some unrelated package
            path = "/etc/passwd"
            read = "deny"
        "#,
    );
    p.cargo("check --features allow").run();
}

/// Test with a build script that tries to run `ping github.com`.
#[cargo_test(public_network_test)]
fn network_access_sandboxed_ping_global() {
    if std::env::var_os("CI").is_some() {
        // GitHub Actions doesn't seem to support `ping`.
        return;
    }

    let p = sandboxed_build_script(
        r#"
            use std::process::Command;

            fn main() {
                let output = Command::new("ping")
                    .arg("-c")
                    .arg("1")
                    .arg("github.com")
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.success(),
                    cfg!(feature = "allow"),
                    "failed to properly sandbox: \n{}{}",
                    String::from_utf8(output.stdout).unwrap(),
                    String::from_utf8(output.stderr).unwrap(),
                );
            }
        "#,
    );

    p.cargo("check")
        .with_stderr_data(str![[r#"
...
[WARNING] hit sandbox restriction in `foo`'s build script:
         ping([..]) deny(1) network-outbound /private/var/run/mDNSResponder
         ping([..]) deny(1) file-read-data /private/etc/hosts

"#]])
        .run();

    p.cargo("clean").run();
    p.sandbox_config(
        r#"
            [build-scripts.foo.network]
            all = "allow"
        "#,
    );
    p.cargo("check --features allow").run();
}

/// Test with a build script that tries to run `curl https://api.github.com/`.
#[cargo_test(public_network_test)]
fn network_access_sandboxed_curl_global() {
    let p = sandboxed_build_script(
        r#"
            use std::process::Command;

            fn main() {
                let output = Command::new("curl")
                    .arg("https://api.github.com/")
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.success(),
                    cfg!(feature = "allow"),
                    "failed to properly sandbox: \n{}",
                    String::from_utf8(output.stderr).unwrap(),
                );
            }
        "#,
    );

    p.cargo("check")
        .with_stderr_data(str![[r#"
...
[WARNING] hit sandbox restriction in `foo`'s build script:
         curl([..]) deny(1) file-read-data /private/etc/ssl/openssl.cnf

"#]])
        .run();

    // Simply allowing `/etc/ssl/openssl.cnf` is not enough.
    p.cargo("clean").run();
    p.sandbox_config(
        r#"
            [[build-scripts.foo.paths]]
            path = "/etc/ssl/openssl.cnf"
            read = "allow"
        "#,
    );
    p.cargo("check")
        .with_stderr_data(str![[r#"
...
[WARNING] hit sandbox restriction in `foo`'s build script:
         curl([..]) deny(1) mach-lookup com.apple.SystemConfiguration.configd
         curl([..]) deny(1) file-read-data /Library/Preferences/com.apple.networkd.plist
         curl([..]) deny(1) necp-client-open
         curl([..]) deny(1) network-outbound /private/var/run/mDNSResponder
         curl([..]) deny(1) file-read-data /private/etc/hosts

"#]])
        .run();

    // We have to fully allow network access.
    p.cargo("clean").run();
    p.sandbox_config(
        r#"
            [build-scripts.foo.network]
            all = "allow"
        "#,
    );
    p.cargo("check --features allow").run();
}

/// Test a build script that sets various weird environment variables for the
/// rustc process.
#[cargo_test]
fn env_vars() {
    return; // TODO
    // let p = sandboxed_build_script(
    //     r#"
    //         fn main() {
    //             // Try to set an environment variable that we use for passing sandbox data.
    //             // TODO: Change this to the actual variable.
    //             println!("cargo::rustc-env=TMPDIR=123");
    //
    //             // Delete inserted libraries.
    //             println!("cargo::rustc-env=DYLD_INSERT_LIBRARIES=");
    //         }
    //     "#,
    // );
    // p.cargo("check").run();
}

#[cargo_test(public_network_test)]
fn cc() {
    let p = project()
        .file(
            sandbox_config_file(),
            r#"
                global = "deny"
            "#,
        )
        .file(
            "Cargo.toml",
            r#"
                [package]
                name = "foo"
                version = "0.1.0"
                edition = "2024"

                [features]
                allow = []

                [build-dependencies]
                cc = "1.0"
            "#,
        )
        .file("src/lib.rs", "")
        .file("foo.c", "int foo() { return 42; }")
        .file(
            "build.rs",
            r#"
                fn main() {
                    cc::Build::new().file("foo.c").compile("foo");
                }
            "#,
        )
        .build();

    // TODO: Make this also work with `--target aarch64-apple-ios` after
    // clearing the `xcrun` cache with `xcrun --kill-cache`.
    p.cargo("check")
        .with_stderr_does_not_contain("[WARNING]")
        .run();
}
