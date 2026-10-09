//! Super Check Partial — the contest-call list the strip matches against as a call is typed.
//!
//! `MASTER.SCP` is a plain-text list of the callsigns seen in contest logs over the past two
//! years, built from Cabrillo logs the contest community contributes and published by
//! supercheckpartial.com (maintained by W9KKN) for contest logging software to download. One call
//! per line; a line beginning `!` or `#` is a header.
//!
//! ⚠️ **NEVER SHIPPED.** The site publishes no licence, so no copy rides in an installer, in the
//! repository or in a test fixture: every list in the tests below is synthetic. Nexus downloads
//! the file on the operator's own machine, under the rules the site's Developers page sets for
//! logging software ("Downloading SCP Files"), and each rule is held by code here:
//!
//! - "Use conditional requests (If-None-Match with the ETag header, or If-Modified-Since)" and
//!   "Check for HTTP 304 Not Modified and use your cached copy": [`ensure`] sends back both
//!   validators the list in use was served with, and a 304 keeps the file untouched.
//! - "Respect Cache-Control headers" and, from the Don'ts, "Poll more frequently than once per
//!   day": [`next_check_after`] is never sooner than a day, and later when `max-age` says so.
//! - "Download only the specific file variant and format your software needs": [`VARIANT`] only.
//! - "Include a User-Agent header identifying your software and version": the shell passes
//!   `Nexus/<version>`, and every request carries it.
//! - "Allow users to configure or disable automatic updates": the daily check is
//!   `Settings::scp_auto_update`, and SCP itself is `Settings::scp_enabled`; off sends nothing.
//! - "Download on every application launch without checking if the file has changed" (a Don't):
//!   nothing here runs at launch, and [`should_ask`] gates every request on the daily clock.
//! - "Hardcode URLs -- use /api/v1/files to discover available files" (a Don't): the listing is
//!   read first, and the variant downloaded is the one it names, from where the site's own pages
//!   link it.
//!
//! The HTTP itself is the shell's (a [`Fetcher`]). This module owns every DECISION — whether to ask
//! at all, what to send, what a reply means, and when a download may replace the list in use — so
//! the rules are unit-tested with a fake fetcher and no test ever reaches the site.
//!
//! A download that is not a good list never replaces one that is: it is checked in memory
//! ([`MIN_CALLS`], [`MAX_BYTES`]), written beside the file in use and renamed over it only then. A
//! failed check keeps the list, records why, and the strip goes on matching against what it has.
//!
//! None of this runs under the Engine lock: the radio loop is the only unkey path, and a download
//! can take as long as the network does. The shell calls [`ensure`] off the lock.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The site's file listing: the discovery endpoint its Developers page names.
pub const LISTING_URL: &str = "https://www.supercheckpartial.com/api/v1/files";
/// Where the site's own pages link each listed file: `/downloads/<name>`.
pub const DOWNLOAD_BASE: &str = "https://www.supercheckpartial.com/downloads/";
/// The one variant Nexus uses: every call active in contests, as plain text.
pub const VARIANT: &str = "MASTER.SCP";
/// The list in use, in the directory the shell passes in.
pub const FILE_NAME: &str = "MASTER.SCP";
/// What Nexus knows about that file: its validators, dates, call count and the last failure.
pub const META_NAME: &str = "MASTER.SCP.meta.json";
/// A download with fewer calls than this is not the list (an error page, a cut-off body) and never
/// replaces one. The list holds tens of thousands.
pub const MIN_CALLS: usize = 5_000;
/// A download larger than this is not the list either. It is about 360 KB.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
/// "Poll more frequently than once per day" is one of the site's Don'ts.
pub const DAY_SECS: i64 = 86_400;
/// The longest `max-age` honoured, so a broken header cannot stop the updates for a year.
const MAX_AGE_CAP_SECS: i64 = 7 * DAY_SECS;

/// One HTTP reply, as much of it as the rules read.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cache_control: Option<String>,
    pub body: Vec<u8>,
}

/// The shell's HTTP GET. `Err` is a transport failure: no reply at all.
pub trait Fetcher {
    fn get(&self, url: &str, headers: &[(&'static str, String)]) -> Result<Reply, String>;
}

/// What Nexus holds, and what it last heard from the site. Kept beside the list in app data,
/// never in settings, so no profile or backup carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScpMeta {
    /// The validators the list in use was served with, sent back on the next check.
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// When the list in use was downloaded (Unix seconds; 0 = never).
    pub fetched_at: i64,
    /// When Nexus last asked the site, whatever the answer (0 = never).
    pub checked_at: i64,
    /// No automatic request before this.
    pub next_check_at: i64,
    /// Calls in the list in use (0 = no list).
    pub count: usize,
    /// Why the last check failed, until one succeeds. The list in use is untouched by a failure.
    pub last_error: Option<String>,
}

/// The switches a request has to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    /// SCP effectively on: the operator's switch, minus Unassisted mode (`Settings::scp_active`).
    pub active: bool,
    /// The daily check (`Settings::scp_auto_update`).
    pub auto_update: bool,
    /// The operator asked: "Update now".
    pub manual: bool,
}

/// The calls in a `.scp` text, in file order. Header lines (`!`, `#`) and anything that is not
/// callsign-shaped are skipped, so an error page served as a 200 parses to almost nothing.
pub fn parse(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('!') && !l.starts_with('#'))
        .map(str::to_ascii_uppercase)
        .filter(|c| is_call_shaped(c))
        .collect()
}

/// Letters, digits and `/`, 3 to 15 of them, with at least one letter and one digit.
pub(crate) fn is_call_shaped(c: &str) -> bool {
    (3..=15).contains(&c.len())
        && c.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
        && c.bytes().any(|b| b.is_ascii_digit())
        && c.bytes().any(|b| b.is_ascii_uppercase())
}

/// The list in use, or empty when there is none.
pub fn load(dir: &Path) -> Vec<String> {
    std::fs::read(dir.join(FILE_NAME))
        .map(|b| parse(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}

/// What Nexus knows about the list; the default when nothing has been recorded.
pub fn read_meta(dir: &Path) -> ScpMeta {
    std::fs::read(dir.join(META_NAME))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, meta: &ScpMeta) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(meta).map_err(std::io::Error::other)?;
    write_atomic(&dir.join(META_NAME), &json)
}

/// Write `bytes` beside `path` and rename them over it, so a reader sees the old file or the new
/// one and never half of either. Rename replaces atomically on Unix and Windows.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    let mut f = std::fs::File::create(&tmp)?;
    std::io::Write::write_all(&mut f, bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)
}

/// May Nexus ask the site now? Every request goes through here.
pub fn should_ask(gate: Gate, meta: &ScpMeta, have_list: bool, now: i64) -> bool {
    if !gate.active {
        return false; // off, or Unassisted: nothing at all, not even "Update now"
    }
    if gate.manual {
        return true;
    }
    if now < meta.next_check_at {
        return false; // never more than once a day
    }
    // The first list downloads whatever the daily switch says: turning SCP on is asking for it.
    !have_list || gate.auto_update
}

/// The earliest automatic check after an answer that carried `cache_control`: a day, or the
/// reply's `max-age` when that is longer (capped at a week).
pub fn next_check_after(now: i64, cache_control: Option<&str>) -> i64 {
    let max_age = cache_control
        .and_then(|cc| {
            cc.split(',').find_map(|d| {
                let (k, v) = d.trim().split_once('=')?;
                if k.trim().eq_ignore_ascii_case("max-age") {
                    v.trim().trim_matches('"').parse::<i64>().ok()
                } else {
                    None
                }
            })
        })
        .unwrap_or(0);
    now + max_age.clamp(DAY_SECS, MAX_AGE_CAP_SECS)
}

/// What the site said.
enum Answer {
    NotModified {
        cache_control: Option<String>,
    },
    Fresh {
        etag: Option<String>,
        last_modified: Option<String>,
        count: usize,
        cache_control: Option<String>,
    },
}

/// One row of the listing. The site sends `etag` and `modified` too; nothing here reads them.
#[derive(Deserialize)]
struct Listed {
    name: String,
    #[serde(default)]
    size: u64,
}

/// Discover the variant in the listing, then fetch it, conditionally when a list is held.
fn ask(
    fetcher: &dyn Fetcher,
    dir: &Path,
    meta: &ScpMeta,
    have_list: bool,
    user_agent: &str,
) -> Result<Answer, String> {
    let ua = ("User-Agent", user_agent.to_string());
    let listing = fetcher.get(LISTING_URL, std::slice::from_ref(&ua))?;
    if listing.status != 200 {
        return Err(format!("the file listing answered HTTP {}", listing.status));
    }
    let files: Vec<Listed> = serde_json::from_slice(&listing.body)
        .map_err(|_| "the file listing could not be read".to_string())?;
    let entry = files
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case(VARIANT))
        .ok_or_else(|| format!("the file listing no longer offers {VARIANT}"))?;
    if entry.size > MAX_BYTES as u64 {
        return Err(format!(
            "{} is listed at {} bytes, more than this list can be",
            entry.name, entry.size
        ));
    }
    let mut headers = vec![ua];
    if have_list {
        if let Some(t) = &meta.etag {
            headers.push(("If-None-Match", t.clone()));
        }
        if let Some(t) = &meta.last_modified {
            headers.push(("If-Modified-Since", t.clone()));
        }
    }
    let reply = fetcher.get(&format!("{DOWNLOAD_BASE}{}", entry.name), &headers)?;
    match reply.status {
        304 if have_list => Ok(Answer::NotModified {
            cache_control: reply.cache_control,
        }),
        200 => {
            if reply.body.len() > MAX_BYTES {
                return Err(format!(
                    "the download was larger than {MAX_BYTES} bytes; the list in use is kept"
                ));
            }
            let count = parse(&String::from_utf8_lossy(&reply.body)).len();
            if count < MIN_CALLS {
                return Err(format!(
                    "the download held {count} calls, not a list (at least {MIN_CALLS}); \
                     the list in use is kept"
                ));
            }
            write_atomic(&dir.join(FILE_NAME), &reply.body)
                .map_err(|e| format!("the list could not be saved: {e}"))?;
            Ok(Answer::Fresh {
                etag: reply.etag,
                last_modified: reply.last_modified,
                count,
                cache_control: reply.cache_control,
            })
        }
        s => Err(format!("{VARIANT} answered HTTP {s}")),
    }
}

/// The one entry point: ask the site if the rules allow it, and say what Nexus now holds.
///
/// The check is recorded BEFORE the request goes out, and a check that cannot be recorded is not
/// made: otherwise a data folder that refuses writes would turn the daily check into one per call.
pub fn ensure(
    fetcher: &dyn Fetcher,
    dir: &Path,
    gate: Gate,
    now: i64,
    user_agent: &str,
) -> ScpMeta {
    let mut meta = read_meta(dir);
    let held = load(dir).len();
    let have_list = held >= MIN_CALLS;
    meta.count = held;
    if !should_ask(gate, &meta, have_list, now) {
        return meta;
    }
    meta.checked_at = now;
    meta.next_check_at = now + DAY_SECS;
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| write_meta(dir, &meta)) {
        meta.last_error = Some(format!("the check could not be recorded: {e}"));
        return meta;
    }
    match ask(fetcher, dir, &meta, have_list, user_agent) {
        Ok(Answer::NotModified { cache_control }) => {
            meta.last_error = None;
            meta.next_check_at = next_check_after(now, cache_control.as_deref());
        }
        Ok(Answer::Fresh {
            etag,
            last_modified,
            count,
            cache_control,
        }) => {
            meta.etag = etag;
            meta.last_modified = last_modified;
            meta.fetched_at = now;
            meta.count = count;
            meta.last_error = None;
            meta.next_check_at = next_check_after(now, cache_control.as_deref());
        }
        Err(why) => meta.last_error = Some(why),
    }
    // Best-effort: the check itself is already on record above.
    let _ = write_meta(dir, &meta);
    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::PathBuf;

    const UA: &str = "Nexus/9.9.9";
    const NOW: i64 = 1_800_000_000;
    const ON: Gate = Gate {
        active: true,
        auto_update: true,
        manual: false,
    };

    /// One request as the fake saw it: the URL and its headers.
    type Seen = (String, Vec<(String, String)>);

    /// A fetcher that answers from a script and remembers every request. Nothing in these tests
    /// reaches the network: a request nobody scripted is a transport failure.
    #[derive(Default)]
    struct Fake {
        replies: RefCell<VecDeque<Result<Reply, String>>>,
        seen: RefCell<Vec<Seen>>,
    }
    impl Fake {
        fn answering(replies: Vec<Result<Reply, String>>) -> Self {
            Fake {
                replies: RefCell::new(replies.into()),
                seen: RefCell::default(),
            }
        }
        fn urls(&self) -> Vec<String> {
            self.seen.borrow().iter().map(|(u, _)| u.clone()).collect()
        }
        fn header(&self, i: usize, name: &str) -> Option<String> {
            self.seen.borrow()[i]
                .1
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        }
    }
    impl Fetcher for Fake {
        fn get(&self, url: &str, headers: &[(&'static str, String)]) -> Result<Reply, String> {
            self.seen.borrow_mut().push((
                url.to_string(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect(),
            ));
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Err("no reply scripted".into()))
        }
    }

    /// A synthetic list in the published shape: the two header lines, then `n` made-up calls.
    fn synthetic(n: usize, prefix: &str) -> String {
        let mut s = String::from("!!Order,1,1\r\n# SUPER CHECK PARTIAL\r\n");
        for i in 0..n {
            let a = (b'A' + (i % 26) as u8) as char;
            let b = (b'A' + ((i / 26) % 26) as u8) as char;
            let c = (b'A' + ((i / 676) % 26) as u8) as char;
            s.push_str(&format!("{prefix}{}{a}{b}{c}\r\n", i / 17_576));
        }
        s
    }

    fn listing() -> Reply {
        Reply {
            status: 200,
            body: br#"[{"name":"MASTER.DTA","size":1332758,"etag":"x","modified":"2026-10-09T00:03:43Z"},
                       {"name":"MASTER.SCP","size":359684,"etag":"y","modified":"2026-10-09T00:03:42Z"},
                       {"name":"MASTERDX.SCP","size":260332,"etag":"z","modified":"2026-10-09T00:03:43Z"}]"#
                .to_vec(),
            ..Reply::default()
        }
    }

    fn file(body: &str, etag: &str) -> Reply {
        Reply {
            status: 200,
            etag: Some(etag.into()),
            last_modified: Some("Fri, 09 Oct 2026 00:03:42 GMT".into()),
            body: body.as_bytes().to_vec(),
            ..Reply::default()
        }
    }

    fn not_modified() -> Reply {
        Reply {
            status: 304,
            ..Reply::default()
        }
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexus-scp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A folder already holding a good list, as a first download leaves it.
    fn holding(tag: &str) -> (PathBuf, String) {
        let d = dir(tag);
        let body = synthetic(6_000, "K");
        let fake = Fake::answering(vec![Ok(listing()), Ok(file(&body, "\"v1\""))]);
        let meta = ensure(&fake, &d, ON, NOW, UA);
        assert_eq!(meta.count, 6_000, "fixture: the first download must land");
        (d, body)
    }

    #[test]
    fn parses_the_published_shape() {
        assert_eq!(
            parse("!!Order,1,1\r\n# SUPER CHECK PARTIAL\r\nAA0AW\r\nAA0BH\r\n"),
            vec!["AA0AW", "AA0BH"]
        );
        // An error page is not a list: nothing in it is callsign-shaped.
        assert_eq!(
            parse("<html><body>Not found</body></html>\n"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn scp_off_sends_nothing_not_even_update_now() {
        let d = dir("off");
        for manual in [false, true] {
            let fake = Fake::answering(vec![Ok(listing())]);
            let gate = Gate {
                active: false,
                auto_update: true,
                manual,
            };
            let meta = ensure(&fake, &d, gate, NOW, UA);
            assert_eq!(
                fake.urls().len(),
                0,
                "SCP off (or Unassisted) must send no request"
            );
            assert_eq!(meta.count, 0);
        }
    }

    #[test]
    fn the_first_download_reads_the_listing_then_fetches_the_variant_it_names() {
        let d = dir("first");
        let body = synthetic(6_000, "K");
        let fake = Fake::answering(vec![Ok(listing()), Ok(file(&body, "\"v1\""))]);
        let meta = ensure(&fake, &d, ON, NOW, UA);
        assert_eq!(
            fake.urls(),
            vec![
                LISTING_URL.to_string(),
                format!("{DOWNLOAD_BASE}MASTER.SCP")
            ]
        );
        assert_eq!(
            fake.header(1, "If-None-Match"),
            None,
            "nothing held, nothing to validate"
        );
        assert_eq!(meta.count, 6_000);
        assert_eq!(meta.etag.as_deref(), Some("\"v1\""));
        assert_eq!(meta.fetched_at, NOW);
        assert_eq!(meta.next_check_at, NOW + DAY_SECS);
        assert_eq!(load(&d).len(), 6_000, "the list in use is the download");
        assert_eq!(
            read_meta(&d),
            meta,
            "what was reported is what was recorded"
        );
    }

    #[test]
    fn every_request_carries_the_nexus_user_agent() {
        let d = dir("ua");
        let fake = Fake::answering(vec![
            Ok(listing()),
            Ok(file(&synthetic(6_000, "K"), "\"v1\"")),
        ]);
        ensure(&fake, &d, ON, NOW, UA);
        assert_eq!(fake.urls().len(), 2);
        assert_eq!(fake.header(0, "User-Agent").as_deref(), Some("Nexus/9.9.9"));
        assert_eq!(fake.header(1, "User-Agent").as_deref(), Some("Nexus/9.9.9"));
    }

    #[test]
    fn a_held_list_is_checked_with_its_own_validators() {
        let (d, _) = holding("validators");
        let fake = Fake::answering(vec![Ok(listing()), Ok(not_modified())]);
        ensure(&fake, &d, ON, NOW + DAY_SECS, UA);
        assert_eq!(fake.header(1, "If-None-Match").as_deref(), Some("\"v1\""));
        assert_eq!(
            fake.header(1, "If-Modified-Since").as_deref(),
            Some("Fri, 09 Oct 2026 00:03:42 GMT")
        );
    }

    #[test]
    fn a_304_leaves_the_file_and_its_mtime_alone() {
        let (d, body) = holding("304");
        let before = std::fs::metadata(d.join(FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        let fake = Fake::answering(vec![Ok(listing()), Ok(not_modified())]);
        let meta = ensure(&fake, &d, ON, NOW + DAY_SECS, UA);
        let after = std::fs::metadata(d.join(FILE_NAME))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(after, before, "a 304 must not rewrite the list");
        assert_eq!(std::fs::read_to_string(d.join(FILE_NAME)).unwrap(), body);
        assert_eq!(meta.count, 6_000);
        assert_eq!(meta.fetched_at, NOW, "still the list downloaded first");
        assert_eq!(meta.checked_at, NOW + DAY_SECS);
        assert_eq!(meta.last_error, None);
    }

    #[test]
    fn a_200_with_twelve_lines_keeps_the_old_list_and_records_why() {
        let (d, body) = holding("short");
        let fake = Fake::answering(vec![Ok(listing()), Ok(file(&synthetic(12, "W"), "\"v2\""))]);
        let meta = ensure(&fake, &d, ON, NOW + DAY_SECS, UA);
        assert_eq!(std::fs::read_to_string(d.join(FILE_NAME)).unwrap(), body);
        assert_eq!(load(&d).len(), 6_000, "the strip still has its list");
        assert_eq!(meta.count, 6_000);
        assert_eq!(
            meta.etag.as_deref(),
            Some("\"v1\""),
            "the old list's validators stay"
        );
        assert!(
            meta.last_error
                .as_deref()
                .unwrap_or("")
                .contains("12 calls"),
            "the failure is recorded: {:?}",
            meta.last_error
        );
    }

    #[test]
    fn a_failed_fetch_keeps_the_list_in_use_and_waits_a_day() {
        let (d, body) = holding("down");
        let fake = Fake::answering(vec![Err("connection refused".into())]);
        let meta = ensure(&fake, &d, ON, NOW + DAY_SECS, UA);
        assert_eq!(std::fs::read_to_string(d.join(FILE_NAME)).unwrap(), body);
        assert_eq!(load(&d).len(), 6_000);
        assert_eq!(meta.last_error.as_deref(), Some("connection refused"));
        assert_eq!(meta.next_check_at, NOW + 2 * DAY_SECS);
        // …and the next call within that day asks nothing.
        let quiet = Fake::answering(vec![]);
        ensure(&quiet, &d, ON, NOW + DAY_SECS + 3_600, UA);
        assert_eq!(quiet.urls().len(), 0);
    }

    #[test]
    fn a_check_23_hours_after_the_last_sends_nothing() {
        let (d, _) = holding("23h");
        let fake = Fake::answering(vec![Ok(listing()), Ok(not_modified())]);
        ensure(&fake, &d, ON, NOW + 23 * 3_600, UA);
        assert_eq!(fake.urls().len(), 0, "never more than once a day");
        ensure(&fake, &d, ON, NOW + 25 * 3_600, UA);
        assert_eq!(fake.urls().len(), 2, "a day later the check runs");
    }

    #[test]
    fn the_daily_switch_off_stops_the_check_but_not_update_now() {
        let (d, _) = holding("daily-off");
        let off = Gate {
            auto_update: false,
            ..ON
        };
        let fake = Fake::answering(vec![Ok(listing()), Ok(not_modified())]);
        ensure(&fake, &d, off, NOW + 3 * DAY_SECS, UA);
        assert_eq!(
            fake.urls().len(),
            0,
            "the operator turned the daily check off"
        );
        ensure(
            &fake,
            &d,
            Gate {
                manual: true,
                ..off
            },
            NOW + 3 * DAY_SECS,
            UA,
        );
        assert_eq!(fake.urls().len(), 2, "Update now still asks");
    }

    #[test]
    fn turning_scp_on_downloads_the_first_list_even_with_the_daily_check_off() {
        let d = dir("first-daily-off");
        let off = Gate {
            auto_update: false,
            ..ON
        };
        let fake = Fake::answering(vec![
            Ok(listing()),
            Ok(file(&synthetic(6_000, "K"), "\"v1\"")),
        ]);
        let meta = ensure(&fake, &d, off, NOW, UA);
        assert_eq!(meta.count, 6_000, "the list downloads on turn-on");
    }

    #[test]
    fn the_listing_decides_what_is_downloaded() {
        let d = dir("listing");
        let gone = Reply {
            status: 200,
            body: br#"[{"name":"MASTERDX.SCP","size":260332}]"#.to_vec(),
            ..Reply::default()
        };
        let fake = Fake::answering(vec![Ok(gone)]);
        let meta = ensure(&fake, &d, ON, NOW, UA);
        assert_eq!(
            fake.urls(),
            vec![LISTING_URL.to_string()],
            "no variant guessed at"
        );
        assert!(meta.last_error.unwrap_or_default().contains("MASTER.SCP"));
        let huge = Reply {
            status: 200,
            body: format!(r#"[{{"name":"MASTER.SCP","size":{}}}]"#, MAX_BYTES + 1).into_bytes(),
            ..Reply::default()
        };
        let fake = Fake::answering(vec![Ok(huge)]);
        ensure(&fake, &d, Gate { manual: true, ..ON }, NOW, UA);
        assert_eq!(
            fake.urls().len(),
            1,
            "a list listed too large is never fetched"
        );
    }

    #[test]
    fn a_longer_max_age_is_respected_and_a_shorter_one_is_not_a_licence_to_poll() {
        assert_eq!(
            next_check_after(NOW, Some("public, max-age=172800")),
            NOW + 172_800
        );
        assert_eq!(
            next_check_after(NOW, Some("public, max-age=3600")),
            NOW + DAY_SECS
        );
        assert_eq!(next_check_after(NOW, None), NOW + DAY_SECS);
        assert_eq!(
            next_check_after(NOW, Some("max-age=999999999")),
            NOW + 7 * DAY_SECS
        );
    }
}
