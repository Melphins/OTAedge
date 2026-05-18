use crate::alerts::AlertManager;
use std::sync::Arc;
use tracing::error;

/// Send deployment failure alert
pub async fn alert_deployment_failure(
    alert_manager: &Arc<tokio::sync::Mutex<AlertManager>>,
    deployment_id: &str,
    device_id: &str,
    error: &str,
) {
    let alert = crate::alerts::create_deployment_failed_alert(deployment_id, device_id, error);

    let mut manager = alert_manager.lock().await;
    if let Err(e) = manager.send_alert(alert).await {
        error!("Failed to send deployment failure alert: {}", e);
    }
}

/// Send device offline alert
pub async fn alert_device_offline(
    alert_manager: &Arc<tokio::sync::Mutex<AlertManager>>,
    device_id: &str,
    last_seen: &str,
) {
    let alert = crate::alerts::create_device_offline_alert(device_id, last_seen);

    let mut manager = alert_manager.lock().await;
    if let Err(e) = manager.send_alert(alert).await {
        error!("Failed to send device offline alert: {}", e);
    }
}

/// Send system error alert
pub async fn alert_system_error(
    alert_manager: &Arc<tokio::sync::Mutex<AlertManager>>,
    component: &str,
    error: &str,
) {
    let alert = crate::alerts::create_system_error_alert(component, error);

    let mut manager = alert_manager.lock().await;
    if let Err(e) = manager.send_alert(alert).await {
        error!("Failed to send system error alert: {}", e);
    }
}
