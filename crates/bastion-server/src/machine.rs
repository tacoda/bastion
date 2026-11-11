//! The Machine seam: the programs, files and platform of this
//! machine. Doctor and the stack routine reach the OS only through it.

use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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
    /// Start a program that outlives the server, with its output appended to `log`.
    fn spawn(&self, program: &str, args: &[&str], dir: &Path, log: &Path) -> Result<u32, String>;
    fn read(&self, path: &Path) -> Result<String, String>;
    fn read_bytes(&self, path: &Path) -> Result<Vec<u8>, String>;
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    /// Make a new file only its owner can read. Refuses to replace one.
    fn write_secret(&self, path: &Path, bytes: &[u8]) -> Result<(), String>;
    /// Remove a file; one already gone is not an error.
    fn remove(&self, path: &Path) -> Result<(), String>;
    /// Make a folder and its parents, if missing.
    fn make_dir(&self, path: &Path) -> Result<(), String>;
    fn exists(&self, path: &Path) -> bool;
    /// `std::env::consts::OS` and `ARCH`.
    fn platform(&self) -> (&'static str, &'static str);
}

pub struct ThisMachine;

/// The environment a long-running program such as the compose daemon is
/// given: the basics to run, and iii's own settings. Model keys live in the
/// vault (PLAN.md D34), never here.
fn passed(name: &str) -> bool {
    const BASICS: [&str; 9] = [
        "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG", "LC_ALL", "TERM",
    ];
    BASICS.contains(&name) || name.starts_with("III_")
}

/// How long a host command may run, such as a `git push` to a slow remote.
const RUN_LIMIT: Duration = Duration::from_secs(300);

/// Wait for `child` to end, reading its output as it runs. Past `limit`,
/// kill it and say so.
fn finish(mut child: Child, limit: Duration) -> Result<Output, String> {
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("stopped after {limit:?}"));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let text = out.join().unwrap_or_default() + &err.join().unwrap_or_default();
    Ok(Output {
        success: status.success(),
        text,
    })
}

impl Machine for ThisMachine {
    fn run(&self, program: &str, args: &[&str], dir: Option<&Path>) -> Result<Output, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            // Nothing may wait on a person: no stdin, and git asks no one.
            .stdin(Stdio::null())
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
        let child = command.spawn().map_err(|e| format!("{program}: {e}"))?;
        finish(child, RUN_LIMIT).map_err(|e| format!("{program} {}: {e}", args.join(" ")))
    }

    // ponytail: the child is never waited on, so if it exits while the server runs it stays a zombie until the server stops.
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
            // The daemon passes its environment on to every worker VM, so it
            // gets only what a program needs to run, and no host secrets.
            .env_clear()
            .envs(std::env::vars().filter(|(name, _)| passed(name)))
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(err)
            // Its own process group, so a signal to the server does not reach it.
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

    fn write_secret(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .and_then(|mut file| file.write_all(bytes))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn remove(&self, path: &Path) -> Result<(), String> {
        match fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        }
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

    impl Default for FakeMachine {
        fn default() -> Self {
            Self::new()
        }
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

        fn write_secret(&self, path: &Path, _bytes: &[u8]) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("write-secret {}", path.display()));
            Ok(())
        }

        fn remove(&self, path: &Path) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("remove {}", path.display()));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[test]
    fn a_spawned_program_gets_the_basics_and_no_secrets() {
        for kept in ["PATH", "HOME", "TMPDIR", "III_URL"] {
            assert!(passed(kept), "{kept}");
        }
        for dropped in [
            "GITHUB_TOKEN",
            "OPENAI_API_KEY",
            "NPM_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
        ] {
            assert!(!passed(dropped), "{dropped}");
        }
    }

    #[test]
    fn a_command_past_its_limit_is_stopped_and_one_within_it_keeps_its_output() {
        let slow = finish(sh("sleep 5"), Duration::from_millis(100));
        assert_eq!(slow, Err("stopped after 100ms".into()));
        let done = finish(sh("echo out; echo err >&2"), Duration::from_secs(5)).unwrap();
        assert_eq!((done.success, done.text.as_str()), (true, "out\nerr\n"));
    }
}
