use std::{
    ffi::{CStr, OsStr},
    os::unix::ffi::OsStrExt,
    path::Path,
};

/// How we should sandbox the process that is about to be spawned.
#[derive(PartialEq, Eq, Hash, Debug, Clone)]
pub enum ProcessKind<'a> {
    BuildScript {
        package: &'a str,
        target_dir: &'a Path,
        // TODO: Add source (git or registry).
        manifest_dir: &'a Path,
    },
    Rustc {
        package: &'a str,
        externs: Vec<&'a str>,
        target_dir: &'a Path,
        manifest_dir: &'a Path,
    },
    Other,
}

impl<'a> ProcessKind<'a> {
    pub fn parse(
        bin: &'a CStr,
        args: impl Iterator<Item = &'a CStr> + Clone,
        env: impl Iterator<Item = &'a CStr> + Clone,
    ) -> Self {
        // TODO: Add test that the user can't overwrite these weirdly.
        let bin = Path::new(OsStr::from_bytes(bin.to_bytes()));

        if bin.file_name().unwrap() == "build-script-build" {
            // TODO: Or maybe use `CARGO_PKG_NAME`? Or can that be overwritten by
            // build scripts / `.cargo/config.toml` too? If so, then we probably
            // cannot use that.
            //
            // TODO: Read from `CARGO_MANIFEST_PATH` to determine what source the
            // build script is from (git or registry).
            let build_dir = bin.parent().unwrap().file_name().unwrap();
            let build_dir = build_dir.to_str().unwrap();
            let (package, _unique_suffix) = build_dir.rsplit_once("-").unwrap();
            let target_dir = bin
                .ancestors()
                .find(|dir| dir.file_name() == Some(OsStr::new("target")))
                .unwrap();

            let manifest_dir = env
                .clone()
                .find_map(|env| env.to_bytes().strip_prefix(b"CARGO_MANIFEST_DIR="))
                .map(|bin| Path::new(OsStr::from_bytes(bin)))
                .unwrap_or_else(|| {
                    panic!(
                        "failed finding CARGO_MANIFEST_DIR in {:#?}",
                        args.clone().collect::<Vec<_>>()
                    )
                });

            ProcessKind::BuildScript {
                package,
                target_dir,
                manifest_dir,
            }
        } else if bin.file_name().unwrap() == "rustc" {
            let first_arg = args.clone().skip(1).next();
            if first_arg == Some(c"-vV")
                || first_arg == Some(c"-")
                || first_arg == Some(c"--print=target-spec-json")
            {
                return ProcessKind::Other;
            }

            // let arg = args
            //     .skip_while(|arg| !arg.to_bytes().starts_with(b"--edition"))
            //     .skip(1)
            //     .next()
            //     .expect("must have edition arg and the `lib.rs` right after");
            //
            // TODO: This is not safe, build scripts could have modified this!
            // But I don't currently see a better way of doing it.
            let package = env
                .clone()
                .find_map(|env| env.to_bytes().strip_prefix(b"CARGO_PKG_NAME="))
                .unwrap_or_else(|| {
                    panic!(
                        "failed finding CARGO_PKG_NAME in {:#?}",
                        args.clone().collect::<Vec<_>>()
                    )
                });
            let package = str::from_utf8(package).unwrap();

            let mut externs = vec![];
            let mut out_dir = None;
            let mut args = args.clone();
            while let Some(arg) = args.next() {
                if arg == c"--extern" {
                    let extern_ = args.next().unwrap();
                    let extern_ = extern_.to_str().unwrap();
                    // NOTE: We're blindly trusting the name here!
                    let extern_ = if let Some((package, _path)) = extern_.split_once("=") {
                        package
                    } else {
                        extern_
                    };
                    externs.push(extern_);
                }
                if arg == c"--out-dir" {
                    out_dir = Some(Path::new(OsStr::from_bytes(
                        args.next().unwrap().to_bytes(),
                    )));
                }
            }
            let target_dir = out_dir
                .unwrap_or_else(|| {
                    Path::new(OsStr::from_bytes(
                        env.clone()
                            .find_map(|env| env.to_bytes().strip_prefix(b"OUT_DIR="))
                            .unwrap(),
                    ))
                })
                .ancestors()
                .find(|dir| dir.file_name() == Some(OsStr::new("target")))
                .unwrap();

            let manifest_dir = env
                .clone()
                .find_map(|env| env.to_bytes().strip_prefix(b"CARGO_MANIFEST_DIR="))
                .map(|bin| Path::new(OsStr::from_bytes(bin)))
                .unwrap_or_else(|| {
                    panic!(
                        "failed finding CARGO_MANIFEST_DIR in {:#?}",
                        args.clone().collect::<Vec<_>>()
                    )
                });

            ProcessKind::Rustc {
                package,
                externs,
                target_dir,
                manifest_dir,
            }
        } else {
            // TODO: Maybe enforce either Cargo or rustup here?
            ProcessKind::Other
        }
    }

    pub fn package(&self) -> &'a str {
        match self {
            Self::BuildScript { package, .. } => package,
            Self::Rustc { package, .. } => package,
            Self::Other => unreachable!(),
        }
    }

    pub fn target_dir(&self) -> &'a Path {
        match self {
            Self::BuildScript { target_dir, .. } => target_dir,
            Self::Rustc { target_dir, .. } => target_dir,
            Self::Other => unreachable!(),
        }
    }

    pub fn manifest_dir(&self) -> &'a Path {
        match self {
            Self::BuildScript { manifest_dir, .. } => manifest_dir,
            Self::Rustc { manifest_dir, .. } => manifest_dir,
            Self::Other => unreachable!(),
        }
    }
}
