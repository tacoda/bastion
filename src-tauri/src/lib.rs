//! Bastion's Rust core. The pages reach the engine only through these commands
//! (PLAN.md D9), and the engine reaches the pages only as events (D10).

use iii_sdk::iii::WorkerMetadata;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::{Error, IIIClient, InitOptions, RegisterFunction, register_worker};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

/// The local engine. A profile replaces this default in A1 (D12).
const ENGINE_URL: &str = "ws://127.0.0.1:49160";

struct Engine(IIIClient);

#[tauri::command]
async fn engine_status(engine: State<'_, Engine>) -> Result<String, String> {
    let mut request = TriggerRequest::new("engine::functions::list", json!({}));
    request.timeout_ms = Some(5_000);
    let out = engine.0.trigger(request.namespace("default")).await.map_err(|e| e.to_string())?;
    let count = out["functions"].as_array().map_or(0, Vec::len);
    Ok(format!("up at {ENGINE_URL}, {count} functions"))
}

/// The page reports each event it received, so a run proves the round trip.
#[tauri::command]
fn seen(event: String) {
    println!("PAGE SAW {event}");
}

fn connect(app: AppHandle) -> IIIClient {
    let url = std::env::var("III_URL").unwrap_or_else(|_| ENGINE_URL.into());
    let options = InitOptions {
        metadata: Some(WorkerMetadata { name: "bastion-app".into(), ..WorkerMetadata::default() }),
        namespace: Some("bastion".into()),
        ..InitOptions::default()
    };
    let iii = register_worker(&url, options);
    // A call here reaches the page as the `changed` event.
    iii.register_function(
        "bastion::app::changed",
        RegisterFunction::new_async(move |event: Value| {
            let app = app.clone();
            async move {
                let _ = app.emit("changed", &event);
                Ok::<_, Error>(json!({}))
            }
        }),
    );
    iii
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(Engine(connect(app.handle().clone())));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![engine_status, seen])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
