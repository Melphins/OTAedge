#[derive(Clone, Debug)]
pub enum WsMessage {
    Pong,
    Text(String),
}
