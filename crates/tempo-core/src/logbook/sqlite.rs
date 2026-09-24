//! The logbook's SQLite schema: a [`QsoRecord`] as rows, and those rows back as a
//! [`QsoRecord`], byte-identically through ADIF.
//!
//! This is SPEC-1 v3's **Stage 1**: SQLite becomes the DURABLE STORE while the whole log
//! stays in memory exactly as it is today. A stamp, a QSL mark or an edit becomes one row
//! UPDATE instead of a 31–100 MB whole-file rewrite. Queries do NOT move to SQL here — the
//! hot predicates (the 300 s dedup guard, the worked-before sets, the contest dupe set) stay
//! in memory in both stages, because the FT sequencer calls them synchronously at every slot
//! boundary and a disk read on that path is precisely what this programme exists to remove.
//!
//! # The two rules the schema is built on
//!
//! **1. The passthrough is not optional.** [`QsoRecord::extra`] preserves every unmodelled
//! ADIF tag verbatim BY CONSTRUCTION — the parser consumes (`remove`s) the 62 tags it models
//! and the remainder lands there — so every other standard ADIF field, and every other
//! logger's `APP_*` namespace, round-trips through `log.adi` today, pinned by
//! `foreign_adif_fields_survive_a_round_trip_verbatim`. Fixed columns alone would silently
//! destroy an operator's imported data on the first save. [`qso_extra`](DDL) is a child table
//! rather than a JSON blob so a foreign `APP_*` can be indexed if one ever becomes queryable,
//! and it is re-emitted `ORDER BY name` to reproduce `extra.sort()` byte for byte (SQLite's
//! default `BINARY` collation and Rust's `String` ordering are both bytewise over UTF-8, and
//! the table's primary key makes the name unique within a record, so the two agree).
//!
//! **2. Every derived column is written AT INSERT, never computed at query time.** Each one
//! exists because its semantics are not a plain SQL predicate: a base call is extracted by a
//! rule SQL cannot express ([`crate::message::base_call`]), a mode folds three different ways
//! that must stay distinct, and the award identity is cty.dat's entity — never the stored
//! `COUNTRY` text, because QRZ writes "Germany" where cty.dat says "Fed. Rep. of Germany".
//!
//! # What this module does NOT do
//!
//! No migration of an existing `log.adi` (C5), no writer thread (C7), no ADIF mirror (C6).
//! It opens or creates the database, applies the schema, and converts records both ways — all
//! of them ([`LogDb::load_all`]) or the ones some ids name ([`LogDb::rows_by_ids`]), through
//! ONE decoder and inside one read transaction — and opens the read-only connections SPEC-2's
//! read path hands out ([`LogDb::open_reader`], pooled by [`super::reader::LogReader`]).
//!
//! # Where this implementation departs from SPEC-1 §v3.11, and why
//!
//! Each of these is a place the DDL as written could not hold what the code puts in it. They
//! are marked `⚠️ DEVIATION` at the column, with the evidence:
//!
//! - **`entity` is TEXT, not INTEGER.** cty.dat's entity is a NAME
//!   (`propagation::dxcc::DxccInfo::entity: &'static str`); the numeric award id is the
//!   separately modelled `dxcc` column.
//! - **`contest_session_id` carries no foreign key.** With `foreign_keys = ON` an imported
//!   foreign row naming a session this database has never seen would be REJECTED.
//! - **`credit_granted` / `credit_submitted` / the five `ota_*` columns / `contest_qid` were
//!   absent from §v3.11 entirely**, and without them the round trip destroys award credit,
//!   every POTA/SOTA/IOTA tag, and the merge identity.
//! - **`qso_contest_adif` is absent from §v3.11 entirely**, and without it a Field Day or
//!   QSO-party contact loses its section, class and state on every export after a restart —
//!   for a QSO party totally, the merge leaving `state: None` so the contacted station's
//!   state exists ONLY as the directed column. See the table for why it is not `qso_extra`.
//! - **`id` is `NOT NULL`.** SQLite lets a non-INTEGER `PRIMARY KEY` hold NULLs.
//!
//! The report that accompanies this commit states each one in full.

use super::{
    ContestFields, Logbook, Ota, QslRcvd, QslSent, QslVia, QsoRecord, RecordId, UploadDetail,
    UploadOutcome, UploadState, UploadStatus,
};
use rusqlite::{params, params_from_iter, Connection};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// The schema this build writes, recorded in `log_meta`. Bump it only alongside a migration:
/// [`LogDb::open`] refuses a database stamped with any other version rather than reading it
/// under the wrong assumptions, because the alternative is a silent partial read of an
/// operator's lifetime log.
pub const SCHEMA_VERSION: i64 = 1;

/// How long a writer waits for another connection's transaction before giving up. The contest
/// insert is a per-keystroke path, so the general log's bulk work must be chunked to keep the
/// real wait in milliseconds; this is the ceiling, not the expectation.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// The page cache of the one-time conversion's connection, in KiB — 64 MiB, against SQLite's
/// default of 2,000 KiB. Measured: the conversion's chunks are big, their rows land at random
/// places in the primary keys, and in 2 MB the key pages were evicted and read back over and
/// over. 64 MiB took 14–25% off the store's time at 150,000 and 300,000 contacts; 128 and 256
/// MiB took at most 4% more. It writes no fewer bytes — that is the chunk size's job — and it is
/// freed when the conversion's connection closes.
const CONVERSION_CACHE_KIB: i64 = 65_536;

/// How many ids or rowids one statement of a partial read ([`LogDb::rows_by_ids`]) names. A
/// bound on statement size, not a rule: a longer list is read in several statements of this
/// many, inside the same read transaction, and comes back in one log order.
const ROWS_PER_STATEMENT: usize = 512;

/// Which records [`LogDb::decode`] reads: every one, or those at these rowids (sorted).
#[derive(Debug, Clone, Copy)]
enum Rows<'a> {
    All,
    At(&'a [i64]),
}

/// `log_meta`, applied BEFORE [`DDL`] so the version can be read first.
///
/// The five watermarks survive into SQLite rather than being replaced by a bare "the table
/// changed" — every cache keys on one of them. The writer thread persists them; this module
/// records `schema_version` and opens the door with [`LogDb::meta`] / [`LogDb::set_meta`].
const META_DDL: &str = "
CREATE TABLE IF NOT EXISTS log_meta (
  k TEXT PRIMARY KEY NOT NULL,
  v INTEGER NOT NULL   -- revision, content_rev, index_rev, key_rev, shape_rev, schema_version
);
";

/// The schema's tables. `IF NOT EXISTS` throughout so opening an existing database is the same
/// call as creating one. Their secondary indexes are [`INDEXES`].
const DDL: &str = r#"
-- ─── The general log ────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS qso (
  -- ⚠️ DEVIATION from §v3.11 (`id TEXT PRIMARY KEY`): NOT NULL is explicit. SQLite permits
  -- NULLs in a PRIMARY KEY that is not an INTEGER alias for the rowid — a documented
  -- long-standing compatibility quirk — so the DDL as written would accept any number of
  -- id-less, unaddressable rows. That is the exact failure `APP_NEXUS_ID` was minted to end.
  id              TEXT PRIMARY KEY NOT NULL,   -- RecordId: 'posid:nonce:seq' or '~hash[.k]'

  -- modelled ADIF, one column each
  call            TEXT NOT NULL,
  band            TEXT NOT NULL,      -- '' is a REAL key, never NULL
  mode            TEXT NOT NULL,
  freq_mhz        REAL,               -- 0.0 means absent, and is stored as 0.0
  freq_rx_mhz     REAL,               -- NULL /= 0.0: the split claim
  when_unix       INTEGER NOT NULL,
  time_off_unix   INTEGER,
  time_known      INTEGER NOT NULL,   -- STORED, not re-derived from a midnight heuristic
  grid TEXT, country TEXT, state TEXT, name TEXT, qth TEXT,
  comment TEXT, notes TEXT, rst_sent TEXT, rst_rcvd TEXT,
  -- ⚠️ REAL cannot hold a NaN: SQLite has no NaN and binds one as NULL. `tx_power` is the one
  -- column where that is observable — a malformed `<TX_PWR:3>NaN` parses to Some(NaN) and the
  -- writer re-emits it, so it round-trips through `log.adi` today and would not round-trip
  -- here. `freq_mhz`/`freq_rx_mhz` are guarded by `is_finite()` on both sides, so they are not
  -- exposed. Recorded rather than papered over; fixing it means storing the token, which is a
  -- behaviour change and not this port.
  tx_power REAL,
  dxcc INTEGER, prop_mode TEXT, sat_name TEXT,
  operator TEXT, station_callsign TEXT, my_grid TEXT, my_rig TEXT,

  -- Confirmations: the RAW token per channel, so a mirror can one day re-emit what arrived.
  -- ⚠️ In Stage 1 these hold what the RECORD can prove, not what the file said: `take_confirmed`
  -- accepts both 'Y' and 'V' and keeps only a bool, so the token a QsoRecord came from no
  -- longer exists by the time the store sees it. They are written 'Y'/NULL — exactly what the
  -- ADIF writer emits — and reading them back applies `take_confirmed`'s own rule.
  qsl_card_rcvd_raw TEXT, lotw_rcvd_raw TEXT, eqsl_rcvd_raw TEXT, qrz_status_raw TEXT,
  qsl_sent INTEGER NOT NULL DEFAULT 0, qsl_sent_via TEXT,
  qsl_sent_date_unix INTEGER, qsl_sent_cleared_unix INTEGER,

  -- ⚠️ DEVIATION: §v3.11 has no home for either list, and §v3.8's `qso_credit` child table did
  -- not reach the DDL. Without them the round trip destroys award credit — and the DDL keeps
  -- `dxcc_credited`, which is DERIVED from `credit_granted`, so it kept the flag and dropped
  -- the fact. Stored as the normalised comma-joined list the ADIF writer emits, which is
  -- lossless because `parse_credit` guarantees no element contains ',' or ':'. The child table
  -- belongs with the parser fix that stops `:source` being discarded — a behaviour change this
  -- port does not make.
  credit_granted   TEXT NOT NULL DEFAULT '',
  credit_submitted TEXT NOT NULL DEFAULT '',

  -- ⚠️ DEVIATION: §v3.11 has no OTA columns at all, so the round trip would destroy every
  -- POTA / SOTA / IOTA tag on the record. No `*_ref_norm` column is added beside them: a
  -- reference may be a comma/semicolon two-fer (`my_refs`), so one normalised column would be
  -- wrong; that is the `qso_ota_ref` child table, and it is Stage 2 query work.
  ota_my_program TEXT, ota_my_ref TEXT,
  ota_their_program TEXT, ota_their_ref TEXT, ota_iota TEXT,

  -- ── derived, written at insert ──
  call_norm       TEXT NOT NULL,      -- upper(trim(call))
  base_call       TEXT NOT NULL,      -- message::base_call — NOT expressible in SQL
  band_norm       TEXT NOT NULL,      -- upper(band), '' preserved
  mode_norm       TEXT NOT NULL,      -- three folding levels, and they must stay
  mode_class      TEXT NOT NULL,      -- distinct from one another
  dedup_mode      TEXT NOT NULL,
  utc_day         INTEGER NOT NULL,   -- five different window widths use it
  grid4           TEXT,               -- grid is compared at 4 chars, and never trimmed
  -- ⚠️ DEVIATION: §v3.11 says `entity INTEGER`. cty.dat's entity is a NAME —
  -- `propagation::dxcc::DxccInfo::entity` is `&'static str` and the command layer stores it as
  -- `Option<String>` — and the numeric award id is already the `dxcc` column above. An INTEGER
  -- column could not hold the award identity the code produces.
  entity          TEXT,
  cq_zone         INTEGER,
  is_sat          INTEGER NOT NULL DEFAULT 0,   -- a separate award universe
  dxcc_credited   INTEGER NOT NULL DEFAULT 0,
  operator_norm   TEXT,
  dupe_key        TEXT,               -- the ruleset's key; NULL until a ruleset is in hand

  -- ── contest, on the row ──
  contest_id      TEXT,               -- OPAQUE STRING, never an enum, and NOT the Cabrillo
                                      -- token — that is derived at export
  -- ⚠️ DEVIATION: §v3.11 wrote `REFERENCES contest_session(id)`. With `foreign_keys = ON` that
  -- constraint REJECTS a legitimate record: `APP_NEXUS_SESSION` is read off any imported file,
  -- and another operator's session has no row in this database. The insert would fail and the
  -- record would be dropped. The column stays; the constraint cannot.
  contest_session_id TEXT,
  -- ⚠️ DEVIATION: §v3.11 omits `APP_NEXUS_QID`, which is the MERGE IDENTITY — the contest →
  -- general merge is idempotent across a restart only because the parser reads it back. No
  -- UNIQUE on it: '' is the value on every non-contest row, and turning the merge's skip into
  -- an insert error is a behaviour change, not a port.
  contest_qid     TEXT,
  stx INTEGER, stx_string TEXT, srx INTEGER, srx_string TEXT,
  contest_points  INTEGER,
  is_dupe         INTEGER NOT NULL DEFAULT 0,   -- logged and marked, never dropped
  multiplier_key  TEXT
);

-- ⚠️ NOT OPTIONAL, and the single biggest correctness risk in the schema. See the module
-- header: every unmodelled tag in an operator's imported log rides here.
CREATE TABLE IF NOT EXISTS qso_extra (
  qso_id TEXT NOT NULL REFERENCES qso(id) ON DELETE CASCADE,
  name   TEXT NOT NULL,               -- uppercased at parse; original casing already lost
  value  TEXT NOT NULL,
  PRIMARY KEY (qso_id, name)
);

-- Replaces the four per-service column groups.
CREATE TABLE IF NOT EXISTS qso_upload (
  qso_id  TEXT NOT NULL REFERENCES qso(id) ON DELETE CASCADE,
  service TEXT NOT NULL,              -- lotw | eqsl | qrz | clublog
  outcome TEXT NOT NULL,
  when_unix INTEGER,
  detail  TEXT,
  PRIMARY KEY (qso_id, service)
);

-- ─── Contest ────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS contest_session (
  id TEXT PRIMARY KEY NOT NULL,
  event_id TEXT NOT NULL, contest_id TEXT NOT NULL, rules_year INTEGER,
  role TEXT, start_unix INTEGER NOT NULL,
  entry_category TEXT, category_power TEXT, category_assisted TEXT,
  transmitter_id TEXT, my_location TEXT, upload_policy TEXT,
  next_serial INTEGER NOT NULL DEFAULT 1   -- never persisted today, so a restart restarts at 1
);

-- Child table WITH `ord`: the sent exchange needs its order, and the domain is needed by both
-- the scorer and the ADIF writer — columns would drop both. `contest_id` is DENORMALISED so
-- the dupe lookup is one index hit on a per-keystroke path, not a join.
CREATE TABLE IF NOT EXISTS contest_exchange (
  qso_id TEXT NOT NULL REFERENCES qso(id) ON DELETE CASCADE,
  contest_id TEXT NOT NULL,
  side   TEXT NOT NULL CHECK (side IN ('tx','rx')),
  ord    INTEGER NOT NULL,
  slot   TEXT NOT NULL,
  domain TEXT,                        -- always NULL from the general log, whose pairs carry none
  raw    TEXT NOT NULL,
  PRIMARY KEY (qso_id, side, ord)
);

-- The exchange's own DIRECTED standard columns ([`ContestFields::adif`]) — `ARRL_SECT` and
-- `CLASS` for the section and class they sent, `MY_ARRL_SECT` for the one I sent, `STATE` for
-- a QSO party. `contest::adif::directed_columns` is the ONLY producer, at merge time, which is
-- why no fixture built by `parse_adif` can prove this table works: the parser never populates
-- the field, by design.
--
-- ⚠️ **Its own table rather than `qso_extra`, and that is correctness, not tidiness.** Seven of
-- the fifteen tags `directed_columns` can emit — `NAME`, `QTH`, `RST_RCVD`, `RST_SENT`,
-- `STATE`, `STX`, `SRX` — are tags the PARSER MODELS into their own fields, so the passthrough
-- is precisely where they do not belong. Putting them there would (1) risk a `PRIMARY KEY
-- (qso_id, name)` collision with a name already in `extra`, which fails the whole transaction
-- and LOSES THE CONTACT; (2) re-emit them last and `ORDER BY name`, instead of inside the
-- contest block in the role order `directed_columns` chose; and (3) blind the ADIF writer's
-- duplicate-`STATE` guard, which reads `contest.adif` and would then write the tag twice —
-- the "undefined territory" LoTW/TQSL reject.
--
-- Keyed on `ord`, never on `tag`: `ord` is unique by construction, so a ruleset that declared
-- one tag twice cannot violate the key, and no colliding row can destroy a QSO.
CREATE TABLE IF NOT EXISTS qso_contest_adif (
  qso_id TEXT NOT NULL REFERENCES qso(id) ON DELETE CASCADE,
  ord    INTEGER NOT NULL,
  tag    TEXT NOT NULL,
  value  TEXT NOT NULL,
  PRIMARY KEY (qso_id, ord)
);
"#;

/// The secondary indexes, one statement each: the store's lookup paths, and none of them a rule.
/// Every uniqueness rule is a table's own `PRIMARY KEY`, which [`DDL`] creates with its table, so
/// a store without these holds exactly the same log — it is only slower to search.
///
/// ⭐ **Apart from [`DDL`] for the one-time conversion.** Kept up row by row while a lifetime log
/// goes in, every chunk's commit rewrote every page of all nine that it touched; built once over
/// the finished table, each is one sorted pass. Measured at 150,000 contacts, with nothing else
/// changed: 8.65 GB written and 21.6 s in the store, against 3.8 GB and 13.1 s with the indexes
/// built at the end (0.5 s of that is the nine builds). [`LogDb::open`] builds them as it always
/// has. The conversion opens with [`LogDb::open_for_conversion`], which does not, and calls
/// [`LogDb::build_indexes`] after its last contact and before it marks itself done.
///
/// `IF NOT EXISTS` on every one, so building them is safe to repeat: a conversion killed half-way
/// through them builds only the rest when it resumes. ⚠️ Do not reformat them. SQLite records
/// each statement's text from the index name onward in `sqlite_master`, so as written a store
/// built by this list carries the same schema, byte for byte, as one built before the split.
const INDEXES: [&str; 9] = [
    "CREATE INDEX IF NOT EXISTS qso_worked   ON qso(call_norm, band_norm, mode_norm)",
    "CREATE INDEX IF NOT EXISTS qso_dedup    ON qso(base_call, band_norm, mode_norm, when_unix)",
    "CREATE INDEX IF NOT EXISTS qso_recent   ON qso(when_unix DESC)",
    "CREATE INDEX IF NOT EXISTS qso_confirm  ON qso(call_norm, band_norm, mode_class, utc_day)",
    "CREATE INDEX IF NOT EXISTS qso_entity   ON qso(entity, band_norm, mode_norm)",
    "CREATE INDEX IF NOT EXISTS qso_callhist ON qso(call_norm, when_unix DESC)",
    "CREATE INDEX IF NOT EXISTS qso_dupekey  ON qso(contest_id, dupe_key)",
    "CREATE INDEX IF NOT EXISTS cx_dupe      ON contest_exchange(contest_id, slot, raw)",
    "CREATE INDEX IF NOT EXISTS up_service   ON qso_upload(service, when_unix DESC)",
];

/// The reads behind the UI's log questions (SPEC-2 v3 C17a): the order vectors, rows by rowid,
/// the call index.
mod query_reads;
pub use query_reads::{ENTITY_COLUMNS, ORDER_COLUMNS};

/// Every `qso` column the store writes, **in bind order**.
///
/// ⭐ The write list and the read list are ONE list, exactly as `contest_fields` and
/// `parse_contest` are: [`bind_qso`] pushes values in this order and [`record_from_row`] reads
/// them in this order, and both statements are built from this constant, so a column added to
/// one direction and not the other cannot compile into a silent drop.
const QSO_COLUMNS: &[&str] = &[
    "id",
    "call",
    "band",
    "mode",
    "freq_mhz",
    "freq_rx_mhz",
    "when_unix",
    "time_off_unix",
    "time_known",
    "grid",
    "country",
    "state",
    "name",
    "qth",
    "comment",
    "notes",
    "rst_sent",
    "rst_rcvd",
    "tx_power",
    "dxcc",
    "prop_mode",
    "sat_name",
    "operator",
    "station_callsign",
    "my_grid",
    "my_rig",
    "qsl_card_rcvd_raw",
    "lotw_rcvd_raw",
    "eqsl_rcvd_raw",
    "qrz_status_raw",
    "qsl_sent",
    "qsl_sent_via",
    "qsl_sent_date_unix",
    "qsl_sent_cleared_unix",
    "credit_granted",
    "credit_submitted",
    "ota_my_program",
    "ota_my_ref",
    "ota_their_program",
    "ota_their_ref",
    "ota_iota",
    "call_norm",
    "base_call",
    "band_norm",
    "mode_norm",
    "mode_class",
    "dedup_mode",
    "utc_day",
    "grid4",
    "entity",
    "cq_zone",
    "is_sat",
    "dxcc_credited",
    "operator_norm",
    "dupe_key",
    "contest_id",
    "contest_session_id",
    "contest_qid",
    "stx",
    "stx_string",
    "srx",
    "srx_string",
    "contest_points",
    "is_dupe",
    "multiplier_key",
];

/// What went wrong. Two kinds, because a caller needs to tell them apart: SQLite said no, or
/// the record was not one the store can address.
#[derive(Debug)]
pub enum Error {
    /// SQLite's own.
    Sql(rusqlite::Error),
    /// A record with no [`RecordId`]. Every record the log HOLDS carries one; a row without an
    /// id could never be addressed again, which is the failure the id exists to end, so the
    /// store refuses it rather than writing it.
    Unidentified {
        /// The callsign, so the operator's log can be pointed at.
        call: String,
    },
    /// The database was written by a different schema. Reading it under this build's
    /// assumptions would be a silent partial read of a lifetime log; migration is C5's.
    SchemaVersion {
        /// What the file says.
        found: i64,
        /// What this build writes.
        expected: i64,
    },
    /// A `u64` that will not fit SQLite's SIGNED INTEGER. `as i64` would WRAP it — `u64::MAX`
    /// becomes −1 — so the value is refused instead of being stored as a different number.
    /// The record columns answer this with NULL (see `stamp`); a watermark cannot, because a
    /// watermark that reads back smaller than it was written is a cache key that lies.
    OutOfRange {
        /// Which value.
        what: &'static str,
        /// What it was.
        value: u64,
    },
    /// A copy of the database ([`copy_database`]) could not be PROVED to hold what the
    /// original holds, so it was not put in place. The original is untouched.
    Copy {
        /// What the proof found, in words an operator can be shown.
        reason: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sql(e) => write!(f, "logbook database: {e}"),
            Error::Unidentified { call } => {
                write!(f, "record for {call} has no id and cannot be stored")
            }
            Error::SchemaVersion { found, expected } => write!(
                f,
                "logbook database is schema version {found}, this build writes {expected}"
            ),
            Error::OutOfRange { what, value } => {
                write!(
                    f,
                    "logbook database: {what} ({value}) will not fit an INTEGER"
                )
            }
            Error::Copy { reason } => write!(f, "logbook database copy: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Sql(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sql(e)
    }
}

/// This module's result.
pub type Result<T> = std::result::Result<T, Error>;

/// The two derived columns tempo-core cannot compute for itself.
///
/// cty.dat lives in `propagation`, which tempo-core deliberately does not depend on (the same
/// boundary that makes `reconcile::mode_class` a hand-aligned copy of
/// `propagation::ModeClass::from_adif`). The award identity is still written AT INSERT, as the
/// schema requires — the caller that owns the table hands it in. [`Default`] is "not resolved",
/// which is honest: an unresolved entity is NULL, never a guess from the `COUNTRY` text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Resolved<'a> {
    /// cty.dat's entity NAME. **The award identity** — never the stored `COUNTRY` text, which
    /// spells the same entity differently depending on which service wrote it.
    pub entity: Option<&'a str>,
    /// cty.dat's CQ zone, for WAZ.
    pub cq_zone: Option<u8>,
}

/// One row on its way to the store: the record AS IT NOW STANDS, plus the two values
/// tempo-core cannot derive for itself (see [`Resolved`], which is the borrowing form this
/// owns).
///
/// It owns its record and its resolution because it outlives the caller's lock — it goes on
/// the writer's queue, and the caller returns to the radio loop. The record is an
/// `Arc<QsoRecord>`, which is the pointer the log already holds, so putting a row on that
/// queue copies a pointer and not 840 bytes.
#[derive(Debug, Clone)]
pub struct RowWrite {
    /// The record, exactly as the log holds it after the change.
    pub rec: Arc<QsoRecord>,
    /// cty.dat's entity NAME. **The award identity** — never the stored `COUNTRY` text.
    pub entity: Option<String>,
    /// cty.dat's CQ zone.
    pub cq_zone: Option<u8>,
}

impl RowWrite {
    /// A row whose entity and zone are not resolved. Honest, not lossy: an unresolved entity
    /// is NULL, never a guess from the `COUNTRY` text.
    pub fn new(rec: Arc<QsoRecord>) -> RowWrite {
        RowWrite {
            rec,
            entity: None,
            cq_zone: None,
        }
    }

    /// A row with cty.dat's answer attached.
    pub fn resolved(rec: Arc<QsoRecord>, r: Resolved<'_>) -> RowWrite {
        RowWrite {
            rec,
            entity: r.entity.map(str::to_string),
            cq_zone: r.cq_zone,
        }
    }

    /// The row's id, or the refusal it is written under. Every record the log HOLDS carries
    /// one; a row without one could never be addressed again.
    pub fn id(&self) -> Result<RecordId> {
        self.rec.id.ok_or_else(|| Error::Unidentified {
            call: self.rec.call.clone(),
        })
    }

    fn resolution(&self) -> Resolved<'_> {
        Resolved {
            entity: self.entity.as_deref(),
            cq_zone: self.cq_zone,
        }
    }
}

/// The five watermarks, as `log_meta` holds them.
///
/// They survive into SQLite rather than being replaced by a bare "the table changed" because
/// every cache over the log keys on one of them — see [`super::OpClass`] for which class moves
/// which. Written in the SAME transaction as the rows they describe, so a watermark read back
/// out of the database names the state the database is actually in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Watermarks {
    /// Moves on every change.
    pub revision: u64,
    /// The last change that was not an append at the end.
    pub content_rev: u64,
    /// The last change a derived index over the rows' content must be rebuilt for.
    pub index_rev: u64,
    /// The last change that moved a row's identifying fields.
    pub key_rev: u64,
    /// The last change a plan built on an earlier snapshot cannot be rebased over.
    pub shape_rev: u64,
}

impl Watermarks {
    /// The log's watermarks as they stand. Take it under whatever lock guards the log, in the
    /// same breath as the change it describes.
    pub fn of(log: &Logbook) -> Watermarks {
        Watermarks {
            revision: log.revision(),
            content_rev: log.content_rev(),
            index_rev: log.index_rev(),
            key_rev: log.key_rev(),
            shape_rev: log.shape_rev(),
        }
    }

    /// The `log_meta` keys, in a fixed order so a stored database always reads the same way.
    fn pairs(&self) -> [(&'static str, u64); 5] {
        [
            ("revision", self.revision),
            ("content_rev", self.content_rev),
            ("index_rev", self.index_rev),
            ("key_rev", self.key_rev),
            ("shape_rev", self.shape_rev),
        ]
    }
}

/// ONE transaction's worth of change, in the order the statements run: the clear, then the
/// removals, then the rows as they now stand, then the watermarks.
///
/// ⚠️ **A batch is a CHUNK, never a whole import.** §v3.13/R8 condition 1 is binding: one
/// transaction spanning a 150,000-row import holds SQLite's write lock for the length of that
/// import, and a contest QSO logged in the middle of it would wait there. The caller splits
/// its work into batches; [`super::writer::LogWriter`] is the caller that does so and the one
/// that owns the connection.
#[derive(Debug, Default, Clone, Copy)]
pub struct Batch<'a> {
    /// Drop every row FIRST. `LogOp::Clear` is one statement, not 150,000 deletes.
    pub clear: bool,
    /// Rows to drop, by id. Their passthrough, stamps and exchange go with them
    /// (`ON DELETE CASCADE`, which the `foreign_keys` pragma makes real).
    pub remove: &'a [RecordId],
    /// Rows as they NOW stand: inserted if the store does not hold the id, replaced if it
    /// does. ONE path for both, because a store that branches on "is this new?" is a store
    /// whose two branches can disagree.
    pub upsert: &'a [RowWrite],
    /// The watermarks this batch leaves the store at, or `None` for a chunk that does not
    /// finish its change. A watermark must never run AHEAD of the rows — a cache keyed on one
    /// that does serves a stale answer, where one that lags only costs a rebuild.
    pub marks: Option<Watermarks>,
}

/// The logbook's database.
#[derive(Debug)]
pub struct LogDb {
    conn: Connection,
}

impl LogDb {
    /// Open `path`, creating and initialising it if it is not there yet — every table and every
    /// index, building any index the file does not have.
    pub fn open(path: &Path) -> Result<LogDb> {
        let db = LogDb::init(Connection::open(path)?)?;
        db.build_indexes(&mut |_, _| {})?;
        Ok(db)
    }

    /// An in-memory database, for tests and for a caller that wants the schema without a file.
    pub fn open_in_memory() -> Result<LogDb> {
        let db = LogDb::init(Connection::open_in_memory()?)?;
        db.build_indexes(&mut |_, _| {})?;
        Ok(db)
    }

    /// Open `path` for the ONE-TIME conversion of an existing `log.adi` ([`super::migrate`]) —
    /// never for ordinary use. It differs from [`LogDb::open`] in three ways, all confined to
    /// this connection and gone when it closes:
    ///
    /// - **No indexes.** The tables only; [`INDEXES`] says why, and the conversion builds them
    ///   with [`LogDb::build_indexes`] once every contact is in. It also means this open never
    ///   WRITES to a store that already has its tables — which matters to the other caller, the
    ///   check a second window makes while the first may still be converting: an ordinary open
    ///   there would build the missing indexes, holding the write lock for as long as that
    ///   takes, out from under the first window's next chunk.
    /// - **`synchronous = NORMAL`**, where the store's own setting is SQLite's default, FULL. In
    ///   WAL mode NORMAL still cannot corrupt the database, and a crashed process loses nothing
    ///   it committed; what it gives up is that an OS crash or a power cut can lose the LAST few
    ///   commits. For a conversion that costs nothing: its watermark is the store's own row
    ///   count, so a lost chunk is simply converted again, from `log.adi`, which the conversion
    ///   never changes. FULL waits for the disk at every commit; measured, that is worth only
    ///   2–5% on a fast disk with the conversion's few big commits, and more where a flush is
    ///   slow. ⛔ Never `OFF`: that one can corrupt the database.
    /// - **A bigger page cache**, [`CONVERSION_CACHE_KIB`], so the key pages a big chunk keeps
    ///   coming back to stay in memory instead of being evicted and read back.
    pub fn open_for_conversion(path: &Path) -> Result<LogDb> {
        let conn = Connection::open(path)?;
        // Before `init`, so every write this connection makes — the version stamp and the tables
        // of a new store included — is made under them. Both are this connection's alone.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "cache_size", -CONVERSION_CACHE_KIB)?;
        LogDb::init(conn)
    }

    /// Open `path` to READ the log — the connection a [`super::reader::LogReader`] hands out,
    /// never the writer's.
    ///
    /// - **It never creates a database**: no `SQLITE_OPEN_CREATE`, so a path that is not one
    ///   is an error, and the file is left as it was.
    /// - **It never writes**: `PRAGMA query_only`, so SQLite itself refuses any statement that
    ///   would change the file — not this module remembering not to issue one. It is opened
    ///   read-write all the same, because a read-only connection to a WAL database cannot open
    ///   at all when the WAL index is absent, and this one may be the first to arrive.
    /// - **It refuses a store of another schema version**, as [`LogDb::open`] does — and,
    ///   unlike it, never stamps one on a database that has none.
    pub fn open_reader(path: &Path) -> Result<LogDb> {
        use rusqlite::OpenFlags;
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))?;
        conn.pragma_update(None, "query_only", true)?;
        let db = LogDb { conn };
        match db.meta("schema_version")? {
            Some(SCHEMA_VERSION) => Ok(db),
            found => Err(Error::SchemaVersion {
                found: found.unwrap_or(0),
                expected: SCHEMA_VERSION,
            }),
        }
    }

    /// Build every index in [`INDEXES`] the store does not have yet, telling `progress` how many
    /// of how many are built: `(0, n)` first, then after each one. Each index is its own
    /// transaction, and an index that is already there is left as it is — so a build cut short
    /// by a crash is finished by calling this again.
    pub fn build_indexes(&self, progress: &mut dyn FnMut(usize, usize)) -> Result<()> {
        progress(0, INDEXES.len());
        for (built, sql) in INDEXES.iter().enumerate() {
            self.conn.execute_batch(sql)?;
            progress(built + 1, INDEXES.len());
        }
        Ok(())
    }

    /// The pragmas, the schema, and the version check — the three things every open does.
    fn init(conn: Connection) -> Result<LogDb> {
        // WAL: a reader never blocks the writer, and a crash cannot leave a torn log. It is a
        // property of the FILE, so it survives the connection; an in-memory database cannot
        // have it and answers "memory", which is not an error but the only journal it has.
        let _journal: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
        // Two connections WILL contend — the writer thread and whatever reads next to it — and
        // the alternative to waiting is an immediate SQLITE_BUSY the caller has to retry by
        // hand. Bounded so a stuck writer surfaces as an error rather than a hang.
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))?;
        // ON, so `ON DELETE CASCADE` on the four child tables is real: deleting a QSO takes
        // its passthrough, its upload stamps, its exchange and its directed contest columns
        // with it. Off (SQLite's default) those clauses are decoration and a delete orphans
        // every child row.
        conn.pragma_update(None, "foreign_keys", true)?;
        // ⚠️ The version is read and claimed BEFORE the rest of the schema is applied. A
        // database a LATER build wrote is refused, and refusing it has to mean touching
        // nothing: running this build's `CREATE TABLE IF NOT EXISTS` first would leave stray
        // empty v1 tables in an operator's newer database on the way to saying no.
        conn.execute_batch(META_DDL)?;
        let db = LogDb { conn };
        match db.meta("schema_version")? {
            None => db.set_meta("schema_version", SCHEMA_VERSION)?,
            Some(SCHEMA_VERSION) => {}
            Some(found) => {
                return Err(Error::SchemaVersion {
                    found,
                    expected: SCHEMA_VERSION,
                })
            }
        }
        db.conn.execute_batch(DDL)?;
        Ok(db)
    }

    /// One `log_meta` value — a watermark, or the schema version. `None` means the key is not
    /// there; a database that could not be READ is an error, never a `None`. The two are not
    /// the same answer, and conflating them would let [`LogDb::init`] stamp its own version
    /// onto a database it had in fact failed to read.
    pub fn meta(&self, k: &str) -> Result<Option<i64>> {
        match self
            .conn
            .query_row("SELECT v FROM log_meta WHERE k = ?1", [k], |r| r.get(0))
        {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write one `log_meta` value.
    pub fn set_meta(&self, k: &str, v: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO log_meta (k, v) VALUES (?1, ?2)
             ON CONFLICT(k) DO UPDATE SET v = excluded.v",
            params![k, v],
        )?;
        Ok(())
    }

    /// `PRAGMA data_version` on this connection: it moves exactly when a DIFFERENT connection —
    /// another process's writer, sharing this data folder — commits. This connection's own
    /// commits never move it, which is what lets the writer thread tell "someone else wrote the
    /// log" apart from its own work without any bookkeeping of its own.
    pub fn data_version(&self) -> Result<i64> {
        data_version(&self.conn)
    }

    /// The file this database was opened from, or `None` for an in-memory one.
    pub fn path(&self) -> Option<std::path::PathBuf> {
        self.conn
            .path()
            .filter(|p| !p.is_empty())
            .map(std::path::PathBuf::from)
    }

    /// How many records the store holds.
    ///
    /// Written for [`super::migrate`], where it IS the resume watermark: a migration inserts
    /// in file order, one transaction per chunk, so a crash leaves whole chunks and the count
    /// names exactly how far it got. A separately stored counter could disagree with the rows
    /// — it would be a second transaction — and a watermark that runs ahead of the data is how
    /// a resume drops records.
    pub fn row_count(&self) -> Result<u64> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM qso", [], |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// Store one record.
    pub fn insert<'a>(&mut self, rec: &'a QsoRecord, resolved: Resolved<'a>) -> Result<()> {
        self.insert_all([(rec, resolved)])
    }

    /// Store many records in ONE transaction.
    ///
    /// One transaction per call, so a caller with a very large import chunks it and calls this
    /// repeatedly — and how big a chunk may be depends on who could be waiting for the write
    /// lock meanwhile. Beside a live session a contest QSO could, and its `busy_timeout` is
    /// measured in milliseconds: that is the writer thread's rule, and its chunks stay small.
    /// The one-time conversion runs before any session, with nothing waiting, and commits tens
    /// of thousands of contacts at a time (`migrate::CHUNK_ROWS` says why).
    pub fn insert_all<'a, I>(&mut self, rows: I) -> Result<()>
    where
        I: IntoIterator<Item = (&'a QsoRecord, Resolved<'a>)>,
    {
        let tx = self.conn.transaction()?;
        {
            let mut qso = tx.prepare(&insert_sql())?;
            let mut extra =
                tx.prepare("INSERT INTO qso_extra (qso_id, name, value) VALUES (?1, ?2, ?3)")?;
            let mut upload = tx.prepare(
                "INSERT INTO qso_upload (qso_id, service, outcome, when_unix, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut exch = tx.prepare(
                "INSERT INTO contest_exchange (qso_id, contest_id, side, ord, slot, domain, raw)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            let mut cadif = tx.prepare(
                "INSERT INTO qso_contest_adif (qso_id, ord, tag, value) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (rec, resolved) in rows {
                let id = rec
                    .id
                    .ok_or_else(|| Error::Unidentified {
                        call: rec.call.clone(),
                    })?
                    .to_string();
                qso.execute(params_from_iter(bind_qso(&id, rec, resolved)))?;
                bind_children(&mut extra, &mut upload, &mut exch, &mut cadif, &id, rec)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Apply one [`Batch`] in ONE transaction: all of it lands, or none of it does.
    ///
    /// This is the statement half of the writer thread ([`super::writer`]) and the only write
    /// path that can CHANGE or REMOVE a row — [`LogDb::insert_all`] is the bulk load, and its
    /// plain `INSERT` is deliberately the one that fails on a duplicate id.
    ///
    /// **An upsert replaces the row and all four of its child tables.** It has to: a record's
    /// passthrough, stamps, exchange and directed contest columns are lists, and a list is
    /// changed by being rewritten, not by having a row updated. The four deletes are index
    /// probes that find nothing for a row being inserted for the first time, which is why one
    /// path serves both cases.
    pub fn apply(&mut self, b: Batch<'_>) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            if b.clear {
                // One statement. The children go with it — `ON DELETE CASCADE` fires on a
                // parent row deleted by any means, and `foreign_keys` is ON.
                tx.execute("DELETE FROM qso", [])?;
            }
            if !b.remove.is_empty() {
                let mut del = tx.prepare("DELETE FROM qso WHERE id = ?1")?;
                for id in b.remove {
                    del.execute([id.to_string()])?;
                }
            }
            if !b.upsert.is_empty() {
                let mut qso = tx.prepare(&upsert_sql())?;
                let mut drop_extra = tx.prepare("DELETE FROM qso_extra WHERE qso_id = ?1")?;
                let mut drop_upload = tx.prepare("DELETE FROM qso_upload WHERE qso_id = ?1")?;
                let mut drop_exch = tx.prepare("DELETE FROM contest_exchange WHERE qso_id = ?1")?;
                let mut drop_cadif =
                    tx.prepare("DELETE FROM qso_contest_adif WHERE qso_id = ?1")?;
                let mut extra =
                    tx.prepare("INSERT INTO qso_extra (qso_id, name, value) VALUES (?1, ?2, ?3)")?;
                let mut upload = tx.prepare(
                    "INSERT INTO qso_upload (qso_id, service, outcome, when_unix, detail)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )?;
                let mut exch = tx.prepare(
                    "INSERT INTO contest_exchange (qso_id, contest_id, side, ord, slot, domain, raw)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                let mut cadif = tx.prepare(
                    "INSERT INTO qso_contest_adif (qso_id, ord, tag, value)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                for w in b.upsert {
                    let id = w.id()?.to_string();
                    qso.execute(params_from_iter(bind_qso(&id, &w.rec, w.resolution())))?;
                    drop_extra.execute([&id])?;
                    drop_upload.execute([&id])?;
                    drop_exch.execute([&id])?;
                    drop_cadif.execute([&id])?;
                    bind_children(&mut extra, &mut upload, &mut exch, &mut cadif, &id, &w.rec)?;
                }
            }
            if let Some(m) = b.marks {
                let mut set = tx.prepare(
                    "INSERT INTO log_meta (k, v) VALUES (?1, ?2)
                     ON CONFLICT(k) DO UPDATE SET v = excluded.v",
                )?;
                for (k, v) in m.pairs() {
                    let v = i64::try_from(v).map_err(|_| Error::OutOfRange {
                        what: "watermark",
                        value: v,
                    })?;
                    set.execute(params![k, v])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Every record, in the order they were stored.
    ///
    /// `rowid` order is arrival order, which is the same thing `log.adi`'s file order carries
    /// today — the log is not sorted by time, and an import appends.
    pub fn load_all(&self) -> Result<Vec<QsoRecord>> {
        self.load_all_observed(&mut || {})
    }

    /// [`Self::load_all`], with `between` run after the child tables are read and before the
    /// rows are — the seam a test uses to commit in exactly the window a torn read would open.
    fn load_all_observed(&self, between: &mut dyn FnMut()) -> Result<Vec<QsoRecord>> {
        self.in_one_snapshot(|db| db.read_all(between))
    }

    /// Run `f` inside ONE read transaction, so every statement it runs sees the same committed
    /// state of the store — or, inside a transaction already open on this connection, within
    /// that one.
    ///
    /// ⛔ **Without it a load of several tables is several pictures.** A statement run on its
    /// own sees the store as it stands when THAT statement begins, so a commit landing between
    /// the child tables and the rows returned a contact without its children: its foreign tags
    /// and upload stamps were read before it existed. A reader that then wrote the contact back
    /// from memory took them out of the store for good. In WAL mode a read transaction takes
    /// its picture at its first read and keeps it until it ends, and it never blocks a writer.
    pub(super) fn in_one_snapshot<T>(&self, f: impl FnOnce(&LogDb) -> Result<T>) -> Result<T> {
        if !self.conn.is_autocommit() {
            return f(self);
        }
        let snapshot = self.conn.unchecked_transaction()?;
        let out = f(self);
        // Read only: ending it by rollback writes nothing and releases the picture.
        drop(snapshot);
        out
    }

    /// Every record, in the order they were stored — [`Self::load_all`]'s body, which runs it in
    /// one read transaction. `between` is its test seam.
    fn read_all(&self, between: &mut dyn FnMut()) -> Result<Vec<QsoRecord>> {
        self.decode(Rows::All, between)
    }

    /// The records these ids name, in the order they were stored — each exactly as
    /// [`Self::load_all`] hands it back, through the same decoder, read in ONE read transaction
    /// ([`Self::in_one_snapshot`]). An id the store does not hold is simply absent, an id asked
    /// for twice comes back once, and the order the ids were asked in is irrelevant: the answer
    /// is always in log order.
    pub fn rows_by_ids(&self, ids: &[RecordId]) -> Result<Vec<QsoRecord>> {
        self.in_one_snapshot(|db| {
            let mut rowids = std::collections::BTreeSet::new();
            for chunk in ids.chunks(ROWS_PER_STATEMENT) {
                let places: Vec<String> = (1..=chunk.len()).map(|i| format!("?{i}")).collect();
                let mut stmt = db.conn.prepare(&format!(
                    "SELECT rowid FROM qso WHERE id IN ({})",
                    places.join(", ")
                ))?;
                let texts: Vec<String> = chunk.iter().map(RecordId::to_string).collect();
                let mut rows = stmt.query(params_from_iter(texts.iter()))?;
                while let Some(row) = rows.next()? {
                    rowids.insert(row.get::<_, i64>(0)?);
                }
            }
            let rowids: Vec<i64> = rowids.into_iter().collect();
            let mut out = Vec::with_capacity(rowids.len());
            for chunk in rowids.chunks(ROWS_PER_STATEMENT) {
                out.extend(db.decode(Rows::At(chunk), &mut || {})?);
            }
            Ok(out)
        })
    }

    /// THE decoder: the records `rows` names, in log order, each assembled from its `qso` row
    /// and its four child tables. [`Self::load_all`] and [`Self::rows_by_ids`] both come
    /// here, so a record cannot read back one way through one and another way through the
    /// other. Run inside a read transaction by its callers; `between` is the load's test seam.
    fn decode(&self, rows: Rows<'_>, between: &mut dyn FnMut()) -> Result<Vec<QsoRecord>> {
        // The rows a child table's sweep is limited to: none, or the ones at `rows`' rowids.
        // The rowids are integers this module read out of the store itself, written as
        // literals — there is no text in them to escape.
        let (of_rows, of_children) = match rows {
            Rows::All => (String::new(), String::new()),
            Rows::At(rowids) => {
                let list = rowids
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                (
                    format!("WHERE rowid IN ({list})"),
                    format!("WHERE qso_id IN (SELECT id FROM qso WHERE rowid IN ({list}))"),
                )
            }
        };
        // The children come back in three sweeps rather than three queries per record: a
        // lifetime log is hundreds of thousands of rows, and 3N statement executions to
        // rebuild what 3 can is the shape of the problem this whole programme is removing.
        let mut extra = self.load_extra(&of_children)?;
        let mut upload = self.load_uploads(&of_children)?;
        let mut exchange = self.load_exchange(&of_children)?;
        let mut directed = self.load_contest_adif(&of_children)?;
        between();

        let sql = format!(
            "SELECT {} FROM qso {of_rows} ORDER BY rowid",
            QSO_COLUMNS.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let mut rec = record_from_row(row)?;
            // `remove`: a `qso` row's id is its primary key, so no other row can want these.
            rec.extra = extra.remove(&id).unwrap_or_default();
            if let Some(u) = upload.remove(&id) {
                rec.upload = u;
            }
            if let (Some(c), Some((sent, rcvd))) =
                (rec.contest.as_deref_mut(), exchange.remove(&id))
            {
                c.sent = sent;
                c.rcvd = rcvd;
            }
            if let (Some(c), Some(adif)) = (rec.contest.as_deref_mut(), directed.remove(&id)) {
                c.adif = adif;
            }
            // The same emptiness test `parse_contest` applies, for the same reason: a `Some`
            // here is a claim that this contact belonged to a contest, and an all-empty block
            // would make that claim of every ordinary QSO.
            if rec.contest.as_deref().is_some_and(contest_is_empty) {
                rec.contest = None;
            }
            out.push(rec);
        }
        Ok(out)
    }

    /// The passthrough, `ORDER BY name` — which is what reproduces `extra.sort()` byte for
    /// byte, SQLite's `BINARY` collation and Rust's `String` ordering both being bytewise over
    /// UTF-8, and the table's primary key making the name unique within a record.
    fn load_extra(&self, of: &str) -> Result<HashMap<String, Vec<(String, String)>>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT qso_id, name, value FROM qso_extra {of} ORDER BY qso_id, name"
        ))?;
        let mut rows = stmt.query([])?;
        let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
        while let Some(row) = rows.next()? {
            out.entry(row.get(0)?)
                .or_default()
                .push((row.get(1)?, row.get(2)?));
        }
        Ok(out)
    }

    /// The directed contest columns, `ORDER BY ord` — which is the ROLE order
    /// `contest::adif::directed_columns` chose (everything I sent, then everything they
    /// sent). Alphabetical, the way [`Self::load_extra`] reads, would reorder the contest
    /// block of every export.
    fn load_contest_adif(&self, of: &str) -> Result<HashMap<String, Vec<(String, String)>>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT qso_id, tag, value FROM qso_contest_adif {of} ORDER BY qso_id, ord"
        ))?;
        let mut rows = stmt.query([])?;
        let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
        while let Some(row) = rows.next()? {
            out.entry(row.get(0)?)
                .or_default()
                .push((row.get(1)?, row.get(2)?));
        }
        Ok(out)
    }

    fn load_uploads(&self, of: &str) -> Result<HashMap<String, UploadState>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT qso_id, service, outcome, when_unix, detail FROM qso_upload {of}"
        ))?;
        let mut rows = stmt.query([])?;
        let mut out: HashMap<String, UploadState> = HashMap::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let service: String = row.get(1)?;
            let code: String = row.get(2)?;
            // A code this build does not know is dropped, exactly as `take_upload` drops it on
            // the ADIF side — the filter that cleans a store an earlier build poisoned.
            let Some(outcome) = UploadOutcome::from_code(&code) else {
                continue;
            };
            let detail: Option<String> = row.get(4)?;
            let st = UploadStatus {
                outcome,
                when_unix: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                detail: detail.as_deref().and_then(UploadDetail::from_code),
            };
            let slot = out.entry(id).or_default();
            match service.as_str() {
                "lotw" => slot.lotw = Some(st),
                "eqsl" => slot.eqsl = Some(st),
                "qrz" => slot.qrz = Some(st),
                "clublog" => slot.clublog = Some(st),
                _ => {}
            }
        }
        Ok(out)
    }

    /// The exchange, as `(sent, rcvd)` pairs per record, each in its own `ord` order — the
    /// order the sent exchange is rendered in, which columns would have dropped.
    #[allow(clippy::type_complexity)]
    fn load_exchange(
        &self,
        of: &str,
    ) -> Result<HashMap<String, (Vec<(String, String)>, Vec<(String, String)>)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT qso_id, side, slot, raw FROM contest_exchange {of} ORDER BY qso_id, side, ord"
        ))?;
        let mut rows = stmt.query([])?;
        let mut out: HashMap<String, (Vec<_>, Vec<_>)> = HashMap::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let side: String = row.get(1)?;
            let pair = (row.get(2)?, row.get(3)?);
            let slot = out.entry(id).or_default();
            if side == "tx" {
                slot.0.push(pair);
            } else {
                slot.1.push(pair);
            }
        }
        Ok(out)
    }
}

/// The database's write lock, held the way ANOTHER process holds it while it writes — until
/// this value is dropped. Every writer that wants the lock meanwhile waits (`busy_timeout`),
/// which is exactly the state "a write that is taking its time" puts the log in.
///
/// For the tests that must show nothing waits on that state under the engine lock, and for a
/// diagnosis that needs to reproduce it; nothing in the app takes it.
pub struct WriteHold {
    conn: Connection,
}

impl WriteHold {
    /// Take the lock on the database at `path` (which must exist).
    pub fn take(path: &Path) -> Result<WriteHold> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))?;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        Ok(WriteHold { conn })
    }
}

impl Drop for WriteHold {
    fn drop(&mut self) {
        let _ = self.conn.execute_batch("ROLLBACK");
    }
}

/// How many times [`copy_database`] takes its picture before giving up because the original
/// kept changing under it. Each attempt is a whole copy, so this is a bound on work, not a wait.
const COPY_ATTEMPTS: usize = 3;

/// Copy the database at `src` to `dst` THROUGH SQLite, and prove the copy holds exactly what the
/// original holds before it takes the name `dst`. Returns the copy's size in bytes.
///
/// ⛔ **Why not `fs::copy`.** In WAL mode a COMMITTED contact lives in `src-wal` until a
/// checkpoint folds it into the main file, so the main file alone is not the log; and copying
/// the main file, the `-wal` and the `-shm` one after another while another connection writes
/// is not one consistent picture of anything. `VACUUM INTO` reads the database as ONE
/// transaction — WAL included — and writes a fresh, self-contained file, so neither sidecar
/// needs carrying at all.
///
/// **The proof.** The copy is attached and compared with the original inside one read
/// transaction: SQLite's own `integrity_check` on the copy, the same tables, and every table's
/// rows equal value for value IN `rowid` ORDER (the order the store hands records back in). The
/// two pictures — the one `VACUUM INTO` took and the one the comparison reads — are the same
/// picture only if nothing committed between them, which `PRAGMA data_version` answers: it
/// moves exactly when ANOTHER connection commits. If it moved, the comparison proves nothing
/// either way, so the attempt is thrown away and taken again (up to [`COPY_ATTEMPTS`]).
///
/// **What it never does.** It opens `src` without the create flag and without this module's
/// schema set-up, so it cannot make a database that was not there or stamp a version onto one a
/// newer build wrote — it copies whatever database it finds. It never writes to `src`. A copy
/// that is not proved never takes the name `dst`: it is written beside it under a temporary
/// name, and removed.
pub fn copy_database(src: &Path, dst: &Path) -> Result<u64> {
    copy_database_observed(src, dst, &mut |_| {})
}

/// [`copy_database`], with `between(attempt)` run after each picture is taken and before it is
/// compared — the seam a test uses to commit in exactly the window the retry exists for.
fn copy_database_observed(src: &Path, dst: &Path, between: &mut dyn FnMut(usize)) -> Result<u64> {
    use rusqlite::OpenFlags;
    let conn = Connection::open_with_flags(
        src,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))?;
    let dir = dst.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| Error::Copy {
        reason: format!("could not create {}: {e}", dir.display()),
    })?;
    let mut tmp_name = dst.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(format!(".copy-{}.tmp", std::process::id()));
    let tmp = dst.with_file_name(tmp_name);
    let tmp_str = tmp.to_str().ok_or_else(|| Error::Copy {
        reason: format!("{} is not a path SQLite can be given", tmp.display()),
    })?;

    let mut why = String::from("the log kept changing while it was being copied");
    let mut proved = false;
    for attempt in 0..COPY_ATTEMPTS {
        remove_with_sidecars(&tmp);
        let before = data_version(&conn)?;
        conn.execute("VACUUM INTO ?1", [tmp_str])?;
        between(attempt);
        let verdict = verify_copy(&conn, tmp_str)?;
        remove_sidecars(&tmp);
        if data_version(&conn)? != before {
            // Another connection committed between the picture and the comparison: the
            // comparison is about two different logs, so it says nothing. Take it again.
            continue;
        }
        match verdict {
            Ok(()) => {
                proved = true;
                break;
            }
            Err(reason) => {
                why = reason;
                break;
            }
        }
    }
    if !proved {
        remove_with_sidecars(&tmp);
        return Err(Error::Copy { reason: why });
    }

    let placed = (|| -> std::io::Result<u64> {
        // The bytes reach the disk BEFORE the name does — the rename is what publishes the copy.
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&tmp)?;
        f.sync_all()?;
        let len = f.metadata()?.len();
        drop(f);
        // A database that used to have this name may have left its own `-wal`/`-shm` behind.
        // SQLite would read them as belonging to the copy, so they go first.
        remove_sidecars(dst);
        std::fs::rename(&tmp, dst)?;
        super::sync_parent_dir(dst);
        Ok(len)
    })();
    placed.map_err(|e| {
        remove_with_sidecars(&tmp);
        Error::Copy {
            reason: format!("could not put the copy in place at {}: {e}", dst.display()),
        }
    })
}

/// `PRAGMA data_version` — moves exactly when a connection OTHER than `conn` commits.
fn data_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA data_version", [], |r| r.get(0))?)
}

/// `path`'s own `-wal` and `-shm`, if any.
fn remove_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm"] {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        let _ = std::fs::remove_file(path.with_file_name(name));
    }
}

fn remove_with_sidecars(path: &Path) {
    let _ = std::fs::remove_file(path);
    remove_sidecars(path);
}

/// Attach the copy at `copy` and compare it with `conn`'s database in ONE read transaction.
/// The outer `Result` is SQLite failing; the inner one is the verdict, with its reason.
fn verify_copy(conn: &Connection, copy: &str) -> Result<std::result::Result<(), String>> {
    conn.execute("ATTACH DATABASE ?1 AS nexus_copy", [copy])?;
    let verdict = compare_attached(conn);
    // Detached whatever the verdict, or the copy stays open and cannot be renamed on Windows.
    let detached = conn.execute("DETACH DATABASE nexus_copy", []);
    let verdict = verdict?;
    detached?;
    Ok(verdict)
}

fn compare_attached(conn: &Connection) -> Result<std::result::Result<(), String>> {
    let tx = conn.unchecked_transaction()?;
    let integrity: String = tx.query_row("PRAGMA nexus_copy.integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        return Ok(Err(format!(
            "the copy failed SQLite's integrity check ({integrity})"
        )));
    }
    let tables = |schema: &str| -> Result<Vec<String>> {
        let mut stmt = tx.prepare(&format!(
            "SELECT name FROM {schema}.sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        ))?;
        let names = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(names)
    };
    let (ours, theirs) = (tables("main")?, tables("nexus_copy")?);
    if ours != theirs {
        return Ok(Err("the copy does not hold the same tables".into()));
    }
    for table in &ours {
        let quoted = table.replace('"', "\"\"");
        let mut a = tx.prepare(&format!("SELECT * FROM main.\"{quoted}\" ORDER BY rowid"))?;
        let mut b = tx.prepare(&format!(
            "SELECT * FROM nexus_copy.\"{quoted}\" ORDER BY rowid"
        ))?;
        let width = a.column_count();
        if b.column_count() != width {
            return Ok(Err(format!("the copy's {table} table has other columns")));
        }
        let (mut ra, mut rb) = (a.query([])?, b.query([])?);
        let mut n = 0u64;
        loop {
            match (ra.next()?, rb.next()?) {
                (None, None) => break,
                (Some(x), Some(y)) => {
                    for i in 0..width {
                        if x.get_ref(i)? != y.get_ref(i)? {
                            return Ok(Err(format!("row {n} of {table} differs in the copy")));
                        }
                    }
                    n += 1;
                }
                _ => {
                    return Ok(Err(format!(
                        "the copy's {table} table holds a different number of rows"
                    )))
                }
            }
        }
    }
    drop(tx);
    Ok(Ok(()))
}

/// `INSERT INTO qso (…) VALUES (?1, …)`, built from [`QSO_COLUMNS`] so the placeholder count
/// can never drift from the column list.
fn insert_sql() -> String {
    let places: Vec<String> = (1..=QSO_COLUMNS.len()).map(|i| format!("?{i}")).collect();
    format!(
        "INSERT INTO qso ({}) VALUES ({})",
        QSO_COLUMNS.join(", "),
        places.join(", ")
    )
}

/// `INSERT … ON CONFLICT(id) DO UPDATE SET …` over every column but the key, built from
/// [`QSO_COLUMNS`] so neither half can drift from the column list.
///
/// One statement rather than an `INSERT` and an `UPDATE` chosen between: the choice would need
/// a "does the store hold this id?" question whose two answers are two code paths, and the
/// only way those stay in step is by there being one of them.
fn upsert_sql() -> String {
    let places: Vec<String> = (1..=QSO_COLUMNS.len()).map(|i| format!("?{i}")).collect();
    let set: Vec<String> = QSO_COLUMNS
        .iter()
        .skip(1) // `id` is the conflict target, and an upsert never moves a row's identity.
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    format!(
        "INSERT INTO qso ({}) VALUES ({}) ON CONFLICT(id) DO UPDATE SET {}",
        QSO_COLUMNS.join(", "),
        places.join(", "),
        set.join(", ")
    )
}

/// One record's three child tables. Shared by the bulk load and the writer so a tag that
/// survives one path cannot be dropped by the other.
///
/// The caller has already removed any rows this id held — [`LogDb::insert_all`] because the
/// row is new, [`LogDb::apply`] because it deletes them first.
fn bind_children(
    extra: &mut rusqlite::Statement<'_>,
    upload: &mut rusqlite::Statement<'_>,
    exch: &mut rusqlite::Statement<'_>,
    cadif: &mut rusqlite::Statement<'_>,
    id: &str,
    rec: &QsoRecord,
) -> Result<()> {
    // Sorted on the way in only because it is sorted on the record; the read side re-sorts
    // with ORDER BY name and does not trust insertion order.
    for (name, value) in &rec.extra {
        extra.execute(params![id, name, value])?;
    }
    for (service, st) in upload_states(&rec.upload) {
        if let Some(st) = st {
            upload.execute(params![
                id,
                service,
                st.outcome.code(),
                st.when_unix,
                st.detail.map(UploadDetail::code)
            ])?;
        }
    }
    if let Some(c) = rec.contest.as_deref() {
        for (side, pairs) in [("tx", &c.sent), ("rx", &c.rcvd)] {
            for (ord, (slot, raw)) in pairs.iter().enumerate() {
                // `domain` is NULL: the general log's exchange is PAIRS, not the contest
                // journal's triples, and a matched domain has no ADIF representation to have
                // arrived through.
                exch.execute(params![
                    id,
                    &c.contest_id,
                    side,
                    ord as i64,
                    slot,
                    None::<&str>,
                    raw
                ])?;
            }
        }
        // The DIRECTED standard columns, in the role order `directed_columns` chose — `ord`
        // is what carries that order, since the tag cannot (the read side would otherwise
        // have to sort, and alphabetical is not it).
        for (ord, (tag, value)) in c.adif.iter().enumerate() {
            cadif.execute(params![id, ord as i64, tag, value])?;
        }
    }
    Ok(())
}

/// The four connectors that leave a per-QSO stamp, and the token each is stored under.
fn upload_states(u: &UploadState) -> [(&'static str, &Option<UploadStatus>); 4] {
    [
        ("lotw", &u.lotw),
        ("eqsl", &u.eqsl),
        ("qrz", &u.qrz),
        ("clublog", &u.clublog),
    ]
}

/// One record's `qso` row, **in [`QSO_COLUMNS`] order**. Read [`record_from_row`] beside it.
fn bind_qso(id: &str, r: &QsoRecord, resolved: Resolved<'_>) -> Vec<rusqlite::types::Value> {
    use rusqlite::types::Value;
    /// `Option<String>` → TEXT or NULL, and `Some("")` stays an empty string: the reader does
    /// not filter `GRIDSQUARE`, so `<GRIDSQUARE:0>` really does yield `Some("")` and the writer
    /// really does emit it again.
    fn text(v: Option<&str>) -> Value {
        v.map_or(Value::Null, |s| Value::Text(s.to_string()))
    }
    fn int(v: Option<i64>) -> Value {
        v.map_or(Value::Null, Value::Integer)
    }
    fn real(v: Option<f64>) -> Value {
        v.map_or(Value::Null, Value::Real)
    }
    fn flag(v: bool) -> Value {
        Value::Integer(v.into())
    }
    /// A Unix stamp. ⚠️ SQLite's INTEGER is SIGNED, and `as i64` would silently WRAP a `u64`
    /// above `i64::MAX` into a date in 1969 — and `cleared_unix` is a bare `parse()` off an
    /// `APP_TEMPO_QSL_SENT_CLEARED` tag, so a hand-edited or corrupt log really can carry
    /// one. Out of range is NULL instead: the `NOT NULL` on `when_unix` turns that into a
    /// loud refusal, and an optional stamp reads back as absent. Neither is a wrong date.
    fn stamp(v: u64) -> Value {
        i64::try_from(v).map_or(Value::Null, Value::Integer)
    }

    // The channel tokens. ⚠️ The ADIF writer's own fallback is applied here, and it has to be:
    // a legacy in-memory record with all four channels false but `award_confirmed` set writes
    // `LOTW_QSL_RCVD=Y` to `log.adi` and reads back confirmed, so a store that wrote four NULLs
    // would be the one mirror of the two that LOST the confirmation.
    let (card, lotw, eqsl, qrz) = if r.qsl_rcvd.any() {
        (
            r.qsl_rcvd.card,
            r.qsl_rcvd.lotw,
            r.qsl_rcvd.eqsl,
            r.qsl_rcvd.qrz,
        )
    } else {
        (
            false,
            r.award_confirmed,
            r.confirmed && !r.award_confirmed,
            false,
        )
    };
    let token = |on: bool, tok: &str| if on { text(Some(tok)) } else { Value::Null };

    let contest = r.contest.as_deref();
    let c_text =
        |pick: fn(&ContestFields) -> &str| text(contest.map(pick).filter(|s| !s.is_empty()));

    vec![
        Value::Text(id.to_string()),
        Value::Text(r.call.clone()),
        Value::Text(r.band.clone()),
        Value::Text(r.mode.clone()),
        Value::Real(r.freq_mhz),
        real(r.freq_rx_mhz),
        stamp(r.when_unix),
        r.time_off_unix.map_or(Value::Null, stamp),
        flag(r.time_known),
        text(r.grid.as_deref()),
        text(r.country.as_deref()),
        text(r.state.as_deref()),
        text(r.name.as_deref()),
        text(r.qth.as_deref()),
        text(r.comment.as_deref()),
        text(r.notes.as_deref()),
        text(r.rst_sent.as_deref()),
        text(r.rst_rcvd.as_deref()),
        real(r.tx_power),
        int(r.dxcc.map(i64::from)),
        text(r.prop_mode.as_deref()),
        text(r.sat_name.as_deref()),
        text(r.operator.as_deref()),
        text(r.station_callsign.as_deref()),
        text(r.my_grid.as_deref()),
        text(r.my_rig.as_deref()),
        token(card, "Y"),
        token(lotw, "Y"),
        token(eqsl, "Y"),
        // QRZ's native confirmation is `APP_QRZLOG_STATUS=C`, not a Y/V enumeration.
        token(qrz, "C"),
        flag(r.qsl_sent.sent),
        text(r.qsl_sent.via.map(QslVia::code)),
        r.qsl_sent.date_unix.map_or(Value::Null, stamp),
        r.qsl_sent.cleared_unix.map_or(Value::Null, stamp),
        // The comma is the separator AND `parse_credit`'s own split, so no element it produced
        // can contain one. Asserted rather than assumed: a hand-built record that broke the
        // invariant would come back as two award codes instead of one, silently.
        Value::Text(joined_credit(&r.credit_granted)),
        Value::Text(joined_credit(&r.credit_submitted)),
        text(r.ota.my_program.as_deref()),
        text(r.ota.my_ref.as_deref()),
        text(r.ota.their_program.as_deref()),
        text(r.ota.their_ref.as_deref()),
        text(r.ota.iota.as_deref()),
        Value::Text(call_norm(&r.call)),
        Value::Text(crate::message::base_call(&r.call)),
        Value::Text(band_norm(&r.band)),
        Value::Text(mode_norm(&r.mode)),
        Value::Text(crate::reconcile::mode_class(&r.mode).to_string()),
        Value::Text(super::dedup_mode(&r.mode)),
        stamp(r.when_unix / 86_400),
        text(r.grid.as_deref().and_then(grid4).as_deref()),
        text(resolved.entity),
        int(resolved.cq_zone.map(i64::from)),
        flag(is_sat(r)),
        flag(dxcc_credited(r)),
        text(r.operator.as_deref().map(call_norm).as_deref()),
        // The ruleset's key. NULL here: `DupeRule` is a property of the running contest, not
        // of the record, and the store is never handed one. It is materialised by the contest
        // writer, and re-materialised when the ruleset's rule changes.
        Value::Null,
        c_text(|c| &c.contest_id),
        c_text(|c| &c.session),
        c_text(|c| &c.qid),
        int(contest.and_then(|c| c.stx).map(i64::from)),
        text(contest.and_then(|c| c.stx_string.as_deref())),
        int(contest.and_then(|c| c.srx).map(i64::from)),
        text(contest.and_then(|c| c.srx_string.as_deref())),
        // Score, the dupe verdict and the multiplier bucket are the contest engine's, and
        // `QsoRecord` carries none of them. Columns exist so score is stored rather than
        // recomputed per render; the general log writes the neutral value.
        Value::Null,
        flag(false),
        Value::Null,
    ]
}

/// One `qso` row back, **in [`QSO_COLUMNS`] order**. Read [`bind_qso`] beside it.
///
/// `extra`, `upload` and the exchange are filled by the caller from their own sweeps.
fn record_from_row(row: &rusqlite::Row<'_>) -> Result<QsoRecord> {
    let id: String = row.get(0)?;
    let text = |i: usize| -> Result<Option<String>> { Ok(row.get(i)?) };
    // The raw token back through `take_confirmed`'s own rule: `Y` and `V` are both a
    // confirmation the operator HOLDS, and nothing else is.
    let confirmed_token = |i: usize| -> Result<bool> {
        Ok(row.get::<_, Option<String>>(i)?.is_some_and(|v| {
            let v = v.trim();
            v.eq_ignore_ascii_case("Y") || v.eq_ignore_ascii_case("V")
        }))
    };
    let qsl_rcvd = QslRcvd {
        card: confirmed_token(26)?,
        lotw: confirmed_token(27)?,
        eqsl: confirmed_token(28)?,
        // `APP_QRZLOG_STATUS`: some exports write `Y`, ours writes `C`.
        qrz: row
            .get::<_, Option<String>>(29)?
            .is_some_and(|v| v.eq_ignore_ascii_case("C") || v.eq_ignore_ascii_case("Y")),
    };
    let contest = ContestFields {
        session: text(56)?.unwrap_or_default(),
        contest_id: text(55)?.unwrap_or_default(),
        stx: row.get::<_, Option<u32>>(58)?,
        stx_string: text(59)?,
        srx: row.get::<_, Option<u32>>(60)?,
        srx_string: text(61)?,
        sent: Vec::new(),
        rcvd: Vec::new(),
        // Filled by `all_records` from `qso_contest_adif`, not from a column here.
        //
        // ⚠️ This used to be left empty, on the claim that `ContestFields::adif` is "derived at
        // export" and that a record loaded here "is in exactly the state one loaded from
        // `log.adi` is in". Both halves were measured and are FALSE. `directed_columns` derives
        // the vector at MERGE time, where the exchange spec is in hand, and nothing downstream
        // can redo it; and the file round trip does not lose the values, it MOVES them — the
        // tags are written into `log.adi` and come back in [`QsoRecord::state`] and
        // [`QsoRecord::extra`], which is what that field's own doc comment describes. A merged
        // row that never went through a file has them in neither place, so dropping them here
        // was a real loss of the section, class and (for a QSO party) the state.
        adif: Vec::new(),
        qid: text(57)?.unwrap_or_default(),
    };
    Ok(QsoRecord {
        id: id.parse::<RecordId>().ok(),
        call: row.get(1)?,
        grid: text(9)?,
        country: text(10)?,
        state: text(11)?,
        band: row.get(2)?,
        freq_mhz: row.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
        freq_rx_mhz: row.get(5)?,
        mode: row.get(3)?,
        rst_sent: text(16)?,
        rst_rcvd: text(17)?,
        name: text(12)?,
        qth: text(13)?,
        comment: text(14)?,
        notes: text(15)?,
        tx_power: row.get(18)?,
        when_unix: row.get(6)?,
        time_known: row.get(8)?,
        time_off_unix: row.get(7)?,
        // Derived from the per-channel truth, exactly as `record_from` derives them: they are
        // a reading of `qsl_rcvd`, not an independent fact, and storing them separately would
        // let the two disagree.
        confirmed: qsl_rcvd.any(),
        award_confirmed: qsl_rcvd.award(),
        qsl_rcvd,
        qsl_sent: QslSent {
            sent: row.get(30)?,
            via: text(31)?.as_deref().and_then(QslVia::from_code),
            date_unix: row.get(32)?,
            cleared_unix: row.get(33)?,
        },
        credit_granted: split_credit(&row.get::<_, String>(34)?),
        credit_submitted: split_credit(&row.get::<_, String>(35)?),
        upload: UploadState::default(),
        ota: Ota {
            my_program: text(36)?,
            my_ref: text(37)?,
            their_program: text(38)?,
            their_ref: text(39)?,
            iota: text(40)?,
        },
        dxcc: row.get(19)?,
        prop_mode: text(20)?,
        sat_name: text(21)?,
        operator: text(22)?,
        station_callsign: text(23)?,
        my_grid: text(24)?,
        my_rig: text(25)?,
        extra: Vec::new(),
        contest: Some(Box::new(contest)),
    })
}

/// `parse_contest`'s own emptiness test: not one contest tag said anything.
fn contest_is_empty(c: &ContestFields) -> bool {
    c.contest_id.is_empty()
        && c.stx.is_none()
        && c.stx_string.is_none()
        && c.srx.is_none()
        && c.srx_string.is_none()
        && c.session.is_empty()
        && c.qid.is_empty()
        && c.sent.is_empty()
        && c.rcvd.is_empty()
        // A block carrying nothing but its directed columns is still a real block: without
        // this the sweep reads those rows and then throws the whole thing away.
        && c.adif.is_empty()
}

/// The credit list as the one string the ADIF writer emits. See [`split_credit`] for why the
/// join is lossless, and the `debug_assert` for the invariant it rests on.
fn joined_credit(codes: &[String]) -> String {
    debug_assert!(
        !codes.iter().any(|c| c.contains(',')),
        "a credit code cannot contain the separator: {codes:?}"
    );
    codes.join(",")
}

/// The stored comma-joined credit list back into the `Vec` the record holds. Lossless in both
/// directions because `parse_credit` has already dropped every `:source` and can leave no
/// comma inside an element.
fn split_credit(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split(',').map(str::to_string).collect()
}

/// `upper(trim(call))` — one fold, written once, so the call sites stop disagreeing.
fn call_norm(call: &str) -> String {
    call.trim().to_ascii_uppercase()
}

/// The band fold. **Not trimmed and never collapsed to NULL:** `""` is a live index key that
/// is not a band, and the two folds in the tree today (the worked-before sets uppercase, the
/// dedup and reconcile keys lowercase) are one case fold of the same string — so one stored
/// column serves both, and the query uses this one.
fn band_norm(band: &str) -> String {
    band.to_ascii_uppercase()
}

/// The plain case fold of the mode, which is the worked-before index's key. Distinct from
/// [`super::dedup_mode`] (which folds USB/LSB → SSB) and from
/// [`crate::reconcile::mode_class`] (CW / Phone / Digital / Other): three levels, and
/// collapsing any two of them tells an FT8 operator that FT4 is not a new mode.
fn mode_norm(mode: &str) -> String {
    mode.to_ascii_uppercase()
}

/// The first four characters, uppercased, and only when there are four — VUCC counts squares,
/// not subsquares, and a 6-character logged grid must match a 4-character decode.
fn grid4(grid: &str) -> Option<String> {
    let g: String = grid.trim().to_ascii_uppercase().chars().take(4).collect();
    (g.len() == 4).then_some(g)
}

/// `PROP_MODE=SAT` diverts a grid out of the per-band terrestrial sets into the band-
/// independent satellite one.
fn is_sat(r: &QsoRecord) -> bool {
    r.prop_mode
        .as_deref()
        .is_some_and(|p| p.trim().eq_ignore_ascii_case("SAT"))
}

/// Whether ARRL has granted DXCC credit against this record — the awards fold's hot predicate,
/// today a prefix scan over the credit `Vec` for every record on every poll.
fn dxcc_credited(r: &QsoRecord) -> bool {
    r.credit_granted.iter().any(|c| c.starts_with("DXCC"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{adif_header, adif_record, parse_adif};

    /// A deterministic generator: a failing case is reproducible from its index alone, which a
    /// seeded RNG over a 20k-row fixture otherwise is not.
    struct Gen(u64);
    impl Gen {
        fn next(&mut self) -> u64 {
            // xorshift64*, for spread without a dependency.
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
            xs[(self.next() % xs.len() as u64) as usize]
        }
        fn chance(&mut self, one_in: u64) -> bool {
            self.next().is_multiple_of(one_in)
        }
    }

    /// One synthetic ADIF record, exercising every representation the schema has to carry:
    /// the `""` band that is a real key, the MODE/SUBMODE/APP_TEMPO_MODE cascade, a grid that
    /// is `Some("")`, portable calls the base-call rule has to split, all four confirmation
    /// channels, the QSL-sent withdrawal, both credit lists, all four upload stamps, the OTA
    /// two-fer, IOTA, a contest block whose exchange values carry the carrier's own separators,
    /// and a handful of foreign tags that can only survive in `qso_extra`.
    fn synthetic(i: usize, g: &mut Gen) -> String {
        let mut f = String::new();
        let mut tag = |name: &str, val: &str| {
            f.push_str(&format!("<{}:{}>{}", name, val.len(), val));
        };
        let call = g.pick(&[
            "W1AW",
            "K1ABC/P",
            "KH8/W1AW",
            "DL1ZZZ",
            "VP2E/AA9A",
            "JA1XYZ/4",
        ]);
        tag("CALL", call);
        tag("QSO_DATE", g.pick(&["20240115", "20260701", "19991231"]));
        if !g.chance(7) {
            tag("TIME_ON", g.pick(&["012345", "235959", "000000"]));
        }
        if g.chance(3) {
            tag("TIME_OFF", "012400");
        }
        // `""` is a real index key, so a fifth of the fixture carries no BAND at all.
        if !g.chance(5) {
            tag("BAND", g.pick(&["20m", "40m", "2m", "70cm", "6M"]));
        }
        tag(
            "MODE",
            g.pick(&["FT8", "SSB", "CW", "MFSK", "RTTY", "DYNAMIC"]),
        );
        if g.chance(3) {
            // The cascade: a submode that is promoted on read, and one that is not.
            tag("SUBMODE", g.pick(&["FT4", "USB", "VARA HF", "Q65"]));
        }
        if g.chance(4) {
            tag("APP_TEMPO_MODE", g.pick(&["TempoFast", "TempoDeep", "JS8"]));
        }
        if !g.chance(4) {
            tag("FREQ", g.pick(&["14.074000", "7.038600", "144.174000"]));
        }
        if g.chance(9) {
            tag("FREQ_RX", "14.095000");
        }
        for (name, vals) in [
            ("GRIDSQUARE", &["FN31pr", "FN31", "", "JO22"][..]),
            (
                "COUNTRY",
                &["United States", "Germany", "Fed. Rep. of Germany"],
            ),
            ("STATE", &["wi", "CA", "NT"]),
            ("NAME", &["Joe", "Jean-Luc", "Ünïcödé"]),
            ("QTH", &["Chicago", "Sankt Pölten"]),
            ("COMMENT", &["tnx 73", "first contact — 59 both ways"]),
            ("NOTES", &["line one\nline two", "rig: FT-710"]),
            ("RST_SENT", &["599", "59", "-12"]),
            ("RST_RCVD", &["599", "+03"]),
            ("TX_PWR", &["100", "5", "1500"]),
            ("DXCC", &["291", "230", "1"]),
            ("OPERATOR", &["kd9taw", "W1AW"]),
            ("STATION_CALLSIGN", &["KD9TAW"]),
            ("MY_GRIDSQUARE", &["EN52"]),
            ("MY_RIG", &["FT-710 + hexbeam"]),
        ] {
            if !g.chance(3) {
                tag(name, g.pick(vals));
            }
        }
        if g.chance(6) {
            tag("PROP_MODE", "SAT");
            tag("SAT_NAME", "RS-44");
        }
        // Confirmations, including the `V` a credited third-party log writes.
        for (name, vals) in [
            ("QSL_RCVD", &["Y", "V", "N"][..]),
            ("LOTW_QSL_RCVD", &["Y", "V"]),
            ("EQSL_QSL_RCVD", &["Y"]),
            ("APP_QRZLOG_STATUS", &["C", "Y"]),
        ] {
            if g.chance(4) {
                tag(name, g.pick(vals));
            }
        }
        if g.chance(4) {
            tag("QSL_SENT", "Y");
            tag("QSL_SENT_VIA", g.pick(&["B", "D", "E"]));
            tag("QSLSDATE", "20240201");
        } else if g.chance(9) {
            tag("APP_TEMPO_QSL_SENT_CLEARED", "1706745600");
        }
        if g.chance(3) {
            tag(
                "CREDIT_GRANTED",
                g.pick(&["DXCC,WAS", "DXCC_BAND:LOTW", "WAZ"]),
            );
        }
        if g.chance(6) {
            tag("CREDIT_SUBMITTED", "DXCC");
        }
        for (name, vals) in [
            (
                "APP_TEMPO_UL_LOTW",
                &["accepted|1700000000|", "rejected|1700000001|record"][..],
            ),
            ("APP_TEMPO_UL_EQSL", &["pending|1700000002|"]),
            ("APP_TEMPO_UL_QRZ", &["authfail|1700000003|credentials"]),
            ("APP_TEMPO_UL_CLUBLOG", &["duplicate|1700000004|"]),
        ] {
            if g.chance(3) {
                tag(name, g.pick(vals));
            }
        }
        if g.chance(4) {
            tag("MY_SIG", "POTA");
            // The two-fer: one QSO, two parks.
            tag("MY_SIG_INFO", g.pick(&["US-0001", "US-0001,US-0002"]));
        }
        if g.chance(5) {
            tag("SOTA_REF", "W7A/MN-001");
        }
        if g.chance(7) {
            tag("IOTA", g.pick(&["NA-001", "EU-005"]));
        }
        if g.chance(3) {
            tag(
                "CONTEST_ID",
                g.pick(&["ARRL-FIELD-DAY", "CQ-WW-SSB", "OH-QSO-PARTY"]),
            );
            tag("APP_NEXUS_SESSION", "ARRL-FIELD-DAY:WI");
            tag("APP_NEXUS_QID", &format!("ARRL-FIELD-DAY:WI:a1b2c3d4:{i}"));
            tag("STX", "42");
            tag("SRX", "7");
            if g.chance(2) {
                tag("STX_STRING", "2A WI");
                tag("SRX_STRING", "1D EMA");
            }
            // ⚠️ Values carrying the carrier's own separators, percent-escaped by `encode_pairs`
            // and decoded back — the shape a QSO party's free-text QTH really takes.
            tag("APP_NEXUS_MYEX", "CLASS::2A;SECTION::WI");
            tag(
                "APP_NEXUS_EX",
                "CLASS::1D;SECTION::EMA;QTH::Lorain%3B%3A%25 Co",
            );
        }
        // The passthrough: foreign tags no build models. The index keeps the names unique
        // within a record, which is what `qso_extra`'s primary key requires.
        for k in 0..(g.next() % 6) {
            tag(
                &format!("APP_OTHERLOG_F{k}"),
                g.pick(&["x", "a value with spaces", "ünïcödé ✓", ""]),
            );
        }
        if g.chance(2) {
            tag("QSL_VIA", "BUREAU");
        }
        if g.chance(4) {
            // A standard ADIF field this build does not model.
            tag("AGE", "42");
        }
        // ⭐ §A2 limit 6, and it is the subtlest tag in the file: `LOTW_QSL_SENT` is INSPECTED
        // with `get`, not consumed with `remove`, so ONE tag produces rows in TWO child tables
        // — it stays in `extra` (a `qso_extra` row) and it synthesises an Accepted /
        // OperatorDeclared LoTW stamp when no `APP_TEMPO_UL_LOTW` overrides it (a `qso_upload`
        // row). The re-export then writes both. A store that consumed it like a modelled field
        // would lose the round trip on exactly the records an operator imported from LoTW.
        if g.chance(4) {
            tag("LOTW_QSL_SENT", "Y");
        }
        // §A2 limit 2: the `:T` data-type indicator is split off and dropped, so the re-export
        // writes `<NAME:len>`. §A2 limit 3: a tag repeated inside one record keeps only the
        // last. Both are the PARSER's, and what is proven here is that the store does not make
        // either worse — nor resurrect what limit 7 destroyed (the invalid IOTA below).
        let oddities = g.chance(6);
        if oddities {
            tag("APP_OTHERLOG_TWICE", "first");
            tag("APP_OTHERLOG_TWICE", "second");
            tag("IOTA", "ZZ-999");
        }
        // Written raw, and last, because `tag` computes the length and a `:T` indicator sits
        // AFTER it — and because the buffer cannot be borrowed while that closure is alive.
        if oddities {
            f.push_str("<APP_OTHERLOG_TYPED:2:N>42");
        }
        f.push_str("<EOR>\n");
        f
    }

    /// `n` synthetic records, as one ADIF file.
    fn synthetic_log(n: usize) -> String {
        let mut g = Gen(0x9E37_79B9_7F4A_7C15);
        let mut s = adif_header();
        for i in 0..n {
            s.push_str(&synthetic(i, &mut g));
        }
        s
    }

    /// ADIF → DB → ADIF, as `(expected, actual)`.
    ///
    /// The baseline is `write(parse(file))`, not the file: parsing normalises (it trims, it
    /// uppercases `STATE`, it resolves the mode cascade), so comparing against the raw input
    /// would measure the parser, not the store. With the baseline taken after one parse, the
    /// DATABASE is the only variable between the two strings.
    ///
    /// `meddle` runs against the stored rows before they are read back — which is how the
    /// positive control proves this comparison can actually fail.
    fn round_trip(adif: &str, meddle: impl FnOnce(&Connection)) -> (String, String, usize) {
        let mut records = parse_adif(adif);
        // Every record the log HOLDS carries an id; the loader assigns one to a row that
        // arrives without. Provisional ids, minted from the record's index, stand in for it.
        for (i, r) in records.iter_mut().enumerate() {
            r.id = Some(RecordId::Provisional {
                hash: i as u64,
                ordinal: 0,
            });
        }
        let expected: String = adif_header() + &records.iter().map(adif_record).collect::<String>();

        let mut db = LogDb::open_in_memory().expect("open");
        db.insert_all(records.iter().map(|r| (r, Resolved::default())))
            .expect("insert");
        meddle(&db.conn);
        let back = db.load_all().expect("load");

        let actual: String = adif_header() + &back.iter().map(adif_record).collect::<String>();
        (expected, actual, records.len())
    }

    /// What the stored fixture actually contains, so the proof below cannot pass vacuously.
    ///
    /// ⚠️ A byte-identical round trip over a fixture that never populated `qso_extra` would be
    /// just as green as one that did, and it would prove the opposite of what it claims. Every
    /// representation the schema has to carry is counted HERE, in the database, after the
    /// insert — not asserted of the generator, which is a proxy for it.
    fn census(conn: &Connection, n: usize) {
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        for (what, sql, floor) in [
            (
                "passthrough rows",
                "SELECT count(*) FROM qso_extra",
                n as i64 / 4,
            ),
            (
                "records with a passthrough",
                "SELECT count(DISTINCT qso_id) FROM qso_extra",
                n as i64 / 4,
            ),
            (
                "upload stamps",
                "SELECT count(*) FROM qso_upload",
                n as i64 / 8,
            ),
            (
                "connectors stamped",
                "SELECT count(DISTINCT service) FROM qso_upload",
                4,
            ),
            (
                "exchange slots",
                "SELECT count(*) FROM contest_exchange",
                n as i64 / 4,
            ),
            (
                "both exchange sides",
                "SELECT count(DISTINCT side) FROM contest_exchange",
                2,
            ),
            (
                "records with no band",
                "SELECT count(*) FROM qso WHERE band = ''",
                1,
            ),
            (
                "records with an empty grid",
                "SELECT count(*) FROM qso WHERE grid = ''",
                1,
            ),
            (
                "contest rows",
                "SELECT count(*) FROM qso WHERE contest_id IS NOT NULL",
                1,
            ),
            (
                "merge identities",
                "SELECT count(*) FROM qso WHERE contest_qid IS NOT NULL",
                1,
            ),
            (
                "granted credit",
                "SELECT count(*) FROM qso WHERE credit_granted != ''",
                1,
            ),
            (
                "submitted credit",
                "SELECT count(*) FROM qso WHERE credit_submitted != ''",
                1,
            ),
            (
                "OTA two-fers",
                "SELECT count(*) FROM qso WHERE ota_my_ref LIKE '%,%'",
                1,
            ),
            (
                "SOTA refs",
                "SELECT count(*) FROM qso WHERE ota_their_program = 'SOTA'",
                1,
            ),
            (
                "IOTA refs",
                "SELECT count(*) FROM qso WHERE ota_iota IS NOT NULL",
                1,
            ),
            // §A2's limits, each present in the fixture rather than merely reasoned about.
            (
                "LOTW_QSL_SENT left in the passthrough (limit 6)",
                "SELECT count(*) FROM qso_extra WHERE name = 'LOTW_QSL_SENT'",
                1,
            ),
            (
                "LoTW stamps synthesised from it",
                "SELECT count(*) FROM qso_upload WHERE service = 'lotw' AND detail = 'declared'",
                1,
            ),
            (
                "a passthrough tag that carried a :T indicator (limit 2)",
                "SELECT count(*) FROM qso_extra WHERE name = 'APP_OTHERLOG_TYPED'",
                1,
            ),
            (
                "a tag repeated in one record (limit 3)",
                "SELECT count(*) FROM qso_extra WHERE name = 'APP_OTHERLOG_TWICE'",
                1,
            ),
            (
                "satellite contacts",
                "SELECT count(*) FROM qso WHERE is_sat = 1",
                1,
            ),
            (
                "DXCC-credited",
                "SELECT count(*) FROM qso WHERE dxcc_credited = 1",
                1,
            ),
            (
                "time-unknown rows",
                "SELECT count(*) FROM qso WHERE time_known = 0",
                1,
            ),
            (
                "card confirmations",
                "SELECT count(*) FROM qso WHERE qsl_card_rcvd_raw IS NOT NULL",
                1,
            ),
            (
                "QRZ confirmations",
                "SELECT count(*) FROM qso WHERE qrz_status_raw IS NOT NULL",
                1,
            ),
            (
                "QSL-sent marks",
                "SELECT count(*) FROM qso WHERE qsl_sent = 1",
                1,
            ),
            (
                "withdrawn QSL marks",
                "SELECT count(*) FROM qso WHERE qsl_sent_cleared_unix IS NOT NULL",
                1,
            ),
            (
                "split contacts",
                "SELECT count(*) FROM qso WHERE freq_rx_mhz IS NOT NULL",
                1,
            ),
            (
                "distinct modes",
                "SELECT count(DISTINCT mode_norm) FROM qso",
                6,
            ),
            (
                "portable base calls",
                "SELECT count(*) FROM qso WHERE base_call != call_norm",
                1,
            ),
        ] {
            let got = count(sql);
            assert!(
                got >= floor,
                "the fixture must exercise {what}: {got} < {floor}"
            );
        }
        // Limit 3 is a ZERO, and a floor cannot say so: only the LAST occurrence of a repeated
        // tag survives the parser, so the first value must be nowhere in the store.
        assert_eq!(
            count(
                "SELECT count(*) FROM qso_extra \
                 WHERE name = 'APP_OTHERLOG_TWICE' AND value = 'first'"
            ),
            0,
            "a repeated tag keeps only its last occurrence"
        );
        // Limit 7: a modelled-but-unparseable value is consumed and destroyed, never parked in
        // `extra` — and the store must not resurrect it in either place.
        assert_eq!(
            count("SELECT count(*) FROM qso WHERE ota_iota = 'ZZ-999'")
                + count("SELECT count(*) FROM qso_extra WHERE name = 'IOTA'"),
            0,
            "an invalid IOTA is destroyed at parse and must not come back"
        );
    }

    /// ⭐ **THE PROOF.** A large synthetic log survives ADIF → SQLite → ADIF byte for byte.
    ///
    /// The fixture is 20,000 records rather than the spec's 150,000: `cargo test` builds
    /// unoptimised, and the cost is dominated by the ADIF parse and the two multi-megabyte
    /// strings, neither of which is what this test measures. 20k exercises every representation
    /// many times over (see [`census`], which is asserted against the stored rows before they
    /// are read back), and the whole test runs in under three seconds where 150k took minutes
    /// in a debug build. See `positive_control_…` below for the proof that it can fail.
    #[test]
    fn a_large_synthetic_log_round_trips_adif_to_db_to_adif_byte_for_byte() {
        let n = 150_000;
        let (expected, actual, parsed) = round_trip(&synthetic_log(n), |conn| census(conn, n));
        assert_eq!(parsed, n, "every synthetic record parsed");
        if expected != actual {
            // Name the first divergent record rather than dumping two megabytes.
            let (e, a) = (
                expected.split("<EOR>").collect::<Vec<_>>(),
                actual.split("<EOR>").collect::<Vec<_>>(),
            );
            let at = e.iter().zip(&a).position(|(x, y)| x != y);
            panic!(
                "the round trip is not byte-identical; first divergence at record {at:?}\n\
                 expected: {:?}\nactual:   {:?}",
                at.and_then(|i| e.get(i)),
                at.and_then(|i| a.get(i)),
            );
        }
    }

    /// ⭐ **THE POSITIVE CONTROL.** Drop ONE `qso_extra` row and the same comparison must FAIL.
    ///
    /// A round-trip test that cannot fail proves nothing: it would pass just as green against a
    /// store that dropped the passthrough entirely. This deletes exactly one foreign tag —
    /// the single failure mode the schema exists to prevent — and asserts the check trips, and
    /// that what goes missing is that tag.
    #[test]
    fn positive_control_dropping_one_qso_extra_row_breaks_the_round_trip() {
        let mut dropped = String::new();
        let (expected, actual, _) = round_trip(&synthetic_log(400), |conn| {
            let (qso_id, name, value): (String, String, String) = conn
                .query_row(
                    "SELECT qso_id, name, value FROM qso_extra ORDER BY qso_id, name LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .expect("the fixture must carry a passthrough row to delete");
            let gone = conn
                .execute(
                    "DELETE FROM qso_extra WHERE qso_id = ?1 AND name = ?2",
                    params![&qso_id, &name],
                )
                .expect("delete");
            assert_eq!(gone, 1, "the control must actually delete a row");
            dropped = format!("<{}:{}>{}", name, value.len(), value);
        });
        assert_ne!(
            expected, actual,
            "dropping a passthrough row MUST break the round trip — a check that cannot \
             fail proves nothing"
        );
        // And the difference is exactly that tag, once: other records in the fixture carry
        // the same foreign tag, so the proof is that the COUNT fell by one.
        assert_eq!(
            expected.matches(&dropped).count(),
            actual.matches(&dropped).count() + 1,
            "exactly one {dropped} must have gone missing"
        );
    }

    /// The WHOLE PIPELINE the store now owns, on disk: an operator's `log.adi` → the real
    /// conversion (`migrate_log`, with its file-backed store) → the load the app starts from →
    /// the file the mirror writes. Every record comes out as the ADIF writer would have written
    /// it from a plain load of the original file — `extra` included — in the same order and
    /// under the same id. `meddle` runs against the store between the conversion and the load.
    fn pipeline(n: usize, meddle: impl FnOnce(&Connection)) -> (Vec<String>, Vec<String>) {
        let dir = std::env::temp_dir().join(format!(
            "nexus-pipeline-{}-{:?}-{n}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log.adi");
        std::fs::write(&log, synthetic_log(n)).unwrap();
        let expected: Vec<String> = Logbook::load(&log)
            .records()
            .iter()
            .map(|r| super::super::adif_record_own_log(r))
            .collect();
        let db = super::super::migrate::database_path(&log);
        super::super::migrate::migrate_log(&log, &db, |_| Resolved::default()).expect("converts");
        meddle(&Connection::open(&db).unwrap());
        let held = Logbook::from_store(LogDb::open(&db).unwrap().load_all().unwrap());
        let mirror = super::super::mirror::mirror_adif(held.records());
        let actual: Vec<String> = held
            .records()
            .iter()
            .map(|r| super::super::adif_record_own_log(r))
            .collect();
        // The file the mirror writes is exactly those records after its header — so a
        // per-record comparison below is a comparison of the FILE, not of a re-parse of it
        // (which would measure the ADIF reader, a different thing: see the note at the call).
        let body = &mirror[mirror.find("<EOH>").expect("a header") + 5..];
        assert_eq!(
            body,
            format!("\n{}", actual.concat()),
            "the mirror is its records"
        );
        let _ = std::fs::remove_dir_all(&dir);
        (expected, actual)
    }

    /// ★ PROPERTY 1, through the new owner: the file the operator had and the file the mirror
    /// writes hold the same records, byte for byte per record, passthrough included, over a
    /// fixture that exercises every representation (the census, asserted on the stored rows).
    ///
    /// ⚠️ It compares the mirror's own bytes, NOT a re-parse of them, and that is deliberate: a
    /// re-parse would also measure the ADIF READER, which is not lossless. A POTA two-fer is
    /// written as `MY_SIG_INFO` = the first park plus `MY_POTA_REF` = both, and the reader
    /// prefers `MY_SIG_INFO` when both are present, so a re-read keeps one park. That is a
    /// reader defect found by this test's first draft and reported, not fixed here; the store
    /// itself never re-reads the mirror, so it keeps both parks across a restart.
    #[test]
    fn the_log_survives_conversion_load_and_mirror_byte_for_byte() {
        let n = 20_000;
        let (expected, actual) = pipeline(n, |conn| census(conn, n));
        assert_eq!(actual.len(), n, "every record came back out");
        if let Some(at) = expected.iter().zip(&actual).position(|(e, a)| e != a) {
            panic!(
                "record {at} differs after conversion, load and mirror\nexpected: {:?}\nactual:   {:?}",
                expected[at], actual[at]
            );
        }
    }

    /// Its positive control: one passthrough row dropped from the store between the conversion
    /// and the load must break the same comparison, by exactly that one tag.
    #[test]
    fn positive_control_the_pipeline_catches_one_dropped_passthrough_row() {
        let mut dropped = String::new();
        let (expected, actual) = pipeline(300, |conn| {
            let (qso_id, name, value): (String, String, String) = conn
                .query_row(
                    "SELECT qso_id, name, value FROM qso_extra ORDER BY qso_id, name LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .expect("a passthrough row to drop");
            assert_eq!(
                conn.execute(
                    "DELETE FROM qso_extra WHERE qso_id = ?1 AND name = ?2",
                    params![&qso_id, &name],
                )
                .unwrap(),
                1
            );
            dropped = format!("<{}:{}>{}", name, value.len(), value);
        });
        assert_ne!(expected, actual, "a dropped passthrough row MUST be caught");
        let count = |v: &[String]| v.iter().map(|r| r.matches(&dropped).count()).sum::<usize>();
        assert_eq!(
            count(&expected),
            count(&actual) + 1,
            "exactly {dropped}, once"
        );
    }

    /// The same property over the nasty end of the value space, where a SQL round trip breaks
    /// if it breaks at all: empty strings that are not NULL, the separators the exchange
    /// carrier escapes, multi-line text, and non-ASCII.
    #[test]
    fn arbitrary_values_round_trip() {
        use proptest::prelude::*;
        // No `<`: it opens a tag, and an ADIF value may not contain one.
        let value = r"[^<]{0,40}";
        let runner = proptest::test_runner::Config {
            cases: 64,
            ..proptest::test_runner::Config::default()
        };
        proptest!(runner, |(
            call in "[A-Z0-9]{1,3}[0-9][A-Z]{1,3}(/[A-Z0-9]{1,3})?",
            band in prop::sample::select(vec!["", "20m", "2M"]),
            grid in value,
            notes in value,
            // `APP_PROPTEST_`-prefixed so a generated name can never collide with a tag this
            // build models — a collision would overwrite the modelled tag (the scanner keeps
            // the LAST occurrence) and make the test flaky rather than strict.
            foreign in prop::collection::hash_map("APP_PROPTEST_[A-Z0-9_]{1,8}", value, 0..6),
            ex in value,
        )| {
            let f = |n: &str, v: &str| format!("<{}:{}>{}", n, v.len(), v);
            let mut adif = f("CALL", &call) + &f("QSO_DATE", "20260701") + &f("TIME_ON", "010203");
            adif += &f("BAND", band);
            adif += &f("MODE", "FT8");
            adif += &f("GRIDSQUARE", &grid);
            adif += &f("NOTES", &notes);
            adif += &f("CONTEST_ID", "CQ-WW-SSB");
            adif += &f("APP_NEXUS_EX", &crate::contest::carrier::encode_pairs(
                &[("QTH".to_string(), ex.clone())],
            ));
            for (k, v) in &foreign {
                adif += &f(k, v);
            }
            adif += "<EOR>\n";
            let (expected, actual, n) = round_trip(&(adif_header() + &adif), |_| {});
            prop_assert_eq!(n, 1);
            prop_assert_eq!(expected, actual);
        });
    }

    /// `""` is a live index key that is not a band, and collapsing it to NULL is the one thing
    /// §v3.8 names explicitly. A record with no band must come back with no band — not with a
    /// NULL that reads as "unknown" and not with a band invented from the frequency.
    #[test]
    fn an_empty_band_stays_an_empty_band_and_an_empty_grid_stays_present() {
        let adif = adif_header()
            + "<CALL:4>W1AW<QSO_DATE:8>20260701<TIME_ON:6>010203<MODE:3>FT8\
               <GRIDSQUARE:0><FREQ:9>47.088000<EOR>\n";
        let (expected, actual, _) = round_trip(&adif, |_| {});
        assert_eq!(expected, actual);
        let mut db = LogDb::open_in_memory().unwrap();
        let mut rec = parse_adif(&adif).remove(0);
        rec.id = Some(RecordId::Provisional {
            hash: 1,
            ordinal: 0,
        });
        assert_eq!(rec.band, "", "the fixture must carry no band");
        assert_eq!(rec.grid.as_deref(), Some(""), "an empty grid is PRESENT");
        db.insert(&rec, Resolved::default()).unwrap();
        let back = &db.load_all().unwrap()[0];
        assert_eq!(back.band, "");
        assert_eq!(back.grid.as_deref(), Some(""));
        let stored: String = db
            .conn
            .query_row("SELECT band_norm FROM qso", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, "", "band_norm is '' and never NULL");
    }

    /// One column of the single stored row, rendered as text so a table of expectations reads
    /// as a table. `None` is SQL NULL, which is a distinct answer from an empty string.
    fn col_of(db: &LogDb, name: &str) -> Option<String> {
        db.conn
            .query_row(&format!("SELECT {name} FROM qso"), [], |r| {
                r.get::<_, rusqlite::types::Value>(0)
            })
            .map(|v| match v {
                rusqlite::types::Value::Null => None,
                rusqlite::types::Value::Text(t) => Some(t),
                rusqlite::types::Value::Integer(i) => Some(i.to_string()),
                rusqlite::types::Value::Real(x) => Some(format!("{x}")),
                rusqlite::types::Value::Blob(_) => Some("<blob>".to_string()),
            })
            .unwrap()
    }

    /// Insert one record built from `adif`, so a test can then read its columns by name.
    fn stored(adif: &str) -> LogDb {
        let mut db = LogDb::open_in_memory().unwrap();
        let mut rec = parse_adif(adif).remove(0);
        rec.id = Some(RecordId::Provisional {
            hash: 7,
            ordinal: 0,
        });
        db.insert(&rec, Resolved::default()).unwrap();
        db
    }

    /// ⚠️ **The round trip above is BLIND to a consistently mis-ordered pair of columns.**
    /// [`bind_qso`] and [`record_from_row`] share one index list, so swapping `comment` and
    /// `notes` in BOTH directions still produces byte-identical ADIF — the store would be
    /// self-consistent and wrong, and every Stage 2 query over those columns would read the
    /// other field. This names each modelled column and asserts what is actually IN it.
    #[test]
    fn every_modelled_column_holds_its_own_field() {
        let f = |n: &str, v: &str| format!("<{}:{}>{}", n, v.len(), v);
        // A distinct value per column, so a swap cannot hide behind two equal ones.
        let adif = adif_header()
            + &f("CALL", "W1AW")
            + &f("QSO_DATE", "20260701")
            + &f("TIME_ON", "010203")
            + &f("QSO_DATE_OFF", "20260701")
            + &f("TIME_OFF", "010405")
            + &f("BAND", "20m")
            + &f("MODE", "FT8")
            + &f("FREQ", "14.074000")
            + &f("FREQ_RX", "14.095000")
            + &f("GRIDSQUARE", "FN31pr")
            + &f("COUNTRY", "CountryX")
            + &f("STATE", "wi")
            + &f("NAME", "NameX")
            + &f("QTH", "QthX")
            + &f("COMMENT", "CommentX")
            + &f("NOTES", "NotesX")
            + &f("RST_SENT", "599")
            + &f("RST_RCVD", "579")
            + &f("TX_PWR", "100")
            + &f("DXCC", "291")
            + &f("PROP_MODE", "sat")
            + &f("SAT_NAME", "RS-44")
            + &f("OPERATOR", "opx")
            + &f("STATION_CALLSIGN", "stnx")
            + &f("MY_GRIDSQUARE", "en52")
            + &f("MY_RIG", "RigX")
            + &f("QSL_RCVD", "V")
            + &f("APP_QRZLOG_STATUS", "C")
            + &f("QSL_SENT", "Y")
            + &f("QSL_SENT_VIA", "D")
            + &f("QSLSDATE", "20260601")
            + &f("CREDIT_GRANTED", "DXCC")
            + &f("CREDIT_SUBMITTED", "WAS")
            + &f("MY_SIG", "POTA")
            + &f("MY_SIG_INFO", "US-0001")
            + &f("SOTA_REF", "W7A/MN-001")
            + &f("IOTA", "NA-001")
            + &f("CONTEST_ID", "CQ-WW-SSB")
            + &f("APP_NEXUS_SESSION", "SessionX")
            + &f("APP_NEXUS_QID", "QidX")
            + &f("STX", "42")
            + &f("SRX", "7")
            + &f("STX_STRING", "StxStringX")
            + &f("SRX_STRING", "SrxStringX")
            + "<EOR>\n";
        let db = stored(&adif);
        for (column, want) in [
            ("call", "W1AW"),
            ("band", "20m"),
            ("mode", "FT8"),
            ("freq_mhz", "14.074"),
            ("freq_rx_mhz", "14.095"),
            ("when_unix", "1782867723"),
            ("time_off_unix", "1782867845"),
            ("time_known", "1"),
            ("grid", "FN31pr"), // the one field the reader does not touch at all
            ("country", "CountryX"),
            ("state", "WI"),
            ("name", "NameX"),
            ("qth", "QthX"),
            ("comment", "CommentX"),
            ("notes", "NotesX"),
            ("rst_sent", "599"),
            ("rst_rcvd", "579"),
            ("tx_power", "100"),
            ("dxcc", "291"),
            ("prop_mode", "SAT"),
            ("sat_name", "RS-44"),
            ("operator", "OPX"),
            ("station_callsign", "STNX"),
            ("my_grid", "EN52"),
            ("my_rig", "RigX"),
            // 'V' arrived; 'Y' is stored, because that is what the record can prove and what
            // the ADIF writer emits. See the column's own comment.
            ("qsl_card_rcvd_raw", "Y"),
            ("qrz_status_raw", "C"),
            ("qsl_sent", "1"),
            ("qsl_sent_via", "D"),
            ("qsl_sent_date_unix", "1780272000"),
            ("credit_granted", "DXCC"),
            ("credit_submitted", "WAS"),
            ("ota_my_program", "POTA"),
            ("ota_my_ref", "US-0001"),
            ("ota_their_program", "SOTA"),
            ("ota_their_ref", "W7A/MN-001"),
            ("ota_iota", "NA-001"),
            ("contest_id", "CQ-WW-SSB"),
            ("contest_session_id", "SessionX"),
            ("contest_qid", "QidX"),
            ("stx", "42"),
            ("srx", "7"),
            ("stx_string", "StxStringX"),
            ("srx_string", "SrxStringX"),
        ] {
            assert_eq!(
                col_of(&db, column).as_deref(),
                Some(want),
                "column {column}"
            );
        }
        // And the columns nothing on this path fills are NULL rather than a guess.
        for column in ["lotw_rcvd_raw", "eqsl_rcvd_raw", "qsl_sent_cleared_unix"] {
            assert_eq!(col_of(&db, column), None, "column {column}");
        }
    }

    /// The derived columns are written AT INSERT, and each one is the fold its consumer uses.
    #[test]
    fn the_derived_columns_are_written_at_insert() {
        let adif = adif_header()
            + "<CALL:8>KH8/W1AW<QSO_DATE:8>20260701<TIME_ON:6>010203<BAND:3>20m\
               <MODE:3>USB<GRIDSQUARE:6>fn31pr<PROP_MODE:3>SAT<SAT_NAME:5>RS-44\
               <CREDIT_GRANTED:8>DXCC,WAS<OPERATOR:6>kd9taw<EOR>\n";
        let mut db = LogDb::open_in_memory().unwrap();
        let mut rec = parse_adif(&adif).remove(0);
        rec.id = Some(RecordId::Provisional {
            hash: 2,
            ordinal: 0,
        });
        db.insert(
            &rec,
            Resolved {
                entity: Some("American Samoa"),
                cq_zone: Some(32),
            },
        )
        .unwrap();
        // One column at a time: a thirteen-wide tuple says nothing about which fold failed,
        // and the assertion message is the point of the test.
        let col = |name: &str| col_of(&db, name).unwrap_or_default();
        assert_eq!(col("call_norm"), "KH8/W1AW");
        assert_eq!(
            col("base_call"),
            "W1AW",
            "the base call is the home call, not the prefix"
        );
        assert_eq!(col("band_norm"), "20M");
        assert_eq!(col("mode_norm"), "USB", "the plain case fold");
        assert_eq!(col("mode_class"), "Phone", "the award fold");
        assert_eq!(
            col("dedup_mode"),
            "SSB",
            "the dedup fold — USB and LSB are one mode"
        );
        assert_eq!(col("utc_day"), "20635", "2026-07-01, in whole UTC days");
        assert_eq!(col("grid4"), "FN31", "four characters, uppercased");
        assert_eq!(
            col("entity"),
            "American Samoa",
            "the award identity, not COUNTRY"
        );
        assert_eq!(col("cq_zone"), "32");
        assert_eq!(
            col("is_sat"),
            "1",
            "PROP_MODE=SAT is its own award universe"
        );
        assert_eq!(col("dxcc_credited"), "1");
        assert_eq!(col("operator_norm"), "KD9TAW");
    }

    /// ⚠️ SQLite's INTEGER is signed and `APP_TEMPO_QSL_SENT_CLEARED` is a bare `u64` parse, so
    /// a hand-edited or corrupt log can carry a stamp above `i64::MAX`. An `as` cast would
    /// store it as −1 and the operator's withdrawal would read back as a date in 1969, which
    /// beats every `QSLSDATE` it is compared against. Absent is the honest answer.
    #[test]
    fn a_stamp_too_large_for_a_signed_integer_is_absent_rather_than_negative() {
        let mut db = LogDb::open_in_memory().unwrap();
        let mut rec =
            parse_adif("<CALL:4>W1AW<QSO_DATE:8>20260701<TIME_ON:6>010203<EOR>").remove(0);
        rec.id = Some(RecordId::Provisional {
            hash: 9,
            ordinal: 0,
        });
        rec.qsl_sent.cleared_unix = Some(u64::MAX);
        db.insert(&rec, Resolved::default()).unwrap();
        assert_eq!(col_of(&db, "qsl_sent_cleared_unix"), None);
        assert_eq!(db.load_all().unwrap()[0].qsl_sent.cleared_unix, None);
    }

    /// A record with no id cannot be addressed again, so it is refused rather than written.
    #[test]
    fn a_record_without_an_id_is_refused() {
        let mut db = LogDb::open_in_memory().unwrap();
        let mut rec = parse_adif("<CALL:4>W1AW<QSO_DATE:8>20260701<EOR>").remove(0);
        rec.id = None;
        match db.insert(&rec, Resolved::default()) {
            Err(Error::Unidentified { call }) => assert_eq!(call, "W1AW"),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert_eq!(db.load_all().unwrap().len(), 0, "and nothing was written");
    }

    /// The pragmas and the version stamp are what make the file safe to share and safe to
    /// reopen; none of them is visible in a round trip, so each is asserted here.
    #[test]
    fn the_pragmas_and_the_schema_version_are_applied() {
        let dir = std::env::temp_dir().join(format!("nexus-logdb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.db");
        let _ = std::fs::remove_file(&path);
        {
            let db = LogDb::open(&path).unwrap();
            let journal: String = db
                .conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(journal.to_ascii_uppercase(), "WAL");
            let fk: i64 = db
                .conn
                .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
                .unwrap();
            assert_eq!(fk, 1, "ON DELETE CASCADE is decoration without this");
            let busy: i64 = db
                .conn
                .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
                .unwrap();
            assert_eq!(busy, i64::from(BUSY_TIMEOUT_MS));
            let sync: i64 = db
                .conn
                .query_row("PRAGMA synchronous", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                sync, 2,
                "FULL: a contact is on the disk when the commit that logged it returns"
            );
            assert_eq!(db.meta("schema_version").unwrap(), Some(SCHEMA_VERSION));
        }
        // Reopening an existing file is the same call, and it keeps the stamp.
        assert_eq!(
            LogDb::open(&path).unwrap().meta("schema_version").unwrap(),
            Some(SCHEMA_VERSION)
        );
        // A database from a build that is not this one is refused, not read.
        {
            let db = LogDb::open(&path).unwrap();
            db.set_meta("schema_version", SCHEMA_VERSION + 1).unwrap();
        }
        match LogDb::open(&path) {
            Err(Error::SchemaVersion { found, expected }) => {
                assert_eq!((found, expected), (SCHEMA_VERSION + 1, SCHEMA_VERSION));
            }
            other => panic!("expected a version refusal, got {:?}", other.map(|_| ())),
        }
        let _ = std::fs::remove_file(&path);
    }

    /// ⛔ **The conversion's durability trade is its own connection's, and nobody else's.** It
    /// runs `synchronous = NORMAL` — safe in WAL mode, never `OFF` — while every ordinary
    /// connection, the writer thread's among them, keeps the store's own FULL: alongside the
    /// conversion, and after it.
    #[test]
    fn only_the_conversions_own_connection_relaxes_synchronous() {
        let dir = std::env::temp_dir().join(format!("nexus-logdb-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.sqlite3");
        let _ = std::fs::remove_file(&path);
        let pragma = |db: &LogDb, name: &str| -> String {
            db.conn
                .query_row(&format!("PRAGMA {name}"), [], |r| {
                    r.get::<_, rusqlite::types::Value>(0)
                })
                .map(|v| format!("{v:?}"))
                .unwrap()
        };

        let conversion = LogDb::open_for_conversion(&path).unwrap();
        assert_eq!(pragma(&conversion, "synchronous"), "Integer(1)", "NORMAL");
        assert_eq!(
            pragma(&conversion, "journal_mode"),
            "Text(\"wal\")",
            "NORMAL is safe only in WAL mode"
        );
        assert_eq!(
            pragma(&conversion, "cache_size"),
            format!("Integer({})", -CONVERSION_CACHE_KIB)
        );
        let ordinary = LogDb::open(&path).unwrap();
        assert_eq!(
            pragma(&ordinary, "synchronous"),
            "Integer(2)",
            "an ordinary connection alongside the conversion keeps FULL"
        );
        assert_eq!(pragma(&ordinary, "cache_size"), "Integer(-2000)");
        drop(conversion);
        drop(ordinary);
        assert_eq!(
            pragma(&LogDb::open(&path).unwrap(), "synchronous"),
            "Integer(2)",
            "and so does every ordinary connection after it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Deleting a QSO takes its children with it — the point of `foreign_keys = ON`.
    #[test]
    fn deleting_a_qso_cascades_to_its_children() {
        let adif = adif_header()
            + "<CALL:4>W1AW<QSO_DATE:8>20260701<TIME_ON:6>010203<BAND:3>20m<MODE:3>FT8\
               <CONTEST_ID:6>CQ-WPX<APP_NEXUS_EX:11>QTH::Lorain<APP_TEMPO_UL_LOTW:20>\
               accepted|1700000000|<QSL_VIA:6>BUREAU<EOR>\n";
        let mut db = LogDb::open_in_memory().unwrap();
        let mut rec = parse_adif(&adif).remove(0);
        rec.id = Some(RecordId::Provisional {
            hash: 3,
            ordinal: 0,
        });
        db.insert(&rec, Resolved::default()).unwrap();
        let count = |t: &str| -> i64 {
            db.conn
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(
            (
                count("qso_extra"),
                count("qso_upload"),
                count("contest_exchange")
            ),
            (1, 1, 1)
        );
        db.conn.execute("DELETE FROM qso", []).unwrap();
        assert_eq!(
            (
                count("qso_extra"),
                count("qso_upload"),
                count("contest_exchange")
            ),
            (0, 0, 0)
        );
    }

    /// A contest row built by the MERGE, which is the only producer of
    /// [`ContestFields::adif`] — the directed standard columns (`ARRL_SECT`, `CLASS`,
    /// `MY_ARRL_SECT`, and a QSO party's `STATE`).
    ///
    /// ⭐ **Built with `merge_into_general`, never `parse_adif`.** Every other fixture in
    /// this module comes from the parser, and the parser by design never populates
    /// `contest.adif` (see the field's own doc comment) — so no record in any existing
    /// corpus can carry one, and the large byte-identical round trip, the census and the
    /// column-identity test are all structurally incapable of seeing it dropped.
    fn merged_fd_row() -> QsoRecord {
        use crate::contest::{merge_into_general, ContestSession};
        use crate::fieldday::{FdEvent, FieldDayLog};

        let mut log = FieldDayLog::new(
            "W9XYZ",
            ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "CW", 0, 1_782_583_500));
        let mut lb = Logbook::new();
        assert_eq!(merge_into_general(&log, "a1b2c3d4", &mut lb).added(), 1);
        (*lb.records()[0]).clone()
    }

    /// ⭐ The directed standard columns of a contest row survive the store.
    ///
    /// Without them a Field Day or QSO-party contact loses its section, class and state
    /// from every ADIF export made after a restart — and for a QSO party the loss is
    /// total, because `record_for` leaves `state: None` and the contacted station's state
    /// exists ONLY as the directed column.
    #[test]
    fn a_merged_contest_row_keeps_its_directed_columns_through_the_store() {
        let rec = merged_fd_row();

        // ── The fixture floor. ──────────────────────────────────────────────────────
        // Asserted on the record BEFORE it is stored, so a later refactor that stops the
        // merge populating `adif` fails HERE rather than turning the round trip below
        // green for the wrong reason.
        let built: Vec<(&str, &str)> = rec
            .contest
            .as_deref()
            .expect("a merged row has contest provenance")
            .adif
            .iter()
            .map(|(t, v)| (t.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            built,
            vec![
                ("MY_ARRL_SECT", "WI"),
                ("CLASS", "2A"),
                ("ARRL_SECT", "EMA")
            ],
            "the fixture must actually carry directed columns, or it proves nothing"
        );

        // ── The round trip. ─────────────────────────────────────────────────────────
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert(&rec, Resolved::default()).unwrap();
        let back = db.load_all().unwrap();
        assert_eq!(back.len(), 1, "the contact itself must survive");
        let adif = adif_record(&back[0]);
        assert!(
            adif.contains("<ARRL_SECT:3>EMA"),
            "the section they sent is gone: {adif}"
        );
        assert!(
            adif.contains("<CLASS:2>2A"),
            "the class they sent is gone: {adif}"
        );

        // ── The module header's claim, stated directly on the FIELD. ────────────────
        // Not "the export looks right" but "the value came back": the whole vector, in
        // order, values included. An export comparison alone cannot see every loss —
        // `contest_fields` SKIPS an empty value, so a directed column that arrived as
        // `("CLASS", "")` would vanish from both exports and compare equal. This is the
        // stronger statement, and the one "those rows back as a `QsoRecord`" means.
        assert_eq!(
            back[0].contest.as_deref().map(|c| c.adif.as_slice()),
            rec.contest.as_deref().map(|c| c.adif.as_slice()),
            "contest.adif must come back exactly as it went in, order included"
        );
        // And nothing else on the record moved to make room for it.
        assert_eq!(back[0], rec, "the whole record round-trips unchanged");

        // ── The census floor. ───────────────────────────────────────────────────────
        // The assertions above read the EXPORT, which a future writer change could
        // satisfy from somewhere else entirely. This one reads the STORE, so the table
        // itself has to be carrying the rows.
        let stored: i64 = db
            .conn
            .query_row(
                "SELECT count(*) FROM qso_contest_adif
                 WHERE tag IN ('ARRL_SECT','CLASS','MY_ARRL_SECT')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, 3, "the directed columns must be IN the store");

        // The fourth child table cascades like the other three.
        // `deleting_a_qso_cascades_to_its_children` cannot cover this one: its fixture
        // comes from `parse_adif`, so it can never hold a directed column.
        db.conn.execute("DELETE FROM qso", []).unwrap();
        let orphans: i64 = db
            .conn
            .query_row("SELECT count(*) FROM qso_contest_adif", [], |r| r.get(0))
            .unwrap();
        assert_eq!(orphans, 0, "the directed columns go with their contact");

        // ── The store changes nothing. ──────────────────────────────────────────────
        // The strongest form of the claim: a stored-and-reloaded record exports the same
        // bytes as the record that was stored, field order included.
        assert_eq!(
            adif_record(&rec),
            adif,
            "a round trip must return the record it was given"
        );
    }

    /// ⭐ Why the round trip is asserted on the FIELD and not only on the exported bytes.
    ///
    /// `contest_fields` omits a tag whose value is empty, so a directed column that arrived
    /// as `("CLASS", "")` is invisible in `adif_record`: a record carrying it and a record
    /// carrying nothing export the same bytes. An export comparison is therefore blind to
    /// losing it, and only a comparison of `contest.adif` itself can say the store kept it.
    /// This is the case that makes the field-level assertion in the test above load-bearing
    /// rather than a restatement of the export one.
    #[test]
    fn an_empty_directed_value_is_invisible_in_the_export_and_still_round_trips() {
        let mut rec = merged_fd_row();
        rec.contest.as_deref_mut().expect("provenance").adif =
            vec![("CLASS".to_string(), String::new())];

        // The blindness, demonstrated rather than asserted: identical bytes either way.
        let mut bare = rec.clone();
        bare.contest.as_deref_mut().expect("provenance").adif = Vec::new();
        assert_eq!(
            adif_record(&rec),
            adif_record(&bare),
            "the writer omits an empty value, so the export cannot witness this column"
        );

        let mut db = LogDb::open_in_memory().unwrap();
        db.insert(&rec, Resolved::default()).unwrap();
        let back = db.load_all().unwrap();
        assert_eq!(
            back[0].contest.as_deref().map(|c| c.adif.as_slice()),
            Some(&[("CLASS".to_string(), String::new())][..]),
            "the store must carry the value the export cannot show"
        );
        assert_eq!(back[0], rec, "and the record is unchanged");
    }

    /// ⚠️ A directed column whose tag is ALREADY in `extra` must not be able to destroy the
    /// contact.
    ///
    /// This is why the directed columns get their own table. In `qso_extra` the name would
    /// hit `PRIMARY KEY (qso_id, name)`, the INSERT would fail, and because the whole batch
    /// is one transaction the QSO would be LOST — where `log.adi` merely writes the tag
    /// twice. Here the two live in different tables, so the collision is not a collision at
    /// all: both values survive, and the export is byte-for-byte what the unstored record
    /// exports.
    #[test]
    fn a_directed_column_colliding_with_the_passthrough_cannot_lose_the_contact() {
        let mut rec = merged_fd_row();
        // A stale `ARRL_SECT` from some earlier import, sitting in the passthrough while the
        // exchange carries its own.
        rec.extra = vec![("ARRL_SECT".to_string(), "WMA".to_string())];

        let mut db = LogDb::open_in_memory().unwrap();
        db.insert(&rec, Resolved::default()).unwrap();
        let back = db.load_all().unwrap();
        assert_eq!(back.len(), 1, "the contact must survive the collision");

        assert_eq!(
            back[0].extra,
            vec![("ARRL_SECT".to_string(), "WMA".to_string())],
            "the passthrough value is kept"
        );
        assert_eq!(
            back[0]
                .contest
                .as_deref()
                .expect("provenance")
                .adif
                .iter()
                .find(|(t, _)| t == "ARRL_SECT")
                .map(|(_, v)| v.as_str()),
            Some("EMA"),
            "and so is the exchange's own"
        );
        // Neither value is dropped, and the store did not change what the record exports:
        // the writer emitting the tag twice is its own pre-existing behaviour, identical
        // with and without the store, and not something the store may silently 'fix' by
        // throwing one of the operator's two values away.
        assert_eq!(adif_record(&rec), adif_record(&back[0]));
    }

    /// ⚠️ The ADIF writer's duplicate-`STATE` guard still fires after a store round trip.
    ///
    /// The guard reads `contest.adif` to decide whether the resolver's `r.state` has already
    /// been written by the exchange (§2.1.1: the exchange wins, and it writes once). Had the
    /// directed columns come back through `extra` instead, the guard would see an empty
    /// `contest.adif`, write `r.state` as well, and hand TQSL two `<STATE>` fields.
    #[test]
    fn the_duplicate_state_guard_still_fires_on_a_stored_record() {
        let mut rec = merged_fd_row();
        // A QSO party's shape: the exchange carries the state, and the DXCC/callbook
        // resolver has since filled in a DIFFERENT guess for the same station.
        //
        // ⭐ The two values must disagree, or this test cannot fail. With both "MA", a build
        // that lost `contest.adif` would emit the resolver's "MA" from `r.state`, still write
        // exactly one `<STATE>`, and pass — the assertion would be inert. Disagreeing values
        // are what make the count AND the winner observable.
        rec.contest
            .as_deref_mut()
            .expect("provenance")
            .adif
            .push(("STATE".to_string(), "MA".to_string()));
        rec.state = Some("RI".to_string());

        let mut db = LogDb::open_in_memory().unwrap();
        db.insert(&rec, Resolved::default()).unwrap();
        let back = db.load_all().unwrap();
        let adif = adif_record(&back[0]);

        assert_eq!(
            adif.matches("<STATE:").count(),
            1,
            "exactly one STATE, or TQSL gets the undefined territory: {adif}"
        );
        assert!(
            adif.contains("<STATE:2>MA"),
            "the EXCHANGE wins — what they told me on the air, not the resolver's guess: {adif}"
        );
        assert!(
            !adif.contains("<STATE:2>RI"),
            "the resolver's guess must not reach the export: {adif}"
        );
    }

    /// ⚠️ The writer's upsert path REPLACES the directed columns rather than appending them.
    ///
    /// An edit to a contest row goes through [`LogDb::apply`], which rewrites each child
    /// table after deleting it. Without that delete the second write re-inserts the same
    /// `ord` values, hits `PRIMARY KEY (qso_id, ord)`, and takes the whole transaction — and
    /// so the operator's edit — down with it.
    #[test]
    fn an_upsert_replaces_the_directed_columns_rather_than_colliding_with_them() {
        let rec = Arc::new(merged_fd_row());
        let mut db = LogDb::open_in_memory().unwrap();
        let write = [RowWrite::new(Arc::clone(&rec))];
        let batch = || Batch {
            clear: false,
            remove: &[],
            upsert: &write,
            marks: None,
        };
        db.apply(batch()).expect("first write");
        db.apply(batch()).expect("the same row written again");

        let rows: i64 = db
            .conn
            .query_row("SELECT count(*) FROM qso_contest_adif", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3, "replaced, not accumulated");
        assert_eq!(
            adif_record(&db.load_all().unwrap()[0]),
            adif_record(&rec),
            "and the row still exports what it exported"
        );
    }

    /// A contest block carrying NOTHING but its directed columns is still a contest block.
    ///
    /// [`contest_is_empty`] decides whether a loaded block is real or an artefact of every
    /// column being NULL. It has to count `adif`, or the sweep's own rows are read and then
    /// immediately thrown away.
    #[test]
    fn a_contest_block_of_only_directed_columns_survives_the_emptiness_test() {
        let mut rec = merged_fd_row();
        {
            let c = rec.contest.as_deref_mut().expect("provenance");
            *c = ContestFields {
                adif: std::mem::take(&mut c.adif),
                ..ContestFields::default()
            };
        }
        let mut db = LogDb::open_in_memory().unwrap();
        db.insert(&rec, Resolved::default()).unwrap();
        let back = db.load_all().unwrap();
        assert_eq!(
            back[0].contest.as_deref().map(|c| c.adif.len()),
            Some(3),
            "the block must not be discarded as empty"
        );
    }

    // ── copy_database ────────────────────────────────────────────────────────

    /// A directory of the test's own, gone with the value. Named for the test AND the thread,
    /// because this module's tests run in parallel inside one process.
    struct CopyDir(std::path::PathBuf);
    impl CopyDir {
        fn new(tag: &str) -> CopyDir {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let p = std::env::temp_dir().join(format!(
                "nexus-dbcopy-{tag}-{}-{:?}-{nanos}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&p).unwrap();
            CopyDir(p)
        }
    }
    impl Drop for CopyDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `n` contacts, each with an id, a `COMMENT` naming its index, and a foreign tag — so
    /// every child table the copy must carry has rows in it.
    fn copy_rows(n: usize, base: u64) -> Vec<QsoRecord> {
        (0..n)
            .map(|i| {
                let mut r = parse_adif(&format!(
                    "<CALL:6>K{:01}COPY<QSO_DATE:8>20260901<TIME_ON:6>{:02}{:02}00<BAND:3>20m\
                     <MODE:3>FT8<COMMENT:7>row{:04}<APP_OTHER_LOGGER:3>xyz\
                     <APP_TEMPO_UL_QRZ:19>accepted|1700000500<EOR>",
                    i % 10,
                    i / 60 % 24,
                    i % 60,
                    i
                ))
                .remove(0);
                r.id = Some(RecordId::Provisional {
                    hash: base + i as u64,
                    ordinal: 0,
                });
                r
            })
            .collect()
    }

    /// A store whose connection is KEPT OPEN: its committed rows sit in the `-wal` file.
    fn live_store(path: &Path, rows: &[QsoRecord]) -> LogDb {
        let mut db = LogDb::open(path).unwrap();
        db.insert_all(rows.iter().map(|r| (r, Resolved::default())))
            .unwrap();
        db
    }

    fn wal_of(path: &Path) -> std::path::PathBuf {
        let mut name = path.file_name().unwrap().to_os_string();
        name.push("-wal");
        path.with_file_name(name)
    }

    /// ★ The whole point: a copy of a LIVE store carries every committed contact, including the
    /// ones that exist only in its `-wal` — and the copy is a single self-contained file, so no
    /// sidecar has to travel with it.
    #[test]
    fn a_copy_of_a_live_store_carries_what_only_the_wal_holds() {
        let d = CopyDir::new("live");
        let (src, dst) = (
            d.0.join("log.sqlite3"),
            d.0.join("moved").join("log.sqlite3"),
        );
        let rows = copy_rows(64, 1_000);
        let live = live_store(&src, &rows);
        assert!(
            std::fs::metadata(wal_of(&src))
                .map(|m| m.len())
                .unwrap_or(0)
                > 0,
            "premise: the contacts are in the WAL, not yet in the main file"
        );

        let bytes = copy_database(&src, &dst).expect("copied");
        assert!(bytes > 0);
        assert!(
            !wal_of(&dst).exists(),
            "the copy is one self-contained file: no WAL travels with it"
        );
        let copied = LogDb::open(&dst).unwrap().load_all().unwrap();
        assert_eq!(copied, rows, "every contact, in order, with its children");
        assert_eq!(
            live.load_all().unwrap(),
            rows,
            "and the original store was not touched"
        );
    }

    /// ⛔ A commit landing BETWEEN the picture and the comparison makes the comparison meaningless
    /// — it compares two different logs — so that attempt must be thrown away and the copy taken
    /// again. The seam commits a row in exactly that window on the first attempt only.
    ///
    /// Control: the row committed in the window is IN the copy that was kept, which it can only
    /// be if the first picture (taken before it) was discarded and a second one taken after.
    #[test]
    fn a_commit_during_the_copy_discards_the_picture_and_takes_another() {
        let d = CopyDir::new("race");
        let (src, dst) = (d.0.join("log.sqlite3"), d.0.join("to").join("log.sqlite3"));
        let rows = copy_rows(20, 2_000);
        let mut other = live_store(&src, &rows);
        let late = copy_rows(1, 9_000).remove(0);
        let mut attempts = Vec::new();
        copy_database_observed(&src, &dst, &mut |attempt| {
            attempts.push(attempt);
            if attempt == 0 {
                other.insert(&late, Resolved::default()).unwrap();
            }
        })
        .expect("the second picture is proved");
        assert_eq!(attempts, vec![0, 1], "the first picture was thrown away");
        let copied = LogDb::open(&dst).unwrap().load_all().unwrap();
        assert_eq!(
            copied.len(),
            21,
            "the copy kept is the one taken AFTER the commit"
        );
        assert_eq!(copied.last(), Some(&late));
    }

    /// A log that never stops changing is refused rather than copied from a picture nobody
    /// proved — and refusing leaves nothing at the destination.
    #[test]
    fn a_log_that_keeps_changing_is_refused_and_leaves_nothing_behind() {
        let d = CopyDir::new("churn");
        let (src, dst) = (d.0.join("log.sqlite3"), d.0.join("to").join("log.sqlite3"));
        let mut other = live_store(&src, &copy_rows(5, 3_000));
        let mut next = 10_000;
        let err = copy_database_observed(&src, &dst, &mut |_| {
            next += 1;
            other
                .insert(&copy_rows(1, next).remove(0), Resolved::default())
                .unwrap();
        })
        .expect_err("never proved");
        assert!(
            matches!(&err, Error::Copy { reason } if reason.contains("kept changing")),
            "{err}"
        );
        assert!(!dst.exists(), "an unproved copy never takes the name");
        let leftovers: Vec<_> = std::fs::read_dir(dst.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(
            leftovers.is_empty(),
            "no temporary left behind: {leftovers:?}"
        );
    }

    /// The comparison is not a formality: a copy that differs from the original by one value in
    /// one row, or by one row, is caught. Driven through `verify_copy` directly, against a copy
    /// that is then deliberately damaged — the positive controls for the proof itself.
    #[test]
    fn the_proof_catches_a_changed_value_and_a_missing_row() {
        let d = CopyDir::new("proof");
        let src = d.0.join("log.sqlite3");
        let _live = live_store(&src, &copy_rows(12, 4_000));
        let conn = Connection::open(&src).unwrap();
        let copy = d.0.join("copy.sqlite3");
        let copy_str = copy.to_str().unwrap();

        conn.execute("VACUUM INTO ?1", [copy_str]).unwrap();
        assert_eq!(
            verify_copy(&conn, copy_str).unwrap(),
            Ok(()),
            "control: an untouched copy is proved"
        );

        // One value in one child row.
        Connection::open(&copy)
            .unwrap()
            .execute(
                "UPDATE qso_extra SET value = 'zzz' WHERE rowid = (SELECT max(rowid) FROM qso_extra)",
                [],
            )
            .unwrap();
        let verdict = verify_copy(&conn, copy_str).unwrap();
        assert!(
            matches!(&verdict, Err(why) if why.contains("qso_extra")),
            "a changed value is caught: {verdict:?}"
        );

        // One whole row.
        let _ = std::fs::remove_file(&copy);
        conn.execute("VACUUM INTO ?1", [copy_str]).unwrap();
        Connection::open(&copy)
            .unwrap()
            .execute(
                "DELETE FROM qso WHERE rowid = (SELECT max(rowid) FROM qso)",
                [],
            )
            .unwrap();
        let verdict = verify_copy(&conn, copy_str).unwrap();
        assert!(
            matches!(&verdict, Err(why) if why.contains("different number of rows")),
            "a missing row is caught: {verdict:?}"
        );
    }

    /// ⛔ A database that used to have the destination's name may have left its own `-wal`
    /// behind. SQLite pairs a WAL with a database by NAME, so a stale one would be replayed
    /// into the copy the first time it is opened. Built for real: a store is written at the
    /// destination, its WAL saved while it holds frames, and restored after the store closed.
    ///
    /// Control: the stale WAL really is live ammunition — opened beside its own database it
    /// brings that store's rows back.
    #[test]
    fn a_stale_wal_at_the_destination_is_not_replayed_into_the_copy() {
        let d = CopyDir::new("stale");
        let src = d.0.join("src").join("log.sqlite3");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        let dst = d.0.join("dst").join("log.sqlite3");
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();

        // The old store at the destination, and a snapshot of its main file + live WAL.
        let old_rows = copy_rows(30, 5_000);
        let old = live_store(&dst, &old_rows);
        let saved_main = std::fs::read(&dst).unwrap();
        let saved_wal = std::fs::read(wal_of(&dst)).unwrap();
        assert!(
            !saved_wal.is_empty(),
            "premise: the old store's rows are in its WAL"
        );
        drop(old);

        // Control: the saved pair, put back, IS the old store — the WAL is live ammunition.
        std::fs::write(&dst, &saved_main).unwrap();
        std::fs::write(wal_of(&dst), &saved_wal).unwrap();
        assert_eq!(
            LogDb::open(&dst).unwrap().row_count().unwrap(),
            30,
            "control: the stale WAL replays the old store's rows"
        );
        // Put the stale pair back once more, then copy a DIFFERENT store over it.
        std::fs::write(&dst, &saved_main).unwrap();
        std::fs::write(wal_of(&dst), &saved_wal).unwrap();

        let new_rows = copy_rows(7, 6_000);
        let _src = live_store(&src, &new_rows);
        copy_database(&src, &dst).expect("copied over the old store");
        assert_eq!(
            LogDb::open(&dst).unwrap().load_all().unwrap(),
            new_rows,
            "the copy is the new store, with nothing of the old one replayed into it"
        );
    }

    /// A copy never creates a database that was not there, and never stamps a version onto one
    /// it did not write: the source is opened without either.
    #[test]
    fn copying_a_missing_database_is_an_error_and_creates_nothing() {
        let d = CopyDir::new("missing");
        let (src, dst) = (
            d.0.join("absent.sqlite3"),
            d.0.join("to").join("log.sqlite3"),
        );
        assert!(copy_database(&src, &dst).is_err());
        assert!(
            !src.exists(),
            "the source was not created by asking to copy it"
        );
        assert!(!dst.exists());
    }

    // ── one picture of the log ───────────────────────────────────────────────

    /// ⛔ A LOAD IS ONE PICTURE OF THE LOG, WHATEVER COMMITS WHILE IT READS. `load_all` reads
    /// five tables — the passthrough, the stamps, the exchange and the directed columns, then
    /// the rows — and read one statement at a time, each statement saw the store as it stood
    /// when THAT statement began. A contact committed between the child tables and the rows came
    /// back without its children: its foreign tags and its upload stamps had been read before it
    /// existed. The re-read after another window's commit (`LogStore::reload`) loads while this
    /// process's own writer, or the other window's, may commit, and a contact read that way and
    /// later written back from memory takes its tags and stamps out of the store for good.
    ///
    /// The contact is committed through a second connection at exactly that point.
    #[test]
    fn a_load_is_one_picture_of_the_log_whatever_commits_during_it() {
        let d = CopyDir::new("torn");
        let path = d.0.join("log.sqlite3");
        let before = copy_rows(8, 7_000);
        // A second writer on the same file — another window's, or this process's own.
        let mut writer = live_store(&path, &before);
        // A contact with a foreign tag and a QRZ stamp: two child tables it must carry.
        let late = copy_rows(1, 9_000);
        assert!(
            !late[0].extra.is_empty() && late[0].upload.qrz.is_some(),
            "premise: the late contact has children to lose"
        );
        let reader = LogDb::open(&path).unwrap();
        let loaded = reader
            .load_all_observed(&mut || {
                writer
                    .insert_all(late.iter().map(|r| (r, Resolved::default())))
                    .expect("the other writer commits");
            })
            .unwrap();
        assert_eq!(
            loaded, before,
            "the load is the log as it stood when the load began — never a contact without \
             its children"
        );
        // Control: the commit really landed, and a load that begins after it sees the contact,
        // whole.
        let after = LogDb::open(&path).unwrap().load_all().unwrap();
        assert_eq!(after.len(), before.len() + 1, "control: the commit landed");
        assert_eq!(after.last(), late.last(), "and the contact is whole");
    }

    // ── a partial read ───────────────────────────────────────────────────────

    /// ★ `rows_by_ids` IS `load_all`, FILTERED — over 150,000 synthetic contacts carrying every
    /// representation the store holds (contest blocks and their exchange, foreign tags, all
    /// four upload stamps, the `""` band, portable calls). Every id is swept in windows of
    /// 10,000, then random picks — repeated ids, ids the store never held, asked for in reverse
    /// — and each answer is exactly the records `load_all` returns for those ids, in log order.
    /// The windows and the larger picks cross `ROWS_PER_STATEMENT`, so the chunking is inside
    /// the proof.
    #[test]
    fn rows_by_ids_is_load_all_filtered() {
        use proptest::prelude::*;
        use std::collections::HashSet;
        let n = 150_000;
        let d = CopyDir::new("byid");
        let path = d.0.join("log.sqlite3");
        {
            let mut records = parse_adif(&synthetic_log(n));
            for (i, r) in records.iter_mut().enumerate() {
                r.id = Some(RecordId::Provisional {
                    hash: i as u64,
                    ordinal: 0,
                });
            }
            let mut db = LogDb::open(&path).unwrap();
            db.insert_all(records.iter().map(|r| (r, Resolved::default())))
                .unwrap();
        }
        let db = LogDb::open(&path).unwrap();
        let all = db.load_all().unwrap();
        assert_eq!(all.len(), n, "premise: every synthetic record stored");
        assert!(
            all.iter().any(|r| r.contest.is_some())
                && all.iter().any(|r| !r.extra.is_empty())
                && all.iter().any(|r| r.upload.clublog.is_some()),
            "premise: the fixture carries the child tables a partial read must assemble"
        );
        let id = |r: &QsoRecord| r.id.expect("every stored record has an id");

        for window in all.chunks(10_000) {
            let ids: Vec<RecordId> = window.iter().map(id).collect();
            assert_eq!(db.rows_by_ids(&ids).unwrap(), window, "a window of ids");
        }

        let runner = proptest::test_runner::Config {
            cases: 32,
            ..proptest::test_runner::Config::default()
        };
        proptest!(runner, |(
            picks in prop::collection::vec(0..n, 0..2_000),
            strangers in 0u64..4,
        )| {
            let mut ids: Vec<RecordId> = picks.iter().map(|&i| id(&all[i])).collect();
            ids.extend((0..strangers).map(|k| RecordId::Provisional {
                hash: 10_000_000 + k,
                ordinal: 0,
            }));
            ids.reverse();
            let wanted: HashSet<RecordId> = picks.iter().map(|&i| id(&all[i])).collect();
            let expected: Vec<QsoRecord> = all
                .iter()
                .filter(|r| wanted.contains(&id(r)))
                .cloned()
                .collect();
            prop_assert_eq!(db.rows_by_ids(&ids).unwrap(), expected);
        });
    }
}
