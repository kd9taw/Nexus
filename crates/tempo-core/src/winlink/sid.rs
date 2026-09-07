//! The FBB/B2F **SID** (System IDentification) line grammar.
//!
//! Each side announces its capabilities in square brackets — e.g. `[NexusWL-1.0-B2FHM$]` — and the
//! trailing flag letters say what the peer can do. Nexus requires `F` (FBB forwarding) and `B2`
//! (the B2 message format) and **tolerates every flag it does not know**: the flag set is open and
//! real gateways carry letters no document lists. Refusing an unknown flag would refuse a working
//! CMS, so the parser is strict about what it needs and lenient about everything else.
//!
//! Not yet implemented — the module exists so the rest of `winlink` can name it.
