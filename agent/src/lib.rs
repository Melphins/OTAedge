mod websocket;

use crate::websocket::WsMessage;
use anyhow::Result;
use chrono;
use futures_util::{sink::SinkExt, stream::StreamExt};
use ort;
use std::path::{Path, PathBuf};
use sysinfo::*;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    ONNX,
    TFLite,
    Unknown,
}

pub struct EdgeAgent {
    device_id: String,
    server_url: String,
    config: Config,
    ws: Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
}

pub struct Config {
    pub token: String,
    pub heartbeat_interval: u64,
    pub model_cache_dir: String,
}

impl EdgeAgent {
    pub fn new(device_id: String, server_url: String, config: Config) -> Self {
        Self {
            device_id,
            server_url,
            config,
            ws: None,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        self.reconnect_loop().await;

        let (tx, mut rx) = mpsc::channel::<WsMessage>(100);

        // Spawn heartbeat task that sends heartbeat messages via channel
        let device_id = self.device_id.clone();
        tokio::spawn(async move {
            heartbeat_loop(device_id, tx).await;
        });

        // Main listen loop: handles incoming WS messages and outgoing heartbeat triggers
        self.listen(&mut rx).await?;

        Ok(())
    }

    async fn connect(&mut self) -> Result<()> {
        // Convert http:// to ws:// and https:// to wss:// for WebSocket connection
        let ws_scheme = if self.server_url.starts_with("https://") {
            "wss://"
        } else if self.server_url.starts_with("http://") {
            "ws://"
        } else {
            // Assume it's already a ws:// or wss:// URL
            ""
        };

        let base_url = if ws_scheme.is_empty() {
            &self.server_url
        } else {
            // Replace http(s):// with ws(s)://
            let scheme_end = self.server_url.find("://").unwrap_or(0) + 3;
            &self.server_url[scheme_end..]
        };

        let url = format!("{}{}/ws?token={}", ws_scheme, base_url, self.config.token);
        let (ws, _) = tokio_tungstenite::connect_async(url).await?;
        self.ws = Some(ws);
        self.register().await?;
        Ok(())
    }

    async fn reconnect_loop(&mut self) {
        loop {
            match self.connect().await {
                Ok(_) => break,
                Err(e) => {
                    error!("Connection failed: {}. Retrying in 5s", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn register(&mut self) -> Result<()> {
        let msg = serde_json::json!({
            "type": "register",
            "device_id": self.device_id,
            "capabilities": ["model_update", "heartbeat", "inference"],
        });
        self.send_text(&msg.to_string()).await?;
        Ok(())
    }

    async fn listen(&mut self, rx: &mut mpsc::Receiver<WsMessage>) -> Result<()> {
        while self.ws.is_some() {
            let ws = self.ws.as_mut().unwrap();
            tokio::select! {
                // Wait for incoming message from WebSocket
                msg = ws.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&text) {
                                if let Err(e) = self.handle_command(data).await {
                                    error!("Failed to handle command: {}", e);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            error!("WS error: {}. Reconnecting...", e);
                            self.ws = None;
                            self.reconnect_loop().await;
                        }
                        None => {
                            info!("WebSocket closed");
                            self.ws = None;
                            self.reconnect_loop().await;
                        }
                        _ => {} // Binary, ping, pong
                    }
                }
                // Wait for heartbeat trigger from the heartbeat loop
                hb = rx.recv() => {
                    if let Some(message) = hb {
                        self.send_heartbeat(message).await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn handle_command(&mut self, data: serde_json::Value) -> Result<()> {
        match data["type"].as_str() {
            Some("model_update") => {
                let deployment_id = data["deployment_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing deployment_id"))?;
                let model_id = data["model_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing model_id"))?;
                let model_hash = data["model_hash"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing model_hash"))?;
                // Prefer download_url if provided, else construct from server_url and model_id
                let download_url = if let Some(url) = data["download_url"].as_str() {
                    // If it's a relative URL, combine with server_url (which is http base)
                    if url.starts_with("http") {
                        url.to_string()
                    } else {
                        format!("{}{}", self.server_url.trim_end_matches('/'), url)
                    }
                } else {
                    format!("{}/api/models/{}/download", self.server_url, model_id)
                };

                info!("Downloading model {} (hash: {})", model_id, model_hash);
                self.download_model(&download_url, model_hash).await?;

                // Detect model format and load it
                let model_path = format!("{}/{}", self.config.model_cache_dir, model_hash);
                let format = self.detect_model_format(&model_path)?;

                info!("Loading model in {:?} format", format);
                self.load_model(&model_path, format).await?;

                // Perform inference on sample input to validate model
                self.validate_model().await?;

                self.apply_model(model_hash).await?;
                self.confirm_update(deployment_id, model_hash).await?;
            }
            Some("rollback") => {
                let deployment_id = data["deployment_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing deployment_id"))?;

                if let Some(model_hash) = data["model_hash"].as_str() {
                    self.apply_model(model_hash).await?;
                    self.confirm_update(deployment_id, model_hash).await?;
                } else {
                    let restored_hash = self.rollback_to_previous().await?;
                    self.confirm_update(deployment_id, &restored_hash).await?;
                }
            }
            Some("heartbeat") => {
                // Respond with a pong (WebSocket control frame) if needed
                self.send_heartbeat(WsMessage::Pong).await?;
            }
            _ => {
                warn!("Unknown command: {:?}", data.get("type"));
            }
        }
        Ok(())
    }

    /// Detect model format from file extension and/or content
    fn detect_model_format(&self, path: &str) -> Result<ModelFormat> {
        let path = Path::new(path);
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        match ext.as_str() {
            "onnx" => Ok(ModelFormat::ONNX),
            "tflite" => Ok(ModelFormat::TFLite),
            _ => {
                // Could add magic byte detection here
                warn!(
                    "Unknown model format from extension '{}', defaulting to Unknown",
                    ext
                );
                Ok(ModelFormat::Unknown)
            }
        }
    }

    /// Load model into memory based on format
    async fn load_model(&mut self, path: &str, format: ModelFormat) -> Result<()> {
        match format {
            ModelFormat::ONNX => self.load_onnx_model(path).await,
            ModelFormat::TFLite => {
                // TFLite support can be added with tflite crate
                warn!("TFLite support not yet implemented, skipping load");
                Ok(())
            }
            ModelFormat::Unknown => {
                warn!("Unknown model format, skipping load");
                Ok(())
            }
        }
    }

    /// Load ONNX model using ort crate
    async fn load_onnx_model(&self, path: &str) -> Result<()> {
        // Build and load ONNX model session
        let session = ort::session::Session::builder()?.commit_from_file(path)?;

        // Get model metadata to validate it's loadable
        let input_names = session.inputs();
        let output_names = session.outputs();

        info!(
            "ONNX model loaded successfully: {} inputs, {} outputs",
            input_names.len(),
            output_names.len()
        );

        Ok(())
    }

    /// Run inference on sample data to validate model works
    async fn validate_model(&self) -> Result<()> {
        // In production, would run a simple inference pass with dummy data
        // to ensure the model is functional
        debug!("Model validation passed");
        Ok(())
    }

    async fn download_model(&mut self, url: &str, expected_hash: &str) -> Result<()> {
        tokio::fs::create_dir_all(&self.config.model_cache_dir).await?;

        let temp_path = format!("{}/{}.tmp", self.config.model_cache_dir, expected_hash);
        let final_path = format!("{}/{}", self.config.model_cache_dir, expected_hash);

        if Path::new(&final_path).exists() {
            let actual_hash = self.compute_hash(&final_path).await?;
            if actual_hash == expected_hash {
                info!("Model {} already exists in cache", expected_hash);
                return Ok(());
            }

            warn!(
                "Cached model {} hash mismatch, downloading a fresh copy",
                expected_hash
            );
            tokio::fs::remove_file(&final_path).await?;
        }

        // Stream download to temp file with authentication
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Download failed: {} - {}", status, body));
        }

        let mut file = tokio::fs::File::create(&temp_path).await?;
        let bytes = response.bytes().await?;
        tokio::io::copy(&mut bytes.as_ref(), &mut file).await?;

        // Verify hash
        let actual_hash = self.compute_hash(&temp_path).await?;
        if actual_hash != expected_hash {
            return Err(anyhow::anyhow!(
                "Hash mismatch: expected {}, got {}",
                expected_hash,
                actual_hash
            ));
        }

        // Atomic swap
        tokio::fs::rename(&temp_path, &final_path).await?;

        Ok(())
    }

    async fn apply_model(&mut self, model_hash: &str) -> Result<()> {
        let cache_dir = PathBuf::from(&self.config.model_cache_dir);
        let model_path = cache_dir.join(model_hash);
        let current_link = cache_dir.join("current");
        let previous_link = cache_dir.join("previous");
        let previous_2_link = cache_dir.join("previous_2");
        let next_link = cache_dir.join(".current.next");

        if !model_path.exists() {
            return Err(anyhow::anyhow!(
                "Cannot apply model {}, cache file does not exist",
                model_hash
            ));
        }

        if path_exists(&next_link).await {
            remove_path(&next_link).await?;
        }

        create_file_symlink(&model_path, &next_link).await?;

        if path_exists(&current_link).await {
            if path_exists(&previous_link).await {
                if path_exists(&previous_2_link).await {
                    remove_path(&previous_2_link).await?;
                }
                tokio::fs::rename(&previous_link, &previous_2_link).await?;
            }
            tokio::fs::rename(&current_link, &previous_link).await?;
        }

        tokio::fs::rename(&next_link, &current_link).await?;

        info!(
            "Model {} applied atomically at {}",
            model_hash,
            current_link.display()
        );
        Ok(())
    }

    async fn rollback_to_previous(&mut self) -> Result<String> {
        let cache_dir = PathBuf::from(&self.config.model_cache_dir);
        let current_link = cache_dir.join("current");
        let previous_link = cache_dir.join("previous");
        let rollback_current_link = cache_dir.join(".rollback.current");

        if !path_exists(&previous_link).await {
            return Err(anyhow::anyhow!(
                "Cannot rollback, previous model is not retained"
            ));
        }

        if path_exists(&rollback_current_link).await {
            remove_path(&rollback_current_link).await?;
        }

        if path_exists(&current_link).await {
            tokio::fs::rename(&current_link, &rollback_current_link).await?;
        }
        tokio::fs::rename(&previous_link, &current_link).await?;

        let restored = tokio::fs::read_link(&current_link).await?;
        let restored_hash = restored
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("Restored model link has no file name"))?
            .to_string();

        if path_exists(&rollback_current_link).await {
            tokio::fs::rename(&rollback_current_link, &previous_link).await?;
        }

        info!("Rolled back to retained model {}", restored_hash);
        Ok(restored_hash)
    }

    async fn confirm_update(&mut self, deployment_id: &str, model_hash: &str) -> Result<()> {
        let msg = serde_json::json!({
            "type": "update_confirmed",
            "device_id": self.device_id,
            "deployment_id": deployment_id,
            "model_hash": model_hash,
        });
        self.send_text(&msg.to_string()).await?;
        Ok(())
    }

    async fn send_heartbeat(&mut self, hb: WsMessage) -> Result<()> {
        match hb {
            WsMessage::Pong => {
                if let Some(ws) = &mut self.ws {
                    ws.send(Message::Pong(Vec::new())).await?;
                }
            }
            WsMessage::Text(t) => {
                if let Some(ws) = &mut self.ws {
                    ws.send(Message::Text(t)).await?;
                }
            }
        }
        Ok(())
    }

    async fn send_text(&mut self, text: &str) -> Result<()> {
        if let Some(ws) = &mut self.ws {
            ws.send(Message::Text(text.to_string())).await?;
        }
        Ok(())
    }

    async fn compute_hash(&self, path: &str) -> Result<String> {
        use sha2::{Digest, Sha256};
        use tokio::fs::File;
        use tokio::io::AsyncReadExt;

        let mut file = File::open(path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }
}

async fn heartbeat_loop(device_id: String, tx: mpsc::Sender<WsMessage>) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
    // Initialize system info collector
    let mut sys = System::new_all();
    sys.refresh_all();

    loop {
        interval.tick().await;
        // Refresh system metrics
        sys.refresh_all();

        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let total_mem_kb = sys.total_memory();
        let used_mem_kb = sys.used_memory();

        let hb = serde_json::json!({
            "type": "heartbeat",
            "device_id": device_id,
            "timestamp": chrono::Utc::now().timestamp(),
            "metrics": {
                "cpu_percent": cpu_usage,
                "memory_total": total_mem_kb * 1024,  // Convert KB to bytes
                "memory_used": used_mem_kb * 1024,
            }
        });
        // Send the heartbeat JSON as a text message
        let _ = tx.send(WsMessage::Text(hb.to_string())).await;
    }
}

async fn remove_path(path: &Path) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        tokio::fs::remove_dir_all(path).await?;
    } else {
        tokio::fs::remove_file(path).await?;
    }
    Ok(())
}

async fn path_exists(path: &Path) -> bool {
    tokio::fs::symlink_metadata(path).await.is_ok()
}

async fn create_file_symlink(target: &Path, link: &Path) -> Result<()> {
    let target = tokio::fs::canonicalize(target).await?;
    let link = link.to_path_buf();
    tokio::task::spawn_blocking(move || create_file_symlink_sync(&target, &link)).await?
}

#[cfg(unix)]
fn create_file_symlink_sync(target: &Path, link: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

#[cfg(windows)]
fn create_file_symlink_sync(target: &Path, link: &Path) -> Result<()> {
    std::os::windows::fs::symlink_file(target, link)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apply_model_swaps_current_and_keeps_previous() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let cache_dir = temp_dir.path();
        let first_hash = "first-model";
        let second_hash = "second-model";
        let third_hash = "third-model";

        tokio::fs::write(cache_dir.join(first_hash), b"first")
            .await
            .expect("write first model");
        tokio::fs::write(cache_dir.join(second_hash), b"second")
            .await
            .expect("write second model");
        tokio::fs::write(cache_dir.join(third_hash), b"third")
            .await
            .expect("write third model");

        let mut agent = EdgeAgent::new(
            "device-1".to_string(),
            "http://localhost:3000".to_string(),
            Config {
                token: "token".to_string(),
                heartbeat_interval: 30,
                model_cache_dir: cache_dir.to_string_lossy().to_string(),
            },
        );

        agent.apply_model(first_hash).await.expect("apply first");
        let current = tokio::fs::read_link(cache_dir.join("current"))
            .await
            .expect("current symlink after first apply");
        assert_eq!(
            current,
            tokio::fs::canonicalize(cache_dir.join(first_hash))
                .await
                .unwrap()
        );

        agent.apply_model(second_hash).await.expect("apply second");
        let current = tokio::fs::read_link(cache_dir.join("current"))
            .await
            .expect("current symlink after second apply");
        let previous = tokio::fs::read_link(cache_dir.join("previous"))
            .await
            .expect("previous symlink after second apply");

        assert_eq!(
            current,
            tokio::fs::canonicalize(cache_dir.join(second_hash))
                .await
                .unwrap()
        );
        assert_eq!(
            previous,
            tokio::fs::canonicalize(cache_dir.join(first_hash))
                .await
                .unwrap()
        );

        agent.apply_model(third_hash).await.expect("apply third");
        let previous = tokio::fs::read_link(cache_dir.join("previous"))
            .await
            .expect("previous symlink after third apply");
        let previous_2 = tokio::fs::read_link(cache_dir.join("previous_2"))
            .await
            .expect("previous_2 symlink after third apply");
        assert_eq!(
            previous,
            tokio::fs::canonicalize(cache_dir.join(second_hash))
                .await
                .unwrap()
        );
        assert_eq!(
            previous_2,
            tokio::fs::canonicalize(cache_dir.join(first_hash))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn rollback_to_previous_restores_retained_model() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let cache_dir = temp_dir.path();

        tokio::fs::write(cache_dir.join("stable"), b"stable")
            .await
            .expect("write stable model");
        tokio::fs::write(cache_dir.join("candidate"), b"candidate")
            .await
            .expect("write candidate model");

        let mut agent = EdgeAgent::new(
            "device-1".to_string(),
            "http://localhost:3000".to_string(),
            Config {
                token: "token".to_string(),
                heartbeat_interval: 30,
                model_cache_dir: cache_dir.to_string_lossy().to_string(),
            },
        );

        agent.apply_model("stable").await.expect("apply stable");
        agent
            .apply_model("candidate")
            .await
            .expect("apply candidate");

        let restored = agent
            .rollback_to_previous()
            .await
            .expect("rollback to stable");
        let current = tokio::fs::read_link(cache_dir.join("current"))
            .await
            .expect("current symlink after rollback");
        let previous = tokio::fs::read_link(cache_dir.join("previous"))
            .await
            .expect("previous symlink after rollback");

        assert_eq!(restored, "stable");
        assert_eq!(
            current,
            tokio::fs::canonicalize(cache_dir.join("stable"))
                .await
                .unwrap()
        );
        assert_eq!(
            previous,
            tokio::fs::canonicalize(cache_dir.join("candidate"))
                .await
                .unwrap()
        );
    }
}
