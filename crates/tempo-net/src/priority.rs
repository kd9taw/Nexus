//! Below-normal priority for a thread that does background compute: a path prediction, which can
//! take seconds of CPU on a slow machine and must never take a decode period from the radio.
//!
//! Measured on this project's review, decode (the FT8 decoder over one period) pinned to the same
//! CPUs as a burst of predictions: up to 2.2x slower at normal priority when more compute threads
//! than CPUs were busy, 1.08x to 1.2x with the predictions at the lowest priority.
//!
//! **One way.** An unprivileged thread on Linux can lower its own priority but not raise it again,
//! so call this only on a thread made for the job, never on a pool thread that will run other work
//! afterwards. A thread it spawns starts at the same priority on Linux; on Windows a new thread
//! starts at normal.

/// Lower the calling thread's scheduling priority below normal. Best effort: a failure leaves the
/// thread as it was.
pub fn lower_this_thread() {
    // Linux: nice is per thread; 19 is the lowest, the level the review measured.
    #[cfg(target_os = "linux")]
    {
        // SAFETY: plain syscalls on the calling thread's own id; nothing is borrowed.
        unsafe {
            let _ = libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 19);
        }
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
        };
        // SAFETY: the pseudo-handle of the calling thread; failure leaves the priority as it was.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    }
}

#[cfg(test)]
mod tests {
    /// The thread that asks is lowered; the thread that spawned it is not.
    #[cfg(target_os = "linux")]
    #[test]
    fn only_the_calling_thread_is_lowered() {
        // SAFETY: reading this thread's own scheduling priority.
        let nice_of_this_thread =
            || unsafe { libc::getpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t) };
        let before = nice_of_this_thread();
        let lowered = std::thread::spawn(move || {
            super::lower_this_thread();
            nice_of_this_thread()
        })
        .join()
        .unwrap();
        assert_eq!(lowered, 19);
        assert_eq!(nice_of_this_thread(), before, "the caller keeps its own");
    }
}
