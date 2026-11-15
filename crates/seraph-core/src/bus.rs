use crate::event::PlayerEvent;
use crossbeam_channel::{unbounded, Receiver, Sender};
use parking_lot::RwLock;
use std::sync::Arc;

/// SYS-02：单个订阅者允许积压的事件数。超过后只丢弃**进度**事件（每秒约 4 条、
/// 新值覆盖旧值，丢了不影响正确性）；状态类事件（切歌、停止、错误等）照常入队。
/// 订阅线程卡住（例如 SMTC 线程卡在 WinRT 调用里）时，通道不再无限增长。
pub const MAX_PENDING_EVENTS: usize = 256;

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
        let droppable = matches!(event, PlayerEvent::Progress { .. });
        // 平时没有订阅者断开：只取读锁，发布之间互不阻塞。
        let mut dead: Vec<Sender<PlayerEvent>> = Vec::new();
        for tx in self.inner.subscribers.read().iter() {
            if droppable && tx.len() >= MAX_PENDING_EVENTS {
                continue;
            }
            if tx.send(event.clone()).is_err() {
                dead.push(tx.clone());
            }
        }
        if !dead.is_empty() {
            self.inner
                .subscribers
                .write()
                .retain(|tx| !dead.iter().any(|gone| gone.same_channel(tx)));
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(seconds: f64) -> PlayerEvent {
        PlayerEvent::Progress {
            track_id: Default::default(),
            seconds,
            total: 100.0,
            output_latency: 0.0,
        }
    }

    #[test]
    fn stalled_subscriber_backlog_is_bounded_but_state_events_still_arrive() {
        let bus = EventBus::new();
        let stalled = bus.subscribe();
        for index in 0..(MAX_PENDING_EVENTS * 4) {
            bus.publish(progress(index as f64));
        }
        assert_eq!(stalled.len(), MAX_PENDING_EVENTS);

        // 积压已满时，状态类事件不能被丢
        bus.publish(PlayerEvent::PlaybackStopped);
        assert_eq!(stalled.len(), MAX_PENDING_EVENTS + 1);
        let last = stalled.try_iter().last().expect("events");
        assert!(matches!(last, PlayerEvent::PlaybackStopped));

        // 订阅者恢复消费后，进度事件重新送达
        bus.publish(progress(1.0));
        assert_eq!(stalled.len(), 1);
    }

    #[test]
    fn dropped_subscribers_are_pruned_without_disturbing_live_ones() {
        let bus = EventBus::new();
        let live = bus.subscribe();
        drop(bus.subscribe());
        bus.publish(PlayerEvent::PlaybackPaused);
        assert_eq!(bus.inner.subscribers.read().len(), 1);
        // 清理断开的订阅者不会给在线订阅者多发事件
        assert_eq!(live.len(), 1);
        bus.publish(PlayerEvent::PlaybackResumed);
        assert_eq!(live.len(), 2);
    }
}
