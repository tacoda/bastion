//! Profiles (PLAN.md D12): which engine the app talks to, and which checkout
//! holds its `worker-compose.yaml`. Stored as TOML in the OS config directory,
//! with no secret in it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "profiles.toml";
const THIS_MACHINE: &str = "this machine";
const LOCAL_ENGINE: &str = "ws://127.0.0.1:49160";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The engine runs on this machine, and the app starts it (D13).
    Local,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub kind: Kind,
    pub engine_url: String,
    pub checkout: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profiles {
    pub active: String,
    pub profile: Vec<Profile>,
}

impl Profiles {
    pub fn this_machine(checkout: PathBuf) -> Self {
        Self {
            active: THIS_MACHINE.into(),
            profile: vec![Profile {
                name: THIS_MACHINE.into(),
                kind: Kind::Local,
                engine_url: LOCAL_ENGINE.into(),
                checkout,
            }],
        }
    }

    pub fn active(&self) -> &Profile {
        self.profile
            .iter()
            .find(|p| p.name == self.active)
            .expect("validated on load")
    }

    /// Read the profiles at `path`, or write the `this machine` profile there
    /// when the file does not exist yet.
    pub fn load_or_create(path: &Path, default_checkout: PathBuf) -> Result<Self, String> {
        if !path.exists() {
            let profiles = Self::this_machine(default_checkout);
            profiles.save(path)?;
            return Ok(profiles);
        }
        let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let profiles: Self =
            toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if !profiles.profile.iter().any(|p| p.name == profiles.active) {
            return Err(format!(
                "{}: active profile {:?} is not one of the profiles listed",
                path.display(),
                profiles.active
            ));
        }
        Ok(profiles)
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bastion-profile-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join(FILE_NAME)
    }

    #[test]
    fn a_missing_file_gets_the_this_machine_profile() {
        let path = scratch("missing");
        let profiles = Profiles::load_or_create(&path, "/repo".into()).unwrap();
        assert_eq!(profiles.active().kind, Kind::Local);
        assert_eq!(profiles.active().engine_url, LOCAL_ENGINE);
        assert_eq!(
            Profiles::load_or_create(&path, "/other".into()).unwrap(),
            profiles
        );
    }

    #[test]
    fn an_active_name_with_no_profile_is_refused() {
        let path = scratch("dangling");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "active = \"box\"\nprofile = []\n").unwrap();
        let error = Profiles::load_or_create(&path, "/repo".into()).unwrap_err();
        assert!(
            error.contains("\"box\" is not one of the profiles"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_field_is_refused_with_the_file_named() {
        let path = scratch("unknown");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = toml::to_string(&Profiles::this_machine("/repo".into())).unwrap();
        text = text.replace("kind = \"local\"", "kind = \"local\"\ntoken = \"x\"");
        fs::write(&path, text).unwrap();
        let error = Profiles::load_or_create(&path, "/repo".into()).unwrap_err();
        assert!(error.starts_with(&path.display().to_string()), "{error}");
    }
}
