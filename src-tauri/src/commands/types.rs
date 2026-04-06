use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbMessage {
    pub id: String,
    pub chat_address: String,
    pub sender_hash: String,
    pub content: String,
    pub timestamp: i64,
    pub r#type: String,
    pub status: String,
    pub attachment_json: Option<String>,
    #[serde(default)]
    pub is_starred: bool,
    #[serde(default)]
    pub is_group: bool,
    pub reply_to_json: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DbChat {
    pub address: String,
    #[serde(default)]
    pub is_group: bool,
    pub alias: Option<String>,
    pub last_msg: Option<String>,
    pub last_timestamp: Option<i64>,
    pub last_sender_hash: Option<String>,
    pub last_status: Option<String>,
    #[serde(default)]
    pub unread_count: i32,
    #[serde(default)]
    pub is_archived: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub trust_level: i32,
    #[serde(default)]
    pub is_blocked: bool,
    #[serde(default = "default_active")]
    pub is_active: bool,
    pub members: Option<Vec<String>>,
}

fn default_active() -> bool { true }

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbContact {
    pub hash: String,
    pub alias: Option<String>,
    pub is_blocked: bool,
    pub trust_level: i32,
}
