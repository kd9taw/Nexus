//! The companion import, beside the radio loop (SPEC-2 v3 §4.6, C19 Part B): the contact WSJT-X
//! logs in companion mode — its `LoggedAdif` datagram, one ADIF record — is imported here, on a
//! thread of its own, and never in the radio tick.
//!
//! An import reads the log: what it adds is checked against the contacts the log already holds,
//! and since C19 those are read from the store. The radio loop never waits on SQL, so the tick
//! hands the record to this worker — a channel send — and moves on; the worker plans the import
//! with the Engine lock released and makes it under the lock, by the Logbook's Import
//! ([`tempo_app::logwrite::import_logged_contact`]). The contact reaches the log, and with it the hot
//! index's B4 and duplicate marks, a few milliseconds after the tick that received it — the
//! operator's call (2026-09-24: "Companion: Yes, move it off the loop"). What lands is what the
//! tick's import wrote: the same record, by the same rules.
//!
//! The loop's shutdown drops the worker before it tells the app it may exit, and the drop waits
//! for every record already handed over: a contact WSJT-X logged a moment before the quit is in
//! the log the quit flushes.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use tempo_app::engine::Engine;

/// The thread that imports WSJT-X's logged contacts ([`ImportWorker::import`]).
pub(crate) struct ImportWorker {
    /// `Option` only so [`Drop`] can close the channel first, then join.
    tx: Option<Sender<String>>,
    handle: Option<JoinHandle<()>>,
}

impl ImportWorker {
    /// The worker, importing into `engine`'s log.
    pub(crate) fn spawn(engine: Arc<Mutex<Engine>>) -> std::io::Result<Self> {
        let (tx, rx) = channel::<String>();
        let handle = std::thread::Builder::new()
            .name("nexus-companion-import".into())
            .spawn(move || {
                // Ends when the sender drops: the loop's shutdown, or the loop going away.
                for adif in rx {
                    // Nobody waits on the disk for a companion contact, as nobody did when the
                    // tick imported it: the writer writes it, and a quit flushes it.
                    let (done, _durability) =
                        tempo_app::logwrite::import_logged_contact(&engine, &adif);
                    if let Err(e) = done {
                        tempo_core::applog::error(
                            "logbook",
                            &format!("a contact WSJT-X logged could not be imported: {e}"),
                        );
                    }
                }
            })?;
        Ok(Self {
            tx: Some(tx),
            handle: Some(handle),
        })
    }

    /// Hand the ADIF record WSJT-X logged to the worker — a channel send, no I/O, no lock.
    pub(crate) fn import(&self, adif: String) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(adif);
        }
    }
}

impl Drop for ImportWorker {
    fn drop(&mut self) {
        // Close the channel so the worker's loop ends once it has imported what it holds, then
        // wait for it.
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
