//! Nonblocking, bounded handoff of authenticated client input to the host.
//! Overflow advances an epoch so the host can neutralize and discard stale data.
use tokio::sync::{mpsc, watch};

#[derive(Debug)]
pub struct InputPacket {
    pub epoch: u64,
    pub payload: Vec<u8>,
}

/// One receiver per GameStream session. Overflow notifications have priority
/// over queued packets at the consumer; an old epoch must never be replayed.
pub struct InputReceiver {
    pub events: mpsc::Receiver<InputPacket>,
    pub overflow: watch::Receiver<u64>,
}

#[derive(Clone)]
pub(crate) struct InputForwarder {
    tx: mpsc::Sender<InputPacket>,
    overflow: watch::Sender<u64>,
}

impl InputForwarder {
    pub(crate) fn channel(capacity: usize) -> (Self, InputReceiver) {
        let (tx, events) = mpsc::channel(capacity);
        let (overflow, notices) = watch::channel(0);
        (Self { tx, overflow }, InputReceiver { events, overflow: notices })
    }

    /// Invalidate every previously queued state on control-peer loss.
    pub(crate) fn reset(&self) {
        self.overflow.send_modify(|e| *e = e.saturating_add(1));
    }

    pub(crate) fn forward(&self, payload: &[u8]) {
        let epoch = *self.overflow.borrow();
        match self.tx.try_send(InputPacket { epoch, payload: payload.to_vec() }) {
            Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => {},
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.reset();
                tracing::warn!("input handoff overflow; neutralizing and discarding old epoch");
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn overflow_advances_epoch_without_blocking_control() {
        let (sender, mut receiver) = InputForwarder::channel(1);
        sender.forward(&[1]);
        sender.forward(&[0]); // release cannot be silently dropped
        receiver.overflow.changed().await.unwrap();
        assert_eq!(*receiver.overflow.borrow_and_update(), 1);
        assert_eq!(receiver.events.recv().await.unwrap().epoch, 0);
        sender.forward(&[0]);
        let next = receiver.events.recv().await.unwrap();
        assert_eq!(next.epoch, 1);
        assert_eq!(next.payload, vec![0]);
        sender.reset();
        receiver.overflow.changed().await.unwrap();
        assert_eq!(*receiver.overflow.borrow_and_update(), 2);
    }
}
