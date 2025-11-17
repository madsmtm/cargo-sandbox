use core::fmt;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::ProcessKind;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(deny_unknown_fields)]
pub struct SandboxConfig {
    /// Whether sandboxing is globally enabled.
    #[serde(default)]
    pub global: Option<SandboxOption>,
    /// The global network configuration.
    #[serde(default)]
    pub network: SandboxNetworkConfig,
    /// Additional paths to globally allow access to.
    ///
    /// TODO: Use `shellexpand` or similar on these?
    #[serde(default)]
    pub paths: Vec<SandboxPathConfig>,

    /// A map of package names to configurations.
    #[serde(default)]
    pub build_scripts: HashMap<String, SandboxPackageConfig>,
    /// A map of package names to configurations.
    #[serde(default)]
    pub proc_macros: HashMap<String, SandboxPackageConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxPackageConfig {
    /// Whether sandboxing is enabled for this item.
    #[serde(rename = "default")]
    pub default_: Option<SandboxOption>,
    /// Whether / how to sandbox networking access.
    #[serde(default)]
    pub network: SandboxNetworkConfig,
    /// Additional paths to allow access to.
    ///
    /// TODO: Use `shellexpand` or similar on these?
    #[serde(default)]
    pub paths: Vec<SandboxPathConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxNetworkConfig {
    /// Whether to sandbox all network access.
    pub all: Option<SandboxOption>,
    /// Whether to sandbox localhost network access.
    pub local: Option<SandboxOption>,
    /// Whether to sandbox global/external network access.
    pub external: Option<SandboxOption>,
}

/// A path to allow read or write access to.
#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields)]
pub struct SandboxPathConfig {
    pub path: PathBuf,
    /// Allow/disallow access to the path in general.
    #[serde(rename = "default")]
    pub default_: Option<SandboxOption>,
    pub read: Option<SandboxOption>,
    pub write: Option<SandboxOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxOption {
    /// Enable sandboxing.
    ///
    /// -> (deny *)
    Deny,
    /// Disable sandboxing and warn on accesses that would have been denied.
    ///
    /// -> (allow * (with report))
    Warn,
    /// Disable sandboxing.
    ///
    /// -> (allow *)
    Allow,
}

impl fmt::Display for SandboxOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Deny => "deny",
            Self::Warn => "warn",
            Self::Allow => "allow",
        };
        write!(f, "{s}")
    }
}

impl SandboxConfig {
    /// Load the configuration from `$CARGO_HOME/cargo-sandbox.toml`.
    ///
    /// Example configuration:
    /// ```toml
    /// # Enable sandboxing. This must be explicitly opted into by the user,
    /// # as it would be too breaking otherwise.
    /// enable = true
    /// # Disallow network access for all crates. This is the default.
    /// allow-network = false
    /// # Allow specific paths. E.g. if the user uses non-standard binaries
    /// # for development, such as if they've set Homebrew's Clang as their
    /// # `CC` or linker.
    /// allow-paths = [
    ///     "/opt/homebrew/bin/clang",
    /// ]
    ///
    /// # Example configuration for e.g. a build script that pulls in its
    /// # contents from the network.
    /// [build-scripts.mylib-sys]
    /// allow-network = true
    ///
    /// # The `sqlx` crate requires local network access to the database.
    /// [proc-macros.sqlx]
    /// allow-network = true
    /// ```
    ///
    /// Project-local configs aren't supported for this, as that'd enable
    /// untrusted projects to just configure away the sandbox.
    pub fn load(cargo_home: &Path) -> io::Result<Self> {
        let config_path = cargo_home.join("cargo-sandbox.toml");
        match fs::read(config_path) {
            Ok(data) => Ok(toml::from_slice(&data).map_err(|err| io::Error::other(err))?),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(SandboxConfig::default()),
            Err(err) => Err(err),
        }
    }

    pub fn config_for(&self, kind: &ProcessKind<'_>) -> SandboxPackageConfig {
        // NOTE: When integrating into Cargo proper, this will probably have
        // to default to `SandboxOption::Allow`.
        let mut cfg = SandboxPackageConfig {
            default_: self.global.clone(),
            network: self.network.clone(),
            paths: self.paths.clone(),
        };

        let overwrite = |current: &mut Option<_>, new| {
            if let Some(new) = new {
                *current = Some(new);
            }
        };

        match kind {
            ProcessKind::BuildScript { package, .. } => {
                if let Some(new) = self.build_scripts.get(*package) {
                    overwrite(&mut cfg.default_, new.default_);
                    overwrite(&mut cfg.network.all, new.network.all);
                    overwrite(&mut cfg.network.external, new.network.external);
                    overwrite(&mut cfg.network.local, new.network.local);
                    cfg.paths.extend(new.paths.clone());
                }
            }
            ProcessKind::Rustc { externs, .. } => {
                // Sandbox rustc invocations based roughly on the proc-macros
                // they use.
                //
                // TODO: Properly look up which of the externs that are
                // actually proc-macros.
                for extern_ in externs {
                    if let Some(new) = self.proc_macros.get(*extern_) {
                        overwrite(&mut cfg.default_, new.default_);
                        overwrite(&mut cfg.network.all, new.network.all);
                        overwrite(&mut cfg.network.external, new.network.external);
                        overwrite(&mut cfg.network.local, new.network.local);
                        cfg.paths.extend(new.paths.clone());
                    }
                }
            }
            ProcessKind::Other => {}
        }

        cfg
    }
}
