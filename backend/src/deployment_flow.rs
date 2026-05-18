use crate::AppState;
use axum::extract::ws::Message;
use sqlx::Row;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

pub async fn push_deployment_phase(
    state: &Arc<AppState>,
    deployment_id: Uuid,
    phase: i32,
) -> Result<(), String> {
    let deployment = state
        .repo
        .get_deployment_by_id(deployment_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Deployment not found".to_string())?;

    let model = state
        .repo
        .get_model_by_id(deployment.model_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Model not found".to_string())?;

    let devices = sqlx::query(
        r#"
        SELECT d.id, d.device_id
        FROM deployment_devices dd
        JOIN devices d ON d.id = dd.device_id
        WHERE dd.deployment_id = $1
          AND dd.phase = $2
          AND dd.status = 'pending'
        ORDER BY d.created_at ASC
        "#,
    )
    .bind(deployment_id)
    .bind(phase)
    .fetch_all(&state.repo.pool)
    .await
    .map_err(|e| e.to_string())?;

    for row in devices {
        let device_uuid: Uuid = row.try_get("id").map_err(|e| e.to_string())?;
        let device_id: String = row.try_get("device_id").map_err(|e| e.to_string())?;

        let sender = {
            let connections = state.connections.lock().await;
            connections.get(&device_id).cloned()
        };

        let Some(sender) = sender else {
            warn!(
                "Device {} is not connected for deployment {} phase {}",
                device_id, deployment_id, phase
            );
            continue;
        };

        let msg = serde_json::json!({
            "type": "model_update",
            "deployment_id": deployment.id,
            "model_id": deployment.model_id,
            "model_hash": model.hash_sha256,
            "download_url": format!("/api/device/models/{}/download", deployment.model_id),
        });

        if let Err(e) = sender.send(Message::Text(msg.to_string())).await {
            error!(
                "Failed to send deployment {} phase {} to device {}: {}",
                deployment_id, phase, device_id, e
            );
            continue;
        }

        let updated = sqlx::query(
            r#"
            UPDATE deployment_devices
            SET status = 'deployed', started_at = COALESCE(started_at, NOW())
            WHERE deployment_id = $1
              AND device_id = $2
              AND status = 'pending'
            "#,
        )
        .bind(deployment_id)
        .bind(device_uuid)
        .execute(&state.repo.pool)
        .await
        .map_err(|e| e.to_string())?;

        if updated.rows_affected() > 0 {
            sqlx::query(
                r#"
                UPDATE deployments
                SET devices_deployed = COALESCE(devices_deployed, 0) + 1,
                    deployed_at = COALESCE(deployed_at, NOW())
                WHERE id = $1
                "#,
            )
            .bind(deployment_id)
            .execute(&state.repo.pool)
            .await
            .map_err(|e| e.to_string())?;

            state
                .metrics
                .deployments_total
                .with_label_values(&["deployed"])
                .inc();
        }
    }

    state.repo.invalidate_deployment_cache(deployment_id).await;
    Ok(())
}

pub async fn advance_deployment_after_device_result(
    state: &Arc<AppState>,
    deployment_id: Uuid,
) -> Result<(), String> {
    let row = sqlx::query(
        r#"
        SELECT
            status,
            current_phase,
            COALESCE(devices_target, 0) AS devices_target,
            COALESCE(devices_deployed, 0) AS devices_deployed,
            COALESCE(devices_succeeded, 0) AS devices_succeeded,
            rollout_config
        FROM deployments
        WHERE id = $1
        "#,
    )
    .bind(deployment_id)
    .fetch_optional(&state.repo.pool)
    .await
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "Deployment not found".to_string())?;

    let status: String = row
        .try_get::<Option<String>, _>("status")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "pending".to_string());
    if status != "deploying" {
        return Ok(());
    }

    let current_phase: i32 = row
        .try_get::<Option<i32>, _>("current_phase")
        .unwrap_or(None)
        .unwrap_or(0);
    let devices_target: i32 = row.try_get("devices_target").unwrap_or(0);
    let devices_deployed: i32 = row.try_get("devices_deployed").unwrap_or(0);
    let devices_succeeded: i32 = row.try_get("devices_succeeded").unwrap_or(0);
    let rollout_config: serde_json::Value = row
        .try_get::<Option<serde_json::Value>, _>("rollout_config")
        .unwrap_or(None)
        .unwrap_or_else(|| serde_json::json!({}));

    if devices_target > 0
        && devices_succeeded >= devices_target
        && devices_deployed >= devices_target
    {
        sqlx::query(
            "UPDATE deployments SET status = 'completed', completed_at = NOW() WHERE id = $1",
        )
        .bind(deployment_id)
        .execute(&state.repo.pool)
        .await
        .map_err(|e| e.to_string())?;
        state.repo.invalidate_deployment_cache(deployment_id).await;
        info!("Deployment {} completed", deployment_id);
        return Ok(());
    }

    let total_in_phase: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM deployment_devices WHERE deployment_id = $1 AND phase = $2",
    )
    .bind(deployment_id)
    .bind(current_phase)
    .fetch_one(&state.repo.pool)
    .await
    .map_err(|e| e.to_string())?;

    let completed_in_phase: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM deployment_devices
        WHERE deployment_id = $1
          AND phase = $2
          AND status IN ('succeeded', 'failed')
        "#,
    )
    .bind(deployment_id)
    .bind(current_phase)
    .fetch_one(&state.repo.pool)
    .await
    .map_err(|e| e.to_string())?;

    if total_in_phase == 0 || completed_in_phase < total_in_phase {
        return Ok(());
    }

    let next_phase = current_phase + 1;
    let phase_count = rollout_config
        .get("phases")
        .and_then(|p| p.as_array())
        .map(|p| p.len() as i32)
        .unwrap_or(1);

    if next_phase >= phase_count {
        return Ok(());
    }

    sqlx::query("UPDATE deployments SET current_phase = $1 WHERE id = $2")
        .bind(next_phase)
        .bind(deployment_id)
        .execute(&state.repo.pool)
        .await
        .map_err(|e| e.to_string())?;
    state.repo.invalidate_deployment_cache(deployment_id).await;
    info!(
        "Deployment {} advanced to phase {}",
        deployment_id, next_phase
    );

    push_deployment_phase(state, deployment_id, next_phase).await
}
