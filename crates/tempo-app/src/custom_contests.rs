//! ⭐ **The operator's own contests on disk** — `custom-contests.json` beside settings.json, and
//! the save / delete / import / export rules the Tauri commands apply.
//!
//! The definitions themselves, and their compilation, live in
//! [`tempo_core::fd_rules::custom`]; the installed set lives in tempo-core's user layer. This
//! module keeps the FILE in step with that layer: every change installs first and then writes the
//! whole installed set back, so the file never holds a definition the layer refused.
//!
//! ⚠️ **An unreadable file is reported and never deleted, nor overwritten.** It holds work the
//! operator built by hand; a save while it could not be read would replace every contest in it
//! with the one just saved. So a load that failed leaves [`Store::locked`] set and every write
//! refuses, naming the file, until the operator moves it aside.
//!
//! ⚠️ **A definition carries no personal data** (the maintainer's ruling on sharing). The values
//! an operator types for its exchange live in `Settings::contest_entered`, which Export never
//! reads.

use std::path::{Path, PathBuf};
use tempo_core::fd_rules::custom::{self, CustomDef};
use tempo_core::fd_rules::{self, CURRENT_RULES_YEAR};

/// The file's name, beside settings.json.
pub const FILE_NAME: &str = "custom-contests.json";

/// The file: a version and the definitions, each kept as raw JSON so one a later build cannot
/// read never costs the operator the others.
#[derive(serde::Serialize, serde::Deserialize)]
struct FileV1 {
    version: u32,
    contests: Vec<serde_json::Value>,
}

/// Where the file lives and whether it may be written.
#[derive(Debug, Clone)]
pub struct Store {
    pub path: PathBuf,
    /// Set when the file exists and could not be read: every write refuses (see the module doc).
    pub locked: Option<String>,
    /// ⭐ **The entries this build could not use**, as the file held them: one a later version
    /// wrote (a downgrade), or one a newer rules download now refuses (its token became a shipped
    /// contest's). Written back untouched beside the installed set, so a save never deletes one.
    skipped: Vec<Unusable>,
}

/// An entry of the contest file this build cannot use, kept as the file held it.
#[derive(Debug, Clone)]
struct Unusable {
    raw: serde_json::Value,
    /// Its `id` and `name`, read off the raw JSON (`""` when it carries none).
    id: String,
    name: String,
    why: String,
}

/// One entry of the contest file this version cannot use — shown so the operator sees it is
/// still there, and kept on disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnusableContestDto {
    pub id: String,
    pub name: String,
    /// Why this version cannot use it, in words.
    pub why: String,
}

/// What [`import`] does when the file's id is already one of the operator's contests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnCollision {
    /// Refuse, naming the contest it would replace — the UI's question.
    Ask,
    /// The imported definition replaces the installed one (the edit lock still applies).
    Replace,
    /// Install it beside the existing one, under a fresh id and Cabrillo token.
    KeepBoth,
}

/// An import refused only because its id is taken: the UI asks Replace or Keep both.
pub const COLLISION_PREFIX: &str = "collision:";

impl Store {
    /// ⭐ **Load the file and install every definition it holds** — at startup, after the rules
    /// file (compiling reads the rules table, and the first read locks it). Returns the store and
    /// the problems to log: one line for an unreadable file, one per refused definition.
    pub fn load(path: PathBuf, now_unix: u64) -> (Store, Vec<String>) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return (
                    Store {
                        path,
                        locked: None,
                        skipped: Vec::new(),
                    },
                    Vec::new(),
                )
            }
            Err(e) => {
                let why = format!(
                    "{} could not be read ({e}); it was left as it is",
                    path.display()
                );
                return (
                    Store {
                        path,
                        locked: Some(why.clone()),
                        skipped: Vec::new(),
                    },
                    vec![why],
                );
            }
        };
        let file: FileV1 = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                let why = format!(
                    "{} is not a contest list ({e}); it was left as it is",
                    path.display()
                );
                return (
                    Store {
                        path,
                        locked: Some(why.clone()),
                        skipped: Vec::new(),
                    },
                    vec![why],
                );
            }
        };
        let mut problems = Vec::new();
        let mut skipped = Vec::new();
        for raw in file.contests {
            let text = |key: &str| raw[key].as_str().unwrap_or_default().to_string();
            let (id, name) = (text("id"), text("name"));
            let why = match serde_json::from_value::<CustomDef>(raw.clone()) {
                Ok(def) => match fd_rules::install_custom(std::slice::from_ref(&def), now_unix)
                    .into_iter()
                    .next()
                {
                    None => continue,
                    Some((_, why)) => why,
                },
                Err(e) => format!("not a contest this version can read ({e})"),
            };
            problems.push(format!(
                "your contest {id:?} in {} can't be used by this version, and is kept as it is: {why}",
                path.display()
            ));
            skipped.push(Unusable { raw, id, name, why });
        }
        (
            Store {
                path,
                locked: None,
                skipped,
            },
            problems,
        )
    }

    /// ⭐ **The file's entries this version cannot use** — each still on disk, and written back
    /// untouched by every save.
    pub fn unusable(&self) -> Vec<UnusableContestDto> {
        self.skipped
            .iter()
            .map(|u| UnusableContestDto {
                id: u.id.clone(),
                name: u.name.clone(),
                why: u.why.clone(),
            })
            .collect()
    }

    /// Write the installed set back, whole, and after it every entry this version could not use,
    /// as the file held it.
    fn write(&self) -> Result<(), String> {
        if let Some(why) = &self.locked {
            return Err(format!(
                "Your contests were not saved: {why}. Move it aside first."
            ));
        }
        let file = FileV1 {
            version: custom::VERSION,
            contests: fd_rules::custom_defs()
                .iter()
                .map(|d| serde_json::to_value(d).expect("a definition serialises"))
                .chain(self.skipped.iter().map(|u| u.raw.clone()))
                .collect(),
        };
        let text = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
        crate::scp::write_atomic(&self.path, &text).map_err(|e| {
            format!(
                "Your contests could not be saved to {}: {e}",
                self.path.display()
            )
        })
    }

    /// ⭐ **Save a definition** — a new contest, or an edit of an installed one.
    ///
    /// ⭐ **The edit lock.** Once a contest has contacts in a journal (`journal_dir`), its
    /// exchange and its Cabrillo token stay as they are: the rows carry the exchange they were
    /// logged with, and a restore finds its rows by the token, so either change would orphan
    /// them. Its name, window and points stay editable. The operator saves it as a new contest
    /// instead.
    pub fn save_contest(
        &mut self,
        def: &CustomDef,
        journal_dir: &Path,
        now_unix: u64,
    ) -> Result<(), String> {
        if let Some(why) = &self.locked {
            return Err(format!(
                "Your contests were not saved: {why}. Move it aside first."
            ));
        }
        if self.skipped.iter().any(|u| u.id == def.id) {
            return Err(format!(
                "Your contest file already holds a contest {:?} this version can't use. Delete \
                 that one first, or give this contest another name.",
                def.id
            ));
        }
        let next = custom::compile(def, now_unix)?;
        if let Some(old) = fd_rules::ruleset_by_id(&def.id, CURRENT_RULES_YEAR).filter(|r| r.custom)
        {
            let changes_rows = *old.exchange != *next.exchange
                || !old.contest_id.eq_ignore_ascii_case(next.contest_id);
            if changes_rows && journal_has_rows(journal_dir, old.contest_id) {
                return Err(format!(
                    "{} has contacts logged, so its exchange and Cabrillo contest name can no \
                     longer change. Save it as a new contest instead.",
                    def.name.trim()
                ));
            }
        }
        if let Some((_, why)) = fd_rules::install_custom(std::slice::from_ref(def), now_unix)
            .into_iter()
            .next()
        {
            return Err(why);
        }
        self.write()
    }

    /// ⭐ **Delete a contest** — refused while it is the picked one (`picked`, the settings'
    /// `fd_event`). Its journals stay on disk.
    /// An entry this version cannot use is deleted from the file the same way.
    pub fn delete(&mut self, id: &str, picked: &str) -> Result<(), String> {
        if let Some(why) = &self.locked {
            return Err(format!(
                "Your contests were not saved: {why}. Move it aside first."
            ));
        }
        if picked.trim() == id {
            return Err(
                "This is the contest you have picked. Pick another contest before you delete it."
                    .into(),
            );
        }
        let before = self.skipped.len();
        self.skipped.retain(|u| u.id != id);
        if !fd_rules::remove_custom(id) && self.skipped.len() == before {
            return Err(format!("There is no contest {id:?} to delete."));
        }
        self.write()
    }

    /// ⭐ **Import a definition file's text.** An id already in use is refused with
    /// [`COLLISION_PREFIX`] and the installed contest's name under [`OnCollision::Ask`]; the
    /// operator then picks Replace or Keep both. Returns the definition as installed (Keep both
    /// changes its id and token).
    pub fn import(
        &mut self,
        text: &str,
        on_collision: OnCollision,
        journal_dir: &Path,
        now_unix: u64,
    ) -> Result<CustomDef, String> {
        let mut def: CustomDef =
            serde_json::from_str(text).map_err(|e| format!("not a custom contest file: {e}"))?;
        if let Some(existing) = fd_rules::custom_def(&def.id) {
            match on_collision {
                OnCollision::Ask => {
                    return Err(format!("{COLLISION_PREFIX}{}", existing.name));
                }
                OnCollision::Replace => {}
                OnCollision::KeepBoth => {
                    def.id = custom::id_for_name(&def.name, |id| {
                        fd_rules::ruleset_by_id(id, CURRENT_RULES_YEAR).is_some()
                            || self.skipped.iter().any(|u| u.id == id)
                    });
                    def.cabrillo_contest = free_token(&def.cabrillo_contest);
                }
            }
        }
        self.save_contest(&def, journal_dir, now_unix)?;
        Ok(def)
    }
}

/// Is `event_id` (a settings `fd_event`) an operator contest's id, installed or not?
pub fn is_custom_id(event_id: &str) -> bool {
    event_id.trim().starts_with(custom::ID_PREFIX)
}

/// ⭐ **The refusal for an operator contest that is not installed** — picked, then deleted, or its
/// file could not be read. It never falls back to Field Day.
pub fn not_installed(event_id: &str) -> String {
    format!(
        "The custom contest '{}' is not installed (deleted, or its file could not be read). \
         Pick a contest.",
        event_id.trim()
    )
}

/// The file Export writes for the installed contest `id`: the definition and nothing else.
pub fn export_text(id: &str) -> Result<String, String> {
    fd_rules::custom_def(id)
        .map(|d| custom::to_file_text(&d))
        .ok_or_else(|| format!("There is no contest {id:?} to export."))
}

/// `token`, or `token-2`, `token-3`… — the first no ruleset holds, cut to 32 characters.
fn free_token(token: &str) -> String {
    let taken = |t: &str| fd_rules::ruleset_by_contest_id(t, CURRENT_RULES_YEAR).is_some();
    if !taken(token) {
        return token.to_string();
    }
    (2u32..)
        .map(|n| {
            let tail = format!("-{n}");
            let head: String = token.chars().take(32 - tail.len()).collect();
            format!("{head}{tail}")
        })
        .find(|t| !taken(t))
        .expect("an unbounded counter finds a free token")
}

/// ⭐ **Does any contest journal in `dir` hold a row of `contest_id`?** Every
/// `fieldday_backup*.adi` beside settings.json is read — one per position, so a two-radio shack's
/// other window counts too — and a row counts by its `CONTEST_ID`, which is what a journal
/// restore finds its rows by. A journal that cannot be read counts as holding rows: the lock is
/// the safe answer when the question cannot be asked.
pub fn journal_has_rows(dir: &Path, contest_id: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("fieldday_backup") && name.ends_with(".adi")) {
            return false;
        }
        match std::fs::read_to_string(e.path()) {
            Ok(text) => tempo_core::logbook::parse_adif(&text).iter().any(|r| {
                r.contest
                    .as_ref()
                    .is_some_and(|c| c.contest_id.trim().eq_ignore_ascii_case(contest_id))
            }),
            Err(_) => true,
        }
    })
}

/// One installed contest as the contest list shows it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomContestDto {
    /// The definition, exactly as Export writes it.
    pub def: CustomDef,
    /// The slots the setup asks the operator to type a value for (`AGE`, `NAME`, `TXT1`).
    pub entered_slots: Vec<String>,
    /// The station settings its exchange sends from (Rust names).
    pub sent_settings: Vec<String>,
    /// Contacts are logged under it: its exchange and Cabrillo contest name are locked
    /// ([`Store::save_contest`]); "Save as a new contest" is offered instead.
    pub exchange_locked: bool,
}

/// ⭐ **The installed contests**, in install order, each with what the setup and the list need.
pub fn list(journal_dir: &Path) -> Vec<CustomContestDto> {
    fd_rules::custom_defs()
        .into_iter()
        .filter_map(|def| {
            let rs = fd_rules::ruleset_by_id(&def.id, CURRENT_RULES_YEAR).filter(|r| r.custom)?;
            let owned = |v: Vec<&str>| v.into_iter().map(str::to_string).collect();
            Some(CustomContestDto {
                entered_slots: owned(rs.entered_slots()),
                sent_settings: owned(rs.sent_settings()),
                exchange_locked: journal_has_rows(journal_dir, rs.contest_id),
                def,
            })
        })
        .collect()
}

/// ⭐ **What the builder's preview shows** — every line rendered here, from the real compiler,
/// session and exporter, so the UI never re-implements a rule.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPreview {
    /// Why the definition cannot be saved, verbatim; `""` when it can. Nothing below is filled
    /// when it is not.
    pub refusal: String,
    /// What this station would send, with the current settings and typed values (the serial as
    /// its first number, 1): `"599 1"`.
    pub you_send: String,
    /// Why the contest could not start with the current settings (`Your AGE is empty…`); `""`
    /// when it could.
    pub start_problem: String,
    /// One sample Cabrillo `QSO:` line from the real exporter.
    pub sample_qso_line: String,
    /// The WSJT-X contest message the exchange matches (`rtty_roundup`…), or `None`.
    pub ft_shape: Option<custom::FtActivity>,
    /// That match, when the operator chose to use it (an RST + serial exchange is asked).
    pub ft_activity: Option<custom::FtActivity>,
    /// What FT8/FT4 actually send in this build, from the sequencer's own choice
    /// ([`FtMessages::for_contest`](tempo_core::fieldday::FtMessages::for_contest)): `"standard"`,
    /// or the activity's name as [`custom::FtActivity`] spells it where this build sends its
    /// messages.
    pub ft_messages: String,
    /// The CQ the contest's Running calls on FT8/FT4 (`CQ RU W9XYZ EN52`), from the Settings call
    /// and locator; `""` with the standard messages.
    pub ft_cq: String,
    /// The exchange those messages carry after the two calls (`579 0001`), as the sequencer would
    /// send it first; `""` with the standard messages.
    pub ft_sends: String,
    /// Why those messages cannot carry this station's exchange with the current settings (a QTH
    /// the RTTY Roundup message has no word for), as the station would refuse it; `""` when they
    /// can.
    pub ft_refusal: String,
    /// The slots the setup will ask the operator to type.
    pub entered_slots: Vec<String>,
}

/// ⭐ **Preview `def` for `settings`** — compiled (never installed), a session started on the
/// station's real settings and the values typed for `def.id`, and a sample row exported.
pub fn preview(
    def: &CustomDef,
    settings: &crate::settings::Settings,
    now_unix: u64,
) -> CustomPreview {
    let rs: &'static fd_rules::FdRuleset = match custom::compile(def, now_unix) {
        Ok(rs) => Box::leak(Box::new(rs)),
        Err(refusal) => {
            return CustomPreview {
                refusal,
                ..Default::default()
            }
        }
    };
    use tempo_core::fieldday::FtMessages;
    let messages = FtMessages::for_contest(rs, "FT8");
    let mut out = CustomPreview {
        ft_shape: custom::ft_shape(def),
        ft_activity: rs.ft_activity,
        ft_messages: match messages {
            FtMessages::Standard => "standard",
            FtMessages::FieldDay => "field_day",
            FtMessages::NaVhf => "na_vhf",
            FtMessages::WwDigi => "ww_digi",
            FtMessages::RttyRoundup => "rtty_roundup",
            FtMessages::EuVhf => "eu_vhf",
        }
        .to_string(),
        entered_slots: rs.entered_slots().into_iter().map(str::to_string).collect(),
        ..Default::default()
    };
    let mut mine = settings.clone();
    mine.fd_event = def.id.clone();
    let station = crate::engine::Engine::contest_station_data_of(&mine);
    let session = match tempo_core::contest::ContestSession::for_ruleset(rs, &station) {
        Ok(s) => s,
        Err(e) => {
            out.start_problem = e;
            return out;
        }
    };
    if messages != FtMessages::Standard {
        let grid = session.field("GRID");
        out.ft_cq = messages.cq_on_air(settings.mycall.trim(), settings.mygrid.trim(), grid);
        out.ft_sends = messages.sample_exchange(&session, settings.mygrid.trim());
        out.ft_refusal = messages
            .exchange_refusal(&session, settings.mygrid.trim())
            .unwrap_or_default();
    }
    let sent: Vec<(String, String)> = session
        .my_exchange
        .iter()
        .map(|v| {
            let serial = matches!(
                rs.exchange.field(v.key).map(|f| f.kind),
                Some(tempo_core::contest::FieldKind::Serial { .. })
            );
            let raw = if serial {
                "1".to_string()
            } else {
                v.raw.clone()
            };
            (v.key.to_string(), raw)
        })
        .collect();
    out.you_send = sent
        .iter()
        .map(|(_, v)| v.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    // The sample contact copies back what this station sends, on 20 m CW.
    let mut log = tempo_core::fieldday::FieldDayLog::new(settings.mycall.trim(), session, "20m");
    log.dial_khz = 14_025;
    if log.log_fields_at("K1ABC", &sent, "CW", "", 0, now_unix) {
        if let Ok(text) = log.cabrillo(14_025) {
            out.sample_qso_line = text
                .lines()
                .find(|l| l.starts_with("QSO:"))
                .unwrap_or_default()
                .to_string();
        }
    }
    out
}
