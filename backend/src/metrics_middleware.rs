use axum::{body::Body, extract::State, http::Request, middleware::Next, response::Response};
use std::sync::Arc;
use std::time::Instant;

pub async fn metrics_middleware(
    State(state): State<Arc<crate::AppState>>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    let _latency = start.elapsed().as_secs_f64();

    // Increment HTTP requests counter
    state
        .metrics
        .http_requests_total
        .with_label_values(&[&method, &path, &status])
        .inc();

    // You could also record latency histogram here if desired

    response
}
