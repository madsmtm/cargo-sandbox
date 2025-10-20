use std::path::PathBuf;

use ui_test::{Config, custom_flags::Flag, run_tests, spanned::Spanned};

fn main() -> ui_test::color_eyre::Result<()> {
    let mut config = Config::rustc("tests/ui");

    // TODO: Why is the ctrlc-check necessary?
    let abort_check = config.abort_check.clone();
    ctrlc::set_handler(move || abort_check.abort())?;

    // config.custom_comments.insert(
    //     "sandbox-run",
    //     |parser, args, span| match Sandbox::from_str(&args) {
    //         Ok(kind) => {
    //             parser.add_custom(key, custom);
    //             parser.set_custom_once(
    //                 "run",
    //                 Run {
    //                     exit_code,
    //                     output_conflict_handling: None,
    //                 },
    //                 args.span(),
    //             );
    //         }
    //         Err(err) => parser.error(args.span(), err),
    //     },
    // );

    // Default to successfully compiling.
    config.comment_defaults.base().exit_status = Some(Spanned::dummy(0)).into();
    config.comment_defaults.base().require_annotations = Some(Spanned::dummy(false)).into();

    config
        .comment_defaults
        .revisioned
        .entry(vec!["sandboxed".into()])
        .or_default()
        .add_custom("sandbox-run", Sandbox);

    run_tests(config)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Sandbox;

impl Flag for Sandbox {
    fn clone_inner(&self) -> Box<dyn Flag> {
        Box::new(self.clone())
    }

    fn apply(
        &self,
        _cmd: &mut std::process::Command,
        _config: &ui_test::per_test_config::TestConfig,
        _build_manager: &ui_test::build_manager::BuildManager,
    ) -> Result<(), ui_test::Errored> {
        todo!()
    }

    fn must_be_unique(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)]
struct SandboxAllow {
    network: bool,
    paths: Vec<PathBuf>,
}

impl Flag for SandboxAllow {
    fn clone_inner(&self) -> Box<dyn Flag> {
        Box::new(self.clone())
    }

    fn must_be_unique(&self) -> bool {
        false
    }
}
