//! The Bastion servers this client knows (D30): `local`, which the client starts,
//! and the ones the owner added. A server's name and URL live in
//! `servers.toml`; its token lives in the Keychain on macOS, and in a file
//! only its owner can read elsewhere.

use std::path::{Path, PathBuf};

use bastion_protocol::{LOCAL, Server};
use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "servers.toml";
/// The Keychain service each server's token is kept under.
#[cfg(target_os = "macos")]
const SERVICE: &str = "dev.tacoda.bastion.server";

/// Where the tokens are kept.
pub trait Vault: Send + Sync {
    fn get(&self, name: &str) -> Result<String, String>;
    /// Keep `server`'s token under its name.
    fn set(&self, server: &Server) -> Result<(), String>;
    fn remove(&self, name: &str) -> Result<(), String>;
}

/// Where this machine keeps tokens: the Keychain on macOS, else files in
/// `dir`.
pub fn vault(dir: PathBuf) -> Box<dyn Vault> {
    #[cfg(target_os = "macos")]
    {
        let _ = dir;
        Box::new(Keychain)
    }
    #[cfg(not(target_os = "macos"))]
    Box::new(TokenFiles { dir })
}

/// The macOS Keychain.
#[cfg(target_os = "macos")]
pub struct Keychain;

#[cfg(target_os = "macos")]
fn entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
impl Vault for Keychain {
    fn get(&self, name: &str) -> Result<String, String> {
        entry(name)?
            .get_password()
            .map_err(|e| format!("{name}'s token: {e}"))
    }

    fn set(&self, server: &Server) -> Result<(), String> {
        let entry = entry(&server.name)?;
        entry.set_password(&server.token).map_err(|e| e.to_string())
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        match entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// One file per token in `dir`, each readable only by its owner, where
/// there is no Keychain.
pub struct TokenFiles {
    pub dir: PathBuf,
}

impl TokenFiles {
    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Vault for TokenFiles {
    fn get(&self, name: &str) -> Result<String, String> {
        let text =
            std::fs::read_to_string(self.file(name)).map_err(|e| format!("{name}'s token: {e}"))?;
        Ok(text.trim().into())
    }

    fn set(&self, server: &Server) -> Result<(), String> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let path = self.file(&server.name);
        let _ = std::fs::remove_file(&path);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .and_then(|mut f| f.write_all(server.token.as_bytes()))
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        match std::fs::remove_file(self.file(name)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

/// One added server as the file keeps it, without its token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Added {
    name: String,
    url: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    server: Vec<Added>,
}

pub struct Servers {
    path: PathBuf,
    local: Server,
    vault: Box<dyn Vault>,
}

impl Servers {
    pub fn new(path: PathBuf, local: Server, vault: Box<dyn Vault>) -> Self {
        Servers { path, local, vault }
    }

    fn read(&self) -> Result<File, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", self.path.display())),
            Err(_) => Ok(File::default()),
        }
    }

    fn write(&self, file: &File) -> Result<(), String> {
        let text = toml::to_string(file).map_err(|e| e.to_string())?;
        write_file(&self.path, &text)
    }

    /// `local` first, then the added servers in the order they were added.
    /// A server whose token cannot be read is listed with none, and a call
    /// to it says so.
    pub fn all(&self) -> Result<Vec<Server>, String> {
        let mut all = vec![self.local.clone()];
        for a in self.read()?.server {
            let token = self.vault.get(&a.name).unwrap_or_default();
            all.push(Server {
                name: a.name,
                url: a.url,
                token,
            });
        }
        Ok(all)
    }

    pub fn add(&self, server: Server) -> Result<(), String> {
        let name = server.name.trim();
        let url = server.url.trim().trim_end_matches('/');
        if name.is_empty() || server.token.trim().is_empty() {
            return Err("a Bastion server needs a name, a URL and a token".into());
        }
        match url.strip_prefix("http://") {
            Some(rest) if !loopback(rest) => {
                return Err(format!(
                    "{url} sends the token in the clear: use https:// beyond this machine"
                ));
            }
            None if !url.starts_with("https://") => {
                return Err(format!("{url} is not an http:// or https:// URL"));
            }
            _ => {}
        }
        let mut file = self.read()?;
        if name == LOCAL || file.server.iter().any(|s| s.name == name) {
            return Err(format!("there is already a Bastion server named {name}"));
        }
        let kept = Server {
            name: name.into(),
            url: url.into(),
            token: server.token.trim().into(),
        };
        self.vault.set(&kept)?;
        file.server.push(Added {
            name: name.into(),
            url: url.into(),
        });
        self.write(&file)
    }

    pub fn remove(&self, name: &str) -> Result<(), String> {
        if name == LOCAL {
            return Err("local is the server this client runs; it cannot be removed".into());
        }
        let mut file = self.read()?;
        file.server.retain(|s| s.name != name);
        self.write(&file)?;
        self.vault.remove(name)
    }
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether the host in `rest`, a URL after its scheme, is this machine.
fn loopback(rest: &str) -> bool {
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    host == "localhost"
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Memory(Mutex<HashMap<String, String>>);

    impl Vault for Memory {
        fn get(&self, name: &str) -> Result<String, String> {
            self.0
                .lock()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or("none".into())
        }
        fn set(&self, server: &Server) -> Result<(), String> {
            let (name, token) = (server.name.clone(), server.token.clone());
            self.0.lock().unwrap().insert(name, token);
            Ok(())
        }
        fn remove(&self, name: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(name);
            Ok(())
        }
    }

    fn server(name: &str, url: &str) -> Server {
        Server {
            name: name.into(),
            url: url.into(),
            token: format!("{name}-token"),
        }
    }

    fn servers(dir: &Path) -> Servers {
        let local = server(LOCAL, "http://127.0.0.1:49190");
        Servers::new(dir.join(FILE_NAME), local, Box::new(Memory::default()))
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bastion-servers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn local_comes_first_and_an_added_server_keeps_its_token_out_of_the_file() {
        let dir = scratch("add");
        let s = servers(&dir);
        s.add(server("box", "https://box.example/")).unwrap();
        let all = s.all().unwrap();
        let names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [LOCAL, "box"]);
        assert_eq!(
            (all[1].url.as_str(), all[1].token.as_str()),
            ("https://box.example", "box-token")
        );
        let file = std::fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        assert!(!file.contains("box-token"), "{file}");
    }

    #[test]
    fn a_server_whose_token_is_gone_is_still_listed_without_one() {
        let s = servers(&scratch("gone"));
        s.add(server("box", "https://box.example")).unwrap();
        s.vault.remove("box").unwrap();
        let all = s.all().unwrap();
        assert_eq!((all[1].name.as_str(), all[1].token.as_str()), ("box", ""));
    }

    #[test]
    fn a_token_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("files");
        let files = TokenFiles { dir: dir.clone() };
        let mut token = server("box", "https://box.example");
        files.set(&token).unwrap();
        token.token = "t2".into();
        files.set(&token).unwrap();
        assert_eq!(files.get("box").unwrap(), "t2");
        let mode = std::fs::metadata(dir.join("box"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        files.remove("box").unwrap();
        files.remove("box").unwrap();
        assert!(files.get("box").is_err());
    }
    #[test]
    fn a_name_taken_a_bad_url_or_removing_local_is_refused() {
        let s = servers(&scratch("refuse"));
        s.add(server("box", "https://box.example")).unwrap();
        assert!(s.add(server("box", "https://other.example")).is_err());
        assert!(s.add(server(LOCAL, "https://other.example")).is_err());
        assert!(s.add(server("ftp", "ftp://box.example")).is_err());
        assert!(s.add(server("plain", "http://box.example")).is_err());
        assert!(s.add(server("plain", "http://10.0.0.5:49190")).is_err());
        s.add(server("here", "http://127.0.0.1:49190")).unwrap();
        s.add(server("here6", "http://[::1]:49190/")).unwrap();
        s.add(server("named", "http://localhost:49190")).unwrap();
        assert!(s.remove(LOCAL).is_err());
        for name in ["box", "here", "here6", "named"] {
            s.remove(name).unwrap();
        }
        assert_eq!(s.all().unwrap().len(), 1);
    }
}
