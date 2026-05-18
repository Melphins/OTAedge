use crate::alert_helpers;
use crate::AppState;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Start the device monitoring background task
pub async fn start_device_monitor(state: Arc<AppState>) {
    let mut interval = time::interval(Duration::from_secs(60)); // Check every minute

    info!("Starting device monitoring task");

    loop {
        interval.tick().await;

        if let Err(e) = check_offline_devices(&state).await {
            error!("Device monitoring check failed: {}", e);
        }
    }
}

/// Check for devices that have gone offline
async fn check_offline_devices(state: &Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    // Check for devices that haven't sent heartbeat in 5 minutes
    let threshold_minutes = 5;

    let offline_devices = state.repo.get_device_offline_candidates(threshold_minutes).await?;

    if offline_devices.is_empty() {
        return Ok(());
    }

    info!("Found {} potentially offline devices", offline_devices.len());

    for device in offline_devices {
        // Skip if device is already marked as offline
        if device.status != "online" {
            continue;
        }

        let last_seen_str = device
            .last_seen
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| "never".to_string());

        warn!(
            "Device {} ({}) appears offline. Last seen: {}",
            device.device_id, device.name, last_seen_str
        );

        // Update device status to offline
        if let Err(e) = state.repo.update_device_status(&device.device_id, "offline").await {
            error!(
                "Failed to update device {} status to offline: {}",
                device.device_id, e
            );
            continue;
        }

        // Send alert for each offline device
        let alert_manager = state.alert_manager.clone();
        let device_id = device.device_id.clone();
        let last_seen = last_seen_str.clone();

        tokio::spawn(async move {
            alert_helpers::alert_device_offline(&alert_manager, &device_id, &last_seen).await;
        });

        // Also update metrics
        state.metrics.device_status_offline.inc();
    }

    Ok(())
}

/// Start the deployment cleanup background task
pub async fn start_deployment_cleanup(state: Arc<AppState>) {
    let mut interval = time::interval(Duration::from_secs(300)); // Check every 5 minutes

    info!("Starting deployment cleanup task");

    loop {
        interval.tick().await;

        if let Err(e) = cleanup_stuck_deployments(&state).await {
            error!("Deployment cleanup failed: {}", e);
        }
    }
}

/// Clean up deployments that have been stuck for too long
async fn cleanup_stuck_deployments(state: &Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    // Find deployments that have been "deploying" for more than 24 hours
    let stuck_threshold_hours = 24;

    let stuck_deployments = state
        .repo
        .get_stuck_deployments(stuck_threshold_hours)
        .await?;

    if stuck_deployments.is_empty() {
        return Ok(());
    }

    warn!("Found {} stuck deployments", stuck_deployments.len());

    for deployment in stuck_deployments {
        error!(
            "Deployment {} has been stuck for more than {} hours",
            deployment.id, stuck_threshold_hours
        );

        // Send critical alert
        let alert_manager = state.alert_manager.clone();
        let deployment_id = deployment.id.to_string();

        tokio::spawn(async move {
            alert_helpers::alert_system_error(
                &alert_manager,
                "deployment",
                &format!(
                    "Deployment {} has been stuck in 'deploying' status for over {} hours",
                    deployment_id, stuck_threshold_hours
                ),
            )
            .await;
        });
    }

    Ok(())
}

/// Start metrics aggregation background task
pub async fn start_metrics_aggregation(state: Arc<AppState>) {
    let mut interval = time::interval(Duration::from_secs(300)); // Aggregate every 5 minutes

    info!("Starting metrics aggregation task");

    loop {
        interval.tick().await;

        if let Err(e) = aggregate_device_metrics(&state).await {
            error!("Metrics aggregation failed: {}", e);
        }
    }
}

/// Aggregate device metrics and check for anomalies
async fn aggregate_device_metrics(state: &Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    // Get all online devices
    let devices = state.repo.get_all_online_devices().await?;

    for device in devices {
        // Check if device has high CPU usage
        let cpu_usage = state
            .metrics
            .device_cpu_usage
            .get_metric_with_label_values(&[&device.device_id])
            .map(|m| m.get() as f64)
            .unwrap_or(0.0);

        if cpu_usage > 90.0 {
            warn!(
                "Device {} has high CPU usage: {:.1}%",
                device.device_id, cpu_usage
            );

            let alert_manager = state.alert_manager.clone();
            let device_id = device.device_id.clone();
            let device_name = device.name.clone();

            tokio::spawn(async move {
                let alert = crate::alerts::Alert {
                    id: Uuid::new_v4().to_string(),
                    severity: crate::alerts::AlertSeverity::Warning,
                    title: format!("High CPU Usage on Device"),
                    description: format!("Device {} ({}) has CPU usage at {:.1}%", device_name, device_id, cpu_usage),
                    source: crate::alerts::AlertSource::Device,
                    metadata: Some(serde_json::json!({
                        "device_id": device_id,
                        "device_name": device_name,
                        "cpu_usage": cpu_usage,
                    })),
                    created_at: chrono::Utc::now(),
                };

                let mut manager = alert_manager.lock().await;
                let _ = manager.send_alert(alert).await;
            });
        }

        // Check if device has low memory
        let memory_total = state
            .metrics
            .device_memory_total
            .get_metric_with_label_values(&[&device.device_id])
            .map(|m| m.get())
            .unwrap_or(0);

        let memory_used = state
            .metrics
            .device_memory_used
            .get_metric_with_label_values(&[&device.device_id])
            .map(|m| m.get())
            .unwrap_or(0);

        if memory_total > 0 {
            let memory_usage_percent = (memory_used as f64 / memory_total as f64) * 100.0;

            if memory_usage_percent > 95.0 {
                warn!(
                    "Device {} has low memory: {:.1}% used",
                    device.device_id, memory_usage_percent
                );

                let alert_manager = state.alert_manager.clone();
                let device_id = device.device_id.clone();
                let device_name = device.name.clone();

                tokio::spawn(async move {
                    let alert = crate::alerts::Alert {
                        id: Uuid::new_v4().to_string(),
                        severity: crate::alerts::AlertSeverity::Critical,
                        title: format!("Low Memory on Device"),
                        description: format!(
                            "Device {} ({}) has {:.1}% memory usage",
                            device_name, device_id, memory_usage_percent
                        ),
                        source: crate::alerts::AlertSource::Device,
                        metadata: Some(serde_json::json!({
                            "device_id": device_id,
                            "device_name": device_name,
                            "memory_usage_percent": memory_usage_percent,
                            "memory_total": memory_total,
                            "memory_used": memory_used,
                        })),
                        created_at: chrono::Utc::now(),
                    };

                    let mut manager = alert_manager.lock().await;
                    let _ = manager.send_alert(alert).await;
                });
            }
        }
    }

    Ok(())
}