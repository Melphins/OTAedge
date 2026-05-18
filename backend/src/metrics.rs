use prometheus::{
    register_int_counter_vec, register_int_gauge, register_int_gauge_vec, IntCounterVec, IntGauge,
    IntGaugeVec,
};

pub struct Metrics {
    pub http_requests_total: IntCounterVec,
    pub ws_connections_active: IntGauge,
    pub deployments_total: IntCounterVec,
    pub auth_attempts_total: IntCounterVec,
    pub rate_limited_total: IntCounterVec,
    pub device_cpu_usage: IntGaugeVec,
    pub device_memory_used: IntGaugeVec,
    pub device_memory_total: IntGaugeVec,
    pub device_disk_used: IntGaugeVec,
    pub device_disk_total: IntGaugeVec,
    pub device_status_offline: IntGauge,
}

impl Metrics {
    pub fn new() -> Self {
        let http_requests_total = register_int_counter_vec!(
            "http_requests_total",
            "Total HTTP requests",
            &["method", "path", "status"]
        )
        .expect("Failed to register http_requests_total metric");

        let ws_connections_active = register_int_gauge!(
            "ws_connections_active",
            "Number of active WebSocket connections"
        )
        .expect("Failed to register ws_connections_active metric");

        let deployments_total = register_int_counter_vec!(
            "deployments_total",
            "Total deployments created",
            &["status"]
        )
        .expect("Failed to register deployments_total metric");

        let auth_attempts_total = register_int_counter_vec!(
            "auth_attempts_total",
            "Total authentication attempts",
            &["outcome", "type"]
        )
        .expect("Failed to register auth_attempts_total metric");

        let rate_limited_total = register_int_counter_vec!(
            "rate_limited_total",
            "Total number of requests rate limited",
            &["limiter_type"]
        )
        .expect("Failed to register rate_limited_total metric");

        let device_cpu_usage = register_int_gauge_vec!(
            "device_cpu_usage_percent",
            "Device CPU usage percentage",
            &["device_id"]
        )
        .expect("Failed to register device_cpu_usage metric");

        let device_memory_used = register_int_gauge_vec!(
            "device_memory_used_bytes",
            "Device memory used in bytes",
            &["device_id"]
        )
        .expect("Failed to register device_memory_used metric");

        let device_memory_total = register_int_gauge_vec!(
            "device_memory_total_bytes",
            "Device total memory in bytes",
            &["device_id"]
        )
        .expect("Failed to register device_memory_total metric");

        let device_disk_used = register_int_gauge_vec!(
            "device_disk_used_bytes",
            "Device disk used in bytes",
            &["device_id"]
        )
        .expect("Failed to register device_disk_used metric");

        let device_disk_total = register_int_gauge_vec!(
            "device_disk_total_bytes",
            "Device total disk in bytes",
            &["device_id"]
        )
        .expect("Failed to register device_disk_total metric");

        let device_status_offline = register_int_gauge!(
            "device_status_offline",
            "Number of devices that appear offline"
        )
        .expect("Failed to register device_status_offline metric");

        Self {
            http_requests_total,
            ws_connections_active,
            deployments_total,
            auth_attempts_total,
            rate_limited_total,
            device_cpu_usage,
            device_memory_used,
            device_memory_total,
            device_disk_used,
            device_disk_total,
            device_status_offline,
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
