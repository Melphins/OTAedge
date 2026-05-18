use serde::{Deserialize, Serialize};
use sqlx::types::chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Device {
    pub id: Uuid,
    pub device_id: String,
    pub name: String,
    pub device_type: Option<String>,
    #[serde(skip)]
    pub token: String,
    pub status: String,
    pub last_seen: Option<DateTime<Utc>>,
    pub user_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub current_model_id: Option<Uuid>,
    pub model_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Model {
    pub id: Uuid,
    pub name: String,
    pub version: i32,
    pub is_active: bool,
    pub release_channel: String,
    pub file_name: String,
    pub file_size_bytes: Option<i64>,
    pub s3_key: String,
    pub hash_sha256: String,
    pub model_format: String,
    pub metadata: Option<sqlx::types::Json<serde_json::Value>>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Deployment {
    pub id: Uuid,
    pub device_id: Option<Uuid>,
    pub model_id: Uuid,
    pub status: String,
    pub rollout_strategy: String,
    pub rollout_percentage: i32,
    pub deployed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub current_phase: Option<i32>,
    pub devices_target: Option<i32>,
    pub devices_deployed: Option<i32>,
    pub devices_succeeded: Option<i32>,
    pub devices_failed: Option<i32>,
    #[sqlx(rename = "rollout_config", json)]
    #[serde(skip)]
    pub rollout_config: Option<sqlx::types::Json<serde_json::Value>>,
    pub rollback_of: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct DeploymentDevice {
    pub deployment_id: Uuid,
    pub device_id: Uuid,
    pub status: Option<String>,
    pub previous_model_id: Option<Uuid>,
    pub current_model_id: Option<Uuid>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
    pub phase: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DeploymentDeviceInfo {
    pub device_id: String,
    pub name: String,
    pub status: String,
    pub phase: i32,
    pub previous_model_id: Option<Uuid>,
    pub current_model_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct AuditLog {
    pub id: Uuid,
    pub actor_type: String,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub old_state: Option<sqlx::types::Json<serde_json::Value>>,
    pub new_state: Option<sqlx::types::Json<serde_json::Value>>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Option<sqlx::types::Json<serde_json::Value>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Alert {
    pub id: Uuid,
    pub severity: String,
    pub title: String,
    pub description: String,
    pub source: String,
    pub status: String,
    pub device_id: Option<Uuid>,
    pub deployment_id: Option<Uuid>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub acknowledged_by: Option<Uuid>,
    pub silenced_until: Option<DateTime<Utc>>,
    pub metadata: Option<sqlx::types::Json<serde_json::Value>>,
    pub created_at: DateTime<Utc>,
}
