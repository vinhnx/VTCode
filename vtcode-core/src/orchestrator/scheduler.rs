use std::collections::VecDeque;

use tokio::sync::Mutex;

use super::ScheduledWork;

/// Simple queue-backed scheduler with FIFO ordering.
#[derive(Debug, Default)]
pub struct Scheduler {
    queue: Mutex<VecDeque<ScheduledWork>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    pub async fn enqueue(&self, work: ScheduledWork) {
        let mut queue = self.queue.lock().await;
        queue.push_back(work);
    }

    pub async fn next(&self) -> Option<ScheduledWork> {
        let mut queue = self.queue.lock().await;
        queue.pop_front()
    }

    pub async fn queue_depth(&self) -> usize {
        let queue = self.queue.lock().await;
        queue.len()
    }
}
