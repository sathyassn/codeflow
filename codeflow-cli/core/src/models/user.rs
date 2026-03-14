use serde::{Deserialize, Serialize};

use super::serde_helpers::{deserialize_record_id, serialize_record_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub email: String,
    pub display_name: Option<String>,
    pub git_username: Option<String>,
    pub role: String,
    pub last_host: Option<String>,
    pub last_machine_fingerprint: Option<String>,
    pub first_seen_at: String,
    pub last_active_at: Option<String>,
    #[serde(default)]
    pub preferences: serde_json::Value,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    #[serde(
        deserialize_with = "deserialize_record_id",
        serialize_with = "serialize_record_id"
    )]
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub git_remote_url: Option<String>,
    pub default_branch: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
