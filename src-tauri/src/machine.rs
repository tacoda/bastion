//! The Machine seam (PLAN.md §6.2): the programs, files and platform of this
//! machine. Doctor and the stack routine reach the OS only through it.

use std::path::Path;
use std::process::Command;

/// What a program printed and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    /// Standard output, then standard error.
    pub text: String,
}

pub trait Machine: Send + Sync {
    /// Run a program to its end. `Err` means it could not start at all.
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<Output, String>;
    fn read(&self, path: &Path) -> Result<String, String>;
    fn exists(&self, path: &Path) -> bool;
    /// `std::env::consts::OS` and `ARCH`.
    fn platform(&self) -> (&'static str, &'static str);
}

pub struct ThisMachine;

impl Machine for ThisMachine {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<Output, String> {
        let mut command = Command::new(program);
        command.args(args);
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        let out = command.output().map_err(|e| format!("{program}: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        Ok(Output {
            success: out.status.success(),
            text,
        })
    }

    fn read(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn platform(&self) -> (&'static str, &'static str) {
        (std::env::consts::OS, std::env::consts::ARCH)
    }
}

#[cfg(test)]
pub mod fake {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use super::{Machine, Output};

    /// Canned runs by program name, files by path, and a fixed platform.
    pub struct FakeMachine {
        pub runs: HashMap<String, Result<Output, String>>,
        pub files: HashMap<PathBuf, String>,
        pub platform: (&'static str, &'static str),
    }

    impl FakeMachine {
        pub fn new() -> Self {
            Self {
                runs: HashMap::new(),
                files: HashMap::new(),
                platform: ("macos", "aarch64"),
            }
        }

        pub fn run_ok(mut self, program: &str, text: &str) -> Self {
            self.runs.insert(
                program.into(),
                Ok(Output {
                    success: true,
                    text: text.into(),
                }),
            );
            self
        }

        pub fn run_fails(mut self, program: &str, text: &str) -> Self {
            self.runs.insert(
                program.into(),
                Ok(Output {
                    success: false,
                    text: text.into(),
                }),
            );
            self
        }

        pub fn file(mut self, path: &str, text: &str) -> Self {
            self.files.insert(path.into(), text.into());
            self
        }
    }

    impl Machine for FakeMachine {
        fn run(
            &self,
            program: &str,
            _args: &[&str],
            _dir: Option<&Path>,
        ) -> Result<Output, String> {
            self.runs
                .get(program)
                .cloned()
                .unwrap_or_else(|| Err(format!("{program}: not found")))
        }

        fn read(&self, path: &Path) -> Result<String, String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{}: not found", path.display()))
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }

        fn platform(&self) -> (&'static str, &'static str) {
            self.platform
        }
    }
}
