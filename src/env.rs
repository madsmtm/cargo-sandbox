use std::path::PathBuf;

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct Env {
    pub cargo_home: PathBuf,
    pub rustup_home: PathBuf,
    pub developer_dir: PathBuf,
    /// PATH.
    pub path: Vec<PathBuf>,
    pub parent_cargo_sandbox_pid: libc::pid_t,
}
