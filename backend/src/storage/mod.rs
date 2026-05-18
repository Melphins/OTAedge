use aws_sdk_s3::{
    config::{BehaviorVersion, Credentials, Region},
    error::ProvideErrorMetadata,
    presigning::PresigningConfig,
    primitives::ByteStream,
    Client,
};
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::error::AppError;
use crate::retry::{retry, RETRY_CONFIGS};

#[derive(Clone)]
pub struct Storage {
    client: Option<Client>,
    bucket: String,
}

impl Storage {
    pub async fn new() -> Result<Self, AppError> {
        let endpoint = env::var("MINIO_ENDPOINT").ok();
        let access_key = env::var("MINIO_ACCESS_KEY").ok();
        let secret_key = env::var("MINIO_SECRET_KEY").ok();
        let bucket = env::var("MINIO_BUCKET").unwrap_or_else(|_| "otaedge-models".to_string());

        if let (Some(endpoint), Some(access_key), Some(secret_key)) =
            (endpoint, access_key, secret_key)
        {
            let region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
            let mut s3_config_builder = aws_sdk_s3::config::Builder::new()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new(region))
                .credentials_provider(Credentials::new(
                    access_key, secret_key, None, None, "custom",
                ));
            // Set custom endpoint if provided
            if !endpoint.is_empty() {
                s3_config_builder = s3_config_builder.endpoint_url(&endpoint);
                // Disable SSL for local MinIO if endpoint is http
                if endpoint.starts_with("http://") {
                    s3_config_builder = s3_config_builder.force_path_style(true);
                }
            }
            let s3_config = s3_config_builder.build();
            let client = Client::from_conf(s3_config);
            ensure_bucket_exists(&client, &bucket).await?;

            Ok(Self {
                client: Some(client),
                bucket,
            })
        } else {
            // Fallback to local storage
            Ok(Self {
                client: None,
                bucket,
            })
        }
    }

    pub fn is_s3_enabled(&self) -> bool {
        self.client.is_some()
    }

    pub async fn object_exists(&self, key: &str) -> Result<bool, AppError> {
        if let Some(client) = &self.client {
            match client
                .head_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(e) => {
                    if let Some(s3_err) = e.as_service_error() {
                        if s3_err.code() == Some("NotFound")
                            || s3_err.code() == Some("NoSuchKey")
                            || s3_err.code() == Some("404")
                        {
                            return Ok(false);
                        }
                    }
                    Err(AppError::Storage(format!("S3 head object failed: {}", e)))
                }
            }
        } else {
            let storage_path =
                env::var("MODEL_STORAGE_PATH").unwrap_or_else(|_| "./storage/models".to_string());
            let file_path = Path::new(&storage_path).join(key);
            Ok(tokio::fs::metadata(file_path).await.is_ok())
        }
    }

    pub async fn presigned_put_url(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<PresignedStorageRequest, AppError> {
        let client = self.client.as_ref().ok_or_else(|| {
            AppError::Validation("Presigned URLs require S3/MinIO storage".to_string())
        })?;
        let config = PresigningConfig::expires_in(expires_in)
            .map_err(|e| AppError::Storage(format!("Invalid presigning config: {}", e)))?;
        let request = client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|e| AppError::Storage(format!("S3 presigned upload failed: {}", e)))?;

        Ok(PresignedStorageRequest::from_request(request, expires_in))
    }

    pub async fn presigned_get_url(
        &self,
        key: &str,
        expires_in: Duration,
    ) -> Result<PresignedStorageRequest, AppError> {
        let client = self.client.as_ref().ok_or_else(|| {
            AppError::Validation("Presigned URLs require S3/MinIO storage".to_string())
        })?;
        let config = PresigningConfig::expires_in(expires_in)
            .map_err(|e| AppError::Storage(format!("Invalid presigning config: {}", e)))?;
        let request = client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|e| AppError::Storage(format!("S3 presigned download failed: {}", e)))?;

        Ok(PresignedStorageRequest::from_request(request, expires_in))
    }

    /// Upload with retry for transient failures
    pub async fn upload(&self, key: &str, bytes: Vec<u8>) -> Result<(), AppError> {
        // Convert bytes to Arc<[u8]> for cheap cloning
        let bytes_arc: Arc<[u8]> = Arc::from(bytes);
        if let Some(client) = &self.client {
            let bucket = &self.bucket;
            retry(
                &RETRY_CONFIGS,
                || {
                    let bytes_clone = bytes_arc.clone();
                    async move {
                        client
                            .put_object()
                            .bucket(bucket)
                            .key(key)
                            .body(ByteStream::from(bytes_clone.to_vec()))
                            .send()
                            .await
                            .map_err(|e| AppError::Storage(format!("S3 upload failed: {}", e)))
                    }
                },
                &format!("S3 upload for key {}", key),
            )
            .await?;
            Ok(())
        } else {
            // Store locally with simple retry
            let storage_path =
                env::var("MODEL_STORAGE_PATH").unwrap_or_else(|_| "./storage/models".to_string());
            tokio::fs::create_dir_all(&storage_path)
                .await
                .map_err(|e| AppError::Io(e.to_string()))?;
            let file_path = Path::new(&storage_path).join(key);
            let file_path_ref = &file_path;

            retry(
                &RETRY_CONFIGS,
                || {
                    let bytes_clone = bytes_arc.clone();
                    async move {
                        tokio::fs::write(file_path_ref, &bytes_clone)
                            .await
                            .map_err(|e| AppError::Io(e.to_string()))
                    }
                },
                &format!("local write for key {}", key),
            )
            .await?;

            Ok(())
        }
    }

    /// Download with retry for transient failures
    pub async fn download(&self, key: &str) -> Result<Vec<u8>, AppError> {
        if let Some(client) = &self.client {
            // Download from S3 with retry
            retry(
                &RETRY_CONFIGS,
                || async move {
                    match client
                        .get_object()
                        .bucket(&self.bucket)
                        .key(key)
                        .send()
                        .await
                    {
                        Ok(resp) => {
                            let bytes = resp.body.collect().await.map_err(|e| {
                                AppError::Storage(format!(
                                    "S3 download body collection failed: {}",
                                    e
                                ))
                            })?;
                            Ok(bytes.into_bytes().to_vec())
                        }
                        Err(e) => {
                            // Check if the error is due to missing object
                            if let Some(s3_err) = e.as_service_error() {
                                // For S3, a NoSuchKey error results in a 404
                                if s3_err.code() == Some("NoSuchKey")
                                    || s3_err.code() == Some("NotFound")
                                {
                                    return Err(AppError::NotFound(format!(
                                        "Model file {} not found",
                                        key
                                    )));
                                }
                            }
                            Err(AppError::Storage(format!("S3 download failed: {}", e)))
                        }
                    }
                },
                &format!("S3 download for key {}", key),
            )
            .await
        } else {
            // Read from local filesystem with retry
            let storage_path =
                env::var("MODEL_STORAGE_PATH").unwrap_or_else(|_| "./storage/models".to_string());
            let file_path = Path::new(&storage_path).join(key);
            let file_path_ref = &file_path;

            retry(
                &RETRY_CONFIGS,
                || async move {
                    match tokio::fs::read(file_path_ref).await {
                        Ok(bytes) => Ok(bytes),
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::NotFound {
                                Err(AppError::NotFound(format!("Model file {} not found", key)))
                            } else {
                                Err(AppError::Io(e.to_string()))
                            }
                        }
                    }
                },
                &format!("local read for key {}", key),
            )
            .await
        }
    }

    /// Delete with retry for transient failures
    pub async fn delete(&self, key: &str) -> Result<(), AppError> {
        if let Some(client) = &self.client {
            retry(
                &RETRY_CONFIGS,
                || async move {
                    client
                        .delete_object()
                        .bucket(&self.bucket)
                        .key(key)
                        .send()
                        .await
                        .map(|_| ())
                        .map_err(|e| AppError::Storage(format!("S3 delete failed: {}", e)))
                },
                &format!("S3 delete for key {}", key),
            )
            .await?;
            Ok(())
        } else {
            let storage_path =
                env::var("MODEL_STORAGE_PATH").unwrap_or_else(|_| "./storage/models".to_string());
            let file_path = Path::new(&storage_path).join(key);
            let file_path_ref = &file_path;

            retry(
                &RETRY_CONFIGS,
                || async move {
                    tokio::fs::remove_file(file_path_ref)
                        .await
                        .map_err(|e| AppError::Io(e.to_string()))
                },
                &format!("local delete for key {}", key),
            )
            .await?;
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresignedStorageRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub expires_in_seconds: u64,
}

impl PresignedStorageRequest {
    fn from_request(
        request: aws_sdk_s3::presigning::PresignedRequest,
        expires_in: Duration,
    ) -> Self {
        Self {
            url: request.uri().to_string(),
            method: request.method().to_string(),
            headers: request
                .headers()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            expires_in_seconds: expires_in.as_secs(),
        }
    }
}

async fn ensure_bucket_exists(client: &Client, bucket: &str) -> Result<(), AppError> {
    match client.head_bucket().bucket(bucket).send().await {
        Ok(_) => Ok(()),
        Err(head_err) => {
            if let Some(s3_err) = head_err.as_service_error() {
                let code = s3_err.code();
                if code != Some("NotFound") && code != Some("404") {
                    return Err(AppError::Storage(format!(
                        "S3 head bucket failed: {}",
                        head_err
                    )));
                }
            }

            match client.create_bucket().bucket(bucket).send().await {
                Ok(_) => Ok(()),
                Err(create_err) => {
                    if let Some(s3_err) = create_err.as_service_error() {
                        let code = s3_err.code();
                        if code == Some("BucketAlreadyOwnedByYou")
                            || code == Some("BucketAlreadyExists")
                        {
                            return Ok(());
                        }
                    }
                    Err(AppError::Storage(format!(
                        "S3 bucket provisioning failed: {}",
                        create_err
                    )))
                }
            }
        }
    }
}
