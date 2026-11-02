//! Coalescing live-event channel from the service to desktop clients.
use std::collections::VecDeque;

use vtcode_exec_events::VersionedThreadEvent;

/// Create the bounded live event channel that streams runtime events from the
/// agent runloop to the desktop service pump.
///
/// Unlike a plain bounded channel, a full queue never fails the producer:
/// `item.updated` snapshots replace the pending snapshot for the same item and
/// consecutive `plan.delta` events for one item merge into one entry, so a
/// slow consumer degrades the stream instead of failing a healthy turn.
/// Ordered events are only dropped when the queue holds no coalescable entry,
/// and every such drop is counted for consumer-side degradation reporting.
pub fn live_event_channel(capacity: usize) -> (LiveEventSender, LiveEventReceiver) {
    assert!(capacity > 0, "live event capacity must be positive");
    let inner = std::sync::Arc::new(LiveEventQueueInner {
        capacity,
        queue: std::sync::Mutex::new(VecDeque::new()),
        senders: std::sync::atomic::AtomicUsize::new(1),
        receiver_alive: std::sync::atomic::AtomicBool::new(true),
        dropped_ordered: std::sync::atomic::AtomicU64::new(0),
        dropped_stale: std::sync::atomic::AtomicU64::new(0),
        notify: tokio::sync::Notify::new(),
    });
    (LiveEventSender { inner: inner.clone() }, LiveEventReceiver { inner })
}

/// How a streaming event combines with the pending entry for the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CoalesceOp {
    /// The new event is a full snapshot that supersedes the pending one.
    Replace,
    /// The new event carries incremental text appended to the pending one.
    Merge,
}

/// One queue entry: a coalescable streaming event keyed by item, or an
/// ordered event that must keep its relative position.
struct LiveEventSlot {
    key: Option<String>,
    op: CoalesceOp,
    event: VersionedThreadEvent,
}

struct LiveEventQueueInner {
    capacity: usize,
    queue: std::sync::Mutex<VecDeque<LiveEventSlot>>,
    senders: std::sync::atomic::AtomicUsize,
    receiver_alive: std::sync::atomic::AtomicBool,
    dropped_ordered: std::sync::atomic::AtomicU64,
    dropped_stale: std::sync::atomic::AtomicU64,
    notify: tokio::sync::Notify,
}

impl LiveEventQueueInner {
    fn classify(event: &VersionedThreadEvent) -> Option<(String, CoalesceOp)> {
        match event.event() {
            vtcode_exec_events::ThreadEvent::ItemUpdated(update) => Some((update.item.id.clone(), CoalesceOp::Replace)),
            vtcode_exec_events::ThreadEvent::PlanDelta(delta) => Some((delta.item_id.clone(), CoalesceOp::Merge)),
            _ => None,
        }
    }

    /// Append an event, coalescing streaming updates and keeping the queue
    /// bounded. Returns `Err(())` only when the consumer half was dropped.
    fn publish(&self, event: VersionedThreadEvent) -> Result<(), ()> {
        if !self.receiver_alive.load(std::sync::atomic::Ordering::Acquire) {
            return Err(());
        }
        let mut queue = self.queue.lock().map_err(|_poisoned| ())?;
        let classified = Self::classify(&event);
        let slot_key = classified.as_ref().map(|(key, _)| key.clone());
        let slot_op = classified.as_ref().map_or(CoalesceOp::Replace, |(_, op)| *op);
        if let Some((key, op)) = classified {
            let pending_slot = queue
                .iter()
                .position(|slot| slot.key.as_deref() == Some(key.as_str()) && slot.op == op)
                .and_then(|index| queue.remove(index).map(|removed| (index, removed)));
            if let Some((index, pending)) = pending_slot {
                // Re-insert at the removed slot's position so the relative
                // order of every other queued event is preserved.
                let merged = match op {
                    CoalesceOp::Replace => event,
                    CoalesceOp::Merge => Self::merge_plan_deltas(pending.event, event),
                };
                queue.insert(index, LiveEventSlot { key: Some(key), op, event: merged });
                drop(queue);
                self.notify.notify_waiters();
                return Ok(());
            }
        }
        if queue.len() >= self.capacity {
            // Prefer displacing a stale streaming entry over dropping an
            // ordered event: the latest snapshot for an item always arrives
            // with its terminal `item.completed` event.
            let victim = queue.iter().position(|slot| slot.key.is_some());
            match victim {
                Some(index) => {
                    drop(queue.remove(index));
                    let _ = self.dropped_stale.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                None => {
                    drop(queue.pop_front());
                    let _ = self.dropped_ordered.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        queue.push_back(LiveEventSlot { key: slot_key, op: slot_op, event });
        drop(queue);
        self.notify.notify_waiters();
        Ok(())
    }

    /// Concatenate two `plan.delta` events for the same item, keeping the
    /// newest metadata and the freshest non-empty recovery snapshot.
    fn merge_plan_deltas(pending: VersionedThreadEvent, next: VersionedThreadEvent) -> VersionedThreadEvent {
        let (pending, next) = (pending.into_event(), next.into_event());
        match (pending, next) {
            (vtcode_exec_events::ThreadEvent::PlanDelta(older), vtcode_exec_events::ThreadEvent::PlanDelta(newer)) => {
                let mut merged = newer.clone();
                let mut text = older.delta;
                text.push_str(&newer.delta);
                merged.delta = text;
                if merged.recovery.is_none() {
                    merged.recovery = older.recovery;
                }
                VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::PlanDelta(merged))
            }
            (_, fallback) => VersionedThreadEvent::new(fallback),
        }
    }
}

/// Producer half of the live event channel.
pub struct LiveEventSender {
    inner: std::sync::Arc<LiveEventQueueInner>,
}

impl Clone for LiveEventSender {
    fn clone(&self) -> Self {
        let _ = self.inner.senders.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self { inner: std::sync::Arc::clone(&self.inner) }
    }
}

impl LiveEventSender {
    /// Publish one event. Coalesces streaming updates when the queue is
    /// saturated; fails only when the consumer half is gone.
    pub fn publish(&self, event: VersionedThreadEvent) -> Result<(), LiveEventSendError> {
        self.inner.publish(event).map_err(|()| LiveEventSendError(()))
    }
}

impl Drop for LiveEventSender {
    fn drop(&mut self) {
        let _ = self.inner.senders.fetch_sub(1, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }
}

/// Consumer half of the live event channel.
pub struct LiveEventReceiver {
    inner: std::sync::Arc<LiveEventQueueInner>,
}

impl LiveEventReceiver {
    /// Receive the next event, waiting while the queue is empty.
    ///
    /// Returns `None` once every sender was dropped and the queue drained.
    pub async fn recv(&self) -> Option<VersionedThreadEvent> {
        loop {
            {
                let mut queue = self.inner.queue.lock().ok()?;
                if let Some(slot) = queue.pop_front() {
                    return Some(slot.event);
                }
                if self.inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return None;
                }
            }
            let notified = self.inner.notify.notified();
            {
                let queue = self.inner.queue.lock().ok()?;
                if !queue.is_empty() {
                    continue;
                }
                if self.inner.senders.load(std::sync::atomic::Ordering::Acquire) == 0 {
                    return None;
                }
            }
            notified.await;
        }
    }

    /// Take and reset the count of ordered events dropped under saturation.
    pub fn take_dropped_ordered(&self) -> u64 {
        self.inner.dropped_ordered.swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// Take and reset the count of superseded streaming snapshots displaced
    /// under saturation. These are best-effort previews, not stream gaps.
    pub fn take_dropped_stale(&self) -> u64 {
        self.inner.dropped_stale.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for LiveEventReceiver {
    fn drop(&mut self) {
        self.inner.receiver_alive.store(false, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }
}

/// The consumer half of the live event channel is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveEventSendError(());

impl std::fmt::Display for LiveEventSendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("live event consumer disconnected")
    }
}

impl std::error::Error for LiveEventSendError {}
