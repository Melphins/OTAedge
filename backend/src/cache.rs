use bincode;
use bincode::serde::{decode_from_slice, encode_to_vec};
use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::{Connection, PoolError};
use serde::Serialize;
use std::fmt::Debug;
use std::time::Duration;

#[derive(Clone)]
pub struct Cache {
    pool: deadpool_redis::Pool,
    default_ttl: Duration,
}

impl Cache {
    pub fn new(pool: deadpool_redis::Pool, default_ttl: Duration) -> Self {
        Self { pool, default_ttl }
    }

    async fn get_connection(&self) -> Result<Connection, PoolError> {
        self.pool.get().await
    }

    pub async fn get<T>(&self, key: &str) -> Result<Option<T>, CacheError>
    where
        T: serde::de::DeserializeOwned + Debug,
    {
        let mut conn = self.get_connection().await?;
        let data: Vec<u8> = conn
            .get(key)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        if data.is_empty() {
            return Ok(None);
        }
        let (value, _) = decode_from_slice(&data, bincode::config::standard())
            .map_err(|e| CacheError::Serialization(e.to_string()))?;
        Ok(Some(value))
    }

    pub async fn set<T>(
        &self,
        key: &str,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<(), CacheError>
    where
        T: Serialize + Debug,
    {
        let mut conn = self.get_connection().await?;
        let data = encode_to_vec(value, bincode::config::standard())
            .map_err(|e| CacheError::Serialization(e.to_string()))?;
        let ttl = ttl.unwrap_or(self.default_ttl);
        conn.set_ex::<_, _, ()>(key, data, ttl.as_secs())
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<(), CacheError> {
        let mut conn = self.get_connection().await?;
        conn.del::<_, ()>(key)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(())
    }

    pub async fn exists(&self, key: &str) -> Result<bool, CacheError> {
        let mut conn = self.get_connection().await?;
        let exists: bool = conn
            .exists(key)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        Ok(exists)
    }

    pub async fn invalidate_pattern(&self, pattern: &str) -> Result<(), CacheError> {
        let mut conn = self.get_connection().await?;
        let keys: Vec<String> = conn
            .keys(pattern)
            .await
            .map_err(|e| CacheError::Redis(e.to_string()))?;
        if !keys.is_empty() {
            conn.del::<_, ()>(keys)
                .await
                .map_err(|e| CacheError::Redis(e.to_string()))?;
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("Redis pool error: {0}")]
    RedisPool(#[from] PoolError),
    #[error("Redis error: {0}")]
    Redis(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Cache miss")]
    Miss,
}

pub type CacheResult<T> = Result<T, CacheError>;
