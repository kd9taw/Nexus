//! The fill job — SPEC-2 v3's **D2-A**, the operator's choice for the country and state a
//! contact's call places it in: *save them*.
//!
//! Until Stage 2 the launch filled both into the log IN MEMORY and wrote nothing ("a launch
//! writes nothing", C9), so the store held what was written and the screens showed what was
//! filled — two answers, and every reader of the store would have had to fill as it read. Now
//! the store holds exactly what every screen shows:
//!
//! - every insert fills both before it writes (`StationCore::fill_with`; `Engine::log_qso`
//!   always did);
//! - what an older build left unfilled is filled ONCE, after the window is up, by this job —
//!   and again only when the resolvers' data changes (a new cty.dat or FCC file, a new build),
//!   which is what `fill_ver` in the store's `log_meta` records.
//!
//! # The job
//!
//! 1. Under the Engine lock: the store's read handles, and nothing else.
//! 2. Off the lock, in ONE read transaction: `fill_ver`. The version it was asked to fill for →
//!    done, and the launch has written nothing. Otherwise every contact lacking a country or a
//!    state — a narrow read of four columns — resolved call by call, still off the lock.
//! 3. [`crate::logwrite::fill`] writes what was found, a chunk at a time: each chunk's rows read
//!    whole from the store with the lock released, and made under it only where the field is
//!    still empty in the row as it now stands — a change in the writer's bulk lane (a contact
//!    logged meanwhile overtakes it). `fill_ver` goes with the last chunk, once every earlier
//!    one is on disk — so the version is on disk only once every fill is.
//!
//! It runs on a thread of its own, never on the radio loop, and nothing it does under the lock
//! touches the disk, calls a resolver, or passes over the log. On the 1.13 path it does
//! nothing: that path fills as it always has, at the launch, into `log.adi` — its store is in
//! memory and gone with the session, so a `fill_ver` there would name nothing the next launch
//! could read.
//!
//! The resolvers are the caller's, and must be the functions the engine's own are
//! (`Engine::set_dxcc_resolver` / `set_state_resolver`): the command layer hands both the same
//! functions, which is what makes a filled row the row an insert would have written.

use std::ops::ControlFlow;
use std::sync::Mutex;

use tempo_core::logbook::sqlite::{Narrow, Order, Scope};

use crate::engine::{engine_lock, Engine};
use crate::logstore::{LogRows, READ_WAIT};
use crate::station::LogFill;

/// The `log_meta` key naming the resolver data the store's fills come from.
pub const FILL_VER: &str = "fill_ver";

/// What the job reads of each contact: the call and grid the resolvers place it from, and the
/// two fields it fills.
const FILLS: Narrow = Narrow {
    columns: &["call", "grid", "country", "state"],
    uploads: false,
};

/// What one run of the job did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FillOutcome {
    /// The store was already filled for this version: nothing read, nothing written.
    pub current: bool,
    /// Contacts lacking a country or a state when the job read them.
    pub lacking: usize,
    /// Contacts that gained a field.
    pub filled: usize,
}

/// Run the fill job once for resolver data `version` (see the module header). `Ok` with nothing
/// done on the 1.13 path, which fills at the launch.
///
/// ⚠️ It reads the store and takes the Engine lock twice, briefly: call it on a thread of its
/// own, with no lock held — never from the radio loop.
pub fn fill_log_store(
    engine: &Mutex<Engine>,
    version: i64,
    country: &dyn Fn(&str) -> Option<String>,
    state: &dyn Fn(&str, Option<&str>) -> Option<String>,
) -> Result<FillOutcome, String> {
    let rows = {
        let eng = engine_lock(engine);
        if eng.log_on_file() {
            return Ok(FillOutcome::default());
        }
        eng.log_rows()
    };
    let LogRows::Store(reads) = rows;
    let ((current, lacking), _) = reads
        .read(READ_WAIT, |db| {
            if db.meta(FILL_VER)? == Some(version) {
                return Ok((true, Vec::new()));
            }
            let mut lacking = Vec::new();
            db.each_narrow(FILLS, Scope::All, Order::Log, &mut |r| {
                if let Some(id) = r.id.filter(|_| r.country.is_none() || r.state.is_none()) {
                    lacking.push((
                        id,
                        r.call.clone(),
                        r.grid.clone(),
                        r.country.is_none(),
                        r.state.is_none(),
                    ));
                }
                ControlFlow::Continue(())
            })?;
            Ok((false, lacking))
        })
        .map_err(|e| e.to_string())?;
    if current {
        return Ok(FillOutcome {
            current: true,
            ..FillOutcome::default()
        });
    }
    let found = lacking.len();
    let fills: Vec<LogFill> = lacking
        .into_iter()
        .filter_map(|(id, call, grid, no_country, no_state)| {
            let country = no_country.then(|| country(&call)).flatten();
            let state = no_state.then(|| state(&call, grid.as_deref())).flatten();
            (country.is_some() || state.is_some()).then_some(LogFill { id, country, state })
        })
        .collect();
    let filled = crate::logwrite::fill(engine, &fills, version)?;
    Ok(FillOutcome {
        current: false,
        lacking: found,
        filled,
    })
}
