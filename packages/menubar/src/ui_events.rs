//! Forward native tray/menu callbacks before waking the UI event loop.

use std::sync::mpsc::{self, Receiver};

pub(crate) fn channel<T: Send + 'static>(
    wake: impl Fn() + Send + Sync + 'static,
) -> (impl Fn(T) + Send + Sync + 'static, Receiver<T>) {
    let (sender, receiver) = mpsc::channel();
    let forward = move |event| {
        if sender.send(event).is_ok() {
            wake();
        }
    };
    (forward, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[test]
    fn native_callback_queues_event_before_waking_a_sleeping_ui() {
        let receiver_slot = Arc::new(Mutex::new(None::<Receiver<u32>>));
        let wake_receiver = Arc::clone(&receiver_slot);
        let (observed, wake) = mpsc::channel();
        let (forward, events) = channel(move || {
            let event = wake_receiver.lock().unwrap().as_ref().unwrap().try_recv();
            observed.send(event).unwrap();
        });
        *receiver_slot.lock().unwrap() = Some(events);
        let native = std::thread::spawn(move || forward(42));
        assert_eq!(wake.recv_timeout(Duration::from_millis(500)), Ok(Ok(42)));
        native.join().unwrap();
    }

    #[test]
    fn native_callback_does_not_wake_a_closed_event_loop() {
        let (sent, wakes) = mpsc::channel();
        let (forward, events) = channel(move || sent.send(()).unwrap());
        drop(events);
        forward(42);
        assert!(wakes.try_recv().is_err());
    }
}
