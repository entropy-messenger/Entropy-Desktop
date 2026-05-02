use crate::app_state::{DbState, NetworkState};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

pub mod handlers;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ReplyTo {
    pub id: String,
    pub content: String,
    pub sender_hash: Option<String>,
    pub sender_alias: Option<String>,
    pub r#type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingText {
    pub recipient: String,
    pub content: String,
    pub reply_to: Option<ReplyTo>,
    pub group_name: Option<String>,
    #[serde(rename = "isGroup", default)]
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingMedia {
    pub recipient: String,
    pub file_path: Option<String>,
    pub file_data: Option<Vec<u8>>,
    pub file_name: Option<String>,
    pub file_type: Option<String>,
    pub msg_type: Option<String>,
    pub group_name: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    #[serde(rename = "isGroup", default)]
    pub is_group: bool,
    pub group_members: Option<Vec<String>>,
    pub reply_to: Option<ReplyTo>,
}

#[tauri::command]
pub async fn process_outgoing_text(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    handlers::text::process_outgoing_text(app, db_state, net_state, payload).await
}

#[tauri::command]
pub async fn process_outgoing_group_text(
    app: AppHandle,
    payload: OutgoingText,
) -> Result<serde_json::Value, String> {
    handlers::text::process_outgoing_group_text(app, payload).await
}

#[tauri::command]
pub fn process_outgoing_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    handlers::media::process_outgoing_media(app, payload)
}

#[tauri::command]
pub async fn process_outgoing_group_media(
    app: AppHandle,
    payload: OutgoingMedia,
) -> Result<serde_json::Value, String> {
    handlers::media::process_outgoing_group_media(app, payload).await
}

#[tauri::command]
pub async fn send_typing_status(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    is_typing: bool,
) -> Result<(), String> {
    handlers::status::send_typing_status(app, db_state, net_state, peer_hash, is_typing).await
}

#[tauri::command]
pub async fn send_receipt(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    msg_ids: Vec<String>,
    status: String,
) -> Result<(), String> {
    handlers::status::send_receipt(app, db_state, net_state, peer_hash, msg_ids, status).await
}

#[tauri::command]
pub async fn send_profile_update(
    app: AppHandle,
    db_state: State<'_, DbState>,
    net_state: State<'_, NetworkState>,
    peer_hash: String,
    alias: Option<String>,
) -> Result<(), String> {
    handlers::status::send_profile_update(app, db_state, net_state, peer_hash, alias).await
}
