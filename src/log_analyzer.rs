//! Stream relevant sandbox logging messages.
//!
//! Note: We could've used the OSLog framework instead, but that's only
//! supported since macOS 10.15, while our minimum supported is macOS 10.12.
use core::fmt;
use std::{
    io::{self, BufRead, BufReader},
    process::{Child, ChildStdout, Command, Stdio},
    sync::mpsc::Sender,
};

use serde::Deserialize;

pub fn stream_logs() -> io::Result<Child> {
    // `interpose-sandbox` communicates the correct PID here by walking its
    // process hierarchy until it finds this process' PID. This allows us to
    // correctly filter messages to only be those that originate from this
    // invocation of `cargo-sandbox`.
    let current_pid = std::process::id();
    let predicate = format!(
        r#"process == "kernel" AND sender == "Sandbox" AND eventMessage CONTAINS "interpose-sandbox({current_pid}""#
    );

    // See `man log` for details on the parameters this takes.
    //
    // Beware: zsh has a builtin `log` function too, which conflicts with
    // this in daily use, so to make that clearer, we reference the full path
    // here (in case you need to copy the Debug output of the command).
    Command::new("/usr/bin/log")
        .arg("stream")
        .arg("--style")
        .arg("json")
        .arg("--predicate")
        .arg(predicate)
        .stdout(Stdio::piped())
        .spawn()
}

pub fn parse_logs(stdout: ChildStdout, sender: Sender<SandboxLogMessage>) -> io::Result<()> {
    let mut stdout = BufReader::new(stdout);

    // Skip first line, that just repeats the predicate.
    stdout.skip_until(b'\n')?;

    // Skip opening `[`.
    stdout.skip_until(b'[')?;

    // Parse each
    let mut json_data = Vec::new();
    loop {
        let n = stdout.read_until(b'}', &mut json_data)?;
        if n == 0 {
            break; // EOF
        }
        if json_data.ends_with(b"\n}") {
            let entry: LogEntry = serde_json::from_slice(&json_data)?;
            sender
                .send(entry.message)
                .expect("reciever was deallocated earlier than expected");
            json_data.clear();
            stdout.skip_until(b',')?;
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct LogEntry {
    #[serde(rename = "eventMessage")]
    message: SandboxLogMessage,
    // The level is probably always set to error, so don't bother parsing it.
}

#[derive(Debug)]
pub struct SandboxLogMessage {
    pub message: String,
    pub kind: SandboxLogKind,
    pub package: String,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SandboxLogKind {
    Rustc,
    BuildScript,
}

impl<'de> serde::Deserialize<'de> for SandboxLogMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SandboxLogEntryVisitor;

        impl<'de> serde::de::Visitor<'de> for SandboxLogEntryVisitor {
            type Value = SandboxLogMessage;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a proper sandbox logging message")
            }

            fn visit_str<E>(self, message: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                // The logging of sandbox events is done by the kernel, so all the
                // useful information is contained inside the event message. E.g. the
                // process identifier is always 0.
                let (sandbox_message, custom) =
                    message.rsplit_once("\ninterpose-sandbox(").unwrap();
                let sandbox_message = sandbox_message
                    .strip_prefix("Sandbox: ")
                    .unwrap_or(sandbox_message);
                let sandbox_message = sandbox_message
                    .split_once(" duplicate reports for Sandbox: ")
                    .map(|(_dups, message)| message)
                    .unwrap_or(sandbox_message);
                let sandbox_message = sandbox_message
                    .split_once(" duplicate report for Sandbox: ")
                    .map(|(_dups, message)| message)
                    .unwrap_or(sandbox_message);

                let custom = custom.strip_suffix(")").unwrap();
                let (_pid, custom) = custom.split_once(", ").unwrap();
                let (kind, package) = custom.split_once(", ").unwrap();

                let kind = match kind {
                    "rustc" => SandboxLogKind::Rustc,
                    "build-script" => SandboxLogKind::BuildScript,
                    _ => return Err(E::custom("failed parsing kind")),
                };

                Ok(SandboxLogMessage {
                    message: sandbox_message.to_string(),
                    kind,
                    package: package.to_string(),
                })
            }
        }

        deserializer.deserialize_str(SandboxLogEntryVisitor)
    }
}
