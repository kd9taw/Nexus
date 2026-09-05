//! ARRL Field Day mode: the Class+Section exchange, an auto-sequencer that runs
//! operator-initiated two-way contacts, a dupe-checked log with scoring and a
//! section multiplier, and ADIF / Cabrillo export.
//!
//! Field Day requires operator-initiated contacts (no fully-automated QSOs), and
//! the exchange is **Class + ARRL/RAC Section** (e.g. `3A WI`). FT1 carries this
//! natively in one frame: `<to> <de> <class> <section>` (and the rogered
//! `<to> <de> R <class> <section>`).

use crate::message::Msg;
use modes::Decode;
use std::collections::HashSet;

/// Which Field Day event is running — they share the exchange SHAPE
/// (designator + ARRL/RAC section) but differ in designator grammar, contest
/// ids, and scoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FdEvent {
    /// ARRL Field Day (4th full June weekend): class like `3A`, phone 1 pt,
    /// CW/digital 2 pts, power multiplier, ~100-pt bonus menu.
    #[default]
    ArrlFd,
    /// Winter Field Day (last full January weekend): category like `2O`
    /// (count + Home/Indoor/Mobile/Outdoor).
    WinterFd,
}

impl FdEvent {
    pub fn contest_id(self) -> &'static str {
        match self {
            FdEvent::ArrlFd => "ARRL-FIELD-DAY",
            FdEvent::WinterFd => "WFD",
        }
    }
    pub fn from_code(s: &str) -> Self {
        if s.trim().eq_ignore_ascii_case("wfd") {
            FdEvent::WinterFd
        } else {
            FdEvent::ArrlFd
        }
    }
}

/// Per-QSO points by operating mode class (both events: phone 1, CW/digital 2 —
/// the long-standing ARRL FD values; WFD currently matches for the base QSO
/// point, with its own multiplier system handled at the score layer).
pub fn qso_points_for_mode(mode: &str) -> u32 {
    match mode.to_ascii_uppercase().as_str() {
        "PH" | "PHONE" | "SSB" | "FM" => 1,
        _ => 2, // CW + digital
    }
}

/// A Field Day exchange: transmitter class (e.g. `3A`) + ARRL/RAC section (`WI`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exchange {
    pub class: String,
    pub section: String,
}

impl Exchange {
    pub fn new(class: &str, section: &str) -> Self {
        Self {
            class: class.to_string(),
            section: section.to_string(),
        }
    }
}

/// A logged Field Day contact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoggedQso {
    pub call: String,
    pub class: String,
    pub section: String,
    pub band: String,
    /// Mode class for scoring + per-band-mode dupes: "DIG" | "CW" | "PH".
    pub mode: String,
    /// The ACTUAL on-air mode behind a "DIG" class (ADIF name, uppercase:
    /// "FT8", "RTTY", "SSTV"…). Empty = not recorded (legacy rows) — exports
    /// fall back to the historical class map. WFD bans the WSJT modes but
    /// allows RTTY/SSTV, so an export must never claim "FT8" for an RTTY QSO.
    pub submode: String,
    pub slot: u64,
    /// Unix seconds when the contact was logged — Cabrillo requires a real
    /// `yyyy-mm-dd hhmm` per QSO line (the old `----------` placeholder would
    /// FAIL ARRL submission). 0 in legacy/test paths = falls back to the
    /// placeholder rather than inventing a date.
    pub when_unix: u64,
    /// Per-position monotonic sequence number — this contact's half of the
    /// club-sync QSO id `(position id, seq)`. Stamped by
    /// [`FieldDayLog::log_submode_at`] at log time (every entry point funnels
    /// there), journaled as `APP_NEXUS_QSEQ`, and restored by
    /// [`FieldDayLog::merge_adif`]. Deliberately clock-free: positions' clocks
    /// disagree at a generator-powered site, so ids never derive from time.
    /// 0 = never assigned (rows built by paths that predate sync).
    pub seq: u64,
}

/// A dupe-checked Field Day log with scoring.
#[derive(Debug)]
pub struct FieldDayLog {
    pub mycall: String,
    pub myexch: Exchange,
    pub band: String,
    pub event: FdEvent,
    /// The ACTUAL on-air digital mode currently keyed (ADIF-style name, e.g.
    /// "FT8", "FT4"), stamped by the engine at FD entry and on every tier
    /// change — the funnel that fills [`LoggedQso::submode`] for the digital
    /// sequencer's own [`log`](Self::log) calls, so a WFD RTTY/FT4 contact is
    /// never exported or pushed as "FT8". Applies to the "DIG" class only:
    /// CW/PH manual entries ARE their on-air mode and keep an empty submode.
    pub current_submode: String,
    qsos: Vec<LoggedQso>,
    worked: HashSet<(String, String, String)>, // (call, band, mode class)
    /// The next [`LoggedQso::seq`] to stamp. Starts at 1; a journal restore
    /// advances it past every restored seq so a fresh session's rows continue
    /// the per-position monotonic sequence instead of colliding with rows the
    /// club host already merged.
    next_seq: u64,
}

impl FieldDayLog {
    pub fn new(mycall: &str, myexch: Exchange, band: &str) -> Self {
        Self {
            mycall: mycall.to_string(),
            myexch,
            band: band.to_string(),
            event: FdEvent::ArrlFd,
            current_submode: String::new(),
            qsos: Vec::new(),
            worked: HashSet::new(),
            next_seq: 1,
        }
    }

    /// The highest [`LoggedQso::seq`] this log has stamped or restored — the
    /// position's sync high-water mark (`join.max_seq`, and the base the
    /// outbox "rows past the host's ack" is computed from). 0 = empty log.
    pub fn max_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// Already worked this call on this band IN THIS MODE CLASS? (ARRL FD
    /// rules: each station counts once per band-mode — CW, digital and phone
    /// are separate contacts.) The digital sequencer always logs "DIG".
    pub fn is_dupe(&self, call: &str) -> bool {
        self.is_dupe_mode(call, "DIG")
    }

    pub fn is_dupe_mode(&self, call: &str, mode: &str) -> bool {
        self.worked.contains(&(
            call.to_uppercase(),
            self.band.clone(),
            mode.to_ascii_uppercase(),
        ))
    }

    /// Whether an EXPLICIT `(call, band, mode class)` key is in the dupe
    /// index — unlike [`is_dupe_mode`](Self::is_dupe_mode) it does not assume
    /// the log's current band. The club sync uses it to subtract own-log keys
    /// from the club dupe set (only club-ONLY keys ship to the UI).
    pub fn worked_key(&self, call: &str, band: &str, mode: &str) -> bool {
        self.worked.contains(&(
            call.to_uppercase(),
            band.to_string(),
            mode.to_ascii_uppercase(),
        ))
    }

    /// Log a contact. Returns false (and logs nothing) if it's a dupe.
    pub fn log(&mut self, call: &str, class: &str, section: &str, slot: u64) -> bool {
        self.log_mode_at(call, class, section, "DIG", slot, now_unix())
    }

    /// As [`log`](Self::log) with an explicit timestamp (tests / replays).
    pub fn log_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        self.log_mode_at(call, class, section, "DIG", slot, when_unix)
    }

    /// All-mode entry (the CW/Phone cockpits log FD contacts too): mode is the
    /// scoring class "DIG" | "CW" | "PH".
    pub fn log_mode_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        // The "DIG" class covers many on-air modes, so a digital entry stamps
        // [`current_submode`](Self::current_submode) (what the engine says is
        // actually keyed). CW/PH ARE their on-air mode — no submode.
        let submode = if mode.eq_ignore_ascii_case("DIG") {
            self.current_submode.clone()
        } else {
            String::new()
        };
        self.log_submode_at(call, class, section, mode, &submode, slot, when_unix)
    }

    /// As [`log_mode_at`](Self::log_mode_at) but also recording the ACTUAL
    /// on-air mode behind the scoring class (e.g. class "DIG", submode "RTTY")
    /// so exports emit the real mode. Dupes stay keyed on the CLASS — FD/WFD
    /// digital is ONE mode class, so an FT8 QSO dupes the same-band RTTY one.
    #[allow(clippy::too_many_arguments)] // log_mode_at + the one extra field
    pub fn log_submode_at(
        &mut self,
        call: &str,
        class: &str,
        section: &str,
        mode: &str,
        submode: &str,
        slot: u64,
        when_unix: u64,
    ) -> bool {
        let mode = mode.to_ascii_uppercase();
        if self.is_dupe_mode(call, &mode) {
            return false;
        }
        self.worked
            .insert((call.to_uppercase(), self.band.clone(), mode.clone()));
        let seq = self.next_seq;
        self.next_seq += 1;
        self.qsos.push(LoggedQso {
            call: call.to_string(),
            class: class.to_string(),
            section: section.to_string(),
            band: self.band.clone(),
            mode,
            submode: submode.trim().to_ascii_uppercase(),
            slot,
            when_unix,
            seq,
        });
        true
    }

    pub fn qso_count(&self) -> usize {
        self.qsos.len()
    }

    pub fn qsos(&self) -> &[LoggedQso] {
        &self.qsos
    }

    /// Distinct ARRL/RAC sections worked (the Field Day multiplier).
    pub fn sections(&self) -> usize {
        self.qsos
            .iter()
            .map(|q| q.section.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    /// The distinct sections worked — the identities behind the
    /// [`sections`](Self::sections) count, sorted for a stable board order
    /// (the worked-sections color board, spec §5).
    pub fn worked_sections(&self) -> Vec<String> {
        let mut sections: Vec<String> = self
            .qsos
            .iter()
            .map(|q| q.section.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        sections.sort();
        sections
    }

    /// Per-mode QSO points (phone 1, CW/digital 2) — power multiplier and
    /// bonuses are applied at the score layer (engine), not here.
    pub fn qso_points(&self) -> u32 {
        self.qsos.iter().map(|q| qso_points_for_mode(&q.mode)).sum()
    }

    /// Export the log as ADIF records (one `<EOR>` per QSO).
    pub fn adif(&self) -> String {
        let mut s = String::from("ADIF Export from Nexus\n<PROGRAMID:5>Nexus\n<EOH>\n");
        for q in &self.qsos {
            s.push_str(&adif_field("CALL", &q.call));
            // ⚠️ A MODE OUTSIDE ADIF'S ENUMERATION IS A DROPPED RECORD, NOT A COSMETIC ONE.
            // This wrote `q.submode` raw, and the tiers stamp names ADIF has never heard of
            // ("TempoFast", "TempoDeep", "FT2") — the main logbook already solved that with
            // `logbook::adif_submode`, whose own comment spells out the cost: a bare
            // <MODE:3>FT2 misses all three legs of TQSL's MODE%SUBMODE → SUBMODE → MODE
            // cascade and the record is DROPPED with "Invalid MODE". The Field Day exporter
            // simply never called it, so a Field Day contact worked on a Tempo tier was lost
            // on upload. Same cascade now, so the two exports cannot disagree.
            let recorded = if q.submode.is_empty() {
                match q.mode.as_str() {
                    "CW" => "CW",
                    "PH" => "SSB",
                    _ => "FT8",
                }
            } else {
                q.submode.as_str()
            };
            match crate::logbook::adif_submode(recorded) {
                Some((parent, sub)) => {
                    s.push_str(&adif_field("MODE", parent));
                    s.push_str(&adif_field("SUBMODE", sub));
                }
                None => s.push_str(&adif_field("MODE", recorded)),
            }
            s.push_str(&adif_field("BAND", &q.band));
            // A real date/time so [`merge_adif`](Self::merge_adif) can restore
            // `when_unix` (and Cabrillo keeps its ARRL-required timestamps
            // across a restart). Legacy rows without a stamp omit both fields
            // rather than inventing a date.
            if q.when_unix > 0 {
                let (date, time) = adif_datetime(q.when_unix);
                s.push_str(&adif_field("QSO_DATE", &date));
                s.push_str(&adif_field("TIME_ON", &time));
            }
            s.push_str(&adif_field("CONTEST_ID", self.event.contest_id()));
            s.push_str(&adif_field("CLASS", &q.class));
            s.push_str(&adif_field("ARRL_SECT", &q.section));
            // The per-position sync sequence (APP_-namespaced per the ADIF
            // spec, so every other consumer ignores it). Legacy 0 rows omit
            // the tag — restore backfills them in row order.
            if q.seq > 0 {
                s.push_str(&adif_field("APP_NEXUS_QSEQ", &q.seq.to_string()));
            }
            s.push_str("<EOR>\n");
        }
        s
    }

    /// Merge a previously-flushed ADIF journal (see [`adif`](Self::adif)) back
    /// into this log — the restore half of the durable Field Day backup, so a
    /// restart mid-event doesn't reset the contest log. Rows missing a CALL or
    /// stamped before `min_when_unix` are skipped (a previous event's journal
    /// self-expires), rows already in the dupe index are skipped, and garbage
    /// input merges nothing — never an error. Restored dupe keys keep the ROW's
    /// band, so they survive a mid-event QSY.
    pub fn merge_adif(&mut self, text: &str, min_when_unix: u64) {
        // Minimal `<NAME:len>value` tokenizer mirroring logbook.rs `parse_adif`
        // (this journal only needs the handful of FD tags).
        let body = match text.to_ascii_uppercase().find("<EOH>") {
            Some(i) => &text[i + 5..],
            None => text,
        };
        let mut cur: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let bytes = body.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'<' {
                i += 1;
                continue;
            }
            let end = match body[i..].find('>') {
                Some(e) => i + e,
                None => break,
            };
            let tag = &body[i + 1..end];
            i = end + 1;
            if tag.eq_ignore_ascii_case("EOR") {
                self.restore_row(&cur, min_when_unix);
                cur.clear();
                continue;
            }
            // NAME:len or NAME:len:type
            let mut parts = tag.splitn(3, ':');
            let name = parts.next().unwrap_or("").to_ascii_uppercase();
            let len: usize = parts
                .next()
                .and_then(|l| l.trim().parse().ok())
                .unwrap_or(0);
            let val = body.get(i..i + len).unwrap_or("").to_string();
            i += len;
            cur.insert(name, val);
        }
    }

    /// One tokenized journal record → the log (the dupe-checked insert half of
    /// [`merge_adif`](Self::merge_adif)).
    fn restore_row(&mut self, f: &std::collections::HashMap<String, String>, min_when_unix: u64) {
        let Some(call) = f.get("CALL").filter(|c| !c.trim().is_empty()) else {
            return;
        };
        // ADIF MODE → (mode class, actual mode): the reverse of the map in
        // [`adif`](Self::adif) — keep the two in step. A digital MODE keeps its
        // identity as the submode so RTTY/SSTV rows survive the round-trip.
        let (mode, submode) = match f.get("MODE").map(|m| m.to_ascii_uppercase()) {
            Some(m) if m == "CW" => ("CW", String::new()),
            Some(m) if m == "SSB" => ("PH", String::new()),
            Some(m) => ("DIG", m),
            None => ("DIG", String::new()),
        };
        // QSO_DATE (yyyymmdd) + TIME_ON (hhmmss) → when_unix. Unparseable rows
        // stamp 0 and fall to the age gate (never a panic on garbage input).
        let parse_dt = |d: &str, t: &str| -> Option<u64> {
            Some(unix_from_ymdhms(
                d.get(0..4)?.parse().ok()?,
                d.get(4..6)?.parse().ok()?,
                d.get(6..8)?.parse().ok()?,
                t.get(0..2)?.parse().ok()?,
                t.get(2..4)?.parse().ok()?,
                t.get(4..6)?.parse().ok()?,
            ))
        };
        let when_unix = match (f.get("QSO_DATE"), f.get("TIME_ON")) {
            (Some(d), Some(t)) => parse_dt(d, t).unwrap_or(0),
            _ => 0,
        };
        if when_unix < min_when_unix {
            return;
        }
        // The ROW's band, not the log's current one — a restored dupe key must
        // keep its original band across a mid-event QSY.
        let band = f.get("BAND").cloned().unwrap_or_default();
        let key = (call.to_uppercase(), band.clone(), mode.to_string());
        if self.worked.contains(&key) {
            return;
        }
        self.worked.insert(key);
        // The journaled sync seq round-trips; a legacy row without the tag
        // backfills the next free seq in row order (1..n on a whole legacy
        // journal), so pre-sync journals join the sequence deterministically.
        let seq = f
            .get("APP_NEXUS_QSEQ")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(self.next_seq);
        self.next_seq = self.next_seq.max(seq + 1);
        self.qsos.push(LoggedQso {
            call: call.clone(),
            class: f.get("CLASS").cloned().unwrap_or_default(),
            section: f.get("ARRL_SECT").cloned().unwrap_or_default(),
            band,
            mode: mode.to_string(),
            submode,
            slot: 0,
            when_unix,
            seq,
        });
    }

    /// Export the log in Cabrillo QSO-line form for the given band frequency (kHz).
    pub fn cabrillo(&self, freq_khz: u32) -> String {
        let mut s = String::new();
        s.push_str("START-OF-LOG: 3.0\n");
        s.push_str(&format!("CONTEST: {}\n", self.event.contest_id()));
        s.push_str(&format!("CALLSIGN: {}\n", self.mycall));
        s.push_str(&format!(
            "CATEGORY-OPERATOR: MULTI-OP\nLOCATION: {}\nCREATED-BY: Nexus\n",
            self.myexch.section
        ));
        // Which rules data scored this log (X- headers are Cabrillo-legal and
        // ignored by robots) — a fetched rules file with different parameters
        // is visible on the artifact an operator actually submits.
        s.push_str(&format!(
            "X-NEXUS-RULES-YEAR: {}\n",
            crate::fd_rules::ruleset(self.event, crate::fd_rules::CURRENT_RULES_YEAR).rules_year
        ));
        for q in &self.qsos {
            // QSO: freq mo date time mycall myexch call exch — ARRL requires a
            // REAL `yyyy-mm-dd hhmm`; the old `----------` placeholder failed
            // submission. Legacy rows without a stamp keep the placeholder so
            // we never invent a time.
            let (date, time) = if q.when_unix > 0 {
                cabrillo_datetime(q.when_unix)
            } else {
                ("----------".to_string(), "----".to_string()) // HHMM is 4 chars
            };
            // Mode token per Cabrillo 3.0: CW, PH phone, RY for RTTY rows
            // (WFD prefers it), DG for other/unrecorded digital — a legal
            // fallback either event.
            let mo = match q.mode.as_str() {
                "CW" => "CW",
                "PH" => "PH",
                _ if q.submode == "RTTY" => "RY",
                _ => "DG",
            };
            // Per-QSO frequency from ITS band. The caller's dial is a fallback ONLY for a
            // row with no band recorded at all — never for a band we simply failed to map,
            // because that is how a 23 cm club contact came to export as 20 m: the club
            // exporter passes a hardcoded 14 MHz and every unmapped band took it. A band we
            // do not recognise now rides through as itself; a wrong-looking token in one
            // field beats a confident lie about which band a contact was made on, and ARRL
            // Field Day requires the worked list sorted BY BAND.
            let mapped = band_to_cabrillo_freq(&q.band);
            let raw = q.band.trim();
            let freq: String = match (mapped, raw.is_empty()) {
                (Some(f), _) => f.to_string(),
                (None, false) => raw.to_string(),
                (None, true) => freq_khz.to_string(),
            };
            s.push_str(&format!(
                "QSO: {freq} {mo} {date} {time} {} {} {} {} {} {}\n",
                self.mycall, self.myexch.class, self.myexch.section, q.call, q.class, q.section
            ));
        }
        s.push_str("END-OF-LOG:\n");
        s
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A band-representative frequency in kHz for the Cabrillo QSO line. Field Day
/// isn't scored by frequency, and we only store the band per contact — so a
/// multi-band log must stamp each row with ITS band, not the single dial the
/// export happened to be sitting on. `None` for an unrecognized band → caller's
/// fallback.
fn band_to_cabrillo_freq(band: &str) -> Option<&'static str> {
    Some(match band.trim().to_ascii_lowercase().as_str() {
        // HF: a representative frequency in kHz.
        "160m" => "1800",
        "80m" => "3500",
        "60m" => "5330",
        "40m" => "7000",
        "30m" => "10100",
        "20m" => "14000",
        "17m" => "18068",
        "15m" => "21000",
        "12m" => "24890",
        "10m" => "28000",
        // ⚠️ 50 MHz AND UP IS A BAND TOKEN, NOT KILOHERTZ. The Cabrillo QSO-data spec:
        // "freq is frequency or band: 1800 or actual frequency in kHz […] 50 / 70 / 144 /
        // 222 / 432 / 902 / 1.2G / 2.3G". Winter Field Day repeats the rule and machine-parses
        // the QSO lines out of whatever is uploaded, so this is the half that actually gets
        // read. We used to emit 50000 / 144000 / 222000, and 70 cm as 420000 — wrong form, and
        // for 70 cm not even a sensible frequency; the token is 432.
        "6m" => "50",
        "4m" => "70",
        "2m" => "144",
        "1.25m" => "222",
        "70cm" => "432",
        "33cm" => "902",
        "23cm" => "1.2G",
        "13cm" => "2.3G",
        "9cm" => "3.4G",
        "6cm" => "5.7G",
        "3cm" => "10G",
        _ => return None,
    })
}

/// Unix seconds → ("yyyy-mm-dd", "hhmm") in UTC for two Cabrillo fields.
fn cabrillo_datetime(unix: u64) -> (String, String) {
    let (y, mo, d, h, mi, _s) = civil_from_unix(unix);
    (format!("{y:04}-{mo:02}-{d:02}"), format!("{h:02}{mi:02}"))
}

/// Unix seconds → ("yyyymmdd", "hhmmss") in UTC for ADIF QSO_DATE / TIME_ON.
fn adif_datetime(unix: u64) -> (String, String) {
    let (y, mo, d, h, mi, s) = civil_from_unix(unix);
    (
        format!("{y:04}{mo:02}{d:02}"),
        format!("{h:02}{mi:02}{s:02}"),
    )
}

/// Unix seconds → civil UTC (y, mo, d, h, mi, s) (Howard Hinnant's
/// civil-from-days; no external date crate needed for a few export fields).
fn civil_from_unix(unix: u64) -> (i64, u32, u32, u32, u32, u32) {
    let secs_of_day = unix % 86_400;
    let days = (unix / 86_400) as i64;
    let (h, mi, s) = (
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
        (secs_of_day % 60) as u32,
    );
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}

/// Civil UTC → Unix seconds — the inverse of [`civil_from_unix`], for restoring
/// journal timestamps (mirrors `logbook.rs::unix_from_ymdhms`).
fn unix_from_ymdhms(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> u64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + (h as i64) * 3600 + (mi as i64) * 60 + s as i64;
    secs.max(0) as u64
}

fn adif_field(name: &str, value: &str) -> String {
    format!("<{}:{}>{} ", name, value.len(), value)
}

// ---- Auto-sequencer -------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdState {
    /// Monitoring; will answer the first CQ heard (search & pounce).
    Listening,
    /// Calling CQ FD (running).
    CallingCq,
    /// (S&P) sent my exchange; awaiting the runner's rogered exchange.
    AwaitExchange,
    /// (Running) got a caller's exchange; awaiting their RR73 confirmation.
    AwaitConfirm,
    /// Contact complete (logged).
    Done,
}

/// One station's Field Day auto-sequencer.
#[derive(Debug)]
pub struct FieldDayStation {
    pub mygrid: String,
    pub state: FdState,
    pub pending: Option<Msg>,
    pub dxcall: Option<String>,
    /// A caller's exchange, remembered until their RR73 lets us log it.
    peer_exch: Option<(String, String, String)>, // (call, class, section)
    pub log: FieldDayLog,
    pub transcript: Vec<String>,
}

impl FieldDayStation {
    fn mycall(&self) -> &str {
        &self.log.mycall
    }

    /// A running station (calls CQ FD).
    pub fn running(mycall: &str, mygrid: &str, exch: Exchange, band: &str) -> Self {
        Self {
            mygrid: mygrid.to_string(),
            state: FdState::CallingCq,
            pending: Some(Msg::Cq {
                de: mycall.to_string(),
                grid: mygrid.to_string(),
                // Field Day's running CQ is the directed "CQ FD <call> <grid>" —
                // the `FD` token advertises a Field Day CQ (`to_text` renders it,
                // `is_cq_dir` accepts it). S&P still answers ANY CQ, FD or plain.
                dir: "FD".to_string(),
            }),
            dxcall: None,
            peer_exch: None,
            log: FieldDayLog::new(mycall, exch, band),
            transcript: Vec::new(),
        }
    }

    /// A search-and-pounce station (answers CQs).
    pub fn search_and_pounce(mycall: &str, mygrid: &str, exch: Exchange, band: &str) -> Self {
        Self {
            mygrid: mygrid.to_string(),
            state: FdState::Listening,
            pending: None,
            dxcall: None,
            peer_exch: None,
            log: FieldDayLog::new(mycall, exch, band),
            transcript: Vec::new(),
        }
    }

    pub fn done(&self) -> bool {
        self.state == FdState::Done && self.pending.is_none()
    }

    pub fn outgoing(&self) -> Option<Msg> {
        self.pending.clone()
    }

    pub fn after_tx(&mut self) {
        if self.state == FdState::Done {
            self.pending = None;
        }
    }

    /// Return a finished station (`Done`, closing frame already sent) to its
    /// starting posture so it works the NEXT contact instead of going silent
    /// after a single QSO — Run → back to calling CQ FD, S&P → back to
    /// listening. The contest log is kept, so it remains the dupe/score source.
    pub fn rearm(&mut self, running: bool) {
        self.dxcall = None;
        self.peer_exch = None;
        if running {
            self.state = FdState::CallingCq;
            self.pending = Some(Msg::Cq {
                de: self.mycall().to_string(),
                grid: self.mygrid.clone(),
                dir: "FD".to_string(),
            });
        } else {
            self.state = FdState::Listening;
            self.pending = None;
        }
    }

    fn my_exch_msg(&self, to: &str, roger: bool) -> Msg {
        Msg::FieldDay {
            to: to.to_string(),
            de: self.mycall().to_string(),
            roger,
            class: self.log.myexch.class.clone(),
            section: self.log.myexch.section.clone(),
        }
    }

    /// Process the signals decoded this slot and advance the exchange.
    pub fn observe(&mut self, decodes: &[Decode], slot: u64) {
        for d in decodes {
            let m = Msg::parse(&d.message);
            match (self.state, &m) {
                // S&P: heard a CQ → answer with my exchange.
                (FdState::Listening, Msg::Cq { de, .. }) => {
                    if self.log.is_dupe(de) {
                        self.transcript.push(format!("skip dupe {de}"));
                        continue;
                    }
                    self.dxcall = Some(de.clone());
                    self.pending = Some(self.my_exch_msg(de, false));
                    self.state = FdState::AwaitExchange;
                    self.transcript.push(format!("answer CQ {de}"));
                }
                // Running: a caller sent their exchange → roger + send mine.
                (
                    FdState::CallingCq,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: false,
                        class,
                        section,
                    },
                ) if to == self.mycall() => {
                    self.dxcall = Some(de.clone());
                    self.peer_exch = Some((de.clone(), class.clone(), section.clone()));
                    self.pending = Some(self.my_exch_msg(de, true));
                    self.state = FdState::AwaitConfirm;
                    self.transcript
                        .push(format!("caller {de} {class} {section} → R + my exch"));
                }
                // Running (out-of-order tolerance): a caller that ROGERED its
                // exchange (roger:true) while we're still calling CQ — the plain
                // exchange was dropped, or the caller pre-rogered. The class +
                // section are carried in the rogered frame, so skip straight to
                // logging and close with RR73 rather than stalling on CQ.
                // Happy-path callers send roger:false, so this never fires on the
                // normal sequence.
                (
                    FdState::CallingCq,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: true,
                        class,
                        section,
                    },
                ) if to == self.mycall() => {
                    self.dxcall = Some(de.clone());
                    self.log.log(de, class, section, slot);
                    self.pending = Some(Msg::Rr73 {
                        to: de.clone(),
                        de: self.mycall().to_string(),
                    });
                    self.state = FdState::Done;
                    self.transcript.push(format!(
                        "caller {de} {class} {section} rogered early → log + RR73"
                    ));
                }
                // S&P: the runner rogered + sent their exchange → log + RR73.
                (
                    FdState::AwaitExchange,
                    Msg::FieldDay {
                        to,
                        de,
                        roger: true,
                        class,
                        section,
                    },
                ) if to == self.mycall() => {
                    self.log.log(de, class, section, slot);
                    self.pending = Some(Msg::Rr73 {
                        to: de.clone(),
                        de: self.mycall().to_string(),
                    });
                    self.state = FdState::Done;
                    self.transcript
                        .push(format!("logged {de} {class} {section}; send RR73"));
                }
                // Running: caller confirmed → log them. A bare `73` (Bye73)
                // completes too — if the caller closes with 73 instead of
                // RR73/RRR the contact must not stall (their exchange is already
                // in hand from the CallingCq step). Happy-path callers send RR73,
                // so the added Bye73 alternative never fires on the normal flow.
                (FdState::AwaitConfirm, Msg::Rr73 { to, .. })
                | (FdState::AwaitConfirm, Msg::Rrr { to, .. })
                | (FdState::AwaitConfirm, Msg::Bye73 { to, .. })
                    if to == self.mycall() =>
                {
                    if let Some((call, class, section)) = self.peer_exch.take() {
                        self.log.log(&call, &class, &section, slot);
                        self.transcript
                            .push(format!("logged {call} {class} {section}"));
                    }
                    self.pending = None;
                    self.state = FdState::Done;
                }
                _ => {}
            }
        }
    }
}

/// Run a Field Day exchange between a running and an S&P station over the
/// virtual channel. Stops when both have logged the contact or `max_slots`.
pub fn run_loopback_fieldday(
    running: &mut FieldDayStation,
    sp: &mut FieldDayStation,
    snr_db: f32,
    max_slots: u64,
) {
    use crate::channel::{to_i16, VirtualAir, ON_TIME_OFFSET};
    use crate::tx;

    let mut air = VirtualAir::new(tempo_fast::SAMPLE_RATE, 0xFD0001);
    for slot in 0..max_slots {
        let (txs, rxs): (&mut FieldDayStation, &mut FieldDayStation) = if slot % 2 == 0 {
            (&mut *running, &mut *sp)
        } else {
            (&mut *sp, &mut *running)
        };
        if let Some(msg) = txs.outgoing() {
            let text = msg.to_text();
            let frame = tx::build(&text, tempo_fast::SAMPLE_RATE, 1500.0);
            let rx_f32 = air.receive(&frame.wave, ON_TIME_OFFSET, snr_db);
            let decodes: Vec<Decode> = tempo_fast::decode_frame(
                &to_i16(&rx_f32),
                200,
                2900,
                3,
                "",
                "",
                0,
                (slot as i64).wrapping_mul(4000), // monotonic ms for IR-HARQ keying
            )
            .into_iter()
            .map(Into::into)
            .collect();
            rxs.observe(&decodes, slot);
            txs.after_tx();
        }
        if running.log.qso_count() >= 1 && sp.log.qso_count() >= 1 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    /// ⚠️ THE CONTEST LOG *HAS* A PER-CONTACT BAND — what it has no way to do
    /// is let a CALLER name it. Pinned because the Field Day guide said "the
    /// Field Day log has no per-contact band field", which is false and sends
    /// a future fixer looking for a field to add rather than a parameter.
    ///
    /// `LoggedQso::band` is real and is written per contact, so one log can
    /// hold two bands and the exports get them right. But every entry point —
    /// `log`, `log_at`, `log_mode_at`, `log_submode_at` — funnels into
    /// `log_submode_at`, which stamps `self.band`, the log's ONE current band.
    /// No signature takes a band. That is why `Engine::fd_log_manual` opening
    /// with `sync_fd_band()` makes a catch-up entry file on the dial you are on
    /// NOW: not a missing field, an unreachable one.
    ///
    /// Goes red the day a caller can name the band — which is the cue to delete
    /// the "log as you work" caveat from docs/guide/contesting-pota.md and
    /// docs/guide/satellites.md.
    #[test]
    fn every_contact_carries_a_band_and_it_is_always_the_logs_own() {
        let mut log = FieldDayLog::new(
            "W9XYZ",
            Exchange {
                class: "3A".into(),
                section: "WI".into(),
            },
            "70cm",
        );
        assert!(log.log_mode_at("W1AW", "1D", "IL", "PH", 0, 100));
        // The only way the band moves: the LOG's band, not an argument.
        log.band = "20m".into();
        assert!(log.log_mode_at("K1ABC", "2A", "EMA", "PH", 0, 200));

        let bands: Vec<&str> = log.qsos().iter().map(|q| q.band.as_str()).collect();
        assert_eq!(
            bands,
            ["70cm", "20m"],
            "the per-contact band field stopped recording the log's band at log time"
        );
    }

    #[test]
    fn arrl_scoring_per_mode_and_band_mode_dupes() {
        // ARRL FD: phone 1 pt, CW/digital 2 pts; a station counts once per
        // band PER MODE CLASS (the old (call, band) dupe key under-counted —
        // K1ABC on 20m CW and 20m FT8 are two legal contacts).
        let mut log = FieldDayLog::new(
            "W9XYZ",
            Exchange {
                class: "3A".into(),
                section: "WI".into(),
            },
            "20m",
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 100));
        assert!(
            log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, 110),
            "same call, new mode"
        );
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "PH", 0, 120));
        assert!(
            !log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 130),
            "band+mode dupe"
        );
        assert!(
            !log.log_mode_at("k1abc", "2A", "CT", "dig", 0, 140),
            "case-insensitive dupe"
        );
        assert_eq!(log.qso_count(), 3);
        assert_eq!(log.qso_points(), 2 + 2 + 1, "DIG 2 + CW 2 + PH 1");
        // Cabrillo mode tokens per row.
        let cab = log.cabrillo(14074);
        assert!(cab.contains(" DG "));
        assert!(cab.contains(" CW "));
        assert!(cab.contains(" PH "));
        // The rules-data stamp (Cabrillo-legal X- header, robots ignore it).
        assert!(cab.contains("X-NEXUS-RULES-YEAR: 2026\n"));
        // WFD event flips the contest ids in both exports.
        log.event = FdEvent::WinterFd;
        assert!(log.cabrillo(14074).contains("CONTEST: WFD"));
        assert!(log.adif().contains("WFD"));
    }

    #[test]
    fn cabrillo_lines_carry_real_utc_timestamps() {
        // 2026-06-27 18:05:00 UTC (Field Day Saturday) = 1782583500.
        let (d, t) = cabrillo_datetime(1_782_583_500);
        assert_eq!((d.as_str(), t.as_str()), ("2026-06-27", "1805"));
        let mut log = FieldDayLog::new(
            "W9XYZ",
            Exchange {
                class: "3A".into(),
                section: "WI".into(),
            },
            "20m",
        );
        assert!(log.log_at("K1ABC", "2A", "CT", 4, 1_782_583_500));
        let cab = log.cabrillo(14074);
        // Frequency is band-derived (20m → 14000 kHz), not the passed dial.
        assert!(
            cab.contains("QSO: 14000 DG 2026-06-27 1805 W9XYZ 3A WI K1ABC 2A CT"),
            "real date/time on the QSO line (ARRL submission requires it): {cab}"
        );
        assert!(!cab.contains("----------"), "no placeholder when stamped");
    }

    #[test]
    fn a_tempo_tier_contact_exports_as_a_mode_adif_actually_defines() {
        // ⚠️ THIS WAS A LOST CONTACT, not a formatting nit. The Field Day ADIF wrote the
        // recorded mode straight into <MODE>, and the tiers stamp names ADIF has never heard
        // of — TempoFast, TempoDeep, FT2. TQSL walks MODE%SUBMODE → SUBMODE → MODE and a bare
        // <MODE:10>TempoFast misses all three legs, so the record is DROPPED with "Invalid
        // MODE". The main logbook has solved this for years in `logbook::adif_submode`; the
        // Field Day exporter just never called it, so a Field Day contact worked on a Tempo
        // tier vanished on upload while an FT8 one beside it went through.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            Exchange {
                class: "3A".into(),
                section: "WI".into(),
            },
            "20m",
        );
        assert!(log.log_submode_at("K1ABC", "2A", "CT", "DIG", "TempoFast", 4, 1_782_583_500));
        let adif = log.adif();
        assert!(
            adif.contains("<MODE:4>MFSK") && adif.contains("<SUBMODE:9>TEMPOFAST"),
            "a Tempo tier must ride as MODE=MFSK + SUBMODE, or TQSL drops the record: {adif}"
        );
        assert!(
            !adif.contains("<MODE:9>TempoFast"),
            "TempoFast is not an ADIF MODE and must never be written as one: {adif}"
        );

        // POSITIVE CONTROL: a mode ADIF really does define is written plainly, with no
        // SUBMODE invented for it — otherwise this test would pass against a change that
        // wrapped everything.
        let mut plain = FieldDayLog::new(
            "W9XYZ",
            Exchange {
                class: "3A".into(),
                section: "WI".into(),
            },
            "20m",
        );
        assert!(plain.log_submode_at("K2DEF", "2A", "CT", "DIG", "RTTY", 4, 1_782_583_500));
        let plain_adif = plain.adif();
        assert!(
            plain_adif.contains("<MODE:4>RTTY"),
            "RTTY is a real MODE: {plain_adif}"
        );
        assert!(
            !plain_adif.contains("SUBMODE"),
            "…and needs no SUBMODE: {plain_adif}"
        );
    }

    #[test]
    fn cabrillo_says_the_band_the_contact_was_actually_made_on() {
        // ⚠️ TWO WAYS THIS LIED ABOUT A CONTACT'S BAND, both found by auditing our output
        // against the Cabrillo spec and Winter Field Day's parser.
        //
        // 1. ABOVE 50 MHz THE FIELD IS A BAND TOKEN, NOT KILOHERTZ. The spec's own words:
        //    "freq is frequency or band: 1800 or actual frequency in kHz […] 50 / 70 / 144 /
        //    222 / 432 / 902 / 1.2G". WFD states the same rule and MACHINE-PARSES the QSO
        //    lines out of whatever is uploaded, so a 6-digit value where a token belongs is a
        //    parse problem there. 70 cm was wrong twice over — the token is 432 and 420000 was
        //    not even a sensible kHz for it.
        // 2. AN UNRECOGNISED BAND TOOK THE CALLER'S FALLBACK, and the club exporter passed a
        //    HARDCODED 14 MHz. A 23 cm club contact exported as 20 m. ARRL Field Day does not
        //    read Cabrillo at all, but it DOES require a list of stations worked sorted BY
        //    BAND — so this corrupted the one artifact they actually use.
        let mut log = FieldDayLog::new(
            "W9XYZ",
            Exchange {
                class: "3A".into(),
                section: "WI".into(),
            },
            "20m",
        );
        for (band, call) in [
            ("20m", "K1ABC"),
            ("6m", "K2DEF"),
            ("2m", "K3GHI"),
            ("70cm", "K4JKL"),
            ("23cm", "K5MNO"),
            ("4m", "K6PQR"),
        ] {
            log.band = band.to_string();
            assert!(
                log.log_at(call, "2A", "CT", 4, 1_782_583_500),
                "{band} logged"
            );
        }
        let cab = log.cabrillo(14_000);

        // HF stays kHz.
        assert!(
            cab.contains("QSO: 14000 DG 2026-06-27 1805 W9XYZ 3A WI K1ABC"),
            "20m: {cab}"
        );
        // Above 50 MHz is the BAND, per the spec's own list.
        for (token, call) in [("50", "K2DEF"), ("144", "K3GHI"), ("432", "K4JKL")] {
            assert!(
                cab.contains(&format!(
                    "QSO: {token} DG 2026-06-27 1805 W9XYZ 3A WI {call}"
                )),
                "expected band token {token} for {call}: {cab}"
            );
        }
        // 4 m and 23 cm are legal Field Day bands and were not in the table at all — they
        // took the caller's fallback and claimed to be somewhere else entirely.
        assert!(
            cab.contains("QSO: 70 DG 2026-06-27 1805 W9XYZ 3A WI K6PQR"),
            "4m: {cab}"
        );
        assert!(
            cab.contains("QSO: 1.2G DG 2026-06-27 1805 W9XYZ 3A WI K5MNO"),
            "23cm: {cab}"
        );
        // THE PIN: nothing above 6 m may claim to be on 20 m.
        assert_eq!(
            cab.matches("QSO: 14000 ").count(),
            1,
            "only the 20 m contact may say 14000 — a band we cannot map must never borrow \
             another band's frequency: {cab}"
        );
    }

    #[test]
    fn adif_round_trip_restores_log_and_dupes() {
        // The FD contest log lives only in memory, so the exit flush's ADIF is
        // its sole durable copy — merging it back into a fresh log (a restart
        // mid-event) must restore the QSOs, the dupe index, the sections and
        // real timestamps (not the '----------' Cabrillo placeholder).
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_560));
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        let adif = log.adif();

        let mut restored = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        restored.merge_adif(&adif, 0);
        assert_eq!(restored.qso_count(), 3, "all three contacts restored");
        for (call, mode) in [("K1ABC", "DIG"), ("K2DEF", "CW"), ("N0GHI", "PH")] {
            assert!(
                restored.is_dupe_mode(call, mode),
                "{call} {mode} restored into the dupe index"
            );
        }
        assert_eq!(restored.sections(), 3, "sections survive the round-trip");
        assert_eq!(restored.qso_points(), 2 + 2 + 1, "DIG 2 + CW 2 + PH 1");
        let cab = restored.cabrillo(14_074);
        assert!(
            cab.contains("2026-06-27"),
            "restored rows keep their real timestamp: {cab}"
        );
        assert!(!cab.contains("----------"), "no placeholder after restore");
    }

    #[test]
    fn adif_and_cabrillo_carry_the_actual_digital_mode() {
        // An RTTY WFD QSO must never export MODE=FT8 (a banned mode there) —
        // the recorded actual mode wins; rows without one keep the legacy
        // FT8/DG fallback so old logs export unchanged.
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("2M", "EPA"), "20m");
        log.event = FdEvent::WinterFd;
        assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", " rtty ", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1O", "EMA", "DIG", 0, 1_782_583_560));
        let adif = log.adif();
        assert!(
            adif.contains("<MODE:4>RTTY"),
            "RTTY row exports its real (normalized) mode: {adif}"
        );
        assert!(adif.contains("<MODE:3>FT8"), "unrecorded row falls back");
        let cab = log.cabrillo(14_080);
        assert!(cab.contains(" RY "), "Cabrillo RY for the RTTY row: {cab}");
        assert!(cab.contains(" DG "), "DG fallback for the unrecorded row");
        // FD/WFD digital is ONE mode class: an RTTY try after the same-band
        // FT8 contact is a dupe, and both rows score the digital 2 points.
        assert!(
            !log.log_submode_at("K2DEF", "1O", "EMA", "DIG", "RTTY", 0, 1_782_583_620),
            "RTTY dupes the same-band digital contact"
        );
        assert_eq!(log.qso_points(), 4);
    }

    #[test]
    fn actual_mode_survives_the_adif_round_trip() {
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("2M", "EPA"), "20m");
        log.event = FdEvent::WinterFd;
        assert!(log.log_submode_at("K1ABC", "1H", "CT", "DIG", "RTTY", 0, 1_782_583_500));
        let mut restored = FieldDayLog::new("W9XYZ", Exchange::new("2M", "EPA"), "20m");
        restored.event = FdEvent::WinterFd;
        restored.merge_adif(&log.adif(), 0);
        assert_eq!(restored.qso_count(), 1);
        let q = &restored.qsos()[0];
        assert_eq!((q.mode.as_str(), q.submode.as_str()), ("DIG", "RTTY"));
        assert!(
            restored.is_dupe_mode("K1ABC", "DIG"),
            "dupe key restored on the mode CLASS"
        );
        assert!(
            restored.adif().contains("<MODE:4>RTTY"),
            "re-export keeps RTTY"
        );
        assert!(restored.cabrillo(14_080).contains(" RY "));
    }

    #[test]
    fn seq_is_stamped_monotonic_and_round_trips_as_app_nexus_qseq() {
        // Every entry point funnels into log_submode_at, so seqs are 1..n in
        // log order regardless of mode/path.
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        assert!(log.log_mode_at("K2DEF", "1D", "EMA", "CW", 0, 1_782_583_560));
        assert_eq!(log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(log.max_seq(), 2);
        let adif = log.adif();
        assert!(
            adif.contains("<APP_NEXUS_QSEQ:1>1") && adif.contains("<APP_NEXUS_QSEQ:1>2"),
            "seq journaled per row: {adif}"
        );

        // Restore keeps each row's seq and the NEXT log entry continues past
        // the restored high-water — the property that makes a restart safe
        // against re-issuing an id the club host already merged.
        let mut restored = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        restored.merge_adif(&adif, 0);
        assert_eq!(
            restored.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [1, 2],
            "seqs survive the round-trip"
        );
        assert!(restored.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        assert_eq!(
            restored.qsos()[2].seq,
            3,
            "fresh rows continue the sequence"
        );
    }

    #[test]
    fn legacy_journal_without_qseq_backfills_in_row_order() {
        // A pre-sync journal has no APP_NEXUS_QSEQ tags at all: restore assigns
        // 1..n in row order (deterministic, so a re-restore re-derives the same
        // ids) and new contacts continue from there.
        let legacy = "<EOH>\n\
            <CALL:5>K1ABC <MODE:3>FT8 <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180500 \
            <CLASS:2>2A <ARRL_SECT:2>CT <EOR>\n\
            <CALL:5>K2DEF <MODE:2>CW <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180600 \
            <CLASS:2>1D <ARRL_SECT:3>EMA <EOR>\n";
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        log.merge_adif(legacy, 0);
        assert_eq!(
            log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [1, 2],
            "legacy rows backfill 1..n in row order"
        );
        assert!(log.log_mode_at("N0GHI", "5A", "MN", "PH", 0, 1_782_583_620));
        assert_eq!(log.qsos()[2].seq, 3);
        // A mixed journal (one stamped row, one legacy) never collides: the
        // backfill takes the next free seq at the point the row restores.
        let mixed = "<EOH>\n\
            <CALL:5>K1ABC <MODE:3>FT8 <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180500 \
            <CLASS:2>2A <ARRL_SECT:2>CT <APP_NEXUS_QSEQ:1>5 <EOR>\n\
            <CALL:5>K2DEF <MODE:2>CW <BAND:3>20m <QSO_DATE:8>20260627 <TIME_ON:6>180600 \
            <CLASS:2>1D <ARRL_SECT:3>EMA <EOR>\n";
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        log.merge_adif(mixed, 0);
        assert_eq!(
            log.qsos().iter().map(|q| q.seq).collect::<Vec<_>>(),
            [5, 6],
            "backfill continues past a restored explicit seq"
        );
        assert_eq!(log.max_seq(), 6);
    }

    #[test]
    fn merge_adif_age_gate_and_garbage_merge_nothing() {
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        assert!(log.log_mode_at("K1ABC", "2A", "CT", "DIG", 0, 1_782_583_500));
        let adif = log.adif();

        // A min_when_unix newer than every row (a previous event's journal)
        // restores nothing — the backup self-expires.
        let mut restored = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        restored.merge_adif(&adif, 1_782_583_501);
        assert_eq!(restored.qso_count(), 0, "expired journal merges nothing");

        // Garbage input merges nothing and never errors.
        restored.merge_adif("not adif <CALL:junk><EOR> \u{fe0f}<QSO_DATE:8>x<EOR>", 0);
        assert_eq!(restored.qso_count(), 0, "garbage merges nothing");
    }

    use super::*;

    fn dec(msg: &str) -> Decode {
        Decode {
            message: msg.to_string(),
            sync: 1.0,
            snr: -5,
            dt: 0.0,
            freq: 1500.0,
            nap: 0,
            qual: 1.0,
            rv: None,
            mode: None,
        }
    }

    #[test]
    fn dupe_check_and_scoring() {
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20M");
        assert!(log.log("K2DEF", "3A", "IL", 1));
        assert!(log.log("N0ABC", "1D", "MN", 2));
        assert!(!log.log("K2DEF", "3A", "IL", 3)); // dupe on same band
        assert_eq!(log.qso_count(), 2);
        assert_eq!(log.sections(), 2); // IL, MN
        assert_eq!(log.qso_points(), 4); // 2 pts each
        assert!(log.adif().contains("ARRL_SECT") && log.adif().contains("K2DEF"));
        let cab = log.cabrillo(14_074);
        assert_eq!(cab.matches("QSO:").count(), 2);
        assert!(cab.contains("W9XYZ 3A WI K2DEF 3A IL"));
    }

    #[test]
    fn cabrillo_frequency_is_per_qso_band_not_the_export_dial() {
        // Each row's frequency comes from the QSO's own band, not the single dial
        // the export happened to pass (which stamped every row before the fix).
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
        assert!(log.log_at("K1ABC", "2A", "CT", 1, 1_782_583_500)); // 20m
        let cab = log.cabrillo(99999); // dial fallback must NOT appear for a known band
        assert!(
            cab.contains("QSO: 14000 "),
            "20m QSO stamped 14000 kHz: {cab}"
        );
        assert!(
            !cab.contains("QSO: 99999 "),
            "the export dial is not stamped on a known band"
        );
    }

    #[test]
    fn worked_sections_returns_the_distinct_set_sorted() {
        let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20M");
        assert!(log.log("K2DEF", "3A", "IL", 1));
        assert!(log.log("N0ABC", "1D", "MN", 2));
        assert!(log.log_mode_at("W1AW", "2A", "IL", "CW", 0, 100)); // IL again, new mode
                                                                    // The count and the identities agree; identities are the distinct set,
                                                                    // sorted (IL once, though worked twice), and stable.
        assert_eq!(log.worked_sections(), vec!["IL", "MN"]);
        assert_eq!(log.worked_sections().len(), log.sections());
    }

    #[test]
    fn observe_runs_sp_side() {
        // S&P station hears a CQ, sends exchange, then the runner's rogered exch.
        let mut sp =
            FieldDayStation::search_and_pounce("K2DEF", "FN31", Exchange::new("2A", "IL"), "20M");
        sp.observe(&[dec("CQ W9XYZ EN37")], 0);
        assert_eq!(sp.state, FdState::AwaitExchange);
        assert_eq!(sp.outgoing().unwrap().to_text(), "W9XYZ K2DEF 2A IL");
        sp.observe(&[dec("K2DEF W9XYZ R 3A WI")], 1);
        assert_eq!(sp.state, FdState::Done);
        assert_eq!(sp.log.qso_count(), 1);
        assert_eq!(sp.log.qsos()[0].class, "3A");
        assert_eq!(sp.log.qsos()[0].section, "WI");
    }

    #[test]
    fn running_station_calls_directed_cq_fd() {
        // The running station advertises a DIRECTED Field Day CQ so it reads as
        // "CQ FD" on the air — and an S&P station still answers it.
        let run = FieldDayStation::running("W9XYZ", "EN37", Exchange::new("3A", "WI"), "20M");
        let cq = run.outgoing().unwrap().to_text();
        assert!(cq.contains("CQ FD"), "directed FD CQ: {cq}");
        assert_eq!(cq, "CQ FD W9XYZ EN37");
        let mut sp =
            FieldDayStation::search_and_pounce("K2DEF", "FN31", Exchange::new("2A", "IL"), "20M");
        sp.observe(&[dec(&cq)], 0);
        assert_eq!(
            sp.state,
            FdState::AwaitExchange,
            "S&P answers a directed FD CQ"
        );
    }

    #[test]
    fn running_station_rearms_and_works_a_second_caller() {
        // Regression for the RUN dead-end: a running station worked exactly one
        // contact and then went silent (Done, no return to CQ). It must re-arm.
        let mut run = FieldDayStation::running("W9XYZ", "EN37", Exchange::new("3A", "WI"), "20M");
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0); // caller's exchange
        assert_eq!(run.state, FdState::AwaitConfirm);
        run.observe(&[dec("W9XYZ K2DEF RR73")], 1); // caller confirms → we log
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        assert!(run.done(), "closed after the first QSO");

        // The fix: the engine re-arms a `done()` RUN station back to calling CQ.
        run.rearm(true);
        assert_eq!(run.state, FdState::CallingCq);
        assert!(
            run.outgoing().unwrap().to_text().contains("CQ FD"),
            "back on CQ FD after re-arm"
        );

        // It now works a SECOND caller, and the first is still a dupe (the
        // in-station contest log survived the re-arm).
        run.observe(&[dec("W9XYZ N0ABC 1D MN")], 2);
        assert_eq!(run.state, FdState::AwaitConfirm, "answers the next caller");
        run.observe(&[dec("W9XYZ N0ABC RR73")], 3);
        assert_eq!(run.log.qso_count(), 2, "second contact logs after re-arm");
        assert!(run.log.is_dupe("K2DEF"), "first caller remains a dupe");
    }

    #[test]
    fn loopback_completes_the_exchange_on_the_happy_path() {
        // The full round-trip over the real modem still completes unchanged after
        // the directed-CQ-FD switch: both stations log the OTHER's exchange.
        let mut running =
            FieldDayStation::running("W9XYZ", "EN37", Exchange::new("3A", "WI"), "20M");
        let mut sp =
            FieldDayStation::search_and_pounce("K2DEF", "FN31", Exchange::new("2A", "IL"), "20M");
        run_loopback_fieldday(&mut running, &mut sp, 15.0, 40);
        assert_eq!(
            running.log.qso_count(),
            1,
            "running: {:?}",
            running.transcript
        );
        assert_eq!(sp.log.qso_count(), 1, "sp: {:?}", sp.transcript);
        assert_eq!(
            running.log.qsos()[0].section,
            "IL",
            "running logged the caller"
        );
        assert_eq!(sp.log.qsos()[0].section, "WI", "S&P logged the runner");
    }

    #[test]
    fn running_logs_on_a_bare_73() {
        // Caller closes with a bare `73` instead of RR73/RRR: the contact must
        // still complete + log (the caller's exchange was captured at CallingCq),
        // not stall in AwaitConfirm.
        let mut run = FieldDayStation::running("W9XYZ", "EN37", Exchange::new("3A", "WI"), "20M");
        run.observe(&[dec("W9XYZ K2DEF 2A IL")], 0); // caller's exchange
        assert_eq!(run.state, FdState::AwaitConfirm);
        run.observe(&[dec("W9XYZ K2DEF 73")], 1); // bare 73, not RR73
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        let q = &run.log.qsos()[0];
        assert_eq!(
            (q.call.as_str(), q.class.as_str(), q.section.as_str()),
            ("K2DEF", "2A", "IL")
        );
    }

    #[test]
    fn running_skips_ahead_on_an_early_rogered_exchange() {
        // A caller that ROGERS its exchange while we're still calling CQ (the
        // plain exchange dropped, or the caller pre-rogered): the class + section
        // are in the rogered frame, so skip straight to logging + close with RR73
        // instead of stalling on CQ.
        let mut run = FieldDayStation::running("W9XYZ", "EN37", Exchange::new("3A", "WI"), "20M");
        assert_eq!(run.state, FdState::CallingCq);
        run.observe(&[dec("W9XYZ K2DEF R 2A IL")], 0); // rogered, skipping the plain form
        assert_eq!(run.state, FdState::Done);
        assert_eq!(run.log.qso_count(), 1);
        let q = &run.log.qsos()[0];
        assert_eq!(
            (q.call.as_str(), q.class.as_str(), q.section.as_str()),
            ("K2DEF", "2A", "IL")
        );
        assert_eq!(
            run.outgoing().unwrap().to_text(),
            "K2DEF W9XYZ RR73",
            "closes with RR73 to the caller"
        );
    }
}
