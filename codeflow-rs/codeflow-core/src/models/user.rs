use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
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
