use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Structured logging context for requests
#[derive(Clone, Debug)]
pub struct LogContext {
    pub request_id: Option<String>,
    pub user_id: Option<String>,
    pub device_id: Option<String>,
}

impl LogContext {
    pub fn new() -> Self {
        Self {
            request_id: None,
            user_id: None,
            device_id: None,
        }
    }

    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub fn with_user_id(mut self, user_id: Uuid) -> Self {
        self.user_id = Some(user_id.to_string());
        self
    }

    pub fn with_device_id(mut self, device_id: String) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Log an info message with context
    pub fn info(&self, message: &str) {
        let mut log_entry = self.base_fields();
        log_entry.push(("message", message.to_string()));
        info!(
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_else(|_| format!(
                "[{}] {}",
                self.short_id(),
                message
            ))
        );
    }

    /// Log a debug message with context
    pub fn debug(&self, message: &str) {
        let mut log_entry = self.base_fields();
        log_entry.push(("message", message.to_string()));
        debug!(
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_else(|_| format!(
                "[{}] {}",
                self.short_id(),
                message
            ))
        );
    }

    /// Log a warning with context
    pub fn warn(&self, message: &str) {
        let mut log_entry = self.base_fields();
        log_entry.push(("message", message.to_string()));
        warn!(
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_else(|_| format!(
                "[{}] {}",
                self.short_id(),
                message
            ))
        );
    }

    /// Log an error with context
    pub fn error(&self, message: &str) {
        let mut log_entry = self.base_fields();
        log_entry.push(("message", message.to_string()));
        error!(
            "{}",
            serde_json::to_string(&log_entry).unwrap_or_else(|_| format!(
                "[{}] {}",
                self.short_id(),
                message
            ))
        );
    }

    fn base_fields(&self) -> Vec<(&str, String)> {
        let mut fields = Vec::new();
        if let Some(ref req_id) = self.request_id {
            fields.push(("request_id", req_id.clone()));
        }
        if let Some(ref uid) = self.user_id {
            fields.push(("user_id", uid.clone()));
        }
        if let Some(ref did) = self.device_id {
            fields.push(("device_id", did.clone()));
        }
        fields
    }

    fn short_id(&self) -> String {
        if let Some(ref req_id) = self.request_id {
            if req_id.len() > 8 {
                return format!("...{}", &req_id[req_id.len() - 8..]);
            }
            req_id.clone()
        } else {
            "-".to_string()
        }
    }
}

#[macro_export]
macro_rules! log_info {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.info(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_debug {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.debug(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.warn(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($ctx:expr, $($arg:tt)*) => {
        $ctx.error(&format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_context() {
        let ctx = LogContext::new()
            .with_request_id("req-12345678".to_string())
            .with_user_id(Uuid::new_v4());

        assert!(ctx.request_id.is_some());
        assert!(ctx.user_id.is_some());
    }
}
