//! The on-disk mailbox — authoritative message blobs plus a re-derivable index.
//!
//! `messages/<MID>.b2f` is the truth: each received or composed message is written whole, exactly
//! as it went over the wire, by the store's atomic write (temp file → `sync_all` → rename) so a
//! crash mid-write can leave a missing message but never a half one. `index.json` is a cache over
//! those blobs and is re-derivable from them in full — which makes rebuilding it both the recovery
//! path after a corrupt or lost index and the positive control proving the index says nothing the
//! blobs do not.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
