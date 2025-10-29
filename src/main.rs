use std::collections::{BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::{Command, ExitCode};
use std::sync::mpsc::channel;
use std::thread;

use log_analyzer::SandboxLogKind;

mod log_analyzer;

fn main() -> ExitCode {
    // Ignore arg0.
    let mut args = std::env::args_os().skip(1).peekable();

    // If called under Cargo as `cargo sandbox xyz`.
    if args.peek().map(|s| &**s) == Some(OsStr::new("sandbox")) {
        // TODO: Provide configuration option to allow this usage.
        eprintln!("must be invoked as `cargo-sandbox` for TODO reason");
        return ExitCode::FAILURE;
    }

    // Find the Cargo binary to call.
    //
    // TODO: Implement some sort of modified `rustup` searching, to avoid
    // malicious `rust-toolchain.toml`.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));

    // Unpack `interpose-sandbox` into temporary directory and build
    // `libinterpose_dylib.dylib`.
    //
    // TODO: Ensure that build scripts etc. can't write to said temporary dir.
    //
    // TODO: Maybe avoid this somehow by shipping `interpose-sandbox` differently?
    let cargo_sandbox_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cmd = Command::new(&cargo)
        .arg("build")
        .arg("--target")
        .arg(env!("HOST_TARGET"))
        .arg("--manifest-path")
        .arg(cargo_sandbox_dir.join("interpose-sandbox/Cargo.toml"))
        .status()
        .unwrap();
    assert!(cmd.success(), "failed building `interpose-sandbox`");
    let dylib_path = cargo_sandbox_dir.join("target/debug/libinterpose_sandbox.dylib");

    // Append interposition lib to `DYLD_INSERT_LIBRARIES`.
    // TODO: Prepend vs. append?
    let mut storage = std::env::var_os("DYLD_INSERT_LIBRARIES");
    let insert_libs = if let Some(libs) = &mut storage {
        libs.push(":");
        libs.push(&dylib_path);
        libs
    } else {
        dylib_path.as_os_str()
    };

    // Prepare logging sandbox output.
    let mut log_child = log_analyzer::stream_logs().unwrap();
    let (log_sender, log_receiver) = channel();

    thread::scope(|s| {
        // Read logs message on a separate thread. This might be unnecessary
        // here, but might be useful when integrating properly into Cargo,
        // where we'd want to associate a log message more directly with the
        // relevant process.
        let stdout = log_child.stdout.take().unwrap();
        let log_thread = s.spawn(|| {
            log_analyzer::parse_logs(stdout, log_sender).unwrap();
        });

        // Forward to the actual Cargo command.
        let status = Command::new(cargo)
            .args(args)
            .env("DYLD_INSERT_LIBRARIES", insert_libs)
            .status()
            .unwrap();

        log_child.kill().unwrap();

        log_thread.join().unwrap();

        let mut entries: HashMap<_, Vec<_>> = HashMap::new();
        for entry in log_receiver {
            // Merge messages with the same source.
            entries
                .entry((entry.kind, entry.package))
                .or_default()
                .push(entry.message);
        }
        for ((kind, package), messages) in entries {
            eprint!(
                "{}warning{}: ",
                CARGO_WARN.render(),
                CARGO_WARN.render_reset()
            );
            match kind {
                SandboxLogKind::Rustc => {
                    eprintln!("hit sandbox restriction in the compilation of `{package}`: ");
                }
                SandboxLogKind::BuildScript => {
                    eprintln!("hit sandbox restriction in `{package}`'s build script: ");
                }
            }
            // Deduplicate messages with the same source while keeping the
            // order they appeared in.
            let mut seen = BTreeSet::new();
            for message in messages {
                if seen.insert(message.clone()) {
                    eprintln!("         {message}");
                }
            }
        }

        if status.success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    })
}

const CARGO_WARN: anstyle::Style = anstyle::AnsiColor::Yellow
    .on_default()
    .effects(anstyle::Effects::BOLD);
