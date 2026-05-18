// Re-export all public modules
pub mod alert_helpers;
pub mod alerts;
pub mod auth;
pub mod cache;
pub mod deployment_flow;
pub mod error;
pub mod logging;
pub mod metrics;
pub mod metrics_middleware;
pub mod models;
pub mod monitor;
pub mod rbac;
pub mod repositories;
pub mod retry;
pub mod storage;
pub mod websocket;

use crate::alerts::AlertManager;
use crate::cache::Cache;
use crate::metrics::Metrics;
use axum::extract::ws::Message;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};

// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub repo: repositories::Repository,
    pub jwt_secret: String,
    pub connections: Arc<Mutex<HashMap<String, mpsc::Sender<Message>>>>,
    pub metrics: Arc<Metrics>,
    pub rate_limiter: Arc<Mutex<HashMap<IpAddr, (Instant, u32)>>>,
    pub user_rate_limiter: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    pub storage: storage::Storage,
    pub alert_manager: Arc<Mutex<AlertManager>>,
    pub cache: Option<Cache>,
}
