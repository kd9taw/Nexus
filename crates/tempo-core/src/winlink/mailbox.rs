//! The on-disk mailbox — authoritative message blobs plus a re-derivable index.
//!
//! `messages/<MID>.b2f` is the truth: each received or composed message is written whole, exactly
//! as it went over the wire, by the store's atomic write (temp file → `sync_all` → rename) so a
//! crash mid-write can leave a missing message but never a half one. `index.json` is a cache over
//! those blobs and is re-derivable from them in full — which makes rebuilding it both the recovery
//! path after a corrupt or lost index and the positive control proving the index says nothing the
//! blobs do not.
//!
//! # Why the index is a cache and not a record
//!
//! Two files that both claim to know what the mailbox holds is two files that can disagree, and
//! the disagreement is discovered by an operator whose message list is missing a message that is
//! sitting on disk. The way out taken here is the one [`crate::store`] takes with its queue: state
//! that can be *derived* is not journaled, it is derived — there `no_acked` and `confirmed` are
//! deliberately left out of the journal and re-resolved from `attempts` and the peer's next reply
//! after a restart, which is self-healing by construction because there is nothing to fall out of
//! step with. Here the whole index is that: [`Mailbox::rebuild_index`] reads the blobs and nothing
//! else, so a lost, truncated, half-written, or hand-mangled `index.json` costs exactly one
//! rebuild and never a message. The index carries no schema version for the same reason — a shape
//! it cannot parse is a rebuild, not an error.
//!
//! **That is also this module's positive control** (CLAUDE.md: a check that found nothing is not a
//! result until a control that MUST trip it passes). "The index looks right" is unfalsifiable on
//! its own — an index that was merely *read back* would look right too. The control is
//! `rebuild_index → delete index.json → rebuild_index → assert equal`: with the cache deleted, an
//! implementation that secretly consulted it cannot produce the first answer twice.
//!
//! # A MID becomes a filename, and a MID comes off the wire
//!
//! [`super::fbb`] keeps a proposal's MID verbatim and deliberately does **not** length-check it,
//! because refusing a thirteen-character MID would refuse a working gateway. That leniency is
//! right at the protocol layer and wrong at this one: here the MID names a path, so a remote
//! station would be choosing a filename in the operator's data directory. [`Mailbox::store`]
//! therefore refuses any MID that is not ASCII alphanumeric / `-` / `_`, 1..=[`MID_MAX`] bytes —
//! which is the alphabet real Winlink MIDs are drawn from, and which cannot express `..`, a path
//! separator, a NUL, a leading dot, or a name the extension juggling below would mangle.
//!
//! ⚠️ Known, accepted limit: on a case-insensitive filesystem (Windows, default macOS) two MIDs
//! differing only in case would collide onto one blob. Winlink MIDs are uppercase in practice, so
//! this is not reachable from a real gateway, and normalising the case would change the MID's
//! identity — which is worse than the collision it avoids.
//!
//! # The index is JSON, but its fields are wire bytes
//!
//! A subject, a callsign or a filename off the air is bytes in whatever encoding the sender used
//! (see [`super::message`]), so the summary fields here are `Vec<u8>` and the JSON encoding is
//! Latin-1: byte `n` is stored as codepoint `U+00nn`. That is lossless for all 256 values and
//! leaves the common all-ASCII case perfectly readable in the file, at the cost of showing
//! non-ASCII text as mojibake — a debugging inconvenience in a cache, where the alternative
//! (decoding the wire as UTF-8) would corrupt what the operator reads. Serialising `Vec<u8>` the
//! default way — a JSON array of integers — is lossless too and was rejected only because it
//! makes even a MID unreadable.
//!
//! # What this module does not do
//!
//! No clock, no socket, no deletion. Nothing here removes a blob: `rebuild_index` skipping a file
//! never means the file went away, and there is no prune path in this batch. The journal and the
//! ordered startup restore are Batch 3's (spec §6), and they layer over this store rather than
//! replacing it.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::message::parse_b2;

/// The blob directory under the mailbox root. Every file the store writes lives here.
pub const MESSAGES_DIR: &str = "messages";
/// The blob extension. Also the glob [`Mailbox::rebuild_index`] derives the index from.
pub const BLOB_EXT: &str = "b2f";
/// The re-derivable index cache, at the mailbox root (beside `messages/`, not inside it, so a
/// stray `index.json` can never be mistaken for a message).
pub const INDEX_JSON: &str = "index.json";
/// The longest MID accepted as a filename. B2F documents "max 12 characters" and `fbb.rs`
/// deliberately does not enforce it; this ceiling exists so a hostile MID cannot be used to blow
/// past a filesystem's name limit, not to police the protocol, hence the generous value.
pub const MID_MAX: usize = 64;

/// The `Date:` header, lifted into [`IndexEntry::date`].
const HDR_DATE: &[u8] = b"Date";
/// The `From:` header, lifted into [`IndexEntry::from`].
const HDR_FROM: &[u8] = b"From";
/// The `To:` header — repeatable, hence [`IndexEntry::to`] is a list.
const HDR_TO: &[u8] = b"To";
/// The `Subject:` header, lifted into [`IndexEntry::subject`].
const HDR_SUBJECT: &[u8] = b"Subject";

/// One message's summary, as the mailbox list needs it: everything a row shows, and nothing that
/// would require opening the blob.
///
/// Every field except [`mid`](Self::mid), [`blob_len`](Self::blob_len) and
/// [`parsed`](Self::parsed) is derived by [`parse_b2`] and is therefore empty when
/// [`parsed`](Self::parsed) is false. `mid` is taken from the **filename**, not from the blob's
/// `Mid:` header, because the filename is what [`Mailbox::store`] was told to call it and is the
/// key the blob is read back by — a blob whose internal `Mid:` disagrees is still that file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// The message identifier, from the blob's filename.
    #[serde(with = "byte_str")]
    pub mid: Vec<u8>,
    /// The `Date:` header value verbatim, or empty. Not parsed — this layer has no opinion about
    /// what a `Date:` means (see [`super::message`]), and ordering is by MID, not by date.
    #[serde(with = "byte_str")]
    pub date: Vec<u8>,
    /// The `From:` header value verbatim, or empty.
    #[serde(with = "byte_str")]
    pub from: Vec<u8>,
    /// Every `To:` header value, in wire order. Winlink messages routinely carry several.
    #[serde(with = "byte_str_list")]
    pub to: Vec<Vec<u8>>,
    /// The `Subject:` header value verbatim, or empty.
    #[serde(with = "byte_str")]
    pub subject: Vec<u8>,
    /// The body's length in bytes — enough for a size column without holding the body.
    pub body_len: usize,
    /// Attachment names in `File:` order. Present so a row can show a paperclip and its filenames
    /// without reading the payloads, which are the large part of a Winlink message.
    #[serde(with = "byte_str_list")]
    pub attachments: Vec<Vec<u8>>,
    /// The blob's size on disk, bytes.
    pub blob_len: usize,
    /// False when the blob is not a readable B2 message.
    ///
    /// Such a blob is still listed, deliberately. It is on disk, it is not going to be deleted,
    /// and an operator who cannot see it has been told the mailbox is empty when it is not —
    /// which is the failure mode this whole "blob is authoritative" design exists to avoid. The
    /// flag is what keeps that row from reading as a well-formed message with no headers.
    pub parsed: bool,
}

/// The whole mailbox index. Two invariants, both imposed by [`Mailbox::write_index`]: entries are
/// **ascending by MID**, and there is **exactly one entry per MID**.
///
/// The ordering is imposed rather than left to the filesystem because `read_dir` order is
/// unspecified, so without it two rebuilds of the same mailbox could differ by a shuffle and the
/// module's own positive control would be comparing two orderings of one set.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Index {
    /// Every message the mailbox holds, ascending by [`IndexEntry::mid`].
    pub entries: Vec<IndexEntry>,
}

/// A mailbox rooted at a directory. Cheap and infallible to make — it touches no disk until asked
/// to, so the app can name a mailbox before the directory exists.
#[derive(Debug, Clone)]
pub struct Mailbox {
    root: PathBuf,
}

/// Names the mailbox rooted at `root`. Creates nothing; see [`Mailbox`].
pub fn open(root: &Path) -> Mailbox {
    Mailbox {
        root: root.to_path_buf(),
    }
}

impl Mailbox {
    /// The blob directory, `<root>/messages`.
    fn messages_dir(&self) -> PathBuf {
        self.root.join(MESSAGES_DIR)
    }

    /// The path a MID's blob lives at, after validation.
    fn blob_path(&self, mid: &[u8]) -> io::Result<PathBuf> {
        Ok(self
            .messages_dir()
            .join(format!("{}.{BLOB_EXT}", mid_stem(mid)?)))
    }

    /// Writes `b2f_blob` as this MID's message and brings `index.json` back into step.
    ///
    /// The blob lands first and atomically, because it is the record: until the rename completes
    /// the message does not exist, and after it the message exists whether or not the index write
    /// that follows succeeds. Re-storing a MID replaces both the blob and its single index entry.
    ///
    /// The index update is an upsert onto the loaded cache rather than a full re-derivation, so
    /// receiving *n* messages in a session costs *n* index writes and not *n²* blob reads. It is
    /// not a second way of computing an entry — both routes call the same [`entry_from_blob`] —
    /// and a cache that will not load is not patched around: it is rebuilt from the blobs, which
    /// is the recovery path doing exactly its job.
    ///
    /// Errors: [`io::ErrorKind::InvalidInput`] for a MID that is not filename-safe (module
    /// header), otherwise whatever the filesystem said.
    pub fn store(&self, mid: &[u8], b2f_blob: &[u8]) -> io::Result<()> {
        let path = self.blob_path(mid)?;
        std::fs::create_dir_all(self.messages_dir())?;
        write_atomic(&path, b2f_blob)?;

        let entry = entry_from_blob(mid, b2f_blob);
        match self.load_index() {
            Ok(mut index) => {
                // Drop-then-push rather than a binary-search insert: it assumes nothing about the
                // order of the cache it just read. `write_index` below re-imposes both index
                // invariants on the result, so a mangled-but-parseable `index.json` cannot leave
                // a store holding a cache the rebuild would disagree with.
                index.entries.retain(|e| e.mid != entry.mid);
                index.entries.push(entry);
                self.write_index(&mut index)
            }
            // Missing or unreadable cache: derive the whole thing from the blobs, which now
            // include the one just written.
            Err(_) => self.rebuild_index().map(|_| ()),
        }
    }

    /// Reads a message's blob back, byte for byte as [`store`](Self::store) received it.
    pub fn read(&self, mid: &[u8]) -> io::Result<Vec<u8>> {
        std::fs::read(self.blob_path(mid)?)
    }

    /// Loads `index.json` as it stands, without touching the blobs.
    ///
    /// Errors when the cache is absent or unreadable — which is a normal condition, not a
    /// disaster: the caller's answer to it is [`rebuild_index`](Self::rebuild_index). It is a
    /// separate function precisely so that "the cache was bad" stays visible instead of being
    /// swallowed by a silent rebuild.
    pub fn load_index(&self) -> io::Result<Index> {
        let bytes = std::fs::read(self.root.join(INDEX_JSON))?;
        serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Re-derives the index **entirely** from `messages/*.b2f`, writes it to `index.json`, and
    /// returns it. The recovery path, and this module's positive control (module header).
    ///
    /// Reads no existing index, by construction — that is the whole point, and a change here that
    /// consulted the cache would make the control in `rebuild_index_reconstructs_from_blobs_alone`
    /// pass for the wrong reason.
    ///
    /// Files in `messages/` that are not `<valid-MID>.b2f` are skipped: a leftover
    /// `<MID>.tmp.<pid>` from a write that crashed before its rename is a half message and must
    /// never be listed as a whole one. A blob that is present but unparseable is **not** skipped —
    /// it is listed with [`IndexEntry::parsed`] false. Nothing is deleted either way.
    pub fn rebuild_index(&self) -> io::Result<Index> {
        let dir = self.messages_dir();
        std::fs::create_dir_all(&dir)?;
        let mut entries = Vec::new();
        for dirent in std::fs::read_dir(&dir)? {
            let dirent = dirent?;
            if !dirent.file_type()?.is_file() {
                continue;
            }
            let name = dirent.file_name();
            let Some(mid) = blob_mid(Path::new(&name)) else {
                continue;
            };
            let bytes = std::fs::read(dirent.path())?;
            entries.push(entry_from_blob(&mid, &bytes));
        }
        let mut index = Index { entries };
        // `write_index` imposes the ordering; the returned value is the canonicalised one, so a
        // caller sees exactly what landed on disk.
        self.write_index(&mut index)?;
        Ok(index)
    }

    /// Publishes an index atomically, after imposing the two [`Index`] invariants on it —
    /// ascending by MID, one entry per MID.
    ///
    /// They are enforced here and not at each call site because they belong to the file, not to
    /// the route that produced it: the upsert in [`store`](Self::store) inherits whatever shape
    /// the cache on disk had, and leaving it to fix that up itself is how the two writing paths
    /// come to publish caches that disagree (measured: a store over a cache carrying a duplicate
    /// row for an unrelated MID carried the duplicate straight through).
    ///
    /// Private: an index is written by the two paths above, never handed in from outside, or the
    /// cache could be made to say something the blobs do not.
    fn write_index(&self, index: &mut Index) -> io::Result<()> {
        index.entries.sort_by(|a, b| a.mid.cmp(&b.mid));
        // Stable sort + dedup keeps the first row for a repeated MID. Which one survives does not
        // matter (both are stale relative to the blob); that exactly one does is the invariant.
        index.entries.dedup_by(|a, b| a.mid == b.mid);
        std::fs::create_dir_all(&self.root)?;
        let json =
            serde_json::to_vec(index).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(&self.root.join(INDEX_JSON), &json)
    }
}

/// Derives one summary from a blob. The single derivation used by both index paths.
///
/// `mid` wins over the blob's own `Mid:` header — see [`IndexEntry::mid`]. A blob `parse_b2`
/// refuses yields an entry that is honest about it rather than no entry at all.
fn entry_from_blob(mid: &[u8], blob: &[u8]) -> IndexEntry {
    let mut entry = IndexEntry {
        mid: mid.to_vec(),
        date: Vec::new(),
        from: Vec::new(),
        to: Vec::new(),
        subject: Vec::new(),
        body_len: 0,
        attachments: Vec::new(),
        blob_len: blob.len(),
        parsed: false,
    };
    let Ok(msg) = parse_b2(blob) else {
        return entry;
    };
    entry.parsed = true;
    entry.body_len = msg.body.len();
    entry.attachments = msg.attachments.into_iter().map(|a| a.name).collect();
    for (name, value) in msg.headers {
        // Header names are case-insensitive on the wire and `message.rs` keeps the case they were
        // sent in, so match on the folded name rather than assuming a gateway's capitalisation.
        if name.eq_ignore_ascii_case(HDR_DATE) {
            entry.date = value;
        } else if name.eq_ignore_ascii_case(HDR_FROM) {
            entry.from = value;
        } else if name.eq_ignore_ascii_case(HDR_TO) {
            entry.to.push(value);
        } else if name.eq_ignore_ascii_case(HDR_SUBJECT) {
            entry.subject = value;
        }
    }
    entry
}

/// Validates a MID and returns it as a filename stem.
///
/// The alphabet and the reasons are in the module header. Refusal is
/// [`io::ErrorKind::InvalidInput`] with the offending MID **not** echoed into the message: it is
/// remote input, and an error string ends up in logs and toasts.
fn mid_stem(mid: &[u8]) -> io::Result<&str> {
    let ok = !mid.is_empty()
        && mid.len() <= MID_MAX
        && mid
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_');
    if !ok {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Winlink MID is not a usable filename: expected 1..={MID_MAX} bytes of ASCII \
                 alphanumerics, '-' or '_'"
            ),
        ));
    }
    // Infallible given the check above (every accepted byte is ASCII), but expressed as a
    // conversion rather than an `unsafe` shortcut.
    std::str::from_utf8(mid).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

/// The MID a blob filename names, or `None` if the file is not one of ours.
///
/// The name must be exactly `<valid MID>.b2f`: the extension check alone would admit a file
/// someone dropped in the directory, and the MID check alone would admit the `.tmp.<pid>`
/// scratch of a write that never completed.
fn blob_mid(name: &Path) -> Option<Vec<u8>> {
    if name.extension()?.to_str()? != BLOB_EXT {
        return None;
    }
    let stem = name.file_stem()?.to_str()?;
    mid_stem(stem.as_bytes())
        .ok()
        .map(|s| s.as_bytes().to_vec())
}

/// Publishes `bytes` at `path` atomically: a private temp file, flushed to the platter, then a
/// rename onto the target.
///
/// The `sync_all` is the half that makes it a *crash* guarantee and not just a concurrency one —
/// without it the rename can be durable while the data behind it is not, and the mailbox comes
/// back holding a zero-length message. The temp name carries the pid (the `logbook.rs` per-process
/// tmp pattern) so two Nexus instances sharing one mailbox cannot interleave writes into one
/// scratch file and publish the splice.
///
/// ⚠️ The parent directory is not fsynced, so on a crash immediately after the rename some
/// filesystems can lose the directory entry — the blob is then missing, never half-written, and
/// this is the project's established pattern (`sstv_store.rs`, `logbook.rs`). A crash can also
/// leave the temp file behind; `rebuild_index` will not mistake it for a message.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Latin-1 codec for one byte-string index field — see the module header for why it is not UTF-8
/// and not an integer array.
mod byte_str {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| b as char).collect()
    }

    /// `None` when a codepoint is above `U+00FF` — i.e. the file was not written by `encode`.
    /// A `None` here is a cache that will not load, which is a rebuild.
    pub(super) fn decode(s: &str) -> Option<Vec<u8>> {
        s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
    }

    pub fn serialize<S: Serializer>(bytes: &[u8], ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(de)?;
        decode(&s)
            .ok_or_else(|| serde::de::Error::custom("index field is not a Latin-1 byte string"))
    }
}

/// [`byte_str`] for a list field ([`IndexEntry::to`], [`IndexEntry::attachments`]).
mod byte_str_list {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(items: &[Vec<u8>], ser: S) -> Result<S::Ok, S::Error> {
        let encoded: Vec<String> = items.iter().map(|b| super::byte_str::encode(b)).collect();
        serde::Serialize::serialize(&encoded, ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<Vec<u8>>, D::Error> {
        let encoded = Vec::<String>::deserialize(de)?;
        encoded
            .iter()
            .map(|s| super::byte_str::decode(s))
            .collect::<Option<Vec<Vec<u8>>>>()
            .ok_or_else(|| serde::de::Error::custom("index field is not a Latin-1 byte string"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::winlink::message::{assemble_b2, Attachment, Message};
    use std::path::PathBuf;

    /// A unique per-test scratch dir under the OS temp dir (the `sstv_store.rs` pattern).
    /// Removed first, so a previous run's leftovers can never make a test pass.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nexus-wl-mailbox-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real B2 blob, so the index is derived by `parse_b2` and not by guesswork.
    fn blob(mid: &[u8], from: &[u8], subject: &[u8], body: &[u8]) -> Vec<u8> {
        assemble_b2(&Message {
            mid: mid.to_vec(),
            headers: vec![
                (b"Date".to_vec(), b"2026/09/06 12:00".to_vec()),
                (b"From".to_vec(), from.to_vec()),
                (b"To".to_vec(), b"N0CALL".to_vec()),
                (b"Subject".to_vec(), subject.to_vec()),
            ],
            body: body.to_vec(),
            attachments: vec![Attachment {
                name: b"ICS 213.xml".to_vec(),
                data: b"<x/>".to_vec(),
            }],
        })
    }

    /// Store a well-formed message. Every call site wants the blob and the MID to agree.
    fn put(mb: &Mailbox, mid: &[u8], from: &[u8], subject: &[u8], body: &[u8]) {
        mb.store(mid, &blob(mid, from, subject, body)).unwrap();
    }

    /// THE positive control for the whole module: destroy `index.json` and rebuild it from the
    /// blobs alone. If `rebuild_index` were reading the cache it claims to re-derive, the second
    /// call could not produce the first call's answer with the cache deleted.
    #[test]
    fn rebuild_index_reconstructs_from_blobs_alone() {
        let dir = scratch("rebuild-control");
        let mb = open(&dir);
        put(&mb, b"MID000000001", b"W1AW", b"one", b"first");
        put(&mb, b"MID000000002", b"K2ABC", b"two", b"second");

        // Half one — POISON. A cache that is perfectly well-formed and simply WRONG: it lists a
        // message that has no blob. A rebuild that consulted the cache would carry the phantom
        // through; one that reads only the blobs cannot see it at all.
        //
        // This half is here because deleting the index is NOT sufficient on its own, and that was
        // measured, not assumed: a `rebuild_index` sabotaged to return the cache whenever it can
        // read one SURVIVED the delete-and-compare below — with the cache gone the sabotage falls
        // through to the honest path, and `store` had already written the same answer, so both
        // sides matched. The poison is what makes the control trip on that cheat.
        let mut poisoned = mb.load_index().unwrap();
        poisoned.entries.insert(
            0,
            IndexEntry {
                mid: b"PHANTOM00001".to_vec(),
                date: Vec::new(),
                from: b"NOBODY".to_vec(),
                to: Vec::new(),
                subject: b"no blob backs this".to_vec(),
                body_len: 0,
                attachments: Vec::new(),
                blob_len: 0,
                parsed: true,
            },
        );
        std::fs::write(dir.join(INDEX_JSON), serde_json::to_vec(&poisoned).unwrap()).unwrap();

        let before = mb.rebuild_index().unwrap();
        assert_eq!(
            before.entries.len(),
            2,
            "a phantom in the cache must not survive a rebuild"
        );
        assert!(
            !before.entries.iter().any(|e| e.mid == b"PHANTOM00001"),
            "rebuild read the index it is supposed to re-derive"
        );

        // Half two — DESTROY. With no cache at all, the same answer must come back.
        std::fs::remove_file(dir.join(INDEX_JSON)).unwrap();
        let after = mb.rebuild_index().unwrap();
        assert_eq!(
            after.entries, before.entries,
            "rebuild must reproduce the index from blobs alone"
        );
        assert!(
            dir.join(INDEX_JSON).exists(),
            "rebuild must write the cache back out"
        );
    }

    #[test]
    fn a_corrupt_index_is_recovered_by_rebuild() {
        let dir = scratch("corrupt-index");
        let mb = open(&dir);
        put(&mb, b"MID000000001", b"W1AW", b"one", b"first");
        std::fs::write(dir.join(INDEX_JSON), b"{ this is not json").unwrap();
        assert!(
            mb.load_index().is_err(),
            "control: the corrupt cache must actually be unreadable"
        );
        let idx = mb.rebuild_index().unwrap(); // must not depend on the corrupt index
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].mid, b"MID000000001".to_vec());
    }

    /// The blob is the truth, so it must come back exactly as it went in.
    #[test]
    fn store_then_read_returns_the_blob_byte_for_byte() {
        let dir = scratch("blob-roundtrip");
        let mb = open(&dir);
        // Deliberately binary, including a NUL and a lone 0xFF: a blob is bytes, never text.
        let bytes: Vec<u8> = (0u8..=255).collect();
        mb.store(b"BINARY000001", &bytes).unwrap();
        assert_eq!(mb.read(b"BINARY000001").unwrap(), bytes);
    }

    #[test]
    fn the_index_summarises_the_parsed_message() {
        let dir = scratch("summary");
        let mb = open(&dir);
        let b = blob(b"MID000000001", b"W1AW", b"Field Day", b"body text");
        mb.store(b"MID000000001", &b).unwrap();
        let idx = mb.rebuild_index().unwrap();
        let e = &idx.entries[0];
        assert!(e.parsed, "a well-formed B2 blob must parse");
        assert_eq!(e.mid, b"MID000000001".to_vec());
        assert_eq!(e.from, b"W1AW".to_vec());
        assert_eq!(e.to, vec![b"N0CALL".to_vec()]);
        assert_eq!(e.subject, b"Field Day".to_vec());
        assert_eq!(e.date, b"2026/09/06 12:00".to_vec());
        assert_eq!(e.body_len, b"body text".len());
        assert_eq!(e.attachments, vec![b"ICS 213.xml".to_vec()]);
        assert_eq!(e.blob_len, b.len());
    }

    /// A blob that is not a readable B2 message must still be listed — the message is on disk and
    /// the operator has to be able to see that it is there and unreadable, not have it vanish.
    #[test]
    fn an_unparseable_blob_still_gets_an_entry() {
        let dir = scratch("unparseable");
        let mb = open(&dir);
        mb.store(b"MID000000001", b"...b2f blob 1...").unwrap();
        let idx = mb.rebuild_index().unwrap();
        assert_eq!(idx.entries.len(), 1);
        assert!(
            !idx.entries[0].parsed,
            "garbage must be marked unparsed, not presented as an empty message"
        );
        assert_eq!(
            idx.entries[0].mid,
            b"MID000000001".to_vec(),
            "the MID comes off the filename, which is what store() named it"
        );
        assert_eq!(idx.entries[0].blob_len, b"...b2f blob 1...".len());
    }

    /// A MID arrives off the wire in an `FC` line (see `fbb.rs`) and becomes a path. Both
    /// directions of the guard, because one direction is half a test.
    #[test]
    fn a_mid_that_is_not_filename_safe_is_refused() {
        let dir = scratch("mid-guard");
        let mb = open(&dir);
        for bad in [
            &b"../escape"[..],
            b"a/b",
            b"a\\b",
            b"",
            b"has space",
            b"dot.dot",
            b"nul\0byte",
            b"caf\xc3\xa9",
        ] {
            let err = mb
                .store(bad, b"x")
                .expect_err("a MID that is not filename-safe must be refused");
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }
        // Positive control: the guard must still admit a real MID.
        mb.store(b"ABCDE1234567", b"x").unwrap();
        assert!(dir.join(MESSAGES_DIR).join("ABCDE1234567.b2f").exists());
        // …and nothing escaped the mailbox root.
        assert!(!dir.parent().unwrap().join("escape.b2f").exists());
    }

    /// The index is JSON but the fields are wire bytes. A subject that is not UTF-8 must survive
    /// the cache round-trip unchanged — decoding it as text would corrupt what the operator reads.
    #[test]
    fn a_non_utf8_subject_survives_the_index_round_trip() {
        let dir = scratch("non-utf8");
        let mb = open(&dir);
        let subject = b"caf\xe9 \xff\xfe".to_vec();
        assert!(
            String::from_utf8(subject.clone()).is_err(),
            "control: the fixture must actually be invalid UTF-8"
        );
        mb.store(
            b"MID000000001",
            &blob(b"MID000000001", b"W1AW", &subject, b"x"),
        )
        .unwrap();
        let built = mb.rebuild_index().unwrap();
        assert_eq!(built.entries[0].subject, subject);
        // Now through the file on disk, not the in-memory value.
        let loaded = mb.load_index().unwrap();
        assert_eq!(loaded.entries, built.entries);
    }

    #[test]
    fn store_keeps_the_cache_in_step_without_a_full_rebuild() {
        let dir = scratch("store-updates");
        let mb = open(&dir);
        put(&mb, b"MID000000001", b"W1AW", b"one", b"a");
        assert_eq!(mb.load_index().unwrap().entries.len(), 1);
        put(&mb, b"MID000000002", b"W1AW", b"two", b"b");
        let loaded = mb.load_index().unwrap();
        assert_eq!(loaded.entries.len(), 2);
        // Whatever route store() took, the cache must equal the re-derivation.
        assert_eq!(loaded.entries, mb.rebuild_index().unwrap().entries);
    }

    /// Re-storing the same MID replaces both the blob and its one index entry — never duplicates.
    #[test]
    fn re_storing_a_mid_replaces_it() {
        let dir = scratch("restore-mid");
        let mb = open(&dir);
        put(&mb, b"MID000000001", b"W1AW", b"one", b"a");
        put(&mb, b"MID000000001", b"W1AW", b"revised", b"bb");
        let idx = mb.load_index().unwrap();
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.entries[0].subject, b"revised".to_vec());
        assert_eq!(idx.entries, mb.rebuild_index().unwrap().entries);
    }

    /// `store` over a cache that cannot be read must recover it, not lose the earlier entries.
    #[test]
    fn store_over_a_corrupt_index_rebuilds_it() {
        let dir = scratch("store-corrupt");
        let mb = open(&dir);
        put(&mb, b"MID000000001", b"W1AW", b"one", b"a");
        std::fs::write(dir.join(INDEX_JSON), b"{ this is not json").unwrap();
        put(&mb, b"MID000000002", b"W1AW", b"two", b"b");
        let idx = mb.load_index().unwrap();
        assert_eq!(
            idx.entries.len(),
            2,
            "the earlier message must survive a corrupt cache"
        );
    }

    /// The upsert must not trust the shape of the cache it reads. A cache that parses but is out
    /// of order and holds a duplicate must come out of a store obeying both index invariants —
    /// otherwise `store` and `rebuild_index` quietly disagree, which is the whole failure mode.
    #[test]
    fn store_repairs_a_mangled_but_parseable_cache() {
        let dir = scratch("mangled-cache");
        let mb = open(&dir);
        put(&mb, b"BBB000000001", b"W1AW", b"b", b"b");
        put(&mb, b"AAA000000001", b"W1AW", b"a", b"a");

        // Reverse it and duplicate a row: still valid JSON, still loads, and wrong both ways.
        let mut mangled = mb.load_index().unwrap();
        mangled.entries.reverse();
        let dup = mangled.entries[0].clone();
        mangled.entries.push(dup);
        std::fs::write(dir.join(INDEX_JSON), serde_json::to_vec(&mangled).unwrap()).unwrap();
        assert_eq!(
            mb.load_index().unwrap().entries.len(),
            3,
            "control: the mangled cache must really load, duplicate and all"
        );

        put(&mb, b"CCC000000001", b"W1AW", b"c", b"c");
        let loaded = mb.load_index().unwrap();
        assert_eq!(loaded.entries.len(), 3);
        assert_eq!(loaded.entries, mb.rebuild_index().unwrap().entries);
    }

    /// `read_dir` order is not defined, so the index must impose one or the control test above
    /// would be comparing two shuffles of the same set.
    #[test]
    fn entries_are_ordered_by_mid() {
        let dir = scratch("ordering");
        let mb = open(&dir);
        for mid in [&b"ZZZ000000001"[..], b"AAA000000001", b"MMM000000001"] {
            mb.store(mid, &blob(mid, b"W1AW", b"s", b"b")).unwrap();
        }
        let mids: Vec<Vec<u8>> = mb
            .rebuild_index()
            .unwrap()
            .entries
            .iter()
            .map(|e| e.mid.clone())
            .collect();
        assert_eq!(
            mids,
            vec![
                b"AAA000000001".to_vec(),
                b"MMM000000001".to_vec(),
                b"ZZZ000000001".to_vec()
            ]
        );
    }

    /// A leftover temp file from a crashed write must not become a phantom message.
    #[test]
    fn a_leftover_temp_file_is_not_a_message() {
        let dir = scratch("leftover-tmp");
        let mb = open(&dir);
        put(&mb, b"MID000000001", b"W1AW", b"one", b"a");
        std::fs::write(dir.join(MESSAGES_DIR).join("MID000000002.tmp.999"), b"half").unwrap();
        assert_eq!(mb.rebuild_index().unwrap().entries.len(), 1);
    }
}
