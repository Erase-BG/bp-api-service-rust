use std::collections::HashMap;
use std::sync::Arc;

use racoon::core::websocket::WebSocket;

use tokio::sync::Mutex;
use uuid::Uuid;

pub struct WsClients {
    inner: Arc<Mutex<HashMap<String, Vec<WebSocket>>>>,
}

impl WsClients {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add(&self, task_group: &Uuid, websocket: WebSocket) {
        let task_group = task_group.to_string();

        let mut inner_lock = self.inner.lock().await;
        if let Some(websockets) = inner_lock.get_mut(&task_group) {
            websockets.push(websocket);
        } else {
            let websockets = vec![websocket];
            inner_lock.insert(task_group, websockets);
        }
    }

    pub async fn get_all(&self, task_group: &Uuid) -> Vec<WebSocket> {
        let task_group = task_group.to_string();

        let inner_lock = self.inner.lock().await;
        if let Some(websocket) = inner_lock.get(&task_group) {
            return websocket.to_owned();
        }

        vec![]
    }

    pub async fn remove(&self, task_group: &Uuid, websocket: WebSocket) {
        let task_group = task_group.to_string();

        let mut inner_lock = self.inner.lock().await;

        if let Some(websockets) = inner_lock.get_mut(&task_group) {
            // Multiple unique websockets are allowed to connect to the same task group.
            // Each websocket connection has unique uid string.
            // If the websocket is cloned, the cloned websocket instance will also have the same
            // unique uid.

            for i in (0..websockets.len()).rev() {
                let current_websocket = &websockets[i];
                if websocket.uid == current_websocket.uid {
                    websockets.remove(i);
                }
            }

            // If there are no websocket connections stored in this task group,
            // removes the task group saved bucket from HashMap.
            if websockets.len() == 0 {
                inner_lock.remove(&task_group);
            }
        }
    }
}
