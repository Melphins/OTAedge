use crate::{deployment_flow, AppState};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use futures::stream::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    // Enforce WSS in production
    if env::var("APP_ENV")
        .map(|v| v == "production")
        .unwrap_or(false)
    {
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        if proto != "https" && proto != "wss" {
            return (
                StatusCode::FORBIDDEN,
                "WebSocket connections must use WSS in production",
            )
                .into_response();
        }
    }

    // Extract token from query parameter
    let token = match query.get("token") {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "Missing token").into_response(),
    };

    // Look up device by token
    let device = match state.repo.get_device_by_token(token).await {
        Ok(Some(device)) => device,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "Invalid token").into_response();
        }
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    let device_id = device.device_id;
    ws.on_upgrade(move |socket| async move { handle_socket(socket, state, device_id).await })
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, device_id: String) {
    let (tx, mut rx) = mpsc::channel::<Message>(32);
    let mut socket = socket;

    // Immediately register the connection
    state
        .connections
        .lock()
        .await
        .insert(device_id.clone(), tx.clone());
    state.metrics.ws_connections_active.inc();
    // Update device status to online and last_seen
    let _ = state.repo.update_device_status(&device_id, "online").await;
    let _ = state.repo.update_device_last_seen(&device_id).await;
    info!("Device {} connected via WebSocket", device_id);

    // Audit log: device connected
    if let Ok(Some(device)) = state.repo.get_device_by_device_id(&device_id).await {
        let _ = state
            .repo
            .insert_audit_log(
                "device",
                Some(device.id),
                "websocket.connect",
                Some("device"),
                Some(device.id),
                None,
                None,
                None,
                None,
                None,
            )
            .await;
    }

    // After registration, check for pending deployments for this device and push them
    // Fetch device by device_id string to get UUID
    if let Ok(Some(device)) = state.repo.get_device_by_device_id(&device_id).await {
        let pending_deployments = state
            .repo
            .get_pending_deployments_by_device(device.id)
            .await;
        if let Ok(deployments) = pending_deployments {
            for deployment in deployments {
                // Get model details
                if let Ok(Some(model)) = state.repo.get_model_by_id(deployment.model_id).await {
                    let msg = serde_json::json!({
                        "type": "model_update",
                        "deployment_id": deployment.id,
                        "model_id": deployment.model_id,
                        "model_hash": model.hash_sha256,
                        "download_url": format!("/api/device/models/{}/download", deployment.model_id),
                    });
                    let connections = state.connections.lock().await;
                    if let Some(sender) = connections.get(&device_id) {
                        if let Err(e) = sender.send(Message::Text(msg.to_string())).await {
                            error!(
                                "Failed to send pending model_update to device {}: {}",
                                device_id, e
                            );
                        } else {
                            // Update deployment_device status to 'deployed' and record start time
                            let _ = state
                                .repo
                                .update_deployment_device_status(
                                    deployment.id,
                                    device.id,
                                    "deployed",
                                    Some(chrono::Utc::now()),
                                    None,
                                    None,
                                )
                                .await;

                            // Increment devices_deployed counter atomically
                            let _ = sqlx::query!(
                                "UPDATE deployments
                                 SET devices_deployed = devices_deployed + 1
                                 WHERE id = $1",
                                deployment.id
                            )
                            .execute(&state.repo.pool)
                            .await;

                            // Also update deployment-level status to 'deployed' (if not already)
                            let _ = state
                                .repo
                                .update_deployment_status(
                                    deployment.id,
                                    "deployed".to_string(),
                                    Some(chrono::Utc::now()),
                                    None,
                                )
                                .await;
                        }
                    }
                }
            }
        }
    }

    // Now handle the connection
    loop {
        tokio::select! {
            // Incoming from client
            msg = socket.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(data) = serde_json::from_str::<Value>(&text) {
                            // Update last_seen on any message
                            let _ = state.repo.update_device_last_seen(&device_id).await;

                            // If heartbeat includes metrics, update device metrics
                            if let Some("heartbeat") = data["type"].as_str() {
                                if let Some(metrics_obj) = data.get("metrics").and_then(|m| m.as_object()) {
                                    // CPU usage percentage
                                    if let Some(cpu) = metrics_obj.get("cpu_percent").and_then(|v| v.as_f64()) {
                                        state.metrics.device_cpu_usage.with_label_values(&[&device_id]).set(cpu as i64);
                                    }
                                    // Memory used bytes
                                    if let Some(mem_used) = metrics_obj.get("memory_used").and_then(|v| v.as_i64()) {
                                        state.metrics.device_memory_used.with_label_values(&[&device_id]).set(mem_used);
                                    }
                                    // Memory total bytes
                                    if let Some(mem_total) = metrics_obj.get("memory_total").and_then(|v| v.as_i64()) {
                                        state.metrics.device_memory_total.with_label_values(&[&device_id]).set(mem_total);
                                    }
                                    // Disk used bytes
                                    if let Some(disk_used) = metrics_obj.get("disk_used").and_then(|v| v.as_i64()) {
                                        state.metrics.device_disk_used.with_label_values(&[&device_id]).set(disk_used);
                                    }
                                    // Disk total bytes
                                    if let Some(disk_total) = metrics_obj.get("disk_total").and_then(|v| v.as_i64()) {
                                        state.metrics.device_disk_total.with_label_values(&[&device_id]).set(disk_total);
                                    }
                                }
                            }

                            // Handle update_confirmed
                            if let Some("update_confirmed") = data["type"].as_str() {
                                if let Some(deployment_id) = data.get("deployment_id").and_then(|d| d.as_str()) {
                                    if let Ok(uuid) = uuid::Uuid::parse_str(deployment_id) {
                                        info!("Model update confirmed by device: {} for deployment {}", device_id, deployment_id);

                                        // First, get the device's UUID (not device_id string) to update deployment_device
                                        if let Ok(Some(device)) = state.repo.get_device_by_device_id(&device_id).await {
                                            // Update deployment_device status to succeeded
                                            let _ = state.repo.update_deployment_device_status(
                                                uuid,
                                                device.id,
                                                "succeeded",
                                                Some(chrono::Utc::now()),
                                                Some(chrono::Utc::now()),
                                                None,
                                            ).await;

                                            // Audit log: deployment success
                                            let _ = state.repo.insert_audit_log(
                                                "device",
                                                Some(device.id),
                                                "deployment.success",
                                                Some("deployment"),
                                                Some(uuid),
                                                None,
                                                Some(serde_json::json!({
                                                    "device_id": device_id,
                                                })),
                                                None,
                                                None,
                                                None,
                                            ).await;

                                            let _ = sqlx::query(
                                                "UPDATE deployments SET devices_succeeded = COALESCE(devices_succeeded, 0) + 1 WHERE id = $1",
                                            )
                                            .bind(uuid)
                                            .execute(&state.repo.pool)
                                            .await;

                                            state.repo.invalidate_deployment_cache(uuid).await;

                                            if let Err(e) = deployment_flow::advance_deployment_after_device_result(&state, uuid).await {
                                                error!(
                                                    "Failed to advance deployment {} after device result: {}",
                                                    uuid, e
                                                );
                                            }
                                        }

                                        state.metrics.deployments_total
                                            .with_label_values(&["completed"])
                                            .inc();
                                    } else {
                                        error!("Invalid deployment_id in update_confirmed: {}", deployment_id);
                                    }
                                }
                            }

                            // Handle update_failed - device reports deployment failure
                            if let Some("update_failed") = data["type"].as_str() {
                                if let Some(deployment_id) = data.get("deployment_id").and_then(|d| d.as_str()) {
                                    if let Ok(uuid) = uuid::Uuid::parse_str(deployment_id) {
                                        info!("Model update failed on device: {} for deployment {}", device_id, deployment_id);

                                        if let Some(error_msg) = data.get("error").and_then(|e| e.as_str()) {
                                            if let Ok(Some(device)) = state.repo.get_device_by_device_id(&device_id).await {
                                                // Update deployment_device status to failed
                                                let _ = state
                                                    .repo
                                                    .update_deployment_device_status(
                                                        uuid,
                                                        device.id,
                                                        "failed",
                                                        None,
                                                        None,
                                                        Some(error_msg.to_string()),
                                                    )
                                                    .await;

                                                // Audit log: deployment failure
                                                let _ = state
                                                    .repo
                                                    .insert_audit_log(
                                                        "device",
                                                        Some(device.id),
                                                        "deployment.failure",
                                                        Some("deployment"),
                                                        Some(uuid),
                                                        None,
                                                        Some(serde_json::json!({
                                                            "device_id": device_id,
                                                            "error": error_msg,
                                                        })),
                                                        None,
                                                        None,
                                                        None,
                                                    )
                                                    .await;

                                                // Increment devices_failed counter
                                                let _ = sqlx::query(
                                                    "UPDATE deployments SET devices_failed = COALESCE(devices_failed, 0) + 1 WHERE id = $1",
                                                )
                                                .bind(uuid)
                                                .execute(&state.repo.pool)
                                                .await;

                                                // Send alert for deployment failure
                                                let alert_manager = state.alert_manager.clone();
                                                let device_id_clone = device_id.clone();
                                                let deployment_id_clone = uuid.to_string();
                                                let error_msg_clone = error_msg.to_string();

                                                tokio::spawn(async move {
                                                    crate::alert_helpers::alert_deployment_failure(
                                                        &alert_manager,
                                                        &deployment_id_clone,
                                                        &device_id_clone,
                                                        &error_msg_clone,
                                                    ).await;
                                                });

                                                // Advance deployment to handle failure
                                                if let Err(e) = deployment_flow::advance_deployment_after_device_result(&state, uuid).await {
                                                    error!(
                                                        "Failed to advance deployment {} after failure: {}",
                                                        uuid, e
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        error!("Invalid deployment_id in update_failed: {}", deployment_id);
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        info!("WebSocket connection closed by device: {}", device_id);
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error for device {}: {}", device_id, e);
                        break;
                    }
                    None => {
                        info!("WebSocket connection ended for device: {}", device_id);
                        break;
                    }
                    _ => {} // binary, ping, pong
                }
            }
            // Outgoing to client
            Some(outgoing) = rx.recv() => {
                if let Err(e) = socket.send(outgoing).await {
                    error!("Failed to send message to device {}: {}", device_id, e);
                    break;
                }
            }
            else => break,
        }
    }

    // Cleanup: update status to offline before removing connection
    let _ = state.repo.update_device_status(&device_id, "offline").await;

    state.connections.lock().await.remove(&device_id);
    state.metrics.ws_connections_active.dec();
    info!("Device {} disconnected", device_id);

    // Audit log: device disconnected
    if let Ok(Some(device)) = state.repo.get_device_by_device_id(&device_id).await {
        let _ = state
            .repo
            .insert_audit_log(
                "device",
                Some(device.id),
                "websocket.disconnect",
                Some("device"),
                Some(device.id),
                None,
                None,
                None,
                None,
                None,
            )
            .await;
    }
}
