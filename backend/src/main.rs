use axum::{
    body::Body,
    extract::{ConnectInfo, Extension, Json, Multipart, Path, Query, State},
    http::{header, HeaderMap, HeaderName, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use backend::{
    alert_helpers, alerts, auth::*, cache::Cache, deployment_flow, error::AppError, logging::*,
    metrics::Metrics, models::*, monitor, repositories::Repository, storage, websocket, AppState,
};
use chrono::{DateTime, Utc};
use prometheus::{self, Encoder, TextEncoder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, warn};
use utoipa::OpenApi;
use uuid::Uuid;

// Validate configuration on startup
fn validate_config() -> Result<(), String> {
    // Check required environment variables
    if env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL not set")?
        .is_empty()
    {
        return Err("DATABASE_URL is empty".to_string());
    }
    if env::var("JWT_SECRET")
        .map_err(|_| "JWT_SECRET not set")?
        .is_empty()
    {
        return Err("JWT_SECRET is empty".to_string());
    }

    // Validate JWT_SECRET strength (minimum 32 characters)
    let jwt_secret = env::var("JWT_SECRET").unwrap();
    if jwt_secret.len() < 32 {
        return Err("JWT_SECRET must be at least 32 characters for security".to_string());
    }

    // Enforce database SSL in production
    if env::var("APP_ENV")
        .map(|v| v == "production")
        .unwrap_or(false)
    {
        let database_url = env::var("DATABASE_URL").unwrap();
        if !database_url.contains("sslmode=require") {
            return Err("DATABASE_URL must include sslmode=require in production".to_string());
        }
    }

    // Check storage directory
    let storage_path =
        env::var("MODEL_STORAGE_PATH").unwrap_or_else(|_| "./storage/models".to_string());
    if let Err(e) = fs::create_dir_all(&storage_path) {
        return Err(format!(
            "Failed to create storage directory '{}': {}",
            storage_path, e
        ));
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt::init();

    // Validate configuration
    validate_config()?;

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    // Initialize Redis cache if configured
    let cache = if let Ok(redis_url) = env::var("REDIS_URL") {
        let cfg = deadpool_redis::Config::from_url(redis_url);
        match cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1)) {
            Ok(pool) => {
                let cache = Cache::new(pool, Duration::from_secs(300)); // 5 min default TTL
                Some(cache)
            }
            Err(e) => {
                warn!("Failed to create Redis pool: {}, caching disabled", e);
                None
            }
        }
    } else {
        None
    };

    let repo = Repository::new(&database_url, cache.clone()).await?;
    let metrics = Arc::new(Metrics::new());
    let storage = storage::Storage::new().await?;

    // Initialize alert manager
    let mut alert_manager = alerts::AlertManager::new(60); // max 60 alerts per minute per type

    // Configure alert channels from environment
    // Email (SendGrid)
    if let (Ok(api_key), Ok(from_email)) =
        (env::var("SENDGRID_API_KEY"), env::var("ALERT_EMAIL_FROM"))
    {
        let to_emails: Vec<String> = env::var("ALERT_EMAIL_TO")
            .map(|s| s.split(',').map(|e| e.trim().to_string()).collect())
            .unwrap_or_default();

        if !to_emails.is_empty() {
            match alerts::SendGridChannel::new(api_key, from_email, to_emails) {
                Ok(channel) => alert_manager.add_channel(Box::new(channel)),
                Err(e) => warn!("Failed to initialize SendGrid channel: {}", e),
            }
        }
    }

    // Slack
    if let Ok(webhook_url) = env::var("SLACK_WEBHOOK_URL") {
        if let Ok(channel) = alerts::SlackChannel::new(webhook_url, None) {
            alert_manager.add_channel(Box::new(channel));
        }
    }

    // Teams
    if let Ok(webhook_url) = env::var("TEAMS_WEBHOOK_URL") {
        if let Ok(channel) = alerts::TeamsChannel::new(webhook_url) {
            alert_manager.add_channel(Box::new(channel));
        }
    }

    // SMS (Twilio)
    if let (Ok(account_sid), Ok(auth_token), Ok(from_number)) = (
        env::var("TWILIO_ACCOUNT_SID"),
        env::var("TWILIO_AUTH_TOKEN"),
        env::var("TWILIO_PHONE_NUMBER"),
    ) {
        let to_numbers: Vec<String> = env::var("ALERT_SMS_TO")
            .map(|s| s.split(',').map(|n| n.trim().to_string()).collect())
            .unwrap_or_default();

        if !to_numbers.is_empty() {
            match alerts::TwilioChannel::new(account_sid, auth_token, from_number, to_numbers) {
                Ok(channel) => alert_manager.add_channel(Box::new(channel)),
                Err(e) => warn!("Failed to initialize Twilio channel: {}", e),
            }
        }
    }

    let alert_manager = Arc::new(Mutex::new(alert_manager));

    // Initialize Redis cache if configured
    let cache = if let Ok(redis_url) = env::var("REDIS_URL") {
        let cfg = deadpool_redis::Config::from_url(redis_url);
        match cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1)) {
            Ok(pool) => {
                let cache = Cache::new(pool, Duration::from_secs(300)); // 5 min default TTL
                Some(cache)
            }
            Err(e) => {
                warn!("Failed to create Redis pool: {}, caching disabled", e);
                None
            }
        }
    } else {
        None
    };

    let state = Arc::new(AppState {
        repo: repo.clone(),
        jwt_secret,
        connections: Arc::new(Mutex::new(HashMap::new())),
        metrics,
        rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        user_rate_limiter: Arc::new(Mutex::new(HashMap::new())),
        storage,
        alert_manager,
        cache,
    });

    // Public routes (no authentication required)
    let public_router = Router::new()
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .route("/api/auth/logout", post(logout))
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/metrics", get(metrics_handler))
        .route("/api-docs/openapi.json", get(openapi_handler))
        .route("/ws", get(websocket::ws_handler));

    // Protected routes (require authentication)
    let auth_layer = middleware::from_fn({
        let state = state.clone();
        move |req: Request<Body>, next: Next| {
            let state = state.clone();
            async move {
                let token = req
                    .headers()
                    .get("Authorization")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|h| h.strip_prefix("Bearer "))
                    .ok_or_else(|| {
                        AppError::Auth("Missing or invalid authorization header".to_string())
                    })?;

                match validate_jwt(token, &state.jwt_secret) {
                    Ok(claims) => {
                        // Per-user rate limiting
                        let user_id = claims.sub.clone();
                        let now = std::time::Instant::now();
                        let window = Duration::from_secs(60);
                        let max_user_requests = 100;

                        let mut user_map = state.user_rate_limiter.lock().await;
                        match user_map.get_mut(&user_id) {
                            Some((start, count)) => {
                                if now.duration_since(*start) >= window {
                                    *start = now;
                                    *count = 1;
                                } else {
                                    if *count >= max_user_requests {
                                        state
                                            .metrics
                                            .rate_limited_total
                                            .with_label_values(&["user"])
                                            .inc();
                                        return Err(AppError::RateLimit);
                                    }
                                    *count += 1;
                                }
                            }
                            None => {
                                user_map.insert(user_id, (now, 1));
                            }
                        }

                        let mut req = req;
                        req.extensions_mut().insert(claims);
                        Ok(next.run(req).await)
                    }
                    Err(_) => Err(AppError::Auth("Invalid token".to_string())),
                }
            }
        }
    });

    let device_router = Router::new().route(
        "/api/device/models/:id/download",
        get(download_model_device),
    );

    let protected_router = Router::new()
        .route("/api/devices", get(get_devices).post(create_device))
        .route("/api/devices/:id", get(get_device))
        .route("/api/models", get(list_models))
        .route("/api/models/active", get(get_active_model))
        .route("/api/models/compare", get(compare_models))
        .route("/api/models/upload", post(upload_model))
        .route("/api/models/presign-upload", post(presign_model_upload))
        .route("/api/models/complete-upload", post(complete_model_upload))
        .route(
            "/api/models/:id/presign-download",
            post(presign_model_download),
        )
        .route("/api/models/:id", get(get_model).delete(delete_model))
        .route("/api/models/:id/activate", post(activate_model))
        .route("/api/models/:id/download", get(download_model))
        .route(
            "/api/deployments",
            get(list_deployments).post(create_deployment),
        )
        .route("/api/deployments/:id", get(get_deployment))
        .route("/api/deployments/:id/rollback", post(rollback_deployment))
        .route("/api/deployments/:id/devices", get(list_deployment_devices))
        .route("/api/alerts", get(list_alerts))
        .route("/api/alerts/test", post(test_alert))
        .route("/api/alerts/:id/acknowledge", post(acknowledge_alert))
        .route("/api/alerts/:id/silence", post(silence_alert))
        .route("/api/alerts/:id/close", post(close_alert))
        .route("/api/audit-logs", get(list_audit_logs))
        .layer(auth_layer);

    // Configure CORS from environment
    let allowed_origin =
        env::var("ALLOWED_ORIGINS").unwrap_or_else(|_| "http://localhost:3000".to_string());

    let cors_layer = if allowed_origin == "*" {
        CorsLayer::permissive()
    } else {
        let origins: Vec<axum::http::HeaderValue> = allowed_origin
            .split(',')
            .filter_map(|origin| axum::http::HeaderValue::from_str(origin.trim()).ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_credentials(true)
            .allow_methods([
                axum::http::Method::GET,
                axum::http::Method::POST,
                axum::http::Method::PUT,
                axum::http::Method::DELETE,
                axum::http::Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
            ])
    };

    let router = public_router
        .merge(device_router)
        .merge(protected_router)
        .layer(middleware::from_fn({
            let state = state.clone();
            move |request: Request<Body>, next: Next| {
                let state = state.clone();
                async move {
                    // Get client IP, fallback to 0.0.0.0 if ConnectInfo missing
                    let ip = request
                        .extensions()
                        .get::<ConnectInfo<SocketAddr>>()
                        .map(|ci| ci.0.ip())
                        .unwrap_or(IpAddr::from([0, 0, 0, 0]));

                    let now = std::time::Instant::now();
                    let window = Duration::from_secs(60);
                    let max_requests = 100;

                    let mut map = state.rate_limiter.lock().await;
                    match map.get_mut(&ip) {
                        Some((start, count)) => {
                            if now.duration_since(*start) >= window {
                                *start = now;
                                *count = 1;
                            } else {
                                if *count >= max_requests {
                                    state.metrics.rate_limited_total.with_label_values(&["ip"]).inc();
                                    return Err(AppError::RateLimit);
                                }
                                *count += 1;
                            }
                        }
                        None => {
                            map.insert(ip, (now, 1));
                        }
                    }

                    Ok(next.run(request).await)
                }
            }
        }))
        .layer(middleware::from_fn(|mut request: Request<Body>, next: Next| {
            let request_id = uuid::Uuid::new_v4().to_string();
            request.extensions_mut().insert(request_id.clone());

            async move {
                let mut response = next.run(request).await;
                response.headers_mut().insert(
                    HeaderName::from_static("x-request-id"),
                    request_id.parse().unwrap()
                );
                Ok::<Response, AppError>(response)
            }
        }))
        .layer(middleware::from_fn(|request: Request<Body>, next: Next| async move {
            // Extract IP address from various sources
            let ip = request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ci| ci.0.ip().to_string())
                .or_else(|| {
                    request.headers()
                        .get("x-forwarded-for")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|h| h.split(',').next())
                        .map(|s| s.trim().to_string())
                })
                .unwrap_or_else(|| "0.0.0.0".to_string());

            let mut req = request;
            req.extensions_mut().insert(ip);

            next.run(req).await
        }))
        .layer(middleware::from_fn(|request: Request<Body>, next: Next| async move {
            let mut response = next.run(request).await;

            // Security headers
            response.headers_mut().insert(
                HeaderName::from_static("strict-transport-security"),
                "max-age=31536000; includeSubDomains".parse().unwrap()
            );
            response.headers_mut().insert(
                HeaderName::from_static("x-content-type-options"),
                "nosniff".parse().unwrap()
            );
            response.headers_mut().insert(
                HeaderName::from_static("x-frame-options"),
                "DENY".parse().unwrap()
            );
            response.headers_mut().insert(
                HeaderName::from_static("referrer-policy"),
                "strict-origin-when-cross-origin".parse().unwrap()
            );
            // Conservative CSP - adjust as needed for your frontend
            response.headers_mut().insert(
                HeaderName::from_static("content-security-policy"),
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".parse().unwrap()
            );

            Ok::<Response, AppError>(response)
        }))
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer)
        .layer(middleware::from_fn({
            let state = state.clone();
            move |req: Request<Body>, next: Next| {
                let state = state.clone();
                async move {
                    let start = Instant::now();
                    let method = req.method().to_string();
                    let path = req.uri().path().to_string();

                    let response = next.run(req).await;

                    let status = response.status().as_u16().to_string();
                    let _latency = start.elapsed().as_secs_f64();

                    state.metrics.http_requests_total
                        .with_label_values(&[&method, &path, &status])
                        .inc();

                    Ok::<Response, AppError>(response)
                }
            }
        }))
        .with_state(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("🚀 OTAedge API listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Graceful shutdown: wait for SIGTERM or SIGINT
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn shutdown signal handler
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            eprintln!("Failed to listen for shutdown signal: {}", e);
        }
        let _ = shutdown_tx.send(());
    });

    // Start background monitoring tasks
    tokio::spawn(monitor::start_device_monitor(state.clone()));
    tokio::spawn(monitor::start_deployment_cleanup(state.clone()));
    tokio::spawn(monitor::start_metrics_aggregation(state.clone()));

    // Use serve_with_graceful_shutdown
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            shutdown_rx.await.ok();
        })
        .await?;

    Ok(())
}

// Request DTOs
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateDeviceRequest {
    pub name: String,
    pub device_type: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LogoutRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct DeviceCreationResponse {
    pub device: Device,
    pub token: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateDeploymentRequest {
    pub device_id: Option<Uuid>,
    pub model_id: Uuid,
    pub rollout_strategy: Option<String>,
    pub rollout_percentage: Option<i32>,
    pub rollout_config: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RollbackRequest {
    pub model_id: Uuid,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PresignedModelUploadRequest {
    pub name: String,
    pub version: i32,
    pub file_name: String,
    pub file_size_bytes: Option<i64>,
    pub hash_sha256: String,
    pub model_format: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub expires_in_seconds: Option<u64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PresignedHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PresignedModelUploadResponse {
    pub upload_url: String,
    pub method: String,
    pub headers: Vec<PresignedHeader>,
    pub expires_in_seconds: u64,
    pub s3_key: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CompleteModelUploadRequest {
    pub name: String,
    pub version: i32,
    pub file_name: String,
    pub file_size_bytes: Option<i64>,
    pub s3_key: String,
    pub hash_sha256: String,
    pub model_format: String,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PresignedModelDownloadResponse {
    pub download_url: String,
    pub method: String,
    pub headers: Vec<PresignedHeader>,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListModelsQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListDeploymentsQuery {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ActiveModelQuery {
    pub name: String,
    pub release_channel: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ActivateModelRequest {
    pub release_channel: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CompareModelsQuery {
    pub base_id: Uuid,
    pub target_id: Uuid,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ModelComparison {
    pub base: Model,
    pub target: Model,
    pub changed_fields: Vec<String>,
    pub version_delta: i32,
    pub file_size_delta_bytes: Option<i64>,
    pub same_hash: bool,
    pub same_format: bool,
    pub same_metadata: bool,
}

// Response DTOs for health checks
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ReadinessResponse {
    pub status: String,
    pub database: String,
    pub timestamp: String,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        register,
        login,
        refresh,
        logout,
        get_devices,
        create_device,
        get_device,
        list_models,
        get_active_model,
        compare_models,
        get_model,
        activate_model,
        delete_model,
        upload_model,
        presign_model_upload,
        complete_model_upload,
        presign_model_download,
        list_deployments,
        get_deployment,
        create_deployment,
        rollback_deployment,
        list_deployment_devices,
        list_alerts,
        test_alert,
        acknowledge_alert,
        silence_alert,
        close_alert,
        list_audit_logs,
        health_check,
        readiness_check,
    ),
    components(schemas(
        RegisterRequest,
        LoginRequest,
        AuthResponse,
        User,
        Device,
        DeviceCreationResponse,
        CreateDeviceRequest,
        RefreshRequest,
        LogoutRequest,
        Model,
        PresignedModelUploadRequest,
        PresignedModelUploadResponse,
        CompleteModelUploadRequest,
        PresignedModelDownloadResponse,
        ListModelsQuery,
        ListDeploymentsQuery,
        ListAlertsQuery,
        SilenceAlertRequest,
        AuditLog,
        ListAuditLogsQuery,
        ActiveModelQuery,
        ActivateModelRequest,
        CompareModelsQuery,
        ModelComparison,
        Deployment,
        DeploymentDevice,
        DeploymentDeviceInfo,
        CreateDeploymentRequest,
        RollbackRequest,
        SilenceAlertRequest,
        HealthResponse,
        ReadinessResponse,
        Claims,
        backend::models::Alert,
    ))
)]
struct ApiDoc;

// Handler: Register
#[utoipa::path(
    post,
    path = "/api/auth/register",
    request_body(content = RegisterRequest),
    responses(
        (status = 200, description = "User registered successfully", body = AuthResponse),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    )
)]
async fn register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    // Input validation
    validate_email(&req.email).map_err(AppError::Validation)?;
    validate_password(&req.password).map_err(AppError::Validation)?;

    if req.username.is_empty() {
        return Err(AppError::Validation("Username is required".to_string()));
    }

    match state.repo.get_user_by_email(&req.email).await {
        Ok(Some(_)) => return Err(AppError::Validation("Email already exists".to_string())),
        Ok(None) => {}
        Err(e) => {
            error!("Error checking user: {}", e);
            return Err(AppError::Database(e.to_string()));
        }
    }

    let password_hash = hash_password(&req.password);
    match state
        .repo
        .create_user(req.email, req.username, password_hash)
        .await
    {
        Ok(user) => match create_jwt(&user, &state.jwt_secret) {
            Ok(token) => match issue_refresh_token(&state.repo, user.id).await {
                Ok(refresh_token) => Ok(Json(AuthResponse {
                    token,
                    refresh_token,
                    user,
                })),
                Err(e) => {
                    error!("Failed to create refresh token: {}", e);
                    Err(AppError::Internal(e.to_string()))
                }
            },
            Err(e) => {
                error!("Failed to create JWT: {}", e);
                Err(AppError::Internal(e.to_string()))
            }
        },
        Err(e) => {
            error!("Error creating user: {}", e);
            Err(AppError::Database(e.to_string()))
        }
    }
}

// Handler: Login
#[utoipa::path(
    post,
    path = "/api/auth/login",
    request_body(content = LoginRequest),
    responses(
        (status = 200, description = "Login successful", body = AuthResponse),
        (status = 401, description = "Invalid credentials"),
        (status = 500, description = "Internal server error")
    )
)]
async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user = state
        .repo
        .get_user_by_email(&req.email)
        .await?
        .ok_or_else(|| {
            state
                .metrics
                .auth_attempts_total
                .with_label_values(&["failure", "login"])
                .inc();
            AppError::Auth("Invalid credentials".to_string())
        })?;

    if !verify_password(&req.password, &user.password_hash) {
        state
            .metrics
            .auth_attempts_total
            .with_label_values(&["failure", "login"])
            .inc();
        return Err(AppError::Auth("Invalid credentials".to_string()));
    }

    let token =
        create_jwt(&user, &state.jwt_secret).map_err(|e| AppError::Internal(e.to_string()))?;

    let refresh_token = issue_refresh_token(&state.repo, user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    state
        .metrics
        .auth_attempts_total
        .with_label_values(&["success", "login"])
        .inc();
    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user,
    }))
}

// Handler: Refresh access token using refresh token
#[utoipa::path(
    post,
    path = "/api/auth/refresh",
    request_body(content = RefreshRequest),
    responses(
        (status = 200, description = "Tokens refreshed", body = AuthResponse),
        (status = 401, description = "Invalid refresh token"),
        (status = 500, description = "Internal server error")
    )
)]
async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let token_hash = format!("{:x}", Sha256::digest(req.refresh_token.as_bytes()));
    let rt = state
        .repo
        .get_valid_refresh_token(&token_hash)
        .await?
        .ok_or_else(|| {
            state
                .metrics
                .auth_attempts_total
                .with_label_values(&["failure", "refresh"])
                .inc();
            AppError::Auth("Invalid or expired refresh token".to_string())
        })?;

    let user_id = rt
        .user_id
        .ok_or_else(|| AppError::NotFound("User ID not associated with token".to_string()))?;
    let user = state
        .repo
        .get_user_by_id(user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

    // Invalidate old refresh token (mark as used)
    state.repo.use_refresh_token(&token_hash).await?;

    let access_token =
        create_jwt(&user, &state.jwt_secret).map_err(|e| AppError::Internal(e.to_string()))?;
    let new_refresh_token = issue_refresh_token(&state.repo, user.id)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    state
        .metrics
        .auth_attempts_total
        .with_label_values(&["success", "refresh"])
        .inc();
    Ok(Json(AuthResponse {
        token: access_token,
        refresh_token: new_refresh_token,
        user,
    }))
}

// Handler: Logout (revoke refresh token)
#[utoipa::path(
    post,
    path = "/api/auth/logout",
    request_body(content = LogoutRequest),
    responses(
        (status = 200, description = "Logged out successfully"),
        (status = 500, description = "Internal server error")
    )
)]
async fn logout(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LogoutRequest>,
) -> Result<Json<()>, AppError> {
    let token_hash = format!("{:x}", Sha256::digest(req.refresh_token.as_bytes()));
    // Revoke the refresh token (if exists)
    let _ = state.repo.revoke_refresh_token(&token_hash).await;
    state
        .metrics
        .auth_attempts_total
        .with_label_values(&["success", "logout"])
        .inc();
    Ok(Json(()))
}

// Handler: Get all devices (user-scoped)
#[utoipa::path(
    get,
    path = "/api/devices",
    responses(
        (status = 200, description = "List of user's devices", body = Vec<Device>),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_devices(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Vec<Device>>, AppError> {
    // Parse user ID from JWT claims
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let log_ctx = LogContext::new().with_user_id(user_id);

    let devices = state.repo.get_devices_by_user_id(user_id).await?;

    log_ctx.info(&format!("Retrieved {} devices", devices.len()));

    // Audit log: device list read
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "devices.list",
            Some("device"),
            None,
            None,
            Some(serde_json::json!({
                "count": devices.len(),
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(devices))
}

// Handler: Create device
#[utoipa::path(
    post,
    path = "/api/devices",
    request_body(content = CreateDeviceRequest),
    responses(
        (status = 200, description = "Device created", body = DeviceCreationResponse),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    )
)]
async fn create_device(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreateDeviceRequest>,
) -> Result<Json<DeviceCreationResponse>, AppError> {
    if req.name.is_empty() {
        return Err(AppError::Validation("Device name is required".to_string()));
    }

    let device_id = Uuid::new_v4().to_string();
    let token = Uuid::new_v4().to_string();

    // Parse user ID from JWT claims and associate device with user
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let device = state
        .repo
        .create_device(
            device_id,
            req.name,
            req.device_type,
            token.clone(),
            Some(user_id), // Associate device with authenticated user
        )
        .await?;

    let log_ctx = LogContext::new().with_user_id(user_id);
    log_ctx.info(&format!(
        "Device created: id={}, name={}",
        device.id, device.name
    ));

    // Audit log: device creation
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "device.create",
            Some("device"),
            Some(device.id),
            None,
            Some(serde_json::json!({
                "device_name": device.name,
                "device_type": device.device_type,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(DeviceCreationResponse { device, token }))
}

// Handler: Get device by ID (user-scoped)
#[utoipa::path(
    get,
    path = "/api/devices/{id}",
    params(
        ("id" = Uuid, Path, description = "Device ID")
    ),
    responses(
        (status = 200, description = "Device found", body = Device),
        (status = 404, description = "Device not found"),
        (status = 403, description = "Forbidden - not your device"),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_device(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Device>, AppError> {
    // Parse user ID from JWT claims
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let device = state
        .repo
        .get_device_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Device not found".to_string()))?;

    // Ensure device belongs to the authenticated user
    if device.user_id != Some(user_id) {
        return Err(AppError::Auth(
            "You do not have permission to access this device".to_string(),
        ));
    }

    LogContext::new()
        .with_user_id(user_id)
        .debug(&format!("Device retrieved: id={}", device.id));

    // Audit log: device read
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "device.read",
            Some("device"),
            Some(device.id),
            None,
            Some(serde_json::json!({
                "device_name": device.name,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(device))
}

// Handler: List all models
#[utoipa::path(
    get,
    path = "/api/models",
    responses(
        (status = 200, description = "List of all models", body = Vec<Model>),
        (status = 500, description = "Internal server error")
    )
)]
async fn list_models(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<ListModelsQuery>,
) -> Result<Json<Vec<Model>>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let log_ctx = LogContext::new().with_user_id(user_id);

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 100);
    let offset = ((page - 1) * per_page) as i64;
    let models = state
        .repo
        .list_models_paginated(per_page as i64, offset)
        .await?;
    log_ctx.info(&format!("Listed {} models", models.len()));

    // Audit log: model list read
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "models.list",
            Some("model"),
            None,
            None,
            Some(serde_json::json!({
                "count": models.len(),
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(models))
}

// Handler: Get model details
#[utoipa::path(
    get,
    path = "/api/models/{id}",
    responses(
        (status = 200, description = "Model details", body = Model),
        (status = 404, description = "Model not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_model(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<Json<Model>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let model = state
        .repo
        .get_model_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Model not found".to_string()))?;

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.read",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "model_name": model.name,
                "version": model.version,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(model))
}

#[utoipa::path(
    get,
    path = "/api/models/active",
    params(
        ("name" = String, Query, description = "Model name"),
        ("release_channel" = Option<String>, Query, description = "Release channel, defaults to stable")
    ),
    responses(
        (status = 200, description = "Active model for the channel", body = Model),
        (status = 404, description = "Active model not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_active_model(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<ActiveModelQuery>,
) -> Result<Json<Model>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let release_channel =
        validate_release_channel(query.release_channel.as_deref().unwrap_or("stable"))?;

    let model = state
        .repo
        .get_active_model(&query.name, &release_channel)
        .await?
        .ok_or_else(|| AppError::NotFound("Active model not found".to_string()))?;

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.active_read",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "model_name": model.name,
                "version": model.version,
                "release_channel": model.release_channel,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(model))
}

#[utoipa::path(
    post,
    path = "/api/models/{id}/activate",
    request_body(content = ActivateModelRequest),
    responses(
        (status = 200, description = "Model activated", body = Model),
        (status = 400, description = "Invalid release channel"),
        (status = 404, description = "Model not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn activate_model(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
    Json(req): Json<ActivateModelRequest>,
) -> Result<Json<Model>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let release_channel =
        validate_release_channel(req.release_channel.as_deref().unwrap_or("stable"))?;

    let model = state
        .repo
        .set_active_model(id, &release_channel)
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Model not found".to_string()),
            other => AppError::Database(other.to_string()),
        })?;

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.activate",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "model_name": model.name,
                "version": model.version,
                "release_channel": model.release_channel,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(model))
}

#[utoipa::path(
    get,
    path = "/api/models/compare",
    params(
        ("base_id" = Uuid, Query, description = "Base model ID"),
        ("target_id" = Uuid, Query, description = "Target model ID")
    ),
    responses(
        (status = 200, description = "Model comparison", body = ModelComparison),
        (status = 400, description = "Invalid comparison"),
        (status = 404, description = "Model not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn compare_models(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<CompareModelsQuery>,
) -> Result<Json<ModelComparison>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    if query.base_id == query.target_id {
        return Err(AppError::Validation(
            "Select two different models to compare".to_string(),
        ));
    }

    let base = state
        .repo
        .get_model_by_id(query.base_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Base model not found".to_string()))?;
    let target = state
        .repo
        .get_model_by_id(query.target_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Target model not found".to_string()))?;

    let mut changed_fields = Vec::new();
    if base.name != target.name {
        changed_fields.push("name".to_string());
    }
    if base.version != target.version {
        changed_fields.push("version".to_string());
    }
    if base.file_name != target.file_name {
        changed_fields.push("file_name".to_string());
    }
    if base.file_size_bytes != target.file_size_bytes {
        changed_fields.push("file_size_bytes".to_string());
    }
    if base.hash_sha256 != target.hash_sha256 {
        changed_fields.push("hash_sha256".to_string());
    }
    if base.model_format != target.model_format {
        changed_fields.push("model_format".to_string());
    }
    if base.metadata != target.metadata {
        changed_fields.push("metadata".to_string());
    }
    if base.is_active != target.is_active || base.release_channel != target.release_channel {
        changed_fields.push("release_state".to_string());
    }

    let comparison = ModelComparison {
        version_delta: target.version - base.version,
        file_size_delta_bytes: match (base.file_size_bytes, target.file_size_bytes) {
            (Some(base_size), Some(target_size)) => Some(target_size - base_size),
            _ => None,
        },
        same_hash: base.hash_sha256 == target.hash_sha256,
        same_format: base.model_format == target.model_format,
        same_metadata: base.metadata == target.metadata,
        base,
        target,
        changed_fields,
    };

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.compare",
            Some("model"),
            None,
            None,
            Some(serde_json::json!({
                "base_id": comparison.base.id,
                "target_id": comparison.target.id,
                "changed_fields": comparison.changed_fields,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(comparison))
}

#[utoipa::path(
    delete,
    path = "/api/models/{id}",
    responses(
        (status = 204, description = "Model deleted"),
        (status = 400, description = "Model is still referenced"),
        (status = 404, description = "Model not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn delete_model(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let model = state
        .repo
        .get_model_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Model not found".to_string()))?;

    let references = state.repo.model_reference_count(id).await?;
    if references > 0 {
        return Err(AppError::Validation(format!(
            "Model is referenced by {} device or deployment records",
            references
        )));
    }

    let s3_key_reference_count = state.repo.s3_key_reference_count(&model.s3_key).await?;
    state.repo.delete_model(id).await.map_err(|e| match e {
        sqlx::Error::RowNotFound => AppError::NotFound("Model not found".to_string()),
        other => AppError::Database(other.to_string()),
    })?;

    if s3_key_reference_count <= 1 {
        if let Err(err) = state.storage.delete(&model.s3_key).await {
            warn!(
                "Deleted model record {} but failed to delete object {}: {}",
                model.id, model.s3_key, err
            );
        }
    }

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.delete",
            Some("model"),
            Some(model.id),
            Some(serde_json::json!({
                "model_name": model.name,
                "version": model.version,
                "s3_key": model.s3_key,
            })),
            None,
            None,
            None,
            None,
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/api/models/presign-upload",
    request_body(content = PresignedModelUploadRequest),
    responses(
        (status = 200, description = "Presigned model upload URL", body = PresignedModelUploadResponse),
        (status = 400, description = "Invalid upload request"),
        (status = 500, description = "Internal server error")
    )
)]
async fn presign_model_upload(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<PresignedModelUploadRequest>,
) -> Result<Json<PresignedModelUploadResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let hash_sha256 = req.hash_sha256.to_lowercase();
    validate_model_upload_fields(
        &req.name,
        req.version,
        &req.file_name,
        req.model_format.as_deref(),
        &hash_sha256,
    )?;

    let expires =
        std::time::Duration::from_secs(req.expires_in_seconds.unwrap_or(900).clamp(60, 3600));
    let presigned = state
        .storage
        .presigned_put_url(&hash_sha256, expires)
        .await?;

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.presign_upload",
            Some("model"),
            None,
            None,
            Some(serde_json::json!({
                "model_name": req.name,
                "version": req.version,
                "file_name": req.file_name,
                "hash": hash_sha256,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(PresignedModelUploadResponse {
        upload_url: presigned.url,
        method: presigned.method,
        headers: presigned
            .headers
            .into_iter()
            .map(|(name, value)| PresignedHeader { name, value })
            .collect(),
        expires_in_seconds: presigned.expires_in_seconds,
        s3_key: hash_sha256,
    }))
}

#[utoipa::path(
    post,
    path = "/api/models/complete-upload",
    request_body(content = CompleteModelUploadRequest),
    responses(
        (status = 200, description = "Model upload completed", body = Model),
        (status = 400, description = "Invalid completion request"),
        (status = 404, description = "Uploaded object not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn complete_model_upload(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CompleteModelUploadRequest>,
) -> Result<Json<Model>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let hash_sha256 = req.hash_sha256.to_lowercase();
    let model_format = validate_model_upload_fields(
        &req.name,
        req.version,
        &req.file_name,
        Some(&req.model_format),
        &hash_sha256,
    )?;

    if req.s3_key != hash_sha256 {
        return Err(AppError::Validation(
            "s3_key must match hash_sha256 for model uploads".to_string(),
        ));
    }

    if !state.storage.object_exists(&req.s3_key).await? {
        return Err(AppError::NotFound(
            "Uploaded model object not found".to_string(),
        ));
    }

    let model = create_model_record(
        &state,
        user_id,
        req.name,
        req.version,
        req.file_name,
        req.file_size_bytes,
        req.s3_key,
        hash_sha256,
        model_format,
        req.metadata.unwrap_or_else(|| serde_json::json!({})),
    )
    .await?;

    Ok(Json(model))
}

#[utoipa::path(
    post,
    path = "/api/models/{id}/presign-download",
    responses(
        (status = 200, description = "Presigned model download URL", body = PresignedModelDownloadResponse),
        (status = 404, description = "Model not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn presign_model_download(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<Json<PresignedModelDownloadResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let model = state
        .repo
        .get_model_by_id(id)
        .await?
        .ok_or_else(|| AppError::NotFound("Model not found".to_string()))?;

    let presigned = state
        .storage
        .presigned_get_url(&model.s3_key, std::time::Duration::from_secs(900))
        .await?;

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.presign_download",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "model_name": model.name,
                "version": model.version,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(PresignedModelDownloadResponse {
        download_url: presigned.url,
        method: presigned.method,
        headers: presigned
            .headers
            .into_iter()
            .map(|(name, value)| PresignedHeader { name, value })
            .collect(),
        expires_in_seconds: presigned.expires_in_seconds,
    }))
}

#[utoipa::path(
    get,
    path = "/api/deployments",
    params(
        ("page" = Option<u32>, Query, description = "Page number, starts at 1"),
        ("per_page" = Option<u32>, Query, description = "Deployments per page")
    ),
    responses(
        (status = 200, description = "List deployments", body = Vec<Deployment>),
        (status = 500, description = "Internal server error")
    )
)]
async fn list_deployments(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<ListDeploymentsQuery>,
) -> Result<Json<Vec<Deployment>>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 100);
    let offset = ((page - 1) * per_page) as i64;
    let deployments = state
        .repo
        .list_deployments_by_user_id(user_id, per_page as i64, offset)
        .await?;

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "deployments.list",
            Some("deployment"),
            None,
            None,
            Some(serde_json::json!({
                "count": deployments.len(),
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(deployments))
}

#[utoipa::path(
    get,
    path = "/api/deployments/{id}",
    params(
        ("id" = Uuid, Path, description = "Deployment ID")
    ),
    responses(
        (status = 200, description = "Deployment details", body = Deployment),
        (status = 404, description = "Deployment not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn get_deployment(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(deployment_id): Path<Uuid>,
) -> Result<Json<Deployment>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let deployment = state
        .repo
        .get_deployment_by_id(deployment_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

    if !state
        .repo
        .user_can_access_deployment(user_id, deployment_id)
        .await?
    {
        return Err(AppError::NotFound("Deployment not found".to_string()));
    }

    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "deployment.read",
            Some("deployment"),
            Some(deployment.id),
            None,
            Some(serde_json::json!({
                "model_id": deployment.model_id,
                "status": deployment.status,
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(deployment))
}

// Handler: Create deployment
#[utoipa::path(
    post,
    path = "/api/deployments",
    request_body(content = CreateDeploymentRequest),
    responses(
        (status = 200, description = "Deployment created", body = Deployment),
        (status = 400, description = "Bad request"),
        (status = 500, description = "Internal server error")
    )
)]
async fn create_deployment(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreateDeploymentRequest>,
) -> Result<Json<Deployment>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let log_ctx = LogContext::new()
        .with_request_id(
            // Will be injected by middleware; fallback if needed
            uuid::Uuid::new_v4().to_string(),
        )
        .with_user_id(user_id);

    log_ctx.info(&format!("Creating deployment for model {}", req.model_id));
    let _model = state
        .repo
        .get_model_by_id(req.model_id)
        .await?
        .ok_or_else(|| {
            log_ctx.error(&format!("Model not found: {}", req.model_id));
            AppError::Validation("Model not found".to_string())
        })?;

    // Determine target devices (user-scoped)
    let target_device_ids: Vec<Uuid> = if let Some(device_uuid) = req.device_id {
        // Single device deployment - verify ownership
        vec![device_uuid]
    } else {
        // Multi-device deployment: all devices belonging to user
        let devices = state.repo.get_devices_by_user_id(user_id).await?;
        devices.into_iter().map(|d| d.id).collect()
    };

    if target_device_ids.is_empty() {
        log_ctx.error("No devices found for deployment");
        return Err(AppError::Validation("No devices found".to_string()));
    }

    log_ctx.info(&format!(
        "Target devices count: {}",
        target_device_ids.len()
    ));

    // Verify ownership for single device deployment
    if let Some(device_uuid) = req.device_id {
        let device = state
            .repo
            .get_device_by_id(device_uuid)
            .await?
            .ok_or_else(|| {
                log_ctx.error(&format!("Device not found: {}", device_uuid));
                AppError::Validation(format!("Device {} not found", device_uuid))
            })?;
        if device.user_id != Some(user_id) {
            log_ctx.error(&format!(
                "User {} does not own device {}",
                user_id, device_uuid
            ));
            return Err(AppError::Auth(
                "You do not have permission to deploy to this device".to_string(),
            ));
        }
    }

    // Apply rollout percentage if specified
    let rollout_percentage_val = req.rollout_percentage.unwrap_or(100);
    let target_count = if rollout_percentage_val < 100 {
        ((target_device_ids.len() as f32) * (rollout_percentage_val as f32 / 100.0)).ceil() as usize
    } else {
        target_device_ids.len()
    };

    // For now, take first N devices
    let selected_devices: Vec<Uuid> = target_device_ids.into_iter().take(target_count).collect();

    // Build rollout_config and extract phases for phase assignment
    let rollout_strategy = req
        .rollout_strategy
        .unwrap_or_else(|| "all_at_once".to_string());

    let rollout_config_value = req.rollout_config.clone().unwrap_or_else(|| {
        if rollout_strategy == "phased" {
            serde_json::json!({
                "strategy": "phased",
                "phases": [
                    { "percentage": 10 },
                    { "percentage": 50 },
                    { "percentage": 100 }
                ]
            })
        } else {
            serde_json::json!({
                "strategy": rollout_strategy,
                "phases": [
                    { "percentage": 100 }
                ]
            })
        }
    });

    // Extract phases as cumulative rollout percentages.
    let mut cumulative_percentages: Vec<f64> = rollout_config_value
        .get("phases")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|phase| phase.get("percentage").and_then(|p| p.as_i64()))
                .map(|pct| (pct as f64 / 100.0).clamp(0.0, 1.0))
                .collect::<Vec<f64>>()
        })
        .unwrap_or_default();
    cumulative_percentages.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    cumulative_percentages.dedup();
    if cumulative_percentages.is_empty() {
        cumulative_percentages.push(1.0);
    } else if let Some(last) = cumulative_percentages.last_mut() {
        *last = 1.0; // ensure final is exactly 1.0
    }

    // Helper: get phase for device index
    let get_phase = |index: usize, total: usize| -> i32 {
        if total == 0 || cumulative_percentages.is_empty() {
            return 0;
        }
        let ratio = index as f64 / total as f64;
        for (phase_idx, &cumulative) in cumulative_percentages.iter().enumerate() {
            if ratio < cumulative {
                return phase_idx as i32;
            }
        }
        (cumulative_percentages.len() - 1) as i32
    };

    // Create deployment with all new fields
    let deployment = state
        .repo
        .create_deployment(
            None, // device_id is not directly used; deployment_devices tracks per-device
            req.model_id,
            "deploying".to_string(), // Start as deploying immediately
            rollout_strategy.clone(),
            rollout_percentage_val,
            Some(rollout_config_value),          // moved here
            Some(0),                             // current_phase = 0
            Some(selected_devices.len() as i32), // devices_target
            Some(0),                             // devices_deployed
            Some(0),                             // devices_succeeded
            Some(0),                             // devices_failed
            None,                                // rollback_of
        )
        .await
        .map_err(|e| {
            log_ctx.error(&format!("Failed to create deployment: {}", e));
            AppError::Database(e.to_string())
        })?;

    log_ctx.info(&format!(
        "Deployment created: id={}, target_devices={}",
        deployment.id,
        selected_devices.len()
    ));

    // Create deployment_devices entries for each selected device with assigned phase
    for (index, device_id) in selected_devices.iter().enumerate() {
        let device_opt = state.repo.get_device_by_id(*device_id).await.map_err(|e| {
            error!("Database error fetching device {}: {}", device_id, e);
            AppError::Database(e.to_string())
        })?;
        let device = device_opt.ok_or_else(|| {
            error!("Device not found: {}", device_id);
            AppError::Validation(format!("Device {} not found", device_id))
        })?;

        let previous_model_id = None; // Could track current model if device is running one
        let phase = get_phase(index, selected_devices.len());

        if let Err(e) = state
            .repo
            .create_deployment_device(
                deployment.id,
                *device_id,
                "pending",
                previous_model_id,
                Some(req.model_id),
                phase,
            )
            .await
        {
            let error_msg = format!("Failed to create deployment device record: {}", e);
            error!("{}", error_msg);
            // Alert on deployment device creation failure
            let alert_manager = state.alert_manager.clone();
            let deployment_id_str = deployment.id.to_string();
            let device_id_str = device.device_id.clone();
            tokio::spawn(async move {
                alert_helpers::alert_deployment_failure(
                    &alert_manager,
                    &deployment_id_str,
                    &device_id_str,
                    &error_msg,
                )
                .await;
            });
            return Err(AppError::Database(e.to_string()));
        }
    }

    // Record deployment created metric
    state
        .metrics
        .deployments_total
        .with_label_values(&["created"])
        .inc();

    deployment_flow::push_deployment_phase(&state, deployment.id, 0)
        .await
        .map_err(AppError::Database)?;

    // Refresh deployment object to return updated counters
    let updated_deployment = state
        .repo
        .get_deployment_by_id(deployment.id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Deployment not found after creation".to_string()))?;

    let log_message = format!(
        "Deployment created successfully: id={}, devices_target={:?}, rollout_strategy={:?}",
        updated_deployment.id,
        updated_deployment.devices_target,
        updated_deployment.rollout_strategy
    );
    log_ctx.info(&log_message);

    // Audit log: deployment creation
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "deployment.create",
            Some("deployment"),
            Some(deployment.id),
            None,
            Some(serde_json::json!({
                "model_id": deployment.model_id,
                "rollout_strategy": rollout_strategy,
                "rollout_percentage": rollout_percentage_val,
                "target_device_count": selected_devices.len(),
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(updated_deployment))
}

// Handler: Rollback deployment
#[utoipa::path(
    post,
    path = "/api/deployments/{id}/rollback",
    params(
        ("id" = Uuid, Path, description = "Deployment ID to rollback")
    ),
    request_body(content = RollbackRequest),
    responses(
        (status = 200, description = "Rollback created", body = Deployment),
        (status = 404, description = "Deployment not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn rollback_deployment(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(deployment_id): Path<Uuid>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<Deployment>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let log_ctx = LogContext::new().with_user_id(user_id);

    log_ctx.info(&format!(
        "Initiating rollback for deployment {} to model {}",
        deployment_id, req.model_id
    ));
    let _original = state
        .repo
        .get_deployment_by_id(deployment_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

    // Validate target model exists
    let _target_model = state
        .repo
        .get_model_by_id(req.model_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::Validation("Model not found".to_string()))?;

    // Determine target devices: copy from original deployment's deployment_devices
    let original_device_entries = state
        .repo
        .get_deployment_devices(deployment_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    let target_device_ids: Vec<Uuid> = original_device_entries
        .into_iter()
        .map(|dd| dd.device_id)
        .collect();

    if target_device_ids.is_empty() {
        return Err(AppError::Validation(
            "No devices found for rollback".to_string(),
        ));
    }

    // Create rollback deployment with 100% rollout, all_at_once strategy
    let rollback = state
        .repo
        .create_deployment(
            None,
            req.model_id,
            "deploying".to_string(),
            "all_at_once".to_string(),
            100,
            Some(serde_json::json!({
                "strategy": "all_at_once",
                "rollback_of": deployment_id
            })),
            Some(0),
            Some(target_device_ids.len() as i32),
            Some(0),
            Some(0),
            Some(0),
            Some(deployment_id),
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    // Create deployment_devices entries (all phase 0 for rollback - immediate)
    for device_id in &target_device_ids {
        let device_opt = state.repo.get_device_by_id(*device_id).await.map_err(|e| {
            error!(
                "Database error fetching device {} during rollback: {}",
                device_id, e
            );
            AppError::Database(e.to_string())
        })?;
        let device = device_opt.ok_or_else(|| {
            error!("Device not found during rollback: {}", device_id);
            AppError::Validation(format!("Device {} not found during rollback", device_id))
        })?;

        if let Err(e) = state
            .repo
            .create_deployment_device(
                rollback.id,
                *device_id,
                "pending",
                None, // previous_model_id - could be populated from original device state
                Some(req.model_id),
                0, // phase: rollback is all_at_once, so all devices in phase 0
            )
            .await
        {
            let error_msg = format!("Failed to create rollback deployment device record: {}", e);
            error!("{}", error_msg);
            let alert_manager = state.alert_manager.clone();
            let rollback_id_str = rollback.id.to_string();
            let device_id_str = device.device_id.clone();
            tokio::spawn(async move {
                alert_helpers::alert_deployment_failure(
                    &alert_manager,
                    &rollback_id_str,
                    &device_id_str,
                    &error_msg,
                )
                .await;
            });
            return Err(AppError::Database(e.to_string()));
        }
    }

    deployment_flow::push_deployment_phase(&state, rollback.id, 0)
        .await
        .map_err(AppError::Database)?;

    let updated_rollback = state
        .repo
        .get_deployment_by_id(rollback.id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| {
            AppError::NotFound("Rollback deployment not found after creation".to_string())
        })?;

    log_ctx.info(&format!(
        "Rollback created successfully: id={}, original_deployment={}",
        updated_rollback.id, deployment_id
    ));

    // Audit log: deployment rollback
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "deployment.rollback",
            Some("deployment"),
            Some(rollback.id),
            None,
            Some(serde_json::json!({
                "original_deployment_id": deployment_id,
                "target_model_id": req.model_id,
                "target_device_count": target_device_ids.len(),
            })),
            None,
            None,
            None,
        )
        .await;

    Ok(Json(updated_rollback))
}

// Handler: List deployment devices with phase info
#[utoipa::path(
    get,
    path = "/api/deployments/{id}/devices",
    params(
        ("id" = Uuid, Path, description = "Deployment ID")
    ),
    responses(
        (status = 200, description = "List of deployment devices with phase info", body = Vec<DeploymentDeviceInfo>),
        (status = 500, description = "Internal server error")
    )
)]
async fn list_deployment_devices(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(deployment_id): Path<Uuid>,
) -> Result<Json<Vec<DeploymentDeviceInfo>>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    if !state
        .repo
        .user_can_access_deployment(user_id, deployment_id)
        .await?
    {
        return Err(AppError::NotFound("Deployment not found".to_string()));
    }

    let devices = state
        .repo
        .get_deployment_devices_detailed(deployment_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(Json(devices))
}

// Health check endpoint (liveness)
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    )
)]
async fn health_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "ok".to_string(),
        }),
    )
}

// Readiness check endpoint (dependencies healthy)
#[utoipa::path(
    get,
    path = "/ready",
    responses(
        (status = 200, description = "Service is ready", body = ReadinessResponse),
        (status = 503, description = "Service unavailable")
    )
)]
async fn readiness_check(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ReadinessResponse>, AppError> {
    // Check database connectivity
    let result = sqlx::query("SELECT 1")
        .fetch_optional(&state.repo.pool)
        .await;

    match result {
        Ok(_) => Ok(Json(ReadinessResponse {
            status: "ready".to_string(),
            database: "connected".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        })),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

// OpenAPI documentation endpoint
#[utoipa::path(
    get,
    path = "/api-docs/openapi.json",
    responses(
        (status = 200, description = "OpenAPI specification in JSON format", content_type = "application/json")
    )
)]
async fn openapi_handler() -> impl IntoResponse {
    let openapi = ApiDoc::openapi();
    (StatusCode::OK, axum::response::Json(openapi))
}

// Metrics endpoint (protected by token if configured)
async fn metrics_handler(headers: HeaderMap) -> impl IntoResponse {
    // Check for metrics token if configured
    if let Ok(expected_token) = env::var("METRICS_TOKEN") {
        if !expected_token.is_empty() {
            let provided_token = headers
                .get("X-Metrics-Token")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");

            if provided_token != expected_token {
                return (StatusCode::UNAUTHORIZED, "Invalid metrics token").into_response();
            }
        }
    }

    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    TextEncoder::new()
        .encode(&metric_families, &mut buffer)
        .unwrap();
    (
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4; charset=utf-8")],
        buffer,
    )
        .into_response()
}

// Handler: Upload model
#[utoipa::path(
    post,
    path = "/api/models/upload",
    responses(
        (status = 200, description = "Model uploaded", body = Model),
        (status = 400, description = "Invalid upload"),
        (status = 500, description = "Internal server error")
    )
)]
async fn upload_model(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    mut multipart: Multipart,
) -> Result<Json<Model>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;
    let log_ctx = LogContext::new().with_user_id(user_id);

    log_ctx.info("Starting model upload");
    let mut version = None;
    let mut file_bytes = None;
    let mut file_name = None;
    let mut name = None;
    let mut requested_model_format = None;
    let mut expected_sha256 = None;
    let mut input_shapes = None;
    let mut output_shapes = None;
    let mut classes = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();

        if field_name == "name" {
            let text = field
                .text()
                .await
                .map_err(|e| AppError::Validation(e.to_string()))?;
            name = Some(text);
        } else if field_name == "version" {
            let text = field
                .text()
                .await
                .map_err(|e| AppError::Validation(e.to_string()))?;
            version = text.parse().ok();
        } else if field_name == "model_format" {
            let text = field
                .text()
                .await
                .map_err(|e| AppError::Validation(e.to_string()))?;
            requested_model_format = Some(text.trim().to_lowercase());
        } else if field_name == "sha256" {
            let text = field
                .text()
                .await
                .map_err(|e| AppError::Validation(e.to_string()))?;
            expected_sha256 = Some(text.trim().to_lowercase());
        } else if field_name == "input_shapes" {
            input_shapes = Some(
                field
                    .text()
                    .await
                    .map_err(|e| AppError::Validation(e.to_string()))?,
            );
        } else if field_name == "output_shapes" {
            output_shapes = Some(
                field
                    .text()
                    .await
                    .map_err(|e| AppError::Validation(e.to_string()))?,
            );
        } else if field_name == "classes" {
            classes = Some(
                field
                    .text()
                    .await
                    .map_err(|e| AppError::Validation(e.to_string()))?,
            );
        } else if field_name == "file" {
            let fname = field.file_name().unwrap_or("unknown").to_string();
            file_name = Some(fname.clone());
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError::Validation(e.to_string()))?;
            file_bytes = Some(bytes.to_vec());
        } else {
            // Skip other fields
            let _ = field.bytes().await;
        }
    }

    let name = name.ok_or_else(|| AppError::Validation("Missing field: name".to_string()))?;
    if name.trim().is_empty() {
        return Err(AppError::Validation("Model name is required".to_string()));
    }

    let file_bytes =
        file_bytes.ok_or_else(|| AppError::Validation("Missing field: file".to_string()))?;
    let version = version.unwrap_or(1);
    if version < 1 {
        return Err(AppError::Validation(
            "Version must be a positive integer".to_string(),
        ));
    }

    let file_name = file_name.unwrap_or_else(|| "unknown".to_string());

    // Validate file size (max 100MB)
    const MAX_UPLOAD_SIZE: usize = 100 * 1024 * 1024; // 100MB
    if file_bytes.len() > MAX_UPLOAD_SIZE {
        return Err(AppError::Validation(format!(
            "File too large, maximum {} bytes allowed",
            MAX_UPLOAD_SIZE
        )));
    }

    // Infer model format from file extension
    let inferred_model_format = infer_model_format(&file_name);
    let model_format = requested_model_format.unwrap_or_else(|| inferred_model_format.clone());
    validate_model_format(&model_format, &inferred_model_format)?;

    // Compute SHA256 hash
    let hash = Sha256::digest(&file_bytes);
    let hash_hex = format!("{:x}", hash);
    let file_size = file_bytes.len();
    if let Some(expected) = expected_sha256 {
        if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::Validation(
                "sha256 must be a 64-character hex string".to_string(),
            ));
        }
        if expected != hash_hex {
            return Err(AppError::Validation(
                "SHA-256 hash verification failed".to_string(),
            ));
        }
    }

    // Store file in S3 or local storage
    state.storage.upload(&hash_hex, file_bytes).await?;

    let hash_hex_for_audit = hash_hex.clone();
    let model = create_model_record(
        &state,
        user_id,
        name,
        version,
        file_name.clone(),
        Some(file_size as i64),
        hash_hex.clone(),
        hash_hex,
        model_format.clone(),
        build_model_metadata(input_shapes, output_shapes, classes)?,
    )
    .await?;

    log_ctx.info(&format!(
        "Model uploaded successfully: id={}, format={}, size={} bytes",
        model.id, model_format, file_size
    ));

    // Audit log: model upload
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.upload",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "file_name": file_name,
                "file_size": file_size,
                "format": model_format,
                "hash": hash_hex_for_audit,
            })),
            None, // IP would come from middleware
            None,
            None,
        )
        .await;

    Ok(Json(model))
}

/// Infer model format from filename extension
fn infer_model_format(file_name: &str) -> String {
    let ext = std::path::Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "onnx" => "onnx".to_string(),
        "tflite" => "tflite".to_string(),
        "pb" => "tensorflow".to_string(),
        "h5" => "tensorflow".to_string(),
        "pth" => "pytorch".to_string(),
        _ => "unknown".to_string(),
    }
}

fn validate_model_format(model_format: &str, inferred_model_format: &str) -> Result<(), AppError> {
    match model_format {
        "onnx" | "tflite" => {}
        _ => {
            return Err(AppError::Validation(
                "Unsupported model format. Only ONNX and TFLite are supported".to_string(),
            ));
        }
    }

    if inferred_model_format == "unknown" {
        return Err(AppError::Validation(
            "Unsupported model file extension. Use .onnx or .tflite".to_string(),
        ));
    }

    if model_format != inferred_model_format {
        return Err(AppError::Validation(format!(
            "model_format '{}' does not match file extension '{}'",
            model_format, inferred_model_format
        )));
    }

    Ok(())
}

fn validate_model_upload_fields(
    name: &str,
    version: i32,
    file_name: &str,
    requested_model_format: Option<&str>,
    hash_sha256: &str,
) -> Result<String, AppError> {
    if name.trim().is_empty() {
        return Err(AppError::Validation("Model name is required".to_string()));
    }
    if version < 1 {
        return Err(AppError::Validation(
            "Version must be a positive integer".to_string(),
        ));
    }
    if hash_sha256.len() != 64 || !hash_sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AppError::Validation(
            "hash_sha256 must be a 64-character hex string".to_string(),
        ));
    }

    let inferred_model_format = infer_model_format(file_name);
    let model_format = requested_model_format
        .map(|value| value.trim().to_lowercase())
        .unwrap_or_else(|| inferred_model_format.clone());
    validate_model_format(&model_format, &inferred_model_format)?;
    Ok(model_format)
}

fn build_model_metadata(
    input_shapes: Option<String>,
    output_shapes: Option<String>,
    classes: Option<String>,
) -> Result<serde_json::Value, AppError> {
    let mut metadata = serde_json::Map::new();

    if let Some(value) = input_shapes {
        metadata.insert(
            "input_shapes".to_string(),
            parse_metadata_json("input_shapes", &value)?,
        );
    }
    if let Some(value) = output_shapes {
        metadata.insert(
            "output_shapes".to_string(),
            parse_metadata_json("output_shapes", &value)?,
        );
    }
    if let Some(value) = classes {
        metadata.insert(
            "classes".to_string(),
            parse_metadata_json("classes", &value)?,
        );
    }

    Ok(serde_json::Value::Object(metadata))
}

fn parse_metadata_json(field_name: &str, value: &str) -> Result<serde_json::Value, AppError> {
    if value.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }

    serde_json::from_str(value)
        .map_err(|_| AppError::Validation(format!("{} must be valid JSON", field_name)))
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db_error| db_error.code())
        .as_deref()
        == Some("23505")
}

fn validate_release_channel(value: &str) -> Result<String, AppError> {
    let channel = value.trim().to_lowercase();
    if channel.is_empty() {
        return Err(AppError::Validation(
            "release_channel cannot be empty".to_string(),
        ));
    }

    let valid = channel
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !valid || channel.len() > 50 {
        return Err(AppError::Validation(
            "release_channel must be 1-50 lowercase letters, numbers, dashes, or underscores"
                .to_string(),
        ));
    }

    Ok(channel)
}

async fn create_model_record(
    state: &Arc<AppState>,
    user_id: Uuid,
    name: String,
    version: i32,
    file_name: String,
    file_size_bytes: Option<i64>,
    s3_key: String,
    hash_sha256: String,
    model_format: String,
    metadata: serde_json::Value,
) -> Result<Model, AppError> {
    let log_ctx = LogContext::new().with_user_id(user_id);

    state
        .repo
        .create_model(
            name,
            version,
            file_name,
            file_size_bytes,
            s3_key,
            hash_sha256,
            model_format,
            metadata,
            Some(user_id),
        )
        .await
        .map_err(|e| {
            log_ctx.error(&format!("Failed to create model record: {}", e));
            if is_unique_violation(&e) {
                AppError::Validation(
                    "A model with this name and version already exists".to_string(),
                )
            } else {
                AppError::Database(e.to_string())
            }
        })
}

// Handler: Download model file (device token auth)
async fn download_model_device(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    // Extract and validate device token
    let token = match headers.get("Authorization") {
        Some(t) => t,
        None => return Err(AppError::Auth("Missing authorization header".to_string())),
    };
    let token = token.to_str().map_err(|e| AppError::Auth(e.to_string()))?;
    let token = token
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Auth("Invalid token format".to_string()))?;

    let device_opt = state.repo.get_device_by_token(token).await?;
    let device = device_opt.ok_or_else(|| AppError::Auth("Invalid device token".to_string()))?;

    let model = state
        .repo
        .get_model_by_id(id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Model not found".to_string()))?;

    // Audit log: model download by device
    let _ = state
        .repo
        .insert_audit_log(
            "device",
            Some(device.id),
            "model.download",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "device_id": device.device_id,
                "model_name": model.name,
                "format": model.model_format,
            })),
            None,
            None,
            None,
        )
        .await;

    let bytes = state.storage.download(&model.s3_key).await?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes))
}

// Handler: Download model file (JWT auth - for UI)
async fn download_model(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let model = state
        .repo
        .get_model_by_id(id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Model not found".to_string()))?;

    // Audit log: model download by user
    let _ = state
        .repo
        .insert_audit_log(
            "user",
            Some(user_id),
            "model.download",
            Some("model"),
            Some(model.id),
            None,
            Some(serde_json::json!({
                "model_name": model.name,
                "format": model.model_format,
            })),
            None,
            None,
            None,
        )
        .await;

    let bytes = state.storage.download(&model.s3_key).await?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream")], bytes))
}

// Handler: Test alert (for development/testing)
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TestAlertRequest {
    pub severity: Option<String>,
    pub title: String,
    pub description: String,
}

#[utoipa::path(
    post,
    path = "/api/alerts/test",
    request_body(content = TestAlertRequest),
    responses(
        (status = 200, description = "Alert sent successfully"),
        (status = 500, description = "Failed to send alert")
    )
)]
async fn test_alert(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TestAlertRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    use alerts::{Alert, AlertSeverity, AlertSource};

    let severity = match req.severity.as_deref() {
        Some("critical") => AlertSeverity::Critical,
        Some("warning") => AlertSeverity::Warning,
        _ => AlertSeverity::Info,
    };

    let alert = Alert {
        id: uuid::Uuid::new_v4().to_string(),
        severity,
        title: req.title,
        description: req.description,
        source: AlertSource::System,
        metadata: None,
        created_at: chrono::Utc::now(),
    };

    // Send alert asynchronously (don't wait for completion)
    let alert_manager = state.alert_manager.clone();
    tokio::spawn(async move {
        if let Err(e) = alert_manager.lock().await.send_alert(alert).await {
            error!("Failed to send test alert: {}", e);
        }
    });

    Ok(Json(serde_json::json!({
        "status": "alert_queued",
        "message": "Test alert sent"
    })))
}

// Alert management handlers

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListAlertsQuery {
    pub status: Option<String>,
    pub severity: Option<String>,
    pub source: Option<String>,
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

#[utoipa::path(
    get,
    path = "/api/alerts",
    params(
        ("status" = Option<String>, Query, description = "Filter by status: open, acknowledged, silenced, closed"),
        ("severity" = Option<String>, Query, description = "Filter by severity: info, warning, critical"),
        ("source" = Option<String>, Query, description = "Filter by source: deployment, device, system, security"),
        ("page" = Option<u32>, Query, description = "Page number"),
        ("per_page" = Option<u32>, Query, description = "Alerts per page")
    ),
    responses(
        (status = 200, description = "List of alerts", body = Vec<Alert>),
        (status = 500, description = "Internal server error")
    )
)]
async fn list_alerts(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<ListAlertsQuery>,
) -> Result<Json<Vec<backend::models::Alert>>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(50).clamp(1, 100);
    let offset = ((page - 1) * per_page) as i64;

    let alerts = state
        .repo
        .list_alerts(
            user_id,
            query.status.as_deref(),
            query.severity.as_deref(),
            query.source.as_deref(),
            per_page as i64,
            offset,
        )
        .await?;

    Ok(Json(alerts))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AcknowledgeAlertRequest {
    #[serde(skip)]
    pub _dummy: (), // No body required, user ID comes from JWT
}

#[utoipa::path(
    post,
    path = "/api/alerts/{id}/acknowledge",
    params(
        ("id" = Uuid, Path, description = "Alert ID")
    ),
    responses(
        (status = 200, description = "Alert acknowledged"),
        (status = 404, description = "Alert not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn acknowledge_alert(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Path(alert_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    state
        .repo
        .acknowledge_alert(alert_id, user_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "acknowledged",
        "message": "Alert acknowledged"
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SilenceAlertRequest {
    pub minutes: u32,
}

#[utoipa::path(
    post,
    path = "/api/alerts/{id}/silence",
    params(
        ("id" = Uuid, Path, description = "Alert ID")
    ),
    request_body(content = SilenceAlertRequest),
    responses(
        (status = 200, description = "Alert silenced"),
        (status = 404, description = "Alert not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn silence_alert(
    State(state): State<Arc<AppState>>,
    Path(alert_id): Path<Uuid>,
    Json(req): Json<SilenceAlertRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let silenced_until = chrono::Utc::now() + chrono::Duration::minutes(req.minutes as i64);

    state
        .repo
        .silence_alert(alert_id, silenced_until)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "silenced",
        "message": format!("Alert silenced for {} minutes", req.minutes)
    })))
}

#[utoipa::path(
    post,
    path = "/api/alerts/{id}/close",
    params(
        ("id" = Uuid, Path, description = "Alert ID")
    ),
    responses(
        (status = 200, description = "Alert closed"),
        (status = 404, description = "Alert not found"),
        (status = 500, description = "Internal server error")
    )
)]
async fn close_alert(
    State(state): State<Arc<AppState>>,
    Path(alert_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, AppError> {
    state
        .repo
        .close_alert(alert_id)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "closed",
        "message": "Alert closed"
    })))
}

// Audit Log API

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ListAuditLogsQuery {
    pub actor_type: Option<String>,
    pub actor_id: Option<Uuid>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/audit-logs",
    params(
        ("actor_type" = Option<String>, Query, description = "Filter by actor type"),
        ("actor_id" = Option<Uuid>, Query, description = "Filter by actor ID"),
        ("action" = Option<String>, Query, description = "Filter by action"),
        ("resource_type" = Option<String>, Query, description = "Filter by resource type"),
        ("resource_id" = Option<Uuid>, Query, description = "Filter by resource ID"),
        ("start_date" = Option<DateTime<Utc>>, Query, description = "Filter by start date"),
        ("end_date" = Option<DateTime<Utc>>, Query, description = "Filter by end date"),
        ("limit" = Option<i64>, Query, description = "Maximum number of results"),
        ("offset" = Option<i64>, Query, description = "Offset for pagination")
    ),
    responses(
        (status = 200, description = "List of audit logs", body = Vec<AuditLog>),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error")
    )
)]
async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<ListAuditLogsQuery>,
) -> Result<Json<Vec<AuditLog>>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Validation("Invalid user ID".to_string()))?;

    // Non-admin users can only see their own audit logs
    let actor_id = if claims.role == "admin" {
        query.actor_id
    } else {
        Some(user_id)
    };

    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);

    let audit_logs = state
        .repo
        .list_audit_logs(
            query.actor_type.as_deref(),
            actor_id,
            query.action.as_deref(),
            query.resource_type.as_deref(),
            query.resource_id,
            query.start_date,
            query.end_date,
            limit,
            offset,
        )
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

    Ok(Json(audit_logs))
}
