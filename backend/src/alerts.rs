use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub severity: AlertSeverity,
    pub title: String,
    pub description: String,
    pub source: AlertSource,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSource {
    Deployment,
    Device,
    System,
    Security,
}

/// Alert channel trait - different providers implement this
#[async_trait::async_trait]
pub trait AlertChannel: Send + Sync {
    async fn send(&self, alert: &Alert) -> Result<(), AlertError>;
    fn channel_type(&self) -> &str;
}

#[derive(Debug, thiserror::Error)]
pub enum AlertError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Network error: {0}")]
    Network(String),
    #[error("Provider error: {0}")]
    Provider(String),
    #[error("Rate limit exceeded")]
    RateLimit,
}

pub type AlertResult<T> = Result<T, AlertError>;

/// Email alert channel using SendGrid
pub struct SendGridChannel {
    api_key: String,
    from_email: String,
    to_emails: Vec<String>,
    client: reqwest::Client,
}

impl SendGridChannel {
    pub fn new(
        api_key: String,
        from_email: String,
        to_emails: Vec<String>,
    ) -> Result<Self, AlertError> {
        if api_key.is_empty() {
            return Err(AlertError::Config(
                "SendGrid API key is required".to_string(),
            ));
        }
        if from_email.is_empty() {
            return Err(AlertError::Config("From email is required".to_string()));
        }
        if to_emails.is_empty() {
            return Err(AlertError::Config(
                "At least one recipient email is required".to_string(),
            ));
        }

        Ok(Self {
            api_key,
            from_email,
            to_emails,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait::async_trait]
impl AlertChannel for SendGridChannel {
    async fn send(&self, alert: &Alert) -> AlertResult<()> {
        let subject = match alert.severity {
            AlertSeverity::Info => format!("[INFO] {}", alert.title),
            AlertSeverity::Warning => format!("[WARNING] {}", alert.title),
            AlertSeverity::Critical => format!("[CRITICAL] {}", alert.title),
        };

        let body = format!(
            "Source: {:?}\nDescription: {}\nTime: {}\nMetadata: {:?}",
            alert.source,
            alert.description,
            alert.created_at,
            alert.metadata.as_ref().unwrap_or(&serde_json::json!({}))
        );

        let payload = serde_json::json!({
            "personalizations": self.to_emails.iter().map(|to| {
                serde_json::json!({
                    "to": [{"email": to}],
                    "subject": subject.clone()
                })
            }).collect::<Vec<_>>(),
            "from": {"email": &self.from_email},
            "subject": subject,
            "content": [{
                "type": "text/plain",
                "value": body
            }]
        });

        let response = self
            .client
            .post("https://api.sendgrid.com/v3/mail/send")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AlertError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AlertError::Provider(format!(
                "SendGrid API error {}: {}",
                status, text
            )));
        }

        info!(
            "Email alert sent via SendGrid to {} recipients",
            self.to_emails.len()
        );
        Ok(())
    }

    fn channel_type(&self) -> &str {
        "sendgrid_email"
    }
}

/// Slack webhook channel
pub struct SlackChannel {
    webhook_url: String,
    channel: Option<String>,
    client: reqwest::Client,
}

impl SlackChannel {
    pub fn new(webhook_url: String, channel: Option<String>) -> Result<Self, AlertError> {
        if webhook_url.is_empty() {
            return Err(AlertError::Config(
                "Slack webhook URL is required".to_string(),
            ));
        }

        Ok(Self {
            webhook_url,
            channel,
            client: reqwest::Client::new(),
        })
    }

    fn format_blocks(&self, alert: &Alert) -> serde_json::Value {
        let emoji = match alert.severity {
            AlertSeverity::Info => "🔵",
            AlertSeverity::Warning => "🟡",
            AlertSeverity::Critical => "🔴",
        };

        let _color = match alert.severity {
            AlertSeverity::Info => "#36a64f",
            AlertSeverity::Warning => "#ffcc00",
            AlertSeverity::Critical => "#ff0000",
        };

        serde_json::json!([
            {
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": format!("{} {}", emoji, alert.title)
                }
            },
            {
                "type": "section",
                "fields": [
                    {"type": "mrkdwn", "text": format!("*Severity:*\n{:?}", alert.severity)},
                    {"type": "mrkdwn", "text": format!("*Source:*\n{:?}", alert.source)},
                    {"type": "mrkdwn", "text": format!("*Time:*\n{}", alert.created_at.format("%Y-%m-%d %H:%M:%S UTC"))},
                ]
            },
            {
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": alert.description
                }
            },
            {
                "type": "context",
                "elements": [
                    {
                        "type": "mrkdwn",
                        "text": format!("OTAedge Alert | ID: {}", alert.id)
                    }
                ]
            }
        ])
    }
}

#[async_trait::async_trait]
impl AlertChannel for SlackChannel {
    async fn send(&self, alert: &Alert) -> AlertResult<()> {
        let blocks = self.format_blocks(alert);

        let mut payload = serde_json::json!({
            "blocks": blocks
        });

        if let Some(ref channel) = self.channel {
            payload["channel"] = serde_json::json!(channel);
        }

        let response = self
            .client
            .post(&self.webhook_url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(|e| AlertError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AlertError::Provider(format!(
                "Slack webhook error {}: {}",
                status, text
            )));
        }

        info!(
            "Slack alert sent to channel: {}",
            self.channel.as_deref().unwrap_or("(default)")
        );
        Ok(())
    }

    fn channel_type(&self) -> &str {
        "slack_webhook"
    }
}

/// Microsoft Teams webhook channel
pub struct TeamsChannel {
    webhook_url: String,
    client: reqwest::Client,
}

impl TeamsChannel {
    pub fn new(webhook_url: String) -> Result<Self, AlertError> {
        if webhook_url.is_empty() {
            return Err(AlertError::Config(
                "Teams webhook URL is required".to_string(),
            ));
        }

        Ok(Self {
            webhook_url,
            client: reqwest::Client::new(),
        })
    }

    fn format_card(&self, alert: &Alert) -> serde_json::Value {
        let color = match alert.severity {
            AlertSeverity::Info => "0078D4",     // Blue
            AlertSeverity::Warning => "FFA500",  // Orange
            AlertSeverity::Critical => "FF0000", // Red
        };

        serde_json::json!({
            "@type": "MessageCard",
            "@context": "http://schema.org/extensions",
            "themeColor": color,
            "summary": format!("OTAedge Alert - {:?}", alert.severity),
            "title": alert.title,
            "sections": [
                {
                    "activityTitle": format!("Source: {:?}", alert.source),
                    "text": alert.description,
                    "facts": [
                        {"name": "Severity", "value": format!("{:?}", alert.severity)},
                        {"name": "Time", "value": alert.created_at.format("%Y-%m-%d %H:%M:%S UTC").to_string()},
                    ]
                }
            ],
            "potentialAction": [
                {
                    "@type": "OpenUri",
                    "name": "View Dashboard",
                    "targets": [
                        {"os": "default", "uri": "http://localhost:3000"}
                    ]
                }
            ]
        })
    }
}

#[async_trait::async_trait]
impl AlertChannel for TeamsChannel {
    async fn send(&self, alert: &Alert) -> AlertResult<()> {
        let card = self.format_card(alert);

        let response = self
            .client
            .post(&self.webhook_url)
            .header("Content-Type", "application/json")
            .json(&card)
            .send()
            .await
            .map_err(|e| AlertError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(AlertError::Provider(format!(
                "Teams webhook error {}: {}",
                status, text
            )));
        }

        info!("Teams alert sent successfully");
        Ok(())
    }

    fn channel_type(&self) -> &str {
        "teams_webhook"
    }
}

/// SMS channel using Twilio
pub struct TwilioChannel {
    account_sid: String,
    auth_token: String,
    from_number: String,
    to_numbers: Vec<String>,
    client: reqwest::Client,
}

impl TwilioChannel {
    pub fn new(
        account_sid: String,
        auth_token: String,
        from_number: String,
        to_numbers: Vec<String>,
    ) -> Result<Self, AlertError> {
        if account_sid.is_empty() {
            return Err(AlertError::Config(
                "Twilio Account SID is required".to_string(),
            ));
        }
        if auth_token.is_empty() {
            return Err(AlertError::Config(
                "Twilio Auth Token is required".to_string(),
            ));
        }
        if from_number.is_empty() {
            return Err(AlertError::Config(
                "From phone number is required".to_string(),
            ));
        }
        if to_numbers.is_empty() {
            return Err(AlertError::Config(
                "At least one recipient phone number is required".to_string(),
            ));
        }

        Ok(Self {
            account_sid,
            auth_token,
            from_number,
            to_numbers,
            client: reqwest::Client::new(),
        })
    }
}

#[async_trait::async_trait]
impl AlertChannel for TwilioChannel {
    async fn send(&self, alert: &Alert) -> AlertResult<()> {
        // Truncate message to fit SMS limits
        let max_len = 160;
        let mut message = format!(
            "[{}] {}: {}",
            match alert.severity {
                AlertSeverity::Info => "INFO",
                AlertSeverity::Warning => "WARN",
                AlertSeverity::Critical => "CRIT",
            },
            alert.title,
            alert.description
        );

        if message.len() > max_len {
            message.truncate(max_len - 3);
            message.push_str("...");
        }

        for to_number in &self.to_numbers {
            let url = format!(
                "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
                self.account_sid
            );

            let response = self
                .client
                .post(&url)
                .basic_auth(&self.account_sid, Some(&self.auth_token))
                .form(&[
                    ("From", &self.from_number),
                    ("To", to_number),
                    ("Body", &message),
                ])
                .send()
                .await
                .map_err(|e| AlertError::Network(e.to_string()))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                error!("Twilio API error {}: {}", status, text);
                // Continue trying other numbers
            } else {
                info!("SMS alert sent to {}", to_number);
            }
        }

        Ok(())
    }

    fn channel_type(&self) -> &str {
        "twilio_sms"
    }
}

/// Alert manager - coordinates multiple channels
pub struct AlertManager {
    channels: Vec<Box<dyn AlertChannel>>,
    rate_limiters: HashMap<String, (chrono::DateTime<chrono::Utc>, u32)>,
    max_alerts_per_minute: u32,
}

impl AlertManager {
    pub fn new(max_alerts_per_minute: u32) -> Self {
        Self {
            channels: Vec::new(),
            rate_limiters: HashMap::new(),
            max_alerts_per_minute,
        }
    }

    pub fn add_channel(&mut self, channel: Box<dyn AlertChannel>) {
        self.channels.push(channel);
    }

    /// Check if we should rate limit this alert type
    fn should_rate_limit(&mut self, alert_key: &str) -> bool {
        let now = chrono::Utc::now();
        let window = chrono::Duration::minutes(1);

        if let Some((last_sent, count)) = self.rate_limiters.get_mut(alert_key) {
            if now - *last_sent < window {
                if *count >= self.max_alerts_per_minute {
                    return true;
                }
                *count += 1;
            } else {
                *last_sent = now;
                *count = 1;
            }
        } else {
            self.rate_limiters.insert(alert_key.to_string(), (now, 1));
        }

        false
    }

    pub async fn send_alert(&mut self, alert: Alert) -> AlertResult<()> {
        // Rate limit per alert type (title + severity)
        let rate_limit_key = format!("{}:{:?}", alert.title, alert.severity);

        if self.should_rate_limit(&rate_limit_key) {
            warn!("Alert rate limited: {}", rate_limit_key);
            return Ok(()); // Silently drop
        }

        let mut failed_channels = Vec::new();

        for channel in &self.channels {
            match channel.send(&alert).await {
                Ok(_) => {
                    info!("Alert sent via {}: {}", channel.channel_type(), alert.title);
                }
                Err(e) => {
                    error!("Failed to send alert via {}: {}", channel.channel_type(), e);
                    failed_channels.push(channel.channel_type().to_string());
                }
            }
        }

        if !failed_channels.is_empty() {
            warn!("Some alert channels failed: {:?}", failed_channels);
        }

        Ok(())
    }
}

/// Helper functions for creating common alerts
pub fn create_deployment_failed_alert(deployment_id: &str, device_id: &str, error: &str) -> Alert {
    Alert {
        id: uuid::Uuid::new_v4().to_string(),
        severity: AlertSeverity::Critical,
        title: format!("Deployment Failed to Device"),
        description: format!(
            "Deployment {} failed on device {}: {}",
            deployment_id, device_id, error
        ),
        source: AlertSource::Deployment,
        metadata: Some(serde_json::json!({
            "deployment_id": deployment_id,
            "device_id": device_id,
            "error": error,
        })),
        created_at: chrono::Utc::now(),
    }
}

pub fn create_device_offline_alert(device_id: &str, last_seen: &str) -> Alert {
    Alert {
        id: uuid::Uuid::new_v4().to_string(),
        severity: AlertSeverity::Warning,
        title: format!("Device Offline"),
        description: format!("Device {} has not been seen since {}", device_id, last_seen),
        source: AlertSource::Device,
        metadata: Some(serde_json::json!({
            "device_id": device_id,
            "last_seen": last_seen,
        })),
        created_at: chrono::Utc::now(),
    }
}

pub fn create_system_error_alert(component: &str, error: &str) -> Alert {
    Alert {
        id: uuid::Uuid::new_v4().to_string(),
        severity: AlertSeverity::Warning,
        title: format!("System Error in {}", component),
        description: format!("{}", error),
        source: AlertSource::System,
        metadata: Some(serde_json::json!({
            "component": component,
        })),
        created_at: chrono::Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_alert_severity_serialization() {
        use super::AlertSeverity;
        let info = AlertSeverity::Info;
        assert_eq!(serde_json::to_string(&info).unwrap(), "\"info\"");

        let warning = AlertSeverity::Warning;
        assert_eq!(serde_json::to_string(&warning).unwrap(), "\"warning\"");

        let critical = AlertSeverity::Critical;
        assert_eq!(serde_json::to_string(&critical).unwrap(), "\"critical\"");
    }

    #[test]
    fn test_alert_source_serialization() {
        use super::AlertSource;
        let deployment = AlertSource::Deployment;
        assert_eq!(
            serde_json::to_string(&deployment).unwrap(),
            "\"deployment\""
        );

        let device = AlertSource::Device;
        assert_eq!(serde_json::to_string(&device).unwrap(), "\"device\"");
    }
}
