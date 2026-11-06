//! The `evals` commands, each a call to `bastion_server::api::evals` (D28; S3 removes them).

use super::prelude::*;

#[tauri::command]
pub async fn evals(cell: State<'_, FactoryCell>) -> Result<Vec<EvalSummary>, String> {
    api::evals::evals(&cell).await
}

#[tauri::command]
pub async fn eval_create(
    cell: State<'_, FactoryCell>,
    machine: State<'_, Arc<dyn Machine>>,
    eval: NewEval,
) -> Result<String, String> {
    api::evals::eval_create(&cell, &machine, eval).await
}

#[tauri::command]
pub async fn eval(cell: State<'_, FactoryCell>, id: String) -> Result<EvalView, String> {
    api::evals::eval(&cell, id).await
}

#[tauri::command]
pub async fn eval_apply(
    cell: State<'_, FactoryCell>,
    id: String,
    arm: String,
) -> Result<(), String> {
    api::evals::eval_apply(&cell, id, arm).await
}
