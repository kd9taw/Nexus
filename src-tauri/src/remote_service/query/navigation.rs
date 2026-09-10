//! Closed planning reads from the native station. A single shared worker computes
//! forecasts away from socket/instrument deadlines, including across reconnects.
//! No browser command can select a native function, file, URL or hardware action.
use super::{Collection, Request, Sources};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const DOCUMENT_BYTES: usize = 2 * 1024 * 1024;
const CHUNK_BYTES: usize = 16 * 1024;
const CACHE_ENTRIES: usize = 128;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const REUSE_MS: u64 = 20_000;
const VALID_MS: u64 = 30_000;

pub(super) fn collection(c: Collection) -> bool {
    matches!(
        c,
        Collection::Connect | Collection::Path | Collection::Satellites | Collection::Satellite
    )
}
pub(super) fn valid_search(c: Collection, search: &str) -> bool {
    match c {
        Collection::Connect | Collection::Satellites => search.is_empty(),
        Collection::Path => {
            let b = search.as_bytes();
            matches!(b.len(), 4 | 6 | 8)
                && b[..2].iter().all(|b| (b'A'..=b'R').contains(b))
                && b[2..4].iter().all(u8::is_ascii_digit)
                && (b.len() < 6 || b[4..6].iter().all(|b| (b'A'..=b'X').contains(b)))
                && (b.len() < 8 || b[6..8].iter().all(u8::is_ascii_digit))
        }
        Collection::Satellite => {
            !search.is_empty()
                && search.len() <= 80
                && search.trim() == search
                && !search.chars().any(char::is_control)
        }
        _ => false,
    }
}

#[derive(Clone)]
struct Context {
    call: String,
    grid: String,
    model: String,
    power: Option<f64>,
    gain: f64,
    log: Arc<()>,
}
impl Context {
    fn read(engine: &crate::SharedEngine) -> Result<Self, &'static str> {
        let e = engine.try_lock().map_err(|_| "applicationBusy")?;
        let s = e.settings();
        if s.mycall.len() > 64
            || s.mygrid.len() > 16
            || s.prop_engine.len() > 32
            || !s.ant_tx_gain_dbi.is_finite()
            || !s.ant_rx_gain_dbi.is_finite()
            || s.station_power_w.is_some_and(|p| !p.is_finite())
        {
            return Err("applicationTooLarge");
        }
        Ok(Self {
            call: s.mycall.clone(),
            grid: s.mygrid.clone(),
            model: s.prop_engine.clone(),
            power: s.station_power_w,
            gain: s.ant_tx_gain_dbi + s.ant_rx_gain_dbi,
            log: e.log_read_token(),
        })
    }
    fn same(&self, other: &Self) -> bool {
        self.call == other.call
            && self.grid == other.grid
            && self.model == other.model
            && self.power == other.power
            && self.gain == other.gain
            && Arc::ptr_eq(&self.log, &other.log)
    }
}
struct Entry {
    id: String,
    kind: Collection,
    search: String,
    context: Context,
    at: Instant,
    value: Result<Arc<Document>, &'static str>,
}
struct Document {
    rows: Vec<Value>,
    meta: Value,
}
#[derive(Default)]
struct Work {
    context: Option<(Context, String)>,
    running: bool,
    entries: VecDeque<Entry>,
}
#[derive(Clone, Default)]
pub struct Source {
    work: Arc<Mutex<Work>>,
    aurora: crate::AuroraCache,
    muf: crate::Kc2gCache,
    protons: crate::ProtonCache,
    scales: crate::ScalesCache,
}
impl Source {
    pub fn new(
        aurora: crate::AuroraCache,
        muf: crate::Kc2gCache,
        protons: crate::ProtonCache,
        scales: crate::ScalesCache,
    ) -> Self {
        Self {
            aurora,
            muf,
            protons,
            scales,
            work: Default::default(),
        }
    }
    pub(super) fn read(
        &self,
        request: &Request,
        engine: &crate::SharedEngine,
        sources: &Sources,
    ) -> Result<(Vec<Value>, usize, Value), &'static str> {
        let context = Context::read(engine)?;
        if matches!(request.collection, Collection::Connect | Collection::Path)
            && crate::unassisted()
        {
            return Err("applicationUnavailable");
        }
        let mut w = self.work.try_lock().map_err(|_| "applicationBusy")?;
        w.entries
            .retain(|v| v.at.elapsed().as_millis() < u128::from(VALID_MS));
        if let Some(entry) = w.entries.iter().rev().find(|v| {
            v.kind == request.collection
                && v.search == request.search
                && v.context.same(&context)
                && v.at.elapsed().as_millis() < u128::from(REUSE_MS)
                && (v.value.is_ok() || v.at.elapsed() < Duration::from_secs(1))
        }) {
            let doc = entry.value.as_ref().map_err(|e| *e)?.clone();
            let mut meta = doc.meta.clone();
            meta["documentAgeMs"] = json!(entry.at.elapsed().as_millis() as u64);
            drop(w);
            return Ok((doc.rows.clone(), doc.rows.len(), meta));
        }
        if w.context.as_ref().is_none_or(|(c, _)| !c.same(&context)) {
            w.context = Some((context.clone(), super::snapshot_id()?));
        }
        let station_context = w
            .context
            .as_ref()
            .ok_or("applicationUnavailable")?
            .1
            .clone();
        if !w.running {
            w.running = true;
            let source = self.clone();
            let sources = sources.clone();
            let engine = engine.clone();
            let kind = request.collection;
            let search = request.search.clone();
            // The work slot remains occupied through completion; a disconnected
            // browser cannot accumulate uncancellable P.533/SGP4 workers.
            std::thread::spawn(move || {
                let at = Instant::now();
                let captured_at = super::super::now_ms();
                let id = super::snapshot_id().unwrap_or_default();
                let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let value = build(kind, &search, &context, &engine, &sources)?;
                    if id.is_empty()
                        || !context.same(&Context::read(&engine)?)
                        || at.elapsed().as_millis() >= u128::from(VALID_MS)
                    {
                        return Err("applicationUnavailable");
                    }
                    encode(&value, kind, &search, &id, &station_context, captured_at).map(Arc::new)
                }))
                .unwrap_or(Err("applicationUnavailable"));
                if let Ok(mut w) = source.work.lock() {
                    w.running = false;
                    // Keep an older still-valid capture through a refresh so
                    // another browser can finish its existing sealed cursor.
                    w.entries
                        .retain(|v| v.at.elapsed().as_millis() < u128::from(VALID_MS));
                    w.entries.push_back(Entry {
                        id,
                        kind,
                        search,
                        context,
                        at,
                        value,
                    });
                    while w.entries.len() > CACHE_ENTRIES
                        || w.entries
                            .iter()
                            .map(|e| {
                                e.value
                                    .as_ref()
                                    .ok()
                                    .map_or(0, |d| d.meta["bytes"].as_u64().unwrap_or(0) as usize)
                            })
                            .sum::<usize>()
                            > CACHE_BYTES
                    {
                        w.entries.pop_front();
                    }
                }
            });
        }
        Err("applicationBusy")
    }
    pub(super) fn validate(
        &self,
        engine: &crate::SharedEngine,
        meta: &Value,
    ) -> Result<(), &'static str> {
        let context = Context::read(engine)?;
        let w = self.work.try_lock().map_err(|_| "applicationBusy")?;
        let entry = w
            .entries
            .iter()
            .find(|v| Some(v.id.as_str()) == meta["contextId"].as_str())
            .ok_or("queryExpired")?;
        if !entry.context.same(&context)
            || entry.at.elapsed().as_millis() >= u128::from(VALID_MS)
            || (matches!(entry.kind, Collection::Connect | Collection::Path) && crate::unassisted())
        {
            return Err("queryExpired");
        }
        Ok(())
    }
}
struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > DOCUMENT_BYTES {
            return Err(std::io::Error::other("Remote document budget"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn bounded(value: &impl Serialize) -> Result<Vec<u8>, &'static str> {
    let mut out = Bounded(Vec::new());
    serde_json::to_writer(&mut out, value).map_err(|_| "applicationTooLarge")?;
    Ok(out.0)
}
fn value(value: &impl Serialize) -> Result<Value, &'static str> {
    serde_json::from_slice(&bounded(value)?).map_err(|_| "applicationUnavailable")
}
fn encode(
    value: &Value,
    kind: Collection,
    search: &str,
    id: &str,
    station_context: &str,
    captured_at: u64,
) -> Result<Document, &'static str> {
    let bytes = bounded(value)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "applicationUnavailable")?;
    let mut rows = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + CHUNK_BYTES).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        rows.push(Value::String(text[start..end].into()));
        start = end;
    }
    let meta = json!({"encoding":"json-utf8", "bytes":bytes.len(), "chunks":rows.len(), "kind":kind,
        "search":search,"contextId":id,"stationContextId":station_context,"capturedAtMs":captured_at,"validForMs":VALID_MS,"documentAgeMs":0});
    Ok(Document { rows, meta })
}
fn cached<T: Serialize>(
    cache: &Arc<Mutex<Option<(Instant, T)>>>,
    ttl: u64,
) -> Result<Value, &'static str> {
    let g = cache.try_lock().map_err(|_| "applicationBusy")?;
    match g.as_ref() {
        Some((at, data)) if at.elapsed().as_millis() < u128::from(ttl) => Ok(
            json!({"value":value(data)?,"ageMs":at.elapsed().as_millis() as u64,"validForMs":ttl}),
        ),
        _ => Ok(Value::Null),
    }
}
fn build(
    kind: Collection,
    search: &str,
    context: &Context,
    engine: &crate::SharedEngine,
    sources: &Sources,
) -> Result<Value, &'static str> {
    match kind {
        Collection::Connect | Collection::Path => connect(kind, search, context, engine, sources),
        Collection::Satellites => satellites(context, engine),
        Collection::Satellite => satellite(search, context, engine),
        _ => Err("applicationUnsupported"),
    }
}
fn connect(
    kind: Collection,
    search: &str,
    context: &Context,
    engine: &crate::SharedEngine,
    sources: &Sources,
) -> Result<Value, &'static str> {
    if crate::unassisted() {
        return Err("applicationUnavailable");
    }
    let now = crate::now_unix();
    let (prop, mut wx, source_age) = {
        let cache = sources
            .propagation
            .try_lock()
            .map_err(|_| "applicationBusy")?;
        let (at, snap, c) = cache.as_ref().ok_or("applicationUnavailable")?;
        let age = (at.elapsed().as_millis() as u64)
            .max(now.saturating_sub(snap.as_of).max(0) as u64 * 1000);
        if c.call != context.call
            || c.grid != context.grid
            || !Arc::ptr_eq(&c.log, &context.log)
            || !crate::is_real_call(&context.call)
            || snap.as_of <= 0
            || snap.as_of > now + 5
            || !["live", "partial", "cached"].contains(&snap.source.as_str())
            || age >= crate::PROP_TTL_SECS * 1000
        {
            return Err("applicationUnavailable");
        }
        let wx = propagation::SpaceWx {
            sfi: snap.space_wx.sfi,
            ssn: *crate::LAST_SSN.try_lock().map_err(|_| "applicationBusy")?,
            kp: snap.space_wx.kp,
            a_index: snap.space_wx.a_index,
            xray_long: if snap.space_wx.flare { 1e-5 } else { 1e-7 },
        };
        (
            if kind == Collection::Connect {
                value(snap)?
            } else {
                Value::Null
            },
            wx,
            age,
        )
    };
    if kind == Collection::Connect && context.model != "p533" {
        wx.ssn = None;
    }
    let me = propagation::geo::maidenhead_to_latlon(context.grid.trim());
    let prediction = if let Some(me) = me {
        let predictor =
            propagation::make_predictor(&context.model, Some(me), context.power, context.gain);
        if kind == Collection::Path {
            let dx =
                propagation::geo::maidenhead_to_latlon(search).ok_or("applicationUnsupported")?;
            value(&predictor.predict(dx, now, &wx))?
        } else {
            // Same native eight-azimuth, 9000-km ring; the shared worker bounds
            // CPU without blocking live publication or changing the chosen model.
            value(
                &tauri::async_runtime::block_on(crate::ring_prediction(
                    context.grid.clone(),
                    context.model.clone(),
                    context.power,
                    context.gain,
                    me,
                    wx,
                    now,
                ))
                .map_err(|_| "applicationUnavailable")?,
            )?
        }
    } else {
        Value::Null
    };
    if kind == Collection::Path {
        return Ok(
            json!({"mygrid":context.grid,"grid":search,"prediction":prediction,"sourceAgeMs":source_age}),
        );
    }
    let reports = {
        let paths = sources
            .live_paths
            .try_lock()
            .map_err(|_| "applicationBusy")?;
        if paths.len() > 20_000 {
            return Err("applicationTooLarge");
        }
        let mut rows = Vec::new();
        for p in paths.recent_iter(now, 1800) {
            if p.tx_call.len() > 64 || p.rx_call.len() > 64 {
                return Err("applicationTooLarge");
            }
            if tempo_core::message::same_call(&p.tx_call, &context.call) {
                rows.push(p.clone());
            }
        }
        rows
    };
    let getting_out = propagation::getting_out(&context.call, &context.grid, &reports, now);
    let xray = {
        let g = crate::LAST_XRAY.try_lock().map_err(|_| "applicationBusy")?;
        match g.as_ref() {
            Some((_, flux, at)) if *at > 0 && *at <= now + 5 && now.saturating_sub(*at) < 120 => {
                json!({"flux":flux,"asOf":at})
            }
            _ => Value::Null,
        }
    };
    let nav = &sources.navigation;
    let coverage = log_context(context, engine)?;
    let pca = {
        let g = nav.protons.try_lock().map_err(|_| "applicationBusy")?;
        if let Some((at, flux)) = g.as_ref().filter(|(at, _)| at.elapsed().as_secs() < 300) {
            let kp = prop["spaceWx"]["kp"]
                .as_f64()
                .ok_or("applicationUnavailable")?;
            let dto = crate::PcaView {
                j10: flux.j10,
                a30_day: propagation::pca::a30_day(flux.j5),
                a30_night: propagation::pca::a30_night(flux.j1),
                cutoff_deg: propagation::pca::cutoff_lat_deg(kp),
                points: propagation::pca::pca_layer(flux.j5, flux.j1, kp, now, 0.5),
            };
            json!({"value":value(&dto)?,"ageMs":at.elapsed().as_millis() as u64,"validForMs":300_000})
        } else {
            Value::Null
        }
    };

    Ok(
        json!({"mycall":context.call,"mygrid":context.grid,"prop":prop,"sourceAgeMs":source_age,
        "sourceValidForMs":crate::PROP_TTL_SECS*1000,"bandOutlook":prediction,"gettingOut":value(&getting_out)?,
        "scales":cached(&nav.scales,900_000)?,"muf":cached(&nav.muf,300_000)?,
        "aurora":cached(&nav.aurora,600_000)?,"pca":pca,"declination":propagation::wmm::declination_for_grid(&context.grid,now),
        "coverage":{"grids":coverage.grids,"zones":coverage.zones,"logCount":coverage.count},"xray":xray}),
    )
}
struct Elements {
    tles: Vec<propagation::sat::Tle>,
    fetched: i64,
    source: String,
    catalog: std::collections::HashMap<u32, propagation::live::tle::SatCatalogEntry>,
    aliases: std::collections::HashMap<String, u32>,
}
fn file_bytes(path: &std::path::Path) -> Result<Option<Vec<u8>>, &'static str> {
    use std::io::Read;
    let before = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("applicationUnavailable"),
    };
    if !before.is_file() || before.len() > DOCUMENT_BYTES as u64 {
        return Err("applicationTooLarge");
    }
    let file = std::fs::File::open(path).map_err(|_| "applicationUnavailable")?;
    let opened = file.metadata().map_err(|_| "applicationUnavailable")?;
    if !opened.is_file()
        || opened.len() != before.len()
        || opened.modified().ok() != before.modified().ok()
    {
        return Err("applicationUnavailable");
    }
    let mut bytes = Vec::new();
    file.take(DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "applicationUnavailable")?;
    if bytes.len() > DOCUMENT_BYTES {
        return Err("applicationTooLarge");
    }
    let after = std::fs::symlink_metadata(path).map_err(|_| "applicationUnavailable")?;
    if !after.is_file()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("applicationUnavailable");
    }
    Ok(Some(bytes))
}
fn elements_from(s: &crate::TleSnapshot) -> Result<Elements, &'static str> {
    if s.elements.len() + s.imported.len() > 1024
        || s.catalog.len() > 4096
        || s.aliases.len() > 4096
    {
        return Err("applicationTooLarge");
    }
    bounded(s)?;
    Ok(Elements {
        tles: crate::tle_merged_elements(s),
        fetched: s.fetched_at,
        source: s.source.clone(),
        catalog: s.catalog.iter().map(|c| (c.norad, c.clone())).collect(),
        aliases: s.aliases.clone(),
    })
}
fn elements() -> Result<Elements, &'static str> {
    {
        let g = crate::TLES.try_lock().map_err(|_| "applicationBusy")?;
        if let Some(s) = g.as_ref() {
            return elements_from(s);
        }
    }
    // Same native cache precedence and bundled floor. The reader never migrates,
    // overwrites or refreshes a file, and no input can name a different path.
    let mut snapshot = None;
    for path in [crate::shared_tles_path(), crate::legacy_tles_path()] {
        if let Some(bytes) = file_bytes(&path)? {
            let text = std::str::from_utf8(&bytes).map_err(|_| "applicationUnavailable")?;
            snapshot = crate::parse_tle_snapshot(text);
            if snapshot.is_none() {
                return Err("applicationUnavailable");
            }
            break;
        }
    }
    if let Some(path) = crate::tle_seed_path() {
        if let Some(bytes) = file_bytes(&path)? {
            let text = std::str::from_utf8(&bytes).map_err(|_| "applicationUnavailable")?;
            let mut seed = propagation::live::tle::parse_mirror_manifest(text)
                .map_err(|_| "applicationUnavailable")?;
            let now = crate::now_unix();
            seed.elements
                .retain(|t| propagation::live::tle::tle_bird_ok(t, now));
            snapshot = Some(crate::tle_seed_floor(snapshot, seed, now));
        }
    }
    elements_from(&snapshot.ok_or("applicationUnavailable")?)
}
fn satellite_catalog() -> Result<Option<crate::SatnogsSnapshot>, &'static str> {
    let g = crate::SATNOGS.try_lock().map_err(|_| "applicationBusy")?;
    if g.is_some() {
        bounded(&*g)?;
        return Ok(g.clone());
    }
    drop(g);
    file_bytes(&crate::satnogs_path())?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| "applicationUnavailable"))
        .transpose()
}
fn satellite_settings(e: &tempo_app::engine::Engine) -> Value {
    let s = e.settings();
    json!({"mygrid":s.mygrid,"rotatorConfigured":s.rotator_model>0 || !s.rotator_host.trim().is_empty(),
        "satDopplerOff":s.sat_doppler_off,"satVfoMap":s.sat_vfo_map,"radioPegged":s.radio_pegged})
}
pub(crate) fn live(e: &tempo_app::engine::Engine) -> Result<Value, &'static str> {
    // Called while the publisher holds only a non-blocking engine guard. Never
    // wait for the pass loop or serialize a whole Settings object.
    let g = crate::SAT_TRACK.try_lock().map_err(|_| "applicationBusy")?;
    if e.settings().mygrid.len() > 16
        || e.sat_transponder_held()
            .is_some_and(|(label, _)| label.len() > 4096)
        || e.sat_binding().is_some_and(|b| {
            b.radio_name.len() > 512
                || b.band.len() > 32
                || b.note.as_ref().is_some_and(|n| n.len() > 4096)
        })
    {
        return Err("applicationTooLarge");
    }
    let out = value(
        &json!({"capturedAtMs":super::super::now_ms(),"track":value(&*g)?,
        "held":value(&crate::satellite_held(e))?,"settings":satellite_settings(e)}),
    )?;
    if out.to_string().len() > 32 * 1024 {
        return Err("applicationTooLarge");
    }
    Ok(out)
}
fn satellites(context: &Context, _engine: &crate::SharedEngine) -> Result<Value, &'static str> {
    let el = elements()?;
    let view = tauri::async_runtime::block_on(crate::satellite_view_from_inputs(
        context.grid.clone(),
        el.tles,
        el.fetched,
        el.source,
        el.catalog,
    ))
    .map_err(|_| "applicationUnavailable")?;
    Ok(json!({"mygrid":context.grid,"view":value(&view)?}))
}
fn satellite(
    search: &str,
    context: &Context,
    engine: &crate::SharedEngine,
) -> Result<Value, &'static str> {
    let el = elements()?;
    let snap = satellite_catalog()?;
    let log = log_context(context, engine)?;
    let status: std::collections::HashMap<u32, String> = el
        .catalog
        .iter()
        .map(|(n, c)| (*n, c.status.clone()))
        .chain(
            snap.as_ref()
                .into_iter()
                .flat_map(|s| s.statuses.iter().map(|s| (s.norad, s.status.clone()))),
        )
        .collect();
    let schedule = if let Some(obs) = propagation::geo::maidenhead_to_latlon(&context.grid) {
        let names = vec![search.to_owned()];
        let mine: Vec<_> = crate::resolve_birds(&el.tles, &el.aliases, &names)
            .into_iter()
            .filter(|(_, t)| {
                propagation::sat::tle_age_days(&t.line1, crate::now_unix())
                    .is_some_and(|a| a <= 30.0)
            })
            .collect();
        let needs = propagation::SatNeeds {
            worked_sat_grids: log.needs.worked_grids_sat(),
            worked_entities: log.needs.worked_entity_names(),
        };
        crate::satellite_needs_passes(obs, &mine, 48, &needs, &status, crate::now_unix())
    } else {
        Vec::new()
    };
    let detail = tauri::async_runtime::block_on(crate::satellite_detail_from_inputs(
        context.grid.clone(),
        search.into(),
        el.tles,
        el.aliases,
        snap,
    ))
    .map_err(|_| "applicationUnavailable")?;
    Ok(
        json!({"mygrid":context.grid,"name":search,"detail":value(&detail)?,"schedule":value(&schedule)?,"logCount":log.count}),
    )
}

struct LogContext {
    needs: propagation::LogNeeds,
    grids: std::collections::BTreeSet<String>,
    zones: std::collections::BTreeSet<u8>,
    count: usize,
}
fn log_context(
    context: &Context,
    engine: &crate::SharedEngine,
) -> Result<LogContext, &'static str> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let count = {
        let e = engine.try_lock().map_err(|_| "applicationBusy")?;
        e.log_records().len()
    };
    if count > 1_000_000 {
        return Err("applicationTooLarge");
    }
    let mut out = LogContext {
        needs: propagation::LogNeeds::new(),
        grids: Default::default(),
        zones: Default::default(),
        count,
    };
    let mut text_bytes = 0usize;
    for offset in (0..count).step_by(128) {
        if Instant::now() >= deadline {
            return Err("applicationBusy");
        }
        let rows = {
            let e = engine.try_lock().map_err(|_| "applicationBusy")?;
            if !Arc::ptr_eq(&context.log, &e.log_read_token())
                || e.settings().mycall != context.call
                || e.settings().mygrid != context.grid
            {
                return Err("applicationBusy");
            }
            let mut rows = Vec::new();
            for q in &e.log_records()[offset..(offset + 128).min(count)] {
                for s in [&q.call, &q.band, &q.mode].into_iter().chain(
                    [q.grid.as_ref(), q.state.as_ref(), q.prop_mode.as_ref()]
                        .into_iter()
                        .flatten(),
                ) {
                    if s.len() > 1024 {
                        return Err("applicationTooLarge");
                    }
                    text_bytes = text_bytes.saturating_add(s.len());
                }
                if text_bytes > 64 * 1024 * 1024 {
                    return Err("applicationTooLarge");
                }
                rows.push((
                    q.call.clone(),
                    q.band.clone(),
                    q.mode.clone(),
                    q.grid.clone(),
                    q.state.clone(),
                    q.award_confirmed,
                    crate::qso_is_sat(q.prop_mode.as_deref()),
                ));
            }
            rows
        };
        for (call, band, mode, grid, state, confirmed, satellite) in rows {
            out.needs.add_qso(
                &call,
                &band,
                &mode,
                grid.as_deref(),
                state.as_deref(),
                confirmed,
                satellite,
            );
            if let Some(g) = grid
                .as_ref()
                .map(|s| s.trim().to_uppercase())
                .filter(|s| s.chars().count() >= 4)
            {
                out.grids.insert(g.chars().take(4).collect());
            }
            if let Some(info) = propagation::dxcc::resolve(&call) {
                if (1..=40).contains(&info.cq_zone) {
                    out.zones.insert(info.cq_zone);
                }
            }
            if out.grids.len() > 65_536 {
                return Err("applicationTooLarge");
            }
        }
    }
    if !context.same(&Context::read(engine)?) {
        return Err("applicationBusy");
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) fn test_fresh_catalog() -> crate::TleSnapshot {
    let now = crate::now_unix();
    let seed = propagation::live::tle::parse_mirror_manifest(include_str!(
        "../../../resources/tles/tles.json"
    ))
    .unwrap();
    let mut seed = crate::tle_seed_floor(None, seed, now);
    let epoch = format!(
        "{:02}{:03}.00000000",
        tempo_core::logbook::datetime_utc(now as u64).0 % 100,
        propagation::geo::day_of_year(now)
    );
    for tle in &mut seed.elements {
        tle.line1.replace_range(18..32, &epoch);
        let checksum = tle.line1[..68]
            .bytes()
            .map(|b| {
                if b.is_ascii_digit() {
                    u32::from(b - b'0')
                } else if b == b'-' {
                    1
                } else {
                    0
                }
            })
            .sum::<u32>()
            % 10;
        tle.line1.replace_range(68..69, &checksum.to_string());
    }
    seed
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (crate::SharedEngine, Sources) {
        let settings = tempo_app::settings::Settings {
            mycall: "W1AW".into(),
            mygrid: "FN31RX09".into(),
            prop_engine: "heuristic".into(),
            clublog_api_key: "never-share-this-test-marker".into(),
            ..Default::default()
        };
        let mut engine = tempo_app::engine::Engine::with_settings(settings);
        let adif:String=(0..2301).map(|i|{
            let (call,grid,sat)=if i==0 {("JA1ABC".into(),"PM95","<PROP_MODE:3>SAT")}else{(format!("K1T{i}"),"FN31","")};
            format!("<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<GRIDSQUARE:4>{grid}{sat}<QSO_DATE:8>20260910<TIME_ON:6>000000<EOR>\n",call.len())
        }).collect();
        engine.import_adif(&adif);
        assert_eq!(engine.log_records().len(), 2301);
        let mut prop = propagation::offline(crate::now_unix() - 1, "W1AW", "FN31RX09");
        prop.source = "live".into();
        let context = crate::PropContext {
            call: "W1AW".into(),
            grid: "FN31RX09".into(),
            log: engine.log_read_token(),
        };
        let sources = Sources {
            spots: Default::default(),
            live_paths: Default::default(),
            region_paths: crate::SharedRegionPaths(Default::default()),
            ota: Default::default(),
            health: Default::default(),
            propagation: Arc::new(Mutex::new(Some((Instant::now(), prop, context)))),
            memories: Default::default(),
            parks: Default::default(),
            sstv: Default::default(),
            navigation: Default::default(),
        };
        (Arc::new(Mutex::new(engine)), sources)
    }
    fn request(kind: Collection, search: &str) -> Request {
        Request {
            request_id: super::super::snapshot_id().unwrap(),
            collection: kind,
            cursor: None,
            search: search.into(),
            unconfirmed: false,
            after: None,
        }
    }
    fn emit(name: &str, raw: &Value) {
        if let Ok(directory) = std::env::var("NEXUS_REMOTE_NAV_FIXTURE_DIR") {
            let directory = std::path::Path::new(&directory);
            assert!(directory.is_absolute());
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(
                directory.join(format!("{name}.json")),
                serde_json::to_vec_pretty(raw).unwrap(),
            )
            .unwrap();
        }
    }
    #[test]
    fn connect_uses_the_actual_prediction_and_complete_log_without_mutating_the_station() {
        let (engine, sources) = fixture();
        let context = Context::read(&engine).unwrap();
        let before = engine.lock().unwrap().snapshot();
        let out = connect(Collection::Connect, "", &context, &engine, &sources).unwrap();
        assert_eq!(out["coverage"]["logCount"], 2301);
        assert_eq!(out["coverage"]["grids"], json!(["FN31", "PM95"]));
        assert!(out["coverage"]["zones"]
            .as_array()
            .unwrap()
            .contains(&json!(25)));
        assert_eq!(
            out["prop"],
            value(&sources.propagation.lock().unwrap().as_ref().unwrap().1).unwrap()
        );
        assert_eq!(
            out["gettingOut"],
            value(&propagation::getting_out(
                "W1AW",
                "FN31RX09",
                &[],
                crate::now_unix()
            ))
            .unwrap()
        );
        assert!(out["scales"].is_null());
        assert!(out["pca"].is_null());
        assert!(out["muf"].is_null());
        assert!(!out.to_string().contains("never-share"));
        let path = connect(Collection::Path, "PM95", &context, &engine, &sources).unwrap();
        assert_eq!(path["grid"], "PM95");
        assert!(!path["prediction"]["bands"].as_array().unwrap().is_empty());
        let after = engine.lock().unwrap().snapshot();
        assert_eq!(before.radio.tx_enabled, after.radio.tx_enabled);
        assert_eq!(before.radio.dial_mhz, after.radio.dial_mhz);
        emit("connect", &out);
        emit("path", &path);
        emit("satellite-live", &live(&engine.lock().unwrap()).unwrap());
    }
    #[test]
    fn a_shared_worker_returns_one_capture_and_rejects_its_pages_after_a_log_revision() {
        let (engine, sources) = fixture();
        let request = request(Collection::Connect, "");
        let deadline = Instant::now() + Duration::from_secs(5);
        let (rows, count, meta) = loop {
            match sources.navigation.read(&request, &engine, &sources) {
                Ok(v) => break v,
                Err("applicationBusy") if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                other => panic!("capture: {other:?}"),
            }
        };
        assert_eq!(count, rows.len());
        assert!(count > 0);
        assert!(sources.navigation.validate(&engine, &meta).is_ok());
        let (_, _, second) = sources
            .navigation
            .clone()
            .read(&request, &engine, &sources)
            .unwrap();
        assert_eq!(meta["contextId"], second["contextId"]);
        assert_eq!(meta["stationContextId"], second["stationContextId"]);
        let mut e = engine.lock().unwrap();
        e.import_adif(
            "<CALL:6>VK1ABC<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260910<TIME_ON:6>120000<EOR>",
        );
        drop(e);
        assert_eq!(
            sources.navigation.validate(&engine, &meta),
            Err("queryExpired")
        );
    }
    #[test]
    fn a_refresh_preserves_another_browsers_still_valid_capture() {
        let (engine, sources) = fixture();
        let request = request(Collection::Connect, "");
        let read = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match sources.navigation.read(&request, &engine, &sources) {
                    Ok(v) => break v,
                    Err("applicationBusy") if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    other => panic!("capture: {other:?}"),
                }
            }
        };
        let (_, _, first) = read();
        // Advance this capture past the reuse threshold, but before its cursor
        // deadline. Refreshing must not retire another browser's in-flight pages.
        sources
            .navigation
            .work
            .lock()
            .unwrap()
            .entries
            .back_mut()
            .unwrap()
            .at = Instant::now() - Duration::from_millis(REUSE_MS + 1);
        let (_, _, second) = read();
        assert_ne!(first["contextId"], second["contextId"]);
        assert_eq!(first["stationContextId"], second["stationContextId"]);
        sources.navigation.validate(&engine, &first).unwrap();
        sources.navigation.validate(&engine, &second).unwrap();
    }
    #[test]
    fn stale_mismatched_and_busy_sources_are_unavailable_instead_of_an_empty_board() {
        let (engine, sources) = fixture();
        let context = Context::read(&engine).unwrap();
        let lock = sources.propagation.lock().unwrap();
        assert_eq!(
            connect(Collection::Connect, "", &context, &engine, &sources),
            Err("applicationBusy")
        );
        drop(lock);
        sources
            .propagation
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .1
            .as_of = crate::now_unix() - 301;
        assert_eq!(
            connect(Collection::Connect, "", &context, &engine, &sources),
            Err("applicationUnavailable")
        );
        sources
            .propagation
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .1
            .as_of = crate::now_unix();
        sources.propagation.lock().unwrap().as_mut().unwrap().2.grid = "AA00".into();
        assert_eq!(
            connect(Collection::Connect, "", &context, &engine, &sources),
            Err("applicationUnavailable")
        );
    }
    #[test]
    fn document_chunking_preserves_unicode_and_refuses_an_oversized_whole() {
        let raw = json!({"text":"漢字📡\"\\".repeat(12_000)});
        let doc = encode(&raw, Collection::Connect, "", "id", "context", 1).unwrap();
        assert!(doc.rows.len() > 1);
        let joined: String = doc.rows.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(serde_json::from_str::<Value>(&joined).unwrap(), raw);
        assert_eq!(doc.meta["bytes"], joined.len());
        assert!(doc
            .rows
            .iter()
            .all(|v| v.as_str().unwrap().len() <= CHUNK_BYTES));
        assert!(encode(
            &json!({"text":"x".repeat(DOCUMENT_BYTES)}),
            Collection::Connect,
            "",
            "id",
            "context",
            1
        )
        .is_err());
    }
    #[test]
    fn a_below_surface_sgp4_position_is_unavailable_while_a_real_iss_position_survives() {
        let bad:propagation::sat::Tle=serde_json::from_str(r#"{"name":"HYPERVIEW-1G","line1":"1 61772U 24199AP  26230.11543498  .07890463  24672-5  93922-3 0  9997","line2":"2 61772  97.2456 114.2453 0006603 245.8477 114.2131 16.36574258144515"}"#).unwrap();
        let good:propagation::sat::Tle=serde_json::from_str(r#"{"name":"ISS (ZARYA)","line1":"1 25544U 98067A   26251.46617104  .00001902  00000+0  42648-4 0  9994","line2":"2 25544  51.6295 247.9233 0004929 112.8116 247.3394 15.49040812584669"}"#).unwrap();
        let at = 1_789_027_200;
        assert!(propagation::sat::subpoint(&bad, at).unwrap().2 < 0.0);
        assert!(crate::satellite_map_position(&bad, at).is_none());
        let point = crate::satellite_map_position(&good, at).unwrap();
        assert!(point.2 > 0.0 && point.3.is_finite() && point.3 > 0.0);
    }
    #[test]
    fn complete_bundled_satellite_catalog_fits_the_bounded_document() {
        let seed = test_fresh_catalog();
        let el = elements_from(&seed).unwrap();
        assert!(el.tles.len() > 300, "exercise the complete shipped catalog");
        let started = Instant::now();
        // A unique grid avoids reusing another test's one-bird pass cache.
        let view = tauri::async_runtime::block_on(crate::satellite_view_from_inputs(
            "EN52".into(),
            el.tles.clone(),
            el.fetched,
            el.source,
            el.catalog,
        ))
        .unwrap()
        .unwrap();
        assert!(
            view.birds.len() > 250,
            "the fixture must include a real complete fresh flock"
        );
        let document = encode(
            &json!({"mygrid":"EN52","view":view}),
            Collection::Satellites,
            "",
            "id",
            "context",
            1,
        )
        .unwrap();
        eprintln!(
            "Complete catalog: {} TLEs, {} document bytes, {} chunks, {:?}",
            el.tles.len(),
            document.meta["bytes"],
            document.rows.len(),
            started.elapsed()
        );
        emit("satellites-complete", &json!({"mygrid":"EN52","view":view}));
    }
    #[test]
    fn satellite_readers_use_real_sgp4_and_keep_held_transponder_and_tx_unchanged() {
        let (engine, _) = fixture();
        let context = Context::read(&engine).unwrap();
        let now = crate::now_unix();
        let seed: Value =
            serde_json::from_str(include_str!("../../../resources/tles/tles.json")).unwrap();
        let raw = seed["elements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "ISS (ZARYA)")
            .unwrap();
        let mut tle: propagation::sat::Tle = serde_json::from_value(raw.clone()).unwrap();
        let epoch = format!(
            "{:02}{:03}.00000000",
            tempo_core::logbook::datetime_utc(now as u64).0 % 100,
            propagation::geo::day_of_year(now)
        );
        tle.line1.replace_range(18..32, &epoch);
        let checksum = tle.line1[..68]
            .bytes()
            .map(|b| {
                if b.is_ascii_digit() {
                    u32::from(b - b'0')
                } else if b == b'-' {
                    1
                } else {
                    0
                }
            })
            .sum::<u32>()
            % 10;
        tle.line1.replace_range(68..69, &checksum.to_string());
        let elements = vec![tle.clone()];
        let aliases = Default::default();
        let catalog = std::collections::HashMap::from([(
            25544,
            propagation::live::tle::SatCatalogEntry {
                norad: 25544,
                name: tle.name.clone(),
                status: "alive".into(),
                amateur: true,
                decayed: false,
                src: None,
                classes: Some(vec!["fm".into()]),
            },
        )]);
        let view = tauri::async_runtime::block_on(crate::satellite_view_from_inputs(
            context.grid.clone(),
            elements.clone(),
            now,
            "mirror".into(),
            catalog,
        ))
        .unwrap()
        .unwrap();
        assert_eq!(view.birds.len(), 1);
        assert!(!view.passes.is_empty());
        let pos = propagation::sat::subpoint(&tle, now).unwrap();
        assert!((view.birds[0].lat - pos.0).abs() < 1.0);
        let detail = tauri::async_runtime::block_on(crate::satellite_detail_from_inputs(
            context.grid.clone(),
            tle.name.clone(),
            elements,
            aliases,
            None,
        ))
        .unwrap();
        let log = log_context(&context, &engine).unwrap();
        let sn = propagation::SatNeeds {
            worked_sat_grids: log.needs.worked_grids_sat(),
            worked_entities: log.needs.worked_entity_names(),
        };
        assert!(sn.worked_sat_grids.contains("PM95"));
        let mine = vec![(tle.name.clone(), &tle)];
        let obs = propagation::geo::maidenhead_to_latlon(&context.grid).unwrap();
        let passes = crate::satellite_needs_passes(obs, &mine, 48, &sn, &Default::default(), now);
        let expected: Vec<_> = propagation::sat::passes(&tle, obs, now - 21_600, 54)
            .into_iter()
            .filter(|p| p.los_unix > now)
            .collect();
        assert_eq!(passes.len(), expected.len());
        for (pass, pure) in passes.iter().zip(expected) {
            assert_eq!(pass.aos_unix, pure.aos_unix);
            assert_eq!(
                value(&pass.earn).unwrap(),
                value(&Some(propagation::pass_earn(
                    &tle,
                    pure.aos_unix,
                    pure.los_unix,
                    &sn
                )))
                .unwrap()
            );
        }
        emit("satellites", &json!({"mygrid":context.grid,"view":view}));
        emit(
            "satellite",
            &json!({"mygrid":context.grid,"name":tle.name,"detail":detail,"schedule":passes,"logCount":log.count}),
        );
        let e = engine.lock().unwrap();
        assert!(!e.snapshot().radio.tx_enabled);
        assert!(crate::satellite_held(&e).is_none());
    }
}
