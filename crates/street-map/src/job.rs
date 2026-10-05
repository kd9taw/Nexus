//! The download: a plan's requests fetched about four at a time into the pack's file, a journal
//! of which are safely on disk, and the checks a finished file passes before it takes its name.
//!
//! The pack is written in place, at its final size, as `street-<id>.pmtiles.part`. The plan
//! fixes where every byte goes, so requests can land in any order. After a request's bytes are
//! written and synced, its index is appended (and synced) to `street-<id>.journal`. A job killed
//! at any moment therefore leaves a journal that understates what is on disk, never overstates
//! it: the next run re-fetches the request that was in flight and nothing else. A journal is
//! used only when its fingerprint matches the new plan and the file is still the plan's size.
//!
//! When every request is on disk the file is checked from scratch (structure, directories, and
//! every tile gunzipped and parsed), hashed, and renamed to `street-<id>.pmtiles`. A file that
//! fails the check is deleted with its journal, so the next attempt starts clean.
//!
//! `cancel` is the job's own flag. The job also sets it to stop its other workers after one of
//! them fails, so it must not be shared with anything else.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};

use crate::area::StreetArea;
use crate::error::{ErrorKind, StreetError};
use crate::http::{with_retry, FetchError, RangeSource};
use crate::plan::{hex, Plan, Request};
use crate::store::{read_exact_at, write_all_at, MapsDir};
use crate::verify::{check_pack_metadata, verify_file};
use crate::Options;

const JOURNAL_MAGIC: &str = "nexus street-map journal 1";

/// What the download sheet shows while a job runs. Sent over a Tauri channel as
/// `{ "phase": "tiles", "done": …, "total": …, "bytesPerSec": …, "etaSecs": … }` and so on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "phase",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StreetProgress {
    /// The fonts and icons, fetched once with the first pack.
    Assets { done: u64, total: u64 },
    /// Map data transferred so far (merged gaps included) of all there is to transfer. A
    /// resumed job starts above zero.
    Tiles {
        done: u64,
        total: u64,
        bytes_per_sec: u64,
        eta_secs: Option<u64>,
    },
    /// A request failed and is tried again after `wait_secs`.
    Retrying {
        attempt: u32,
        wait_secs: u64,
        reason: String,
    },
    /// The finished file's tiles being checked.
    Verifying { done: u64, total: u64 },
}

/// A finished, checked pack file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub bytes: u64,
    pub sha256: String,
}

/// What a journal says about its download, for listing unfinished downloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalInfo {
    pub pack_id: String,
    pub area: StreetArea,
    pub build_id: String,
    pub build_url: String,
    pub data_date: String,
    pub download_bytes: u64,
    pub pack_bytes: u64,
    pub requests: usize,
}

struct Journal {
    fingerprint: String,
    info: JournalInfo,
    /// (request index, its length), in the order they finished.
    done: Vec<(usize, u64)>,
}

fn journal_head(plan: &Plan) -> String {
    let info = JournalInfo {
        pack_id: plan.pack_id.clone(),
        area: plan.area,
        build_id: plan.build.id.clone(),
        build_url: plan.build.url.clone(),
        data_date: plan.build.date.clone(),
        download_bytes: plan.download_bytes(),
        pack_bytes: plan.total_len(),
        requests: plan.requests.len(),
    };
    format!(
        "{JOURNAL_MAGIC} {}\n{}\n",
        plan.fingerprint,
        serde_json::to_string(&info).expect("plain JSON values serialise")
    )
}

/// Parse a journal. A last line cut short by a kill is ignored, as is anything after a line
/// that does not parse.
fn parse_journal(bytes: &[u8]) -> Option<Journal> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.split_inclusive('\n');
    let fingerprint = lines
        .next()?
        .strip_suffix('\n')?
        .strip_prefix(JOURNAL_MAGIC)?
        .strip_prefix(' ')?
        .to_string();
    let info: JournalInfo = serde_json::from_str(lines.next()?.strip_suffix('\n')?).ok()?;
    let mut done = Vec::new();
    for line in lines {
        let Some((i, len)) = line.strip_suffix('\n').and_then(|l| l.split_once(' ')) else {
            break;
        };
        match (i.parse(), len.parse()) {
            (Ok(i), Ok(len)) => done.push((i, len)),
            _ => break,
        }
    }
    Some(Journal {
        fingerprint,
        info,
        done,
    })
}

fn read_journal(path: &std::path::Path) -> Option<Journal> {
    parse_journal(&fs::read(path).ok()?)
}

/// The requests of `plan` already on disk, if its journal and file belong to this plan.
fn resumable(dir: &MapsDir, plan: &Plan) -> Option<Vec<bool>> {
    let j = read_journal(&dir.journal(&plan.pack_id))?;
    if j.fingerprint != plan.fingerprint {
        return None;
    }
    if fs::metadata(dir.part(&plan.pack_id)).ok()?.len() != plan.total_len() {
        return None;
    }
    let mut done = vec![false; plan.requests.len()];
    for (i, len) in j.done {
        if plan.requests.get(i)?.len != len {
            return None;
        }
        done[i] = true;
    }
    Some(done)
}

/// Bytes already downloaded by an unfinished job for this very plan, and the size of its file.
pub fn resume_state(dir: &MapsDir, plan: &Plan) -> Option<(u64, u64)> {
    let done = resumable(dir, plan)?;
    let bytes = plan
        .requests
        .iter()
        .zip(&done)
        .filter(|(_, d)| **d)
        .map(|(r, _)| r.len)
        .sum();
    Some((bytes, plan.total_len()))
}

/// Downloads that stopped before they finished, from their journals.
pub fn unfinished(dir: &MapsDir) -> Vec<(JournalInfo, u64)> {
    let Ok(entries) = fs::read_dir(dir.root()) else {
        return Vec::new();
    };
    let mut out: Vec<(JournalInfo, u64)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.starts_with("street-") && name.ends_with(".journal")
        })
        .filter_map(|e| read_journal(&e.path()))
        .filter(|j| dir.part(&j.info.pack_id).is_file())
        .map(|j| {
            let done = j.done.iter().map(|(_, len)| len).sum();
            (j.info, done)
        })
        .collect();
    out.sort_by(|a, b| a.0.pack_id.cmp(&b.0.pack_id));
    out
}

fn io_err(what: &'static str) -> impl Fn(std::io::Error) -> StreetError {
    move |e| StreetError::io(what, &e)
}

/// Write one request's kept segments, sync them, then record the request in the journal.
fn store_request(
    part: &File,
    plan: &Plan,
    req: &Request,
    bytes: &[u8],
    index: usize,
    journal: &Mutex<File>,
) -> Result<(), StreetError> {
    let base = plan.prefix.len() as u64;
    for s in &req.segments {
        let at = (s.src - req.src) as usize;
        write_all_at(part, &bytes[at..at + s.len as usize], base + s.dst)
            .map_err(io_err("write the street map"))?;
    }
    // On disk before the journal says so: a kill between the two re-fetches this request.
    part.sync_data().map_err(io_err("write the street map"))?;
    let mut j = journal.lock().unwrap_or_else(PoisonError::into_inner);
    writeln!(j, "{index} {}", req.len).map_err(io_err("write the download journal"))?;
    j.sync_data().map_err(io_err("write the download journal"))
}

pub(crate) fn sha256_file(f: &File, len: u64) -> Result<String, StreetError> {
    let mut ctx = Context::new(&SHA256);
    let mut buf = vec![0u8; 1 << 20];
    let mut off = 0;
    while off < len {
        let n = (len - off).min(buf.len() as u64) as usize;
        read_exact_at(f, &mut buf[..n], off).map_err(io_err("hash the street map"))?;
        ctx.update(&buf[..n]);
        off += n as u64;
    }
    Ok(hex(ctx.finish().as_ref()))
}

/// A throttle: true at most once per `every`, and always when forced.
struct Every {
    every: Duration,
    last: Mutex<Option<Instant>>,
}

impl Every {
    fn new(every: Duration) -> Self {
        Self {
            every,
            last: Mutex::new(None),
        }
    }

    fn due(&self, force: bool) -> bool {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        if force || last.is_none_or(|t| now.duration_since(t) >= self.every) {
            *last = Some(now);
            return true;
        }
        false
    }
}

/// Fetch `plan` from `src` into the maps folder, resuming an earlier attempt where its journal
/// allows, and check, hash and rename the finished file.
pub fn download(
    plan: &Plan,
    src: &dyn RangeSource,
    dir: &MapsDir,
    opts: &Options,
    cancel: &AtomicBool,
    progress: &(dyn Fn(StreetProgress) + Sync),
) -> Result<Finished, StreetError> {
    let part_path = dir.part(&plan.pack_id);
    let journal_path = dir.journal(&plan.pack_id);
    let total = plan.total_len();
    let n = plan.requests.len();

    let resumed = resumable(dir, plan);
    let part = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part_path)
        .map_err(io_err("create the street map file"))?;
    let (done, journal) = match resumed {
        Some(done) => {
            let j = OpenOptions::new()
                .append(true)
                .open(&journal_path)
                .map_err(io_err("open the download journal"))?;
            (done, j)
        }
        None => {
            // A fresh start: an empty file of the final size, and a journal naming this plan.
            part.set_len(0)
                .map_err(io_err("reset the street map file"))?;
            part.set_len(total)
                .map_err(io_err("size the street map file"))?;
            let mut j =
                File::create(&journal_path).map_err(io_err("create the download journal"))?;
            j.write_all(journal_head(plan).as_bytes())
                .and_then(|()| j.sync_data())
                .map_err(io_err("write the download journal"))?;
            (vec![false; n], j)
        }
    };
    write_all_at(&part, &plan.prefix, 0).map_err(io_err("write the street map"))?;

    let pending: Vec<usize> = (0..n).filter(|&i| !done[i]).collect();
    let already: u64 = (0..n)
        .filter(|&i| done[i])
        .map(|i| plan.requests[i].len)
        .sum();
    let total_dl = plan.download_bytes();
    let next = AtomicUsize::new(0);
    let fetched = AtomicU64::new(0);
    let finished = AtomicUsize::new(0);
    let failure: Mutex<Option<StreetError>> = Mutex::new(None);
    let journal = Mutex::new(journal);
    let started = Instant::now();
    let tick = Every::new(opts.progress_every);
    let report = |force: bool| {
        if !tick.due(force) {
            return;
        }
        let session = fetched.load(Ordering::Relaxed);
        let secs = started.elapsed().as_secs_f64();
        let rate = if secs > 0.5 {
            (session as f64 / secs) as u64
        } else {
            0
        };
        let done = already + session;
        progress(StreetProgress::Tiles {
            done,
            total: total_dl,
            bytes_per_sec: rate,
            eta_secs: (rate > 0).then(|| total_dl.saturating_sub(done) / rate),
        });
    };
    let on_retry = |attempt: u32, wait: Duration, why: &str| {
        progress(StreetProgress::Retrying {
            attempt,
            wait_secs: wait.as_secs(),
            reason: why.to_string(),
        });
    };
    report(true);

    std::thread::scope(|s| {
        for _ in 0..opts.parallel.clamp(1, pending.len().max(1)) {
            s.spawn(|| loop {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some(&i) = pending.get(k) else {
                    return;
                };
                let req = &plan.requests[i];
                let got = with_retry(&opts.retry, cancel, &on_retry, || {
                    let b = src.read(req.src, req.len, cancel)?;
                    if b.len() as u64 != req.len {
                        return Err(FetchError::Changed(format!(
                            "the map file ends {} bytes into a {}-byte request",
                            b.len(),
                            req.len
                        )));
                    }
                    Ok(b)
                })
                .map_err(|e| e.into_street(ErrorKind::Paused))
                .and_then(|b| store_request(&part, plan, req, &b, i, &journal));
                match got {
                    Ok(()) => {
                        fetched.fetch_add(req.len, Ordering::Relaxed);
                        finished.fetch_add(1, Ordering::Relaxed);
                        report(false);
                    }
                    Err(e) => {
                        let mut f = failure.lock().unwrap_or_else(PoisonError::into_inner);
                        f.get_or_insert(e);
                        drop(f);
                        // Stop the other workers; their own reads now end as cancelled.
                        cancel.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            });
        }
    });
    report(true);

    let failure = failure.into_inner().unwrap_or_else(PoisonError::into_inner);
    if finished.load(Ordering::Relaxed) < pending.len() {
        let e = failure.unwrap_or_else(|| StreetError::new(ErrorKind::Cancelled, "cancelled"));
        if matches!(e.kind, ErrorKind::BuildGone | ErrorKind::BuildChanged) {
            // That build can never complete this file; keep nothing of it.
            drop((part, journal));
            let _ = fs::remove_file(&part_path);
            let _ = fs::remove_file(&journal_path);
        }
        return Err(e);
    }

    // Every request is on disk. Check the whole file before it takes its name.
    part.sync_all().map_err(io_err("write the street map"))?;
    let contents = plan.header.tile_contents;
    let verified = verify_file(&part, total, cancel, &mut |d, t| {
        if tick.due(d == t) {
            progress(StreetProgress::Verifying { done: d, total: t });
        }
    })
    .and_then(|checked| {
        check_pack_metadata(&checked.metadata)?;
        if checked.header != plan.header || checked.contents != contents {
            return Err(StreetError::archive(
                "the finished file does not match its plan",
            ));
        }
        Ok(())
    });
    if let Err(e) = verified {
        if e.kind != ErrorKind::Cancelled {
            drop((part, journal));
            let _ = fs::remove_file(&part_path);
            let _ = fs::remove_file(&journal_path);
        }
        return Err(e);
    }
    let sha256 = sha256_file(&part, total)?;
    drop((part, journal));
    fs::rename(&part_path, dir.pack(&plan.pack_id)).map_err(io_err("name the street map file"))?;
    let _ = fs::remove_file(&journal_path);
    Ok(Finished {
        bytes: total,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_journal_cut_short_keeps_only_its_whole_lines() {
        let info = JournalInfo {
            pack_id: "b-streets-50km-0n0e".into(),
            area: StreetArea {
                lat: 0.0,
                lon: 0.0,
                km: 50,
                detail: crate::area::Detail::Streets,
            },
            build_id: "b".into(),
            build_url: "http://h/b.pmtiles".into(),
            data_date: "2026-10-04".into(),
            download_bytes: 300,
            pack_bytes: 400,
            requests: 3,
        };
        let head = format!(
            "{JOURNAL_MAGIC} abc\n{}\n",
            serde_json::to_string(&info).unwrap()
        );
        let j = parse_journal(format!("{head}2 100\n0 100\n1 1").as_bytes()).unwrap();
        assert_eq!(j.fingerprint, "abc");
        assert_eq!(j.info, info);
        assert_eq!(
            j.done,
            vec![(2, 100), (0, 100)],
            "the torn last line is not trusted"
        );
        assert!(parse_journal(b"something else\n").is_none());
        assert!(parse_journal(format!("{JOURNAL_MAGIC} abc\n{{not json\n").as_bytes()).is_none());
    }
}
