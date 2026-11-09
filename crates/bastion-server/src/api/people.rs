//! Managing the people on this Bastion server (PLAN.md D31). Only an
//! admin reaches these; `access` checks that before any of them runs.

use bastion_core::people::{Person, Role};
use bastion_protocol::PersonView;

use super::factory::{FactoryCell, factory};
use crate::factory::now;
use crate::people_store::{NewToken, TokenInfo, hash, new_token};

/// A person as the API carries them.
pub fn view(p: &Person) -> PersonView {
    PersonView {
        name: p.name.clone(),
        role: p.role.word().into(),
    }
}

pub async fn people(cell: &FactoryCell) -> Result<Vec<PersonView>, String> {
    let all = factory(cell)?.store().people().await?;
    Ok(all.iter().map(view).collect())
}

pub async fn add(cell: &FactoryCell, person: Person) -> Result<(), String> {
    let name = person.name.trim();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err("a person needs a name with no spaces".into());
    }
    let factory = factory(cell)?;
    let store = factory.store();
    if store.people().await?.iter().any(|p| p.name == name) {
        return Err(format!("{name} is already here"));
    }
    store
        .add_person(&Person {
            name: name.into(),
            role: person.role,
        })
        .await
}

pub async fn set_role(cell: &FactoryCell, name: String, role: Role) -> Result<(), String> {
    factory(cell)?.store().set_role(&name, role).await
}

/// Issue a new token for `name`, labeled. The token itself is returned
/// once, here; the server keeps only its hash.
pub async fn issue(cell: &FactoryCell, name: String, label: String) -> Result<String, String> {
    let factory = factory(cell)?;
    let store = factory.store();
    if !store.people().await?.iter().any(|p| p.name == name) {
        return Err(format!("no person named {name}"));
    }
    let token = new_token();
    let token_hash = hash(&token);
    let t = NewToken {
        person: &name,
        hash: &token_hash,
        label: label.trim(),
        at: now(),
    };
    store.add_token(t).await?;
    Ok(token)
}

pub async fn tokens(cell: &FactoryCell) -> Result<Vec<TokenInfo>, String> {
    factory(cell)?.store().tokens().await
}

pub async fn revoke(cell: &FactoryCell, id: String) -> Result<(), String> {
    factory(cell)?.store().remove_token(&id).await
}
