use crate::models::*;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub exp: usize,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterRequest {
    pub email: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub user: User,
}

pub fn create_jwt(user: &User, secret: &str) -> Result<String, jsonwebtoken::errors::Error> {
    let expiration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 3600; // 1 hour

    let claims = Claims {
        sub: user.id.to_string(),
        username: user.username.clone(),
        role: user.role.clone(),
        exp: expiration as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn validate_jwt(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let validation = Validation::default();
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
}

pub fn hash_password(password: &str) -> String {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}

// Issue a new refresh token for a user
pub async fn issue_refresh_token(
    repo: &crate::repositories::Repository,
    user_id: Uuid,
) -> Result<String, crate::error::AppError> {
    let token = Uuid::new_v4().to_string();
    let token_hash = format!("{:x}", Sha256::digest(token.as_bytes()));
    let expires_at = Utc::now() + Duration::days(30);

    repo.create_refresh_token(user_id, token_hash, expires_at)
        .await
        .map_err(|e| crate::error::AppError::Database(e.to_string()))?;

    Ok(token)
}

// Validation functions
lazy_static::lazy_static! {
    static ref EMAIL_REGEX: Regex = Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$").unwrap();
}

pub fn validate_email(email: &str) -> Result<(), String> {
    if email.is_empty() {
        return Err("Email is required".to_string());
    }
    if email.len() > 254 {
        return Err("Email is too long".to_string());
    }
    if !EMAIL_REGEX.is_match(email) {
        return Err("Invalid email format".to_string());
    }
    Ok(())
}

pub fn validate_password(password: &str) -> Result<(), String> {
    if password.is_empty() {
        return Err("Password is required".to_string());
    }
    if password.len() < 8 {
        return Err("Password must be at least 8 characters".to_string());
    }
    // Basic strength check: at least one uppercase, one lowercase, one digit
    let has_upper = password.chars().any(|c| c.is_uppercase());
    let has_lower = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());

    if !(has_upper && has_lower && has_digit) {
        return Err("Password must contain at least one uppercase letter, one lowercase letter, and one digit".to_string());
    }
    Ok(())
}
