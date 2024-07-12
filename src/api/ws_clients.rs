use std::{collections::HashMap, sync::Arc};

use racoon::core::websocket::WebSocket;

use tokio::sync::Mutex;

pub struct WsClients {
    inner: Arc<Mutex<HashMap<String, WebSocket>>>,
}

impl WsClients {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add(&self, websocket: WebSocket) {
        let mut inner_lock = self.inner.lock().await;
        inner_lock.insert(websocket.uid.clone(), websocket);
    }

    pub async fn remove(&self, websocket: WebSocket) {
        let mut inner_lock = self.inner.lock().await;
        inner_lock.remove(&websocket.uid);
    }
}
