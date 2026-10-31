//! Commands for held calls, the two ladders and the audit (PLAN.md A5).

use bastion_core::audit;
use bastion_protocol::{AuditEntryView, AuditView, CapabilityView, ConstraintView, HeldCallRef};
use serde_json::json;
use tauri::State;

use super::factory::{FactoryCell, factory, name};

#[tauri::command]
pub async fn held_allow(cell: State<'_, FactoryCell>, held: HeldCallRef) -> Result<(), String> {
    factory(&cell)?
        .gate()
        .allow_once(&held.job, &held.call)
        .await
}

#[tauri::command]
pub async fn held_refuse(
    cell: State<'_, FactoryCell>,
    held: HeldCallRef,
    feedback: String,
) -> Result<(), String> {
    if feedback.trim().is_empty() {
        return Err("say what the agent should do instead".into());
    }
    factory(&cell)?
        .gate()
        .refuse(&held.job, &held.call, feedback.trim())
        .await
}

#[tauri::command]
pub async fn constraints(cell: State<'_, FactoryCell>) -> Result<Vec<ConstraintView>, String> {
    let all = factory(&cell)?.store().constraints().await?;
    Ok(all
        .into_iter()
        .map(|c| ConstraintView {
            id: c.id,
            rule: c.rule,
            function: c.function,
            contains: c.contains,
            rung: name(c.rung),
            source: name(c.source),
        })
        .collect())
}

#[tauri::command]
pub async fn constraint_move(
    cell: State<'_, FactoryCell>,
    id: String,
    up: bool,
) -> Result<(), String> {
    factory(&cell)?.gate().move_constraint(&id, up).await
}

#[tauri::command]
pub async fn capabilities(cell: State<'_, FactoryCell>) -> Result<Vec<CapabilityView>, String> {
    let all = factory(&cell)?.store().capabilities().await?;
    Ok(all
        .into_iter()
        .map(|g| CapabilityView {
            stage: name(g.stage),
            function: g.function,
            rung: name(g.rung),
        })
        .collect())
}

#[tauri::command]
pub async fn capability_move(
    cell: State<'_, FactoryCell>,
    stage: String,
    function: String,
    up: bool,
) -> Result<(), String> {
    let stage =
        serde_json::from_value(json!(stage)).map_err(|_| format!("{stage:?} is not a stage"))?;
    factory(&cell)?
        .gate()
        .move_capability(stage, &function, up)
        .await
}

/// The whole chain, verified as it is read.
#[tauri::command]
pub async fn audit(cell: State<'_, FactoryCell>) -> Result<AuditView, String> {
    let chain = factory(&cell)?.store().audit().await?;
    let broken_at = audit::verify(&chain).err();
    let entries = chain
        .into_iter()
        .map(|e| AuditEntryView {
            seq: e.seq,
            at: e.at,
            kind: e.record.kind,
            subject: e.record.subject,
            detail: e.record.detail,
            hash: e.hash,
        })
        .collect();
    Ok(AuditView { entries, broken_at })
}
