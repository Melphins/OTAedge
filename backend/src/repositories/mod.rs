use crate::models::*;
use sha2::{Digest, Sha256};
use sqlx::{
    postgres::PgPoolOptions,
    types::chrono::{DateTime, Utc},
    PgPool,
};
use tracing::debug;
use uuid::Uuid;

#[derive(Clone)]
pub struct Repository {
    pub pool: PgPool,
    pub cache: Option<crate::cache::Cache>,
}

impl Repository {
    pub async fn new(
        database_url: &str,
        cache: Option<crate::cache::Cache>,
    ) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(database_url)
            .await?;
        Ok(Self { pool, cache })
    }

    // Cache helper methods
    async fn cache_get<T>(&self, key: &str) -> Result<Option<T>, sqlx::Error>
    where
        T: serde::de::DeserializeOwned + std::fmt::Debug + std::marker::Send + 'static,
    {
        if let Some(cache) = &self.cache {
            match cache.get::<T>(key).await {
                Ok(Some(value)) => return Ok(Some(value)),
                Ok(None) => return Ok(None),
                Err(_) => {
                    // Cache error, fall through to DB
                }
            }
        }
        Ok(None)
    }

    async fn cache_set<T>(&self, key: &str, value: &T, ttl_secs: Option<u64>)
    where
        T: serde::ser::Serialize + std::fmt::Debug + std::marker::Send,
    {
        if let Some(cache) = &self.cache {
            let ttl = ttl_secs.map(std::time::Duration::from_secs);
            let _ = cache.set(key, value, ttl).await;
        }
    }

    async fn cache_delete(&self, key: &str) {
        if let Some(cache) = &self.cache {
            let _ = cache.delete(key).await;
        }
    }

    pub async fn invalidate_deployment_cache(&self, deployment_id: Uuid) {
        self.cache_delete(&format!("deployment:{}", deployment_id))
            .await;
        self.cache_delete(&format!("deployment_devices:{}", deployment_id))
            .await;
        self.cache_delete("deployments:pending").await;
    }

    // Device CRUD with caching
    pub async fn create_user(
        &self,
        email: String,
        username: String,
        password_hash: String,
    ) -> Result<User, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO users (email, username, password_hash)
            VALUES ($1, $2, $3)
            RETURNING id, email, username, password_hash, role, created_at, updated_at
            "#,
            email,
            username,
            password_hash
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(User {
            id: row.id,
            email: row.email,
            username: row.username,
            password_hash: row.password_hash,
            role: row.role.expect("role should not be null"),
            created_at: row.created_at.expect("created_at should not be null"),
            updated_at: row.updated_at.expect("updated_at should not be null"),
        })
    }

    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>, sqlx::Error> {
        let row = sqlx::query!("SELECT * FROM users WHERE email = $1", email)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| User {
            id: r.id,
            email: r.email,
            username: r.username,
            password_hash: r.password_hash,
            role: r.role.expect("role should not be null"),
            created_at: r.created_at.expect("created_at should not be null"),
            updated_at: r.updated_at.expect("updated_at should not be null"),
        }))
    }

    pub async fn get_user_by_id(&self, id: Uuid) -> Result<Option<User>, sqlx::Error> {
        let row = sqlx::query!("SELECT * FROM users WHERE id = $1", id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| User {
            id: r.id,
            email: r.email,
            username: r.username,
            password_hash: r.password_hash,
            role: r.role.expect("role should not be null"),
            created_at: r.created_at.expect("created_at should not be null"),
            updated_at: r.updated_at.expect("updated_at should not be null"),
        }))
    }

    // Device CRUD
    pub async fn create_device(
        &self,
        device_id: String,
        name: String,
        device_type: Option<String>,
        token: String,
        user_id: Option<Uuid>,
    ) -> Result<Device, sqlx::Error> {
        // Hash the token before storing
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));

        let row = sqlx::query!(
            r#"
            INSERT INTO devices (device_id, name, device_type, token, user_id)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, device_id, name, device_type, token, status, last_seen, user_id, created_at, current_model_id, model_version
            "#,
            device_id, name, device_type, token_hash, user_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(Device {
            id: row.id,
            device_id: row.device_id,
            name: row.name,
            device_type: row.device_type,
            token: row.token,
            status: row.status.expect("status should not be null"),
            last_seen: row.last_seen,
            user_id: row.user_id,
            created_at: row.created_at.expect("created_at should not be null"),
            current_model_id: row.current_model_id,
            model_version: row.model_version,
        })
    }

    pub async fn get_device_by_token(&self, token: &str) -> Result<Option<Device>, sqlx::Error> {
        // Hash the token to compare with stored hash
        let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));

        let row = sqlx::query!("SELECT * FROM devices WHERE token = $1", token_hash)
            .fetch_optional(&self.pool)
            .await?;

        Ok(row.map(|r| Device {
            id: r.id,
            device_id: r.device_id,
            name: r.name,
            device_type: r.device_type,
            token: r.token,
            status: r.status.expect("status should not be null"),
            last_seen: r.last_seen,
            user_id: r.user_id,
            created_at: r.created_at.expect("created_at should not be null"),
            current_model_id: r.current_model_id,
            model_version: r.model_version,
        }))
    }

    pub async fn get_device_by_id(&self, id: Uuid) -> Result<Option<Device>, sqlx::Error> {
        let cache_key = format!("device:{}", id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Device>(&cache_key).await? {
            return Ok(Some(cached));
        }

        let row = sqlx::query!("SELECT * FROM devices WHERE id = $1", id)
            .fetch_optional(&self.pool)
            .await?;

        let device = row.map(|r| Device {
            id: r.id,
            device_id: r.device_id.clone(),
            name: r.name.clone(),
            device_type: r.device_type.clone(),
            token: r.token.clone(),
            status: r.status.expect("status should not be null"),
            last_seen: r.last_seen,
            user_id: r.user_id,
            created_at: r.created_at.expect("created_at should not be null"),
            current_model_id: r.current_model_id,
            model_version: r.model_version,
        });

        if let Some(ref dev) = device {
            self.cache_set(&cache_key, dev, Some(300)).await; // 5 min TTL
        }

        Ok(device)
    }

    pub async fn get_device_by_device_id(
        &self,
        device_id: &str,
    ) -> Result<Option<Device>, sqlx::Error> {
        let cache_key = format!("device_by_device_id:{}", device_id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Device>(&cache_key).await? {
            return Ok(Some(cached));
        }

        let row = sqlx::query!("SELECT * FROM devices WHERE device_id = $1", device_id)
            .fetch_optional(&self.pool)
            .await?;

        let device = row.map(|r| Device {
            id: r.id,
            device_id: r.device_id.clone(),
            name: r.name.clone(),
            device_type: r.device_type.clone(),
            token: r.token.clone(),
            status: r.status.expect("status should not be null"),
            last_seen: r.last_seen,
            user_id: r.user_id,
            created_at: r.created_at.expect("created_at should not be null"),
            current_model_id: r.current_model_id,
            model_version: r.model_version,
        });

        if let Some(ref dev) = device {
            self.cache_set(&cache_key, dev, Some(300)).await; // 5 min TTL
        }

        Ok(device)
    }

    pub async fn get_devices_by_user_id(&self, user_id: Uuid) -> Result<Vec<Device>, sqlx::Error> {
        let cache_key = format!("user_devices:{}", user_id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Vec<Device>>(&cache_key).await? {
            return Ok(cached);
        }

        let rows = sqlx::query!(
            "SELECT * FROM devices WHERE user_id = $1 ORDER BY created_at DESC",
            user_id
        )
        .fetch_all(&self.pool)
        .await?;

        let devices = rows
            .into_iter()
            .map(|r| Device {
                id: r.id,
                device_id: r.device_id.clone(),
                name: r.name.clone(),
                device_type: r.device_type.clone(),
                token: r.token.clone(),
                status: r.status.expect("status should not be null"),
                last_seen: r.last_seen,
                user_id: r.user_id,
                created_at: r.created_at.expect("created_at should not be null"),
                current_model_id: r.current_model_id,
                model_version: r.model_version,
            })
            .collect();

        self.cache_set(&cache_key, &devices, Some(60)).await; // 60 sec TTL for device list
        Ok(devices)
    }

    pub async fn get_all_devices(&self) -> Result<Vec<Device>, sqlx::Error> {
        let rows = sqlx::query!("SELECT * FROM devices ORDER BY created_at DESC")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| Device {
                id: r.id,
                device_id: r.device_id,
                name: r.name,
                device_type: r.device_type,
                token: r.token,
                status: r.status.expect("status should not be null"),
                last_seen: r.last_seen,
                user_id: r.user_id,
                created_at: r.created_at.expect("created_at should not be null"),
                current_model_id: r.current_model_id,
                model_version: r.model_version,
            })
            .collect())
    }

    pub async fn update_device_last_seen(&self, device_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE devices SET last_seen = NOW() WHERE device_id = $1",
            device_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_device_status(
        &self,
        device_id: &str,
        status: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE devices SET status = $1 WHERE device_id = $2",
            status,
            device_id
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // Model CRUD
    pub async fn create_model(
        &self,
        name: String,
        version: i32,
        file_name: String,
        file_size_bytes: Option<i64>,
        s3_key: String,
        hash_sha256: String,
        model_format: String,
        metadata: serde_json::Value,
        created_by: Option<Uuid>,
    ) -> Result<Model, sqlx::Error> {
        let model = sqlx::query_as::<_, Model>(
            r#"
            INSERT INTO models (name, version, file_name, file_size_bytes, s3_key, hash_sha256, model_format, metadata, created_by)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            "#,
        )
        .bind(name)
        .bind(version)
        .bind(file_name)
        .bind(file_size_bytes)
        .bind(s3_key)
        .bind(hash_sha256)
        .bind(model_format)
        .bind(metadata)
        .bind(created_by)
        .fetch_one(&self.pool)
        .await?;

        // Invalidate models list cache
        self.cache_delete("models:list").await;

        Ok(model)
    }

    pub async fn get_model_by_id(&self, id: Uuid) -> Result<Option<Model>, sqlx::Error> {
        let cache_key = format!("model:{}", id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Model>(&cache_key).await? {
            return Ok(Some(cached));
        }

        let model = sqlx::query_as::<_, Model>(
            r#"
            SELECT
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            FROM models
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(ref m) = model {
            self.cache_set(&cache_key, m, Some(600)).await; // 10 min TTL
        }

        Ok(model)
    }

    pub async fn get_models_by_name(&self, name: &str) -> Result<Vec<Model>, sqlx::Error> {
        let cache_key = format!("models:name:{}", name);

        // Check cache first
        if let Some(cached) = self.cache_get::<Vec<Model>>(&cache_key).await? {
            return Ok(cached);
        }

        let models = sqlx::query_as::<_, Model>(
            r#"
            SELECT
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            FROM models
            WHERE name = $1
            ORDER BY version DESC
            "#,
        )
        .bind(name)
        .fetch_all(&self.pool)
        .await?;

        self.cache_set(&cache_key, &models, Some(300)).await; // 5 min TTL
        Ok(models)
    }

    pub async fn list_models(&self) -> Result<Vec<Model>, sqlx::Error> {
        let cache_key = "models:list";

        // Check cache first
        if let Some(cached) = self.cache_get::<Vec<Model>>(&cache_key).await? {
            return Ok(cached);
        }

        let models = sqlx::query_as::<_, Model>(
            r#"
            SELECT
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            FROM models
            ORDER BY created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        self.cache_set(&cache_key, &models, Some(300)).await; // 5 min TTL
        Ok(models)
    }

    pub async fn list_models_paginated(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Model>, sqlx::Error> {
        sqlx::query_as::<_, Model>(
            r#"
            SELECT
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            FROM models
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn get_active_model(
        &self,
        name: &str,
        release_channel: &str,
    ) -> Result<Option<Model>, sqlx::Error> {
        sqlx::query_as::<_, Model>(
            r#"
            SELECT
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            FROM models
            WHERE name = $1 AND release_channel = $2 AND is_active = TRUE
            ORDER BY version DESC
            LIMIT 1
            "#,
        )
        .bind(name)
        .bind(release_channel)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn set_active_model(
        &self,
        model_id: Uuid,
        release_channel: &str,
    ) -> Result<Model, sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        let model = sqlx::query_as::<_, Model>(
            r#"
            SELECT
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            FROM models
            WHERE id = $1
            "#,
        )
        .bind(model_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(sqlx::Error::RowNotFound)?;

        sqlx::query("UPDATE models SET is_active = FALSE WHERE name = $1 AND release_channel = $2")
            .bind(&model.name)
            .bind(release_channel)
            .execute(&mut *tx)
            .await?;

        let updated = sqlx::query_as::<_, Model>(
            r#"
            UPDATE models
            SET is_active = TRUE, release_channel = $2, updated_at = NOW()
            WHERE id = $1
            RETURNING
                id, name, version, file_name, file_size_bytes, s3_key, hash_sha256,
                model_format, metadata, created_by, created_at, updated_at,
                is_active, release_channel
            "#,
        )
        .bind(model_id)
        .bind(release_channel)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        self.cache_delete("models:list").await;
        self.cache_delete(&format!("model:{}", model_id)).await;
        self.cache_delete(&format!("models:name:{}", updated.name))
            .await;

        Ok(updated)
    }

    pub async fn model_reference_count(&self, model_id: Uuid) -> Result<i64, sqlx::Error> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT
                (
                    SELECT COUNT(*) FROM deployments WHERE model_id = $1
                ) +
                (
                    SELECT COUNT(*) FROM deployment_devices
                    WHERE previous_model_id = $1 OR current_model_id = $1
                ) +
                (
                    SELECT COUNT(*) FROM devices WHERE current_model_id = $1
                ) AS "count!"
            "#,
        )
        .bind(model_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    pub async fn s3_key_reference_count(&self, s3_key: &str) -> Result<i64, sqlx::Error> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM models WHERE s3_key = $1")
            .bind(s3_key)
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    pub async fn delete_model(&self, model_id: Uuid) -> Result<(), sqlx::Error> {
        let result = sqlx::query("DELETE FROM models WHERE id = $1")
            .bind(model_id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(sqlx::Error::RowNotFound);
        }

        self.cache_delete("models:list").await;
        self.cache_delete(&format!("model:{}", model_id)).await;

        Ok(())
    }

    // Deployment CRUD
    pub async fn create_deployment(
        &self,
        device_id: Option<Uuid>,
        model_id: Uuid,
        status: String,
        rollout_strategy: String,
        rollout_percentage: i32,
        rollout_config: Option<serde_json::Value>,
        current_phase: Option<i32>,
        devices_target: Option<i32>,
        devices_deployed: Option<i32>,
        devices_succeeded: Option<i32>,
        devices_failed: Option<i32>,
        rollback_of: Option<Uuid>,
    ) -> Result<Deployment, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO deployments (
                device_id, model_id, status, rollout_strategy, rollout_percentage,
                rollout_config, current_phase, devices_target, devices_deployed,
                devices_succeeded, devices_failed, rollback_of
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, device_id, model_id, status, rollout_strategy, rollout_percentage,
                      deployed_at, completed_at, created_at,
                      current_phase, devices_target, devices_deployed,
                      devices_succeeded, devices_failed,
                      rollout_config, rollback_of
            "#,
            device_id,
            model_id,
            status,
            rollout_strategy,
            rollout_percentage,
            rollout_config,
            current_phase,
            devices_target,
            devices_deployed,
            devices_succeeded,
            devices_failed,
            rollback_of
        )
        .fetch_one(&self.pool)
        .await?;

        let deployment = Deployment {
            id: row.id,
            device_id: row.device_id,
            model_id: row.model_id.expect("model_id should not be null"),
            status: row.status.expect("status should not be null"),
            rollout_strategy: row
                .rollout_strategy
                .expect("rollout_strategy should not be null"),
            rollout_percentage: row
                .rollout_percentage
                .expect("rollout_percentage should not be null"),
            deployed_at: row.deployed_at,
            completed_at: row.completed_at,
            created_at: row.created_at.expect("created_at should not be null"),
            current_phase: row.current_phase,
            devices_target: row.devices_target,
            devices_deployed: row.devices_deployed,
            devices_succeeded: row.devices_succeeded,
            devices_failed: row.devices_failed,
            rollout_config: row.rollout_config.map(sqlx::types::Json),
            rollback_of: row.rollback_of,
        };

        // Invalidate pending deployments cache if this deployment is pending
        if status == "pending" {
            self.cache_delete("deployments:pending").await;
        }

        Ok(deployment)
    }

    pub async fn get_deployment_by_id(&self, id: Uuid) -> Result<Option<Deployment>, sqlx::Error> {
        let cache_key = format!("deployment:{}", id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Deployment>(&cache_key).await? {
            return Ok(Some(cached));
        }

        let row = sqlx::query!("SELECT * FROM deployments WHERE id = $1", id)
            .fetch_optional(&self.pool)
            .await?;

        let deployment = row.map(|r| Deployment {
            id: r.id,
            device_id: r.device_id,
            model_id: r.model_id.expect("model_id should not be null"),
            status: r.status.expect("status should not be null"),
            rollout_strategy: r
                .rollout_strategy
                .expect("rollout_strategy should not be null"),
            rollout_percentage: r
                .rollout_percentage
                .expect("rollout_percentage should not be null"),
            deployed_at: r.deployed_at,
            completed_at: r.completed_at,
            created_at: r.created_at.expect("created_at should not be null"),
            current_phase: r.current_phase,
            devices_target: r.devices_target,
            devices_deployed: r.devices_deployed,
            devices_succeeded: r.devices_succeeded,
            devices_failed: r.devices_failed,
            rollout_config: r.rollout_config.map(sqlx::types::Json),
            rollback_of: r.rollback_of,
        });

        if let Some(ref dep) = deployment {
            self.cache_set(&cache_key, dep, Some(300)).await; // 5 min TTL
        }

        Ok(deployment)
    }

    pub async fn list_deployments_by_user_id(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Deployment>, sqlx::Error> {
        sqlx::query_as::<_, Deployment>(
            r#"
            SELECT DISTINCT
                dep.id, dep.device_id, dep.model_id, dep.status, dep.rollout_strategy,
                dep.rollout_percentage, dep.deployed_at, dep.completed_at, dep.created_at,
                dep.current_phase, dep.devices_target, dep.devices_deployed,
                dep.devices_succeeded, dep.devices_failed, dep.rollout_config, dep.rollback_of
            FROM deployments dep
            LEFT JOIN deployment_devices dd ON dd.deployment_id = dep.id
            LEFT JOIN devices d ON d.id = dd.device_id OR d.id = dep.device_id
            WHERE d.user_id = $1
            ORDER BY dep.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn user_can_access_deployment(
        &self,
        user_id: Uuid,
        deployment_id: Uuid,
    ) -> Result<bool, sqlx::Error> {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT dep.id)
            FROM deployments dep
            LEFT JOIN deployment_devices dd ON dd.deployment_id = dep.id
            LEFT JOIN devices d ON d.id = dd.device_id OR d.id = dep.device_id
            WHERE dep.id = $1 AND d.user_id = $2
            "#,
        )
        .bind(deployment_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }

    pub async fn get_pending_deployments(&self) -> Result<Vec<Deployment>, sqlx::Error> {
        let cache_key = "deployments:pending";

        // Check cache first
        if let Some(cached) = self.cache_get::<Vec<Deployment>>(&cache_key).await? {
            return Ok(cached);
        }

        let rows = sqlx::query!(
            "SELECT * FROM deployments WHERE status = 'pending' ORDER BY created_at ASC"
        )
        .fetch_all(&self.pool)
        .await?;

        let deployments = rows
            .into_iter()
            .map(|r| Deployment {
                id: r.id,
                device_id: r.device_id,
                model_id: r.model_id.expect("model_id should not be null"),
                status: r.status.expect("status should not be null"),
                rollout_strategy: r
                    .rollout_strategy
                    .expect("rollout_strategy should not be null"),
                rollout_percentage: r
                    .rollout_percentage
                    .expect("rollout_percentage should not be null"),
                deployed_at: r.deployed_at,
                completed_at: r.completed_at,
                created_at: r.created_at.expect("created_at should not be null"),
                current_phase: r.current_phase,
                devices_target: r.devices_target,
                devices_deployed: r.devices_deployed,
                devices_succeeded: r.devices_succeeded,
                devices_failed: r.devices_failed,
                rollout_config: r.rollout_config.map(sqlx::types::Json),
                rollback_of: r.rollback_of,
            })
            .collect();

        self.cache_set(&cache_key, &deployments, Some(30)).await; // 30 sec TTL
        Ok(deployments)
    }

    pub async fn get_pending_deployments_by_device(
        &self,
        device_id: Uuid,
    ) -> Result<Vec<Deployment>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                d.id, d.device_id, d.model_id, d.status as d_status, d.rollout_strategy,
                d.rollout_percentage, d.deployed_at, d.completed_at, d.created_at,
                d.current_phase, d.devices_target, d.devices_deployed,
                d.devices_succeeded, d.devices_failed,
                d.rollout_config, d.rollback_of
            FROM deployments d
            JOIN deployment_devices dd ON d.id = dd.deployment_id
            WHERE dd.device_id = $1 AND dd.status = 'pending' AND d.status = 'deploying'
              AND dd.phase = d.current_phase
            ORDER BY d.created_at ASC
            "#,
            device_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Deployment {
                id: r.id,
                device_id: r.device_id,
                model_id: r.model_id.expect("model_id should not be null"),
                status: r.d_status.expect("status should not be null"),
                rollout_strategy: r
                    .rollout_strategy
                    .expect("rollout_strategy should not be null"),
                rollout_percentage: r
                    .rollout_percentage
                    .expect("rollout_percentage should not be null"),
                deployed_at: r.deployed_at,
                completed_at: r.completed_at,
                created_at: r.created_at.expect("created_at should not be null"),
                current_phase: r.current_phase,
                devices_target: r.devices_target,
                devices_deployed: r.devices_deployed,
                devices_succeeded: r.devices_succeeded,
                devices_failed: r.devices_failed,
                rollout_config: r.rollout_config.map(sqlx::types::Json),
                rollback_of: r.rollback_of,
            })
            .collect())
    }

    pub async fn update_deployment_status(
        &self,
        id: Uuid,
        status: String,
        deployed_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE deployments SET status = $1, deployed_at = $2, completed_at = $3 WHERE id = $4",
            status,
            deployed_at,
            completed_at,
            id
        )
        .execute(&self.pool)
        .await?;

        // Invalidate caches
        let deployment_key = format!("deployment:{}", id);
        self.cache_delete(&deployment_key).await;
        self.cache_delete("deployments:pending").await;

        Ok(())
    }

    // Deployment device tracking for phased rollout
    pub async fn create_deployment_device(
        &self,
        deployment_id: Uuid,
        device_id: Uuid,
        status: &str,
        previous_model_id: Option<Uuid>,
        current_model_id: Option<Uuid>,
        phase: i32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            INSERT INTO deployment_devices (deployment_id, device_id, status, previous_model_id, current_model_id, phase)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (deployment_id, device_id) DO NOTHING
            "#,
            deployment_id,
            device_id,
            status,
            previous_model_id,
            current_model_id,
            phase
        )
        .execute(&self.pool)
        .await?;

        // Invalidate caches
        let deployment_cache_key = format!("deployment_devices:{}", deployment_id);
        let device_cache_key = format!("device_deployments:{}", device_id);
        self.cache_delete(&deployment_cache_key).await;
        self.cache_delete(&device_cache_key).await;

        Ok(())
    }

    pub async fn get_deployment_devices(
        &self,
        deployment_id: Uuid,
    ) -> Result<Vec<DeploymentDevice>, sqlx::Error> {
        let cache_key = format!("deployment_devices:{}", deployment_id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Vec<DeploymentDevice>>(&cache_key).await? {
            return Ok(cached);
        }

        let rows = sqlx::query!(
            r#"SELECT * FROM deployment_devices WHERE deployment_id = $1"#,
            deployment_id
        )
        .fetch_all(&self.pool)
        .await?;

        let deployment_devices = rows
            .into_iter()
            .map(|r| DeploymentDevice {
                deployment_id: r.deployment_id,
                device_id: r.device_id,
                status: r.status,
                previous_model_id: r.previous_model_id,
                current_model_id: r.current_model_id,
                started_at: r.started_at,
                completed_at: r.completed_at,
                error_message: r.error_message,
                phase: r.phase.unwrap_or(0),
            })
            .collect();

        self.cache_set(&cache_key, &deployment_devices, Some(300))
            .await; // 5 min TTL
        Ok(deployment_devices)
    }

    pub async fn get_deployment_devices_by_status(
        &self,
        deployment_id: Uuid,
        status: &str,
    ) -> Result<Vec<DeploymentDevice>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"SELECT * FROM deployment_devices WHERE deployment_id = $1 AND status = $2"#,
            deployment_id,
            status
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| DeploymentDevice {
                deployment_id: r.deployment_id,
                device_id: r.device_id,
                status: r.status,
                previous_model_id: r.previous_model_id,
                current_model_id: r.current_model_id,
                started_at: r.started_at,
                completed_at: r.completed_at,
                error_message: r.error_message,
                phase: r.phase.unwrap_or(0),
            })
            .collect())
    }

    pub async fn update_deployment_device_status(
        &self,
        deployment_id: Uuid,
        device_id: Uuid,
        status: &str,
        started_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
        error_message: Option<String>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE deployment_devices
            SET status = $1, started_at = $2, completed_at = $3, error_message = $4
            WHERE deployment_id = $5 AND device_id = $6
            "#,
            status,
            started_at,
            completed_at,
            error_message,
            deployment_id,
            device_id
        )
        .execute(&self.pool)
        .await?;

        // Invalidate caches
        let deployment_cache_key = format!("deployment_devices:{}", deployment_id);
        let device_cache_key = format!("device_deployments:{}", device_id);
        self.cache_delete(&deployment_cache_key).await;
        self.cache_delete(&device_cache_key).await;

        Ok(())
    }

    pub async fn get_device_deployment_status(
        &self,
        device_id: Uuid,
    ) -> Result<Vec<DeploymentDevice>, sqlx::Error> {
        let cache_key = format!("device_deployments:{}", device_id);

        // Check cache first
        if let Some(cached) = self.cache_get::<Vec<DeploymentDevice>>(&cache_key).await? {
            return Ok(cached);
        }

        let rows = sqlx::query!(
            r#"SELECT * FROM deployment_devices WHERE device_id = $1"#,
            device_id
        )
        .fetch_all(&self.pool)
        .await?;

        let deployment_devices = rows
            .into_iter()
            .map(|r| DeploymentDevice {
                deployment_id: r.deployment_id,
                device_id: r.device_id,
                status: r.status,
                previous_model_id: r.previous_model_id,
                current_model_id: r.current_model_id,
                started_at: r.started_at,
                completed_at: r.completed_at,
                error_message: r.error_message,
                phase: r.phase.unwrap_or(0),
            })
            .collect();

        self.cache_set(&cache_key, &deployment_devices, Some(300))
            .await; // 5 min TTL
        Ok(deployment_devices)
    }

    pub async fn rollback_deployment(
        &self,
        original_deployment_id: Uuid,
        model_id: Uuid,
        _user_id: Uuid,
    ) -> Result<Deployment, sqlx::Error> {
        // Get the original deployment to capture rollback_of relationship
        let original = self.get_deployment_by_id(original_deployment_id).await?;

        if original.is_none() {
            return Err(sqlx::Error::RowNotFound);
        }

        let original = original.unwrap();

        // Create a rollback deployment that reverts to the previous model
        let rollback = sqlx::query!(
            r#"
            INSERT INTO deployments (
                device_id, model_id, status, rollout_strategy, rollout_percentage,
                rollback_of, rollout_config, current_phase, devices_target,
                devices_deployed, devices_succeeded, devices_failed
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, device_id, model_id, status, rollout_strategy, rollout_percentage,
                      deployed_at, completed_at, created_at,
                      current_phase, devices_target, devices_deployed,
                      devices_succeeded, devices_failed,
                      rollout_config, rollback_of
            "#,
            original.device_id,
            model_id,
            "pending",
            original.rollout_strategy,
            original.rollout_percentage,
            Some(original_deployment_id),
            original.rollout_config.map(|j| j.0),
            original.current_phase,
            original.devices_target,
            original.devices_deployed,
            original.devices_succeeded,
            original.devices_failed
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(Deployment {
            id: rollback.id,
            device_id: rollback.device_id,
            model_id: rollback.model_id.expect("model_id should not be null"),
            status: rollback.status.expect("status should not be null"),
            rollout_strategy: rollback
                .rollout_strategy
                .expect("rollout_strategy should not be null"),
            rollout_percentage: rollback
                .rollout_percentage
                .expect("rollout_percentage should not be null"),
            deployed_at: rollback.deployed_at,
            completed_at: rollback.completed_at,
            created_at: rollback.created_at.expect("created_at should not be null"),
            current_phase: rollback.current_phase,
            devices_target: rollback.devices_target,
            devices_deployed: rollback.devices_deployed,
            devices_succeeded: rollback.devices_succeeded,
            devices_failed: rollback.devices_failed,
            rollout_config: rollback.rollout_config.map(sqlx::types::Json),
            rollback_of: rollback.rollback_of,
        })
    }

    pub async fn get_deployment_devices_detailed(
        &self,
        deployment_id: Uuid,
    ) -> Result<Vec<crate::models::DeploymentDeviceInfo>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                d.device_id as device_id_str,
                d.name as device_name,
                dd.status,
                dd.phase,
                dd.previous_model_id,
                dd.current_model_id
            FROM deployment_devices dd
            JOIN devices d ON dd.device_id = d.id
            WHERE dd.deployment_id = $1
            ORDER BY dd.phase, d.device_id
            "#,
            deployment_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            result.push(crate::models::DeploymentDeviceInfo {
                device_id: row.device_id_str,
                name: row.device_name,
                status: row.status.unwrap_or_else(|| "pending".to_string()),
                phase: row.phase.unwrap_or(0),
                previous_model_id: row.previous_model_id,
                current_model_id: row.current_model_id,
            });
        }
        Ok(result)
    }

    // Refresh token management
    pub async fn create_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: String,
        expires_at: DateTime<Utc>,
    ) -> Result<crate::models::RefreshToken, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
            VALUES ($1, $2, $3)
            RETURNING id, user_id, token_hash, expires_at, revoked_at, created_at, used_at
            "#,
            user_id,
            token_hash,
            expires_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(crate::models::RefreshToken {
            id: row.id,
            user_id: row.user_id,
            token_hash: row.token_hash,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            created_at: row.created_at,
            used_at: row.used_at,
        })
    }

    pub async fn get_valid_refresh_token(
        &self,
        token_hash: &str,
    ) -> Result<Option<crate::models::RefreshToken>, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT * FROM refresh_tokens
            WHERE token_hash = $1
              AND revoked_at IS NULL
              AND used_at IS NULL
              AND expires_at > NOW()
            "#,
            token_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        debug!(
            "Refresh token lookup for hash {}: found={}",
            token_hash,
            row.is_some()
        );
        Ok(row.map(|r| crate::models::RefreshToken {
            id: r.id,
            user_id: r.user_id,
            token_hash: r.token_hash,
            expires_at: r.expires_at,
            revoked_at: r.revoked_at,
            created_at: r.created_at,
            used_at: r.used_at,
        }))
    }

    pub async fn revoke_refresh_token(&self, token_hash: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE refresh_tokens SET revoked_at = NOW() WHERE token_hash = $1 AND revoked_at IS NULL",
            token_hash
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn use_refresh_token(&self, token_hash: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE refresh_tokens SET used_at = NOW() WHERE token_hash = $1 AND used_at IS NULL",
            token_hash
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn insert_audit_log(
        &self,
        actor_type: &str,
        actor_id: Option<Uuid>,
        action: &str,
        resource_type: Option<&str>,
        resource_id: Option<Uuid>,
        old_state: Option<serde_json::Value>,
        new_state: Option<serde_json::Value>,
        ip_address: Option<String>,
        user_agent: Option<String>,
        metadata: Option<serde_json::Value>,
    ) -> Result<AuditLog, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO audit_logs (
                actor_type, actor_id, action, resource_type, resource_id,
                old_state, new_state, ip_address, user_agent, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, actor_type, actor_id, action, resource_type, resource_id,
                      old_state, new_state, ip_address, user_agent, metadata, created_at
            "#,
            actor_type,
            actor_id,
            action,
            resource_type,
            resource_id,
            old_state
                .as_ref()
                .map(|v| v)
                .unwrap_or(&serde_json::json!(null)),
            new_state
                .as_ref()
                .map(|v| v)
                .unwrap_or(&serde_json::json!(null)),
            ip_address,
            user_agent,
            metadata
                .as_ref()
                .map(|v| v)
                .unwrap_or(&serde_json::json!(null)),
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(AuditLog {
            id: row.id,
            actor_type: row.actor_type,
            actor_id: row.actor_id,
            action: row.action,
            resource_type: row.resource_type,
            resource_id: row.resource_id,
            old_state: row.old_state.map(|s| sqlx::types::Json(s)),
            new_state: row.new_state.map(|s| sqlx::types::Json(s)),
            ip_address: row.ip_address,
            user_agent: row.user_agent,
            metadata: row.metadata.map(|m| sqlx::types::Json(m)),
            created_at: row.created_at.expect("created_at should not be null"),
        })
    }

    pub async fn list_audit_logs(
        &self,
        actor_type: Option<&str>,
        actor_id: Option<Uuid>,
        action: Option<&str>,
        resource_type: Option<&str>,
        resource_id: Option<Uuid>,
        start_date: Option<chrono::DateTime<chrono::Utc>>,
        end_date: Option<chrono::DateTime<chrono::Utc>>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditLog>, sqlx::Error> {
        // Build dynamic query with proper parameter binding
        let mut conditions = Vec::new();
        let mut param_count = 1;

        if actor_type.is_some() {
            conditions.push(format!("actor_type = ${}", param_count));
            param_count += 1;
        }
        if actor_id.is_some() {
            conditions.push(format!("actor_id = ${}", param_count));
            param_count += 1;
        }
        if action.is_some() {
            conditions.push(format!("action = ${}", param_count));
            param_count += 1;
        }
        if resource_type.is_some() {
            conditions.push(format!("resource_type = ${}", param_count));
            param_count += 1;
        }
        if resource_id.is_some() {
            conditions.push(format!("resource_id = ${}", param_count));
            param_count += 1;
        }
        if start_date.is_some() {
            conditions.push(format!("created_at >= ${}", param_count));
            param_count += 1;
        }
        if end_date.is_some() {
            conditions.push(format!("created_at <= ${}", param_count));
            param_count += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let query = format!(
            "SELECT * FROM audit_logs {} ORDER BY created_at DESC LIMIT {} OFFSET {}",
            where_clause, limit, offset
        );

        let mut query_builder = sqlx::query_as::<_, AuditLog>(&query);

        if let Some(at) = actor_type {
            query_builder = query_builder.bind(at);
        }
        if let Some(aid) = actor_id {
            query_builder = query_builder.bind(aid);
        }
        if let Some(act) = action {
            query_builder = query_builder.bind(act);
        }
        if let Some(rt) = resource_type {
            query_builder = query_builder.bind(rt);
        }
        if let Some(rid) = resource_id {
            query_builder = query_builder.bind(rid);
        }
        if let Some(sd) = start_date {
            query_builder = query_builder.bind(sd);
        }
        if let Some(ed) = end_date {
            query_builder = query_builder.bind(ed);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        Ok(rows)
    }

    // Alert management
    pub async fn create_alert(
        &self,
        severity: &str,
        title: &str,
        description: &str,
        source: &str,
        device_id: Option<Uuid>,
        deployment_id: Option<Uuid>,
        metadata: Option<serde_json::Value>,
    ) -> Result<crate::models::Alert, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            INSERT INTO alerts (
                severity, title, description, source, status,
                device_id, deployment_id, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, severity, title, description, source, status,
                      device_id, deployment_id, acknowledged_at, acknowledged_by,
                      silenced_until, metadata, created_at
            "#,
            severity,
            title,
            description,
            source,
            "open",
            device_id,
            deployment_id,
            metadata
                .as_ref()
                .map(|v| v)
                .unwrap_or(&serde_json::json!(null)),
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(crate::models::Alert {
            id: row.id,
            severity: row.severity,
            title: row.title,
            description: row.description,
            source: row.source,
            status: row.status,
            device_id: row.device_id,
            deployment_id: row.deployment_id,
            acknowledged_at: row.acknowledged_at,
            acknowledged_by: row.acknowledged_by,
            silenced_until: row.silenced_until,
            metadata: row.metadata.map(|m| sqlx::types::Json(m)),
            created_at: row.created_at,
        })
    }

    pub async fn list_alerts(
        &self,
        user_id: Uuid,
        status: Option<&str>,
        severity: Option<&str>,
        source: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<crate::models::Alert>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT a.*, d.user_id as device_user_id
            FROM alerts a
            LEFT JOIN devices d ON a.device_id = d.id
            LEFT JOIN deployments dep ON a.deployment_id = dep.id
            LEFT JOIN models m ON dep.model_id = m.id
            WHERE d.user_id = $1 OR m.created_by = $1 OR a.device_id IS NULL
              AND (COALESCE($2, a.status) = COALESCE($2, a.status))
              AND (COALESCE($3, a.severity) = COALESCE($3, a.severity))
              AND (COALESCE($4, a.source) = COALESCE($4, a.source))
            ORDER BY a.created_at DESC
            LIMIT $5 OFFSET $6
            "#,
            user_id,
            status,
            severity,
            source,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| crate::models::Alert {
                id: r.id,
                severity: r.severity,
                title: r.title,
                description: r.description,
                source: r.source,
                status: r.status,
                device_id: r.device_id,
                deployment_id: r.deployment_id,
                acknowledged_at: r.acknowledged_at,
                acknowledged_by: r.acknowledged_by,
                silenced_until: r.silenced_until,
                metadata: r.metadata.map(|m| sqlx::types::Json(m)),
                created_at: r.created_at,
            })
            .collect())
    }

    pub async fn acknowledge_alert(
        &self,
        alert_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE alerts SET status = $1, acknowledged_at = NOW(), acknowledged_by = $2 WHERE id = $3",
            "acknowledged",
            user_id,
            alert_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn silence_alert(
        &self,
        alert_id: Uuid,
        silenced_until: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE alerts SET silenced_until = $1 WHERE id = $2",
            silenced_until,
            alert_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn close_alert(&self, alert_id: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "UPDATE alerts SET status = $1 WHERE id = $2",
            "closed",
            alert_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_device_offline_candidates(
        &self,
        threshold_minutes: i64,
    ) -> Result<Vec<Device>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT * FROM devices
            WHERE status = 'online'
              AND (last_seen IS NULL OR last_seen < NOW() - (INTERVAL '1 minute' * $1))
            "#,
            threshold_minutes as f64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Device {
                id: r.id,
                device_id: r.device_id,
                name: r.name,
                device_type: r.device_type,
                token: r.token,
                status: r.status.expect("status should not be null"),
                last_seen: r.last_seen,
                user_id: r.user_id,
                created_at: r.created_at.expect("created_at should not be null"),
                current_model_id: r.current_model_id,
                model_version: r.model_version,
            })
            .collect())
    }

    pub async fn get_stuck_deployments(
        &self,
        threshold_hours: i64,
    ) -> Result<Vec<Deployment>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT * FROM deployments
            WHERE status = 'deploying'
              AND created_at < NOW() - (INTERVAL '1 hour' * $1)
            "#,
            threshold_hours as f64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| Deployment {
                id: r.id,
                device_id: r.device_id,
                model_id: r.model_id.expect("model_id should not be null"),
                status: r.status.expect("status should not be null"),
                rollout_strategy: r
                    .rollout_strategy
                    .expect("rollout_strategy should not be null"),
                rollout_percentage: r
                    .rollout_percentage
                    .expect("rollout_percentage should not be null"),
                deployed_at: r.deployed_at,
                completed_at: r.completed_at,
                created_at: r.created_at.expect("created_at should not be null"),
                current_phase: r.current_phase,
                devices_target: r.devices_target,
                devices_deployed: r.devices_deployed,
                devices_succeeded: r.devices_succeeded,
                devices_failed: r.devices_failed,
                rollout_config: r.rollout_config.map(sqlx::types::Json),
                rollback_of: r.rollback_of,
            })
            .collect())
    }

    pub async fn get_all_online_devices(&self) -> Result<Vec<Device>, sqlx::Error> {
        let rows = sqlx::query!("SELECT * FROM devices WHERE status = 'online'")
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| Device {
                id: r.id,
                device_id: r.device_id,
                name: r.name,
                device_type: r.device_type,
                token: r.token,
                status: r.status.expect("status should not be null"),
                last_seen: r.last_seen,
                user_id: r.user_id,
                created_at: r.created_at.expect("created_at should not be null"),
                current_model_id: r.current_model_id,
                model_version: r.model_version,
            })
            .collect())
    }
}
