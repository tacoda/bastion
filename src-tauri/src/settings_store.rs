//! The owner's settings as versions (PLAN.md A7). Each change writes a whole
//! new version with a note; restoring writes an old version's values again
//! as the newest. `Store` extends this trait.

use bastion_core::settings::Settings;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::store::{Answer, SqlStore, Store, parse};

/// One saved version of the settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub version: u64,
    pub at: u64,
    /// What changed, or which version this restored.
    pub note: String,
}

pub trait SettingsStore {
    /// Every version, newest first.
    fn versions(&self) -> Answer<'_, Vec<Version>>;
    /// One version's values, or none if there is no such version.
    fn settings_at(&self, version: u64) -> Answer<'_, Option<Settings>>;
    /// Save `settings` as the newest version. Returns its number.
    fn add_version<'a>(&'a self, settings: &'a Settings, note: &'a str, at: u64)
    -> Answer<'a, u64>;
}

/// The settings in force: the newest version, or, before the first one, the
/// defaults with the checkpoint dials as they were kept before A7.
pub async fn current(store: &dyn Store) -> Result<Settings, String> {
    match store.versions().await?.first() {
        Some(v) => Ok(store.settings_at(v.version).await?.unwrap_or_default()),
        None => Ok(Settings {
            dials: store.dials().await?,
            ..Settings::default()
        }),
    }
}

/// Save a change as a new version, noting what moved. Nothing moved, no version.
pub async fn change(store: &dyn Store, next: &Settings, at: u64) -> Result<Option<u64>, String> {
    let was = current(store).await?;
    let changes = next.changes(&was);
    if changes.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        store.add_version(next, &changes.join("; "), at).await?,
    ))
}

/// Make an old version the newest again.
pub async fn restore(store: &dyn Store, version: u64, at: u64) -> Result<u64, String> {
    let old = store
        .settings_at(version)
        .await?
        .ok_or(format!("no settings version {version}"))?;
    let was = current(store).await?;
    let note = format!(
        "restored version {version}: {}",
        old.changes(&was).join("; ")
    );
    store.add_version(&old, &note, at).await
}

pub const SCHEMA: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS settings_versions (
        version INTEGER PRIMARY KEY AUTOINCREMENT,
        at INTEGER NOT NULL,
        note TEXT NOT NULL)",
    "CREATE TABLE IF NOT EXISTS settings_values (
        version INTEGER NOT NULL REFERENCES settings_versions(version),
        key TEXT NOT NULL CHECK (length(key) > 0),
        value TEXT NOT NULL,
        PRIMARY KEY (version, key))",
];

fn version_of(r: &Value) -> Result<Version, String> {
    Ok(Version {
        version: parse(r, "version")?,
        at: parse(r, "at")?,
        note: parse(r, "note")?,
    })
}

fn pair_of(r: &Value) -> Result<(String, String), String> {
    Ok((parse(r, "key")?, parse(r, "value")?))
}

impl SettingsStore for SqlStore {
    fn versions(&self) -> Answer<'_, Vec<Version>> {
        Box::pin(async move {
            let sql = "SELECT version, at, note FROM settings_versions ORDER BY version DESC";
            self.rows(sql, json!([]), version_of).await
        })
    }

    fn settings_at(&self, version: u64) -> Answer<'_, Option<Settings>> {
        Box::pin(async move {
            let sql = "SELECT key, value FROM settings_values WHERE version = ?";
            let pairs = self.rows(sql, json!([version]), pair_of).await?;
            Ok((!pairs.is_empty()).then(|| Settings::from_pairs(&pairs)))
        })
    }

    fn add_version<'a>(
        &'a self,
        settings: &'a Settings,
        note: &'a str,
        at: u64,
    ) -> Answer<'a, u64> {
        Box::pin(async move {
            let rows = self
                .execute(
                    "INSERT INTO settings_versions (at, note) VALUES (?, ?) RETURNING version",
                    json!([at, note]),
                )
                .await?;
            let version: u64 = rows
                .first()
                .map(|r| parse(r, "version"))
                .ok_or("no version came back")??;
            let statements = settings
                .to_pairs()
                .into_iter()
                .map(|(k, v)| json!({ "sql": "INSERT INTO settings_values (version, key, value) VALUES (?, ?, ?)", "params": [version, k, v] }))
                .collect();
            self.transaction(statements).await?;
            Ok(version)
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::*;
    use crate::store::fake::FakeStore;

    /// Versions in memory, oldest first.
    #[derive(Default)]
    pub struct Versions(pub Mutex<Vec<(Version, Settings)>>);

    impl SettingsStore for FakeStore {
        fn versions(&self) -> Answer<'_, Vec<Version>> {
            let all = self
                .versions
                .0
                .lock()
                .unwrap()
                .iter()
                .rev()
                .map(|(v, _)| v.clone())
                .collect();
            Box::pin(async move { Ok(all) })
        }

        fn settings_at(&self, version: u64) -> Answer<'_, Option<Settings>> {
            let found = self
                .versions
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|(v, _)| v.version == version)
                .map(|(_, s)| s.clone());
            Box::pin(async move { Ok(found) })
        }

        fn add_version<'a>(
            &'a self,
            settings: &'a Settings,
            note: &'a str,
            at: u64,
        ) -> Answer<'a, u64> {
            let mut all = self.versions.0.lock().unwrap();
            let version = all.len() as u64 + 1;
            all.push((
                Version {
                    version,
                    at,
                    note: note.into(),
                },
                settings.clone(),
            ));
            Box::pin(async move { Ok(version) })
        }
    }
}

#[cfg(test)]
mod tests {
    use bastion_core::settings::{Forge, Model};

    use super::*;
    use crate::store::fake::FakeStore;

    #[test]
    fn a_change_is_a_version_and_restore_brings_an_old_one_back_as_the_newest() {
        let store = FakeStore::default();
        tauri::async_runtime::block_on(async {
            let first = Settings::default();
            assert_eq!(
                change(&store, &first, 1).await,
                Ok(None),
                "nothing moved from the defaults"
            );
            let github = Settings {
                forge: Forge::Github,
                ..Settings::default()
            };
            assert_eq!(change(&store, &github, 2).await, Ok(Some(1)));
            let bigger = Settings {
                build_model: Model {
                    provider: "openai".into(),
                    model: "gpt-5".into(),
                },
                ..github.clone()
            };
            assert_eq!(change(&store, &bigger, 3).await, Ok(Some(2)));
            assert_eq!(current(&store).await.unwrap().build_model.model, "gpt-5");

            assert_eq!(restore(&store, 1, 4).await, Ok(3));
            let now = current(&store).await.unwrap();
            assert_eq!(
                (now.build_model.model.as_str(), now.forge),
                ("gpt-5-mini", Forge::Github)
            );
            let newest = store.versions().await.unwrap()[0].clone();
            assert_eq!(
                newest.note,
                "restored version 1: model.build.model: gpt-5 -> gpt-5-mini"
            );
            assert!(restore(&store, 9, 5).await.is_err());
        });
    }
}
