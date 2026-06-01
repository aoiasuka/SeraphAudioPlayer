use crate::event::PlayerEvent;
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::RwLock;
use std::sync::Arc;

/// 简易事件总线。
///
/// 使用 crossbeam-channel 作为底层，多消费者通过 [`EventBus::subscribe`]
/// 各自拿到一个独立的 receiver。
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

struct EventBusInner {
    subscribers: RwLock<Vec<Sender<PlayerEvent>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(EventBusInner {
                subscribers: RwLock::new(Vec::new()),
            }),
        }
    }

    pub fn subscribe(&self) -> Receiver<PlayerEvent> {
        let (tx, rx) = unbounded();
        self.inner.subscribers.write().push(tx);
        rx
    }

    pub fn publish(&self, event: PlayerEvent) {
        let mut guard = self.inner.subscribers.write();
        guard.retain(|tx| tx.send(event.clone()).is_ok());
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
