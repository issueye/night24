#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

const DESKTOP_USER_AGENT: &str = "red_panda Desktop/0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRequest {
    pub text: String,
    pub provider: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub request_retries: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub name: Option<String>,
    pub session_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub base_url: String,
    pub client: Client,
}

#[tauri::command]
async fn send_message(
    state: State<'_, AppState>,
    request: MessageRequest,
) -> Result<String, String> {
    let url = format!("{}/reply", state.base_url);
    let payload = serde_json::json!({
        "text": request.text,
        "provider": request.provider,
        "api_key": request.api_key,
        "base_url": request.base_url,
        "model": request.model,
        "session_id": request.session_id,
        "request_retries": request.request_retries,
    });

    let resp = state
        .client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("HTTP {}: {}", status, body))
    }
}

#[tauri::command]
async fn create_session(
    state: State<'_, AppState>,
    request: SessionRequest,
) -> Result<String, String> {
    let url = format!("{}/sessions", state.base_url);
    let payload = serde_json::json!({
        "name": request.name,
        "session_type": request.session_type,
    });

    let resp = state
        .client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("HTTP {}: {}", status, body))
    }
}

#[tauri::command]
async fn list_sessions(state: State<'_, AppState>) -> Result<String, String> {
    let url = format!("{}/sessions", state.base_url);

    let resp = state
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("HTTP {}: {}", status, body))
    }
}

#[tauri::command]
async fn get_session_history(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<String, String> {
    let url = format!("{}/sessions/{}/history", state.base_url, session_id);

    let resp = state
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("HTTP {}: {}", status, body))
    }
}

#[tauri::command]
async fn select_directory(app: AppHandle) -> Result<Option<String>, String> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| {
            path.into_path()
                .map(|path| path.to_string_lossy().to_string())
                .map_err(|error| error.to_string())
        })
        .transpose()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            base_url: "http://localhost:17787".to_string(),
            client: Client::builder()
                .user_agent(DESKTOP_USER_AGENT)
                .build()
                .expect("failed to build HTTP client"),
        })
        .invoke_handler(tauri::generate_handler![
            send_message,
            create_session,
            list_sessions,
            get_session_history,
            select_directory
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
