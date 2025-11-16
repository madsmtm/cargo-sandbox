use std::path::PathBuf;

use cargo_test_support::Project;

mod build_scripts;

// Grabbed from Cargo.
pub mod prelude {
    use std::path::PathBuf;

    pub use cargo_test_support::prelude::*;
    use cargo_test_support::{Execs, Project, compare};

    pub trait CargoProjectExt {
        fn cargo(&self, cmd: &str) -> Execs;
    }

    impl CargoProjectExt for Project {
        fn cargo(&self, cmd: &str) -> Execs {
            let cargo = cargo_exe();
            let mut execs = self.process(&cargo);
            execs.env("CARGO", cargo);
            execs.arg_line(cmd);
            execs
        }
    }

    pub fn cargo_exe() -> PathBuf {
        snapbox::cmd::cargo_bin!("cargo-sandbox").to_path_buf()
    }

    pub trait CargoCommandExt {
        fn cargo_ui() -> Self;
    }

    impl CargoCommandExt for snapbox::cmd::Command {
        fn cargo_ui() -> Self {
            Self::new(cargo_exe())
                .with_assert(compare::assert_ui())
                .env("CARGO_TERM_COLOR", "always")
                .env("CARGO_TERM_HYPERLINKS", "true")
                .test_env()
        }
    }
}

pub fn sandbox_config_file() -> PathBuf {
    cargo_test_support::paths::home().join(".cargo/cargo-sandbox.toml")
}

pub trait SandboxCommandExt {
    fn sandbox_config(&self, cmd: &str);
}

impl SandboxCommandExt for Project {
    fn sandbox_config(&self, config: &str) {
        self.change_file(
            sandbox_config_file(),
            &format!(
                r#"
                    global = "deny"
                    {config}
                "#
            ),
        )
    }
}
