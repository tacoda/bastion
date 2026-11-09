//! The Bastion servers this client knows (D30): `local`, which the client starts,
//! and the ones the owner added. A server's name and URL live in
//! `servers.toml`; its token lives in the Keychain.

use std::path::{Path, PathBuf};

use bastion_protocol::{LOCAL, Server};
use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "servers.toml";
/// The Keychain service each server's token is kept under.
const SERVICE: &str = "dev.tacoda.bastion.server";

/// Where the tokens are kept.
pub trait Vault: Send + Sync {
    fn get(&self, name: &str) -> Result<String, String>;
    fn set(&self, name: &str, token: &str) -> Result<(), String>;
    fn remove(&self, name: &str) -> Result<(), String>;
}

/// The macOS Keychain.
pub struct Keychain;

fn entry(name: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, name).map_err(|e| e.to_string())
}

impl Vault for Keychain {
    fn get(&self, name: &str) -> Result<String, String> {
        entry(name)?
            .get_password()
            .map_err(|e| format!("{name}'s token: {e}"))
    }

    fn set(&self, name: &str, token: &str) -> Result<(), String> {
        entry(name)?.set_password(token).map_err(|e| e.to_string())
    }

    fn remove(&self, name: &str) -> Result<(), String> {
        match entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
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
    pub fn all(&self) -> Result<Vec<Server>, String> {
        let mut all = vec![self.local.clone()];
        for a in self.read()?.server {
            let token = self.vault.get(&a.name)?;
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
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(format!("{url} is not an http:// or https:// URL"));
        }
        let mut file = self.read()?;
        if name == LOCAL || file.server.iter().any(|s| s.name == name) {
            return Err(format!("there is already a Bastion server named {name}"));
        }
        self.vault.set(name, server.token.trim())?;
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
        fn set(&self, name: &str, token: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(name.into(), token.into());
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
    fn a_name_taken_a_bad_url_or_removing_local_is_refused() {
        let s = servers(&scratch("refuse"));
        s.add(server("box", "https://box.example")).unwrap();
        assert!(s.add(server("box", "https://other.example")).is_err());
        assert!(s.add(server(LOCAL, "https://other.example")).is_err());
        assert!(s.add(server("ftp", "ftp://box.example")).is_err());
        assert!(s.remove(LOCAL).is_err());
        s.remove("box").unwrap();
        assert_eq!(s.all().unwrap().len(), 1);
    }
}
