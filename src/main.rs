use anstream::{eprint, eprintln};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::sync::mpsc::channel;
use std::thread;

use log_analyzer::SandboxLogKind;

mod log_analyzer;

fn main() -> ExitCode {
    // Ignore arg0.
    let mut args = env::args_os().skip(1).peekable();

    // If called under Cargo as `cargo sandbox xyz`.
    if args.peek().map(|s| &**s) == Some(OsStr::new("sandbox")) {
        // TODO: Provide configuration option to allow this usage.
        eprintln!("must be invoked as `cargo-sandbox` for TODO reason");
        return ExitCode::FAILURE;
    }

    // Find the `cargo-sandbox-helper` binary.
    let current_bin = env::current_exe().expect("must be able to get the current executable");
    let mut helper_dylib = current_bin.with_file_name("cargo-sandbox-helper");
    // Some platforms' executable files have extensions (e.g. Windows).
    helper_dylib.set_extension(std::env::consts::EXE_EXTENSION);

    // Find the Cargo binary to call.
    //
    // TODO: Implement some sort of modified `rustup` searching, to avoid
    // malicious `rust-toolchain.toml`.
    let cargo = if let Some(cargo) = env::var_os("CARGO")
        && let cargo = PathBuf::from(cargo)
        && cargo.file_stem() != Some(OsStr::new("cargo-sandbox"))
    {
        cargo
    } else {
        // TODO: Maybe invoke `rustup which cargo` here, to avoid having to
        // "carry forwards" the interposition library.
        PathBuf::from(OsString::from("cargo"))
    };

    // Append interposition lib to `DYLD_INSERT_LIBRARIES` / `LD_PRELOAD`.
    let env_name = if cfg!(target_os = "macos") {
        "DYLD_INSERT_LIBRARIES"
    } else {
        "LD_PRELOAD"
    };
    let mut storage = env::var_os(env_name);
    let insert_libs = if let Some(libs) = &mut storage {
        libs.push(":");
        libs.push(&helper_dylib);
        libs
    } else {
        helper_dylib.as_os_str()
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
            .env(env_name, insert_libs)
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
                    eprintln!("hit sandbox restriction in `{package}`'s build script:");
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
