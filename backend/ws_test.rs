use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use futures_util::{SinkExt, StreamExt};
use url::Url;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let token = "e0392a23-25f4-47b3-86c2-2a0df1070e6e";
    let url = format!("ws://localhost:3000/ws?token={}", token);
    let (mut ws_stream, _) = connect_async(Url::parse(&url).unwrap()).await.expect("Failed to connect");

    println!("Connected to WebSocket");

    // Wait for message from server
    if let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                println!("Received: {}", text);
                // Send update_confirmed
                let deployment_id = "1137ae55-7444-49a4-be3d-840d396aad17";
                let confirm = serde_json::json!({
                    "type": "update_confirmed",
                    "deployment_id": deployment_id
                }).to_string();
                ws_stream.send(Message::Text(confirm)).await.expect("Failed to send");
                println!("Sent update_confirmed");
            }
            _ => println!("Received non-text message"),
        }
    }

    // Give time for server to process
    tokio::time::sleep(Duration::from_secs(2)).await;
    ws_stream.close().await.ok();
    println!("Disconnected");
}