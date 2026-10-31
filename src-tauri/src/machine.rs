//! The Machine seam (PLAN.md §6.2): the programs, files and platform of this
//! machine. Doctor and the stack routine reach the OS only through it.

use std::fs::{self, OpenOptions};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

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
    /// Start a program that outlives the app, with its output appended to `log`.
    fn spawn(&self, program: &str, args: &[&str], dir: &Path, log: &Path) -> Result<u32, String>;
    fn read(&self, path: &Path) -> Result<String, String>;
    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, String>;
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    /// Make a folder and its parents, if missing.
    fn make_dir(&self, path: &Path) -> Result<(), String>;
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

    // ponytail: the child is never waited on, so if it exits while the app runs it stays a zombie until the app quits.
    fn spawn(&self, program: &str, args: &[&str], dir: &Path, log: &Path) -> Result<u32, String> {
        if let Some(parent) = log.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(|e| format!("{}: {e}", log.display()))?;
        let err = file.try_clone().map_err(|e| e.to_string())?;
        Command::new(program)
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(err)
            // Its own process group, so a signal to the app does not reach it.
            .process_group(0)
            .spawn()
            .map(|child| child.id())
            .map_err(|e| format!("{program}: {e}"))
    }

    fn read(&self, path: &Path) -> Result<String, String> {
        fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn make_dir(&self, path: &Path) -> Result<(), String> {
        fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
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
    use std::sync::Mutex;

    use super::{Machine, Output};

    /// Canned runs by program name, files by path, and a fixed platform. A
    /// program given several answers returns them in turn, then keeps the last.
    pub struct FakeMachine {
        runs: Mutex<HashMap<String, Vec<Result<Output, String>>>>,
        pub files: HashMap<PathBuf, String>,
        pub platform: (&'static str, &'static str),
        /// Every run and spawn, as `program arg arg`.
        pub calls: Mutex<Vec<String>>,
    }

    impl FakeMachine {
        pub fn new() -> Self {
            Self {
                runs: Mutex::new(HashMap::new()),
                files: HashMap::new(),
                platform: ("macos", "aarch64"),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn answer(self, program: &str, answer: Result<Output, String>) -> Self {
            self.runs
                .lock()
                .unwrap()
                .entry(program.into())
                .or_default()
                .push(answer);
            self
        }

        pub fn run_ok(self, program: &str, text: &str) -> Self {
            let output = Output {
                success: true,
                text: text.into(),
            };
            self.answer(program, Ok(output))
        }

        pub fn run_fails(self, program: &str, text: &str) -> Self {
            let output = Output {
                success: false,
                text: text.into(),
            };
            self.answer(program, Ok(output))
        }

        pub fn file(mut self, path: &str, text: &str) -> Self {
            self.files.insert(path.into(), text.into());
            self
        }

        pub fn called(&self, prefix: &str) -> bool {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .any(|c| c.starts_with(prefix))
        }
    }

    impl Machine for FakeMachine {
        fn run(&self, program: &str, args: &[&str], _dir: Option<&Path>) -> Result<Output, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("{program} {}", args.join(" ")));
            let mut runs = self.runs.lock().unwrap();
            match runs.get_mut(program) {
                Some(answers) if answers.len() > 1 => answers.remove(0),
                Some(answers) => answers[0].clone(),
                None => Err(format!("{program}: not found")),
            }
        }

        fn spawn(
            &self,
            program: &str,
            args: &[&str],
            _dir: &Path,
            _log: &Path,
        ) -> Result<u32, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("spawn {program} {}", args.join(" ")));
            Ok(4242)
        }

        fn read(&self, path: &Path) -> Result<String, String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{}: not found", path.display()))
        }

        fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, String> {
            Ok(self
                .files
                .get(path)
                .cloned()
                .unwrap_or_default()
                .into_bytes())
        }

        fn write_bytes(&self, path: &Path, _bytes: &[u8]) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("write {}", path.display()));
            Ok(())
        }

        fn make_dir(&self, path: &Path) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("mkdir {}", path.display()));
            Ok(())
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.contains_key(path)
        }

        fn platform(&self) -> (&'static str, &'static str) {
            self.platform
        }
    }
}
