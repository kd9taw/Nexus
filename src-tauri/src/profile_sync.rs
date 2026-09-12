//! Coalesce profile-change notifications off the radio owner. A notification
//! carries no settings: the task resolves current state when it executes.
use std::sync::{mpsc, Arc, Mutex};

/// Read state only after the device owner is acquired. The snapshot closure
/// must release its state lock before returning; apply retains only the owner.
pub(super) fn with_current<T, S>(
    owner: &Mutex<T>,
    snapshot: impl FnOnce() -> S,
    apply: impl FnOnce(&mut T, S),
) {
    let Ok(mut owner) = owner.lock() else { return };
    let current = snapshot();
    apply(&mut owner, current);
}

pub(super) fn notifier(
    mut task: impl FnMut() + Send + 'static,
) -> std::io::Result<Arc<dyn Fn() + Send + Sync>> {
    let (send, receive) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("profile-sync".into())
        .spawn(move || {
            while receive.recv().is_ok() {
                task();
            }
        })?;
    Ok(Arc::new(move || {
        // Full means one more current-state sync is already pending. This is
        // not a command queue: intermediate profiles must not be replayed.
        let _ = send.try_send(());
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_burst_coalesces_and_reads_current_state_without_blocking_the_sender() {
        let state = Arc::new(AtomicUsize::new(1));
        let worker_state = state.clone();
        let (observed, readings) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let mut first = true;
        let notify = notifier(move || {
            observed.send(worker_state.load(Ordering::SeqCst)).unwrap();
            if first {
                first = false;
                wait.recv().unwrap();
            }
        })
        .unwrap();
        notify();
        assert_eq!(readings.recv_timeout(Duration::from_secs(2)).unwrap(), 1);
        state.store(7, Ordering::SeqCst);
        // The worker is blocked on the test barrier, not a timing assumption.
        // All notifications must return without waiting for it.
        for _ in 0..1000 {
            notify();
        }
        release.send(()).unwrap();
        assert_eq!(readings.recv_timeout(Duration::from_secs(2)).unwrap(), 7);
        drop(notify);
        assert!(matches!(
            readings.recv_timeout(Duration::from_secs(2)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn waiting_for_the_device_owner_reads_new_state_and_releases_state_before_io() {
        let owner = Arc::new(Mutex::new(0));
        let state = Arc::new(Mutex::new(1));
        let held = owner.lock().unwrap();
        let (started, ready) = mpsc::channel();
        let worker_owner = owner.clone();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            with_current(
                &worker_owner,
                || *worker_state.lock().unwrap(),
                |device, snapshot| {
                    assert!(
                        worker_state.try_lock().is_ok(),
                        "device I/O must not hold state"
                    );
                    *device = snapshot;
                },
            );
        });
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        *state.lock().unwrap() = 7;
        drop(held);
        worker.join().unwrap();
        assert_eq!(*owner.lock().unwrap(), 7);
    }

    #[test]
    fn dropping_the_last_notifier_releases_the_worker_and_its_task() {
        let (done, completion) = mpsc::channel();
        struct OnDrop(mpsc::Sender<()>);
        impl Drop for OnDrop {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let finished = OnDrop(done);
        let notify = notifier(move || {
            let _ = &finished;
        })
        .unwrap();
        let clone = notify.clone();
        drop(notify);
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(clone);
        completion.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
