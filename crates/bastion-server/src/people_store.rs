//! The people on this Bastion server and their tokens (PLAN.md D31). A token
//! is kept only as its SHA-256, so the database never holds one that works.
//! `Store` extends this trait.

use bastion_core::people::{Person, Role};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::store::{Answer, SqlStore, name, parse};

/// A token as the server shows it: never the token, only what names it.
pub use bastion_protocol::TokenView as TokenInfo;

/// A token to keep: whose it is, its hash, a label, and when it was made.
pub struct NewToken<'a> {
    pub person: &'a str,
    pub hash: &'a str,
    pub label: &'a str,
    pub at: u64,
}

pub trait PeopleStore {
    fn people(&self) -> Answer<'_, Vec<Person>>;
    /// Add a person. Adding one who exists changes nothing.
    fn add_person<'a>(&'a self, person: &'a Person) -> Answer<'a, ()>;
    fn set_role<'a>(&'a self, name: &'a str, role: Role) -> Answer<'a, ()>;
    /// Keep a token's hash for `person`.
    fn add_token<'a>(&'a self, token: NewToken<'a>) -> Answer<'a, ()>;
    fn tokens(&self) -> Answer<'_, Vec<TokenInfo>>;
    /// Revoke the token whose hash starts with `id`.
    fn remove_token<'a>(&'a self, id: &'a str) -> Answer<'a, ()>;
    /// Who holds the token with this hash, noting that it was used at `at`.
    fn person_for<'a>(&'a self, hash: &'a str, at: u64) -> Answer<'a, Option<Person>>;
}

/// A token's SHA-256, as hex.
pub fn hash(token: &str) -> String {
    Sha256::digest(token.trim().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A new token: 128 random bits, as hex.
pub fn new_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// How many characters of a hash name its token.
pub const ID_LEN: usize = 12;

pub const SCHEMA: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS people (
        name TEXT PRIMARY KEY CHECK (length(name) > 0),
        role TEXT NOT NULL CHECK (role IN ('admin','member','viewer')),
        created_at INTEGER NOT NULL)",
    "CREATE TABLE IF NOT EXISTS tokens (
        hash TEXT PRIMARY KEY CHECK (length(hash) = 64),
        person TEXT NOT NULL REFERENCES people(name),
        label TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        last_used INTEGER)",
];

fn person_of(r: &Value) -> Result<Person, String> {
    Ok(Person {
        name: parse(r, "name")?,
        role: parse(r, "role")?,
    })
}

fn token_of(r: &Value) -> Result<TokenInfo, String> {
    let hash: String = parse(r, "hash")?;
    Ok(TokenInfo {
        id: hash.chars().take(ID_LEN).collect(),
        person: parse(r, "person")?,
        label: parse(r, "label")?,
        created_at: parse(r, "created_at")?,
        last_used: parse(r, "last_used")?,
    })
}

impl PeopleStore for SqlStore {
    fn people(&self) -> Answer<'_, Vec<Person>> {
        Box::pin(async move {
            let sql = "SELECT name, role FROM people ORDER BY name";
            self.rows(sql, json!([]), person_of).await
        })
    }

    fn add_person<'a>(&'a self, person: &'a Person) -> Answer<'a, ()> {
        Box::pin(async move {
            let sql =
                "INSERT OR IGNORE INTO people (name, role, created_at) VALUES (?, ?, unixepoch())";
            self.execute(sql, json!([person.name, name(person.role)]))
                .await
                .map(|_| ())
        })
    }

    fn set_role<'a>(&'a self, person: &'a str, role: Role) -> Answer<'a, ()> {
        Box::pin(async move {
            let rows = self
                .execute(
                    "UPDATE people SET role = ? WHERE name = ? RETURNING name",
                    json!([name(role), person]),
                )
                .await?;
            if rows.is_empty() {
                return Err(format!("no person named {person}"));
            }
            Ok(())
        })
    }

    fn add_token<'a>(&'a self, t: NewToken<'a>) -> Answer<'a, ()> {
        Box::pin(async move {
            let sql = "INSERT INTO tokens (hash, person, label, created_at) VALUES (?, ?, ?, ?)";
            self.execute(sql, json!([t.hash, t.person, t.label, t.at]))
                .await
                .map(|_| ())
        })
    }

    fn tokens(&self) -> Answer<'_, Vec<TokenInfo>> {
        Box::pin(async move {
            let sql = "SELECT hash, person, label, created_at, last_used FROM tokens ORDER BY person, created_at";
            self.rows(sql, json!([]), token_of).await
        })
    }

    fn remove_token<'a>(&'a self, id: &'a str) -> Answer<'a, ()> {
        Box::pin(async move {
            if id.len() < ID_LEN {
                return Err(format!("a token id has {ID_LEN} characters"));
            }
            let rows = self
                .execute(
                    "DELETE FROM tokens WHERE substr(hash, 1, ?) = ? RETURNING hash",
                    json!([id.len(), id]),
                )
                .await?;
            if rows.is_empty() {
                return Err(format!("no token {id}"));
            }
            Ok(())
        })
    }

    fn person_for<'a>(&'a self, hash: &'a str, at: u64) -> Answer<'a, Option<Person>> {
        Box::pin(async move {
            let sql = "UPDATE tokens SET last_used = ? WHERE hash = ? RETURNING person";
            let rows = self.execute(sql, json!([at, hash])).await?;
            let Some(person) = rows
                .first()
                .map(|r| parse::<String>(r, "person"))
                .transpose()?
            else {
                return Ok(None);
            };
            let sql = "SELECT name, role FROM people WHERE name = ?";
            Ok(self.rows(sql, json!([person]), person_of).await?.pop())
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::*;
    use crate::store::fake::FakeStore;

    /// People and tokens in memory.
    #[derive(Default)]
    pub struct People {
        pub people: Mutex<Vec<Person>>,
        pub tokens: Mutex<Vec<(String, TokenInfo)>>,
    }

    impl PeopleStore for FakeStore {
        fn people(&self) -> Answer<'_, Vec<Person>> {
            let all = self.people.people.lock().unwrap().clone();
            Box::pin(async move { Ok(all) })
        }

        fn add_person<'a>(&'a self, person: &'a Person) -> Answer<'a, ()> {
            let mut all = self.people.people.lock().unwrap();
            if !all.iter().any(|p| p.name == person.name) {
                all.push(person.clone());
            }
            Box::pin(async { Ok(()) })
        }

        fn set_role<'a>(&'a self, name: &'a str, role: Role) -> Answer<'a, ()> {
            let mut all = self.people.people.lock().unwrap();
            let found = all
                .iter_mut()
                .find(|p| p.name == name)
                .map(|p| p.role = role);
            let answer = found.ok_or(format!("no person named {name}"));
            Box::pin(async move { answer })
        }

        fn add_token<'a>(&'a self, t: NewToken<'a>) -> Answer<'a, ()> {
            let (hash, person, label, at) = (t.hash, t.person, t.label, t.at);
            let info = TokenInfo {
                id: hash.chars().take(ID_LEN).collect(),
                person: person.into(),
                label: label.into(),
                created_at: at,
                last_used: None,
            };
            self.people.tokens.lock().unwrap().push((hash.into(), info));
            Box::pin(async { Ok(()) })
        }

        fn tokens(&self) -> Answer<'_, Vec<TokenInfo>> {
            let all = self.people.tokens.lock().unwrap();
            let infos = all.iter().map(|(_, t)| t.clone()).collect();
            Box::pin(async move { Ok(infos) })
        }

        fn remove_token<'a>(&'a self, id: &'a str) -> Answer<'a, ()> {
            let mut all = self.people.tokens.lock().unwrap();
            let before = all.len();
            all.retain(|(h, _)| !h.starts_with(id));
            let answer = if all.len() < before {
                Ok(())
            } else {
                Err(format!("no token {id}"))
            };
            Box::pin(async move { answer })
        }

        fn person_for<'a>(&'a self, hash: &'a str, at: u64) -> Answer<'a, Option<Person>> {
            let mut tokens = self.people.tokens.lock().unwrap();
            let holder = tokens.iter_mut().find(|(h, _)| h == hash).map(|(_, t)| {
                t.last_used = Some(at);
                t.person.clone()
            });
            let people = self.people.people.lock().unwrap();
            let found = holder.and_then(|n| people.iter().find(|p| p.name == n).cloned());
            Box::pin(async move { Ok(found) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::fake::FakeStore;

    #[test]
    fn a_token_is_kept_as_its_hash_and_names_its_person_until_revoked() {
        let store = FakeStore::default();
        crate::rt::block_on(async {
            let ana = Person {
                name: "ana".into(),
                role: Role::Member,
            };
            store.add_person(&ana).await.unwrap();
            let token = new_token();
            store
                .add_token(NewToken {
                    person: "ana",
                    hash: &hash(&token),
                    label: "laptop",
                    at: 1,
                })
                .await
                .unwrap();
            assert_eq!(store.person_for(&hash(&token), 5).await, Ok(Some(ana)));
            let listed = store.tokens().await.unwrap();
            assert_eq!(listed[0].last_used, Some(5));
            assert!(!listed[0].id.contains(&token));
            store.remove_token(&listed[0].id).await.unwrap();
            assert_eq!(store.person_for(&hash(&token), 6).await, Ok(None));
        });
    }

    #[test]
    fn hashes_are_stable_hex_and_ignore_surrounding_space() {
        assert_eq!(hash("abc"), hash(" abc\n"));
        assert_eq!(hash("abc").len(), 64);
        assert_ne!(hash("abc"), hash("abd"));
    }
}
