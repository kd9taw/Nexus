//! The command link, this computer's end: the road to a paired station at protocol 2, for client
//! mode, which hands every station command its screens make to [`Link::call`] and answers their
//! polls from what the station pushes ([`Link::latest`]). The other end is the shack's command lane
//! (src-tauri's `remote_service::lan::commands`); the wire is [`crate::wire`]'s.
//!
//! 1. **The road, and the same build** (the operator's ruling of 2026-10-05, "the same version at
//!    both ends"). The window's road reaches the station ([`super::road::reach`]: TLS 1.3 with the
//!    station's key pinned, the hello, the welcome). The welcome names the station's app and
//!    contract, and anything but this build's is refused here, saying which side to update, before
//!    this computer opens anything; the station checks the `open` the same way.
//! 2. **Calls.** Each has an id that rises across every link of this run of Nexus ([`run_id`]), so
//!    the station's journal runs each at most once whichever link carried it. The dispatcher routes
//!    each first ([`crate::dispatch::route`]): one that runs on this computer, or is refused here,
//!    never leaves it. A Stop, or the off half of a transmit switch, goes ahead of everything
//!    waiting to be written, and a call bigger than a frame goes in pieces, so a Stop goes between
//!    them. A link that ends with calls in flight answers them [`Answer::Gone`]; the next one asks
//!    after them ([`Link::outcomes`]), and the station says what became of each.
//! 3. **Feeds** arrive numbered, and each value must carry the next number of its feed: a gap or a
//!    step back means this computer's copy is no longer the station's, and the link ends.
//! 4. **Events** that cross, for the screens ([`Link::events`]).
//! 5. **The lease, and Stop's first road**: protocol 1's operation requests ([`Link::operation`]),
//!    a `stopTransmit` ahead of everything.
//! 6. **Stop's second road** ([`Link::stop_road`]), when the welcome names one: a road of its own
//!    beside this link (`super::stop_road`), keyed from this link's TLS session. It is told when the
//!    link ends, and outlives it by a minute for a Stop pressed as it drops.
//! 7. **The audio road** ([`Link::audio_road`]), when the welcome names one and this computer has a
//!    headset: the station's audio and this computer's microphone (`super::audio_road`), keyed
//!    under its own label. It ends with the link. The station's status line says whether its rig is
//!    keyed, which mutes the station's audio here (M9), kept with the over's state ([`OnAir`]).
//! 8. **Internet roads** in their place ([`Link::connect_over`]), when this computer asks for them
//!    and the welcome named no road of the LAN's: each road its own socket and ICE (`crate::ice`),
//!    keyed under the internet roads' own labels. This computer's credentials and candidates go in
//!    its `open`, the station's come back in its `opened`, and each end's later candidates follow
//!    on the link as they are found, never on any relay lane.
//! 9. **The station's key for the internet road** ([`Link::station_key`]), when its `opened` names
//!    one: said on a link pinned to the station, so it can be checked against the key the Remote
//!    service lists for that station.
//!
//! Anything the station sends that no station of this build sends ends the link: it fails closed.
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use futures_util::stream::SplitStream;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tempo_stream::protocol;
use tokio::sync::{broadcast, oneshot, Notify};
use tokio_tungstenite::tungstenite::Message;

use super::audio_road::{AudioRoad, Headset};
use super::road::{self, Ids, Socket};
use super::stop_road::{OnAir, StopRoad};
use crate::datagram;
use crate::dispatch::{forwards, is_stop, route, Outcome, Profile, Refusal, Route};
use crate::ice::{Gather, IceRoad, Offer, Relay, Role};
use crate::outbox::Outbox;
use crate::wire::{self, Assembly, End, Inflate, Whole, OUTCOMES};
use crate::PairedStation;

/// The operation requests the link carries: the window's road's, and Take control, which only this
/// computer's own banner asks for.
const OPERATIONS: [&str; 6] = [
    "state",
    "acquire",
    "heartbeat",
    "release",
    "stopTransmit",
    "takeControl",
];
/// Events kept for a screen that has fallen behind.
const EVENTS: usize = 64;
/// How often this computer's newly found candidates go to the station while an internet road
/// gathers.
const TRICKLE_EVERY: Duration = Duration::from_millis(20);

/// What this computer's internet roads gather, each for itself (one socket per road): the address
/// their sockets bind (any port), whether they offer their direct addresses (left out to force the
/// relay), a STUN server, and a TURN server with the credential for it.
pub struct Internet {
    pub bind: std::net::IpAddr,
    pub direct: bool,
    pub stun: Option<std::net::SocketAddr>,
    pub relay: Option<Relay>,
}

impl Internet {
    fn gather(&self) -> Gather {
        Gather {
            direct: self.direct,
            stun: self.stun,
            relay: self.relay.clone(),
        }
    }
}

/// One internet road as the station named it in its `opened`: its tag, and its ICE offer.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InternetOffer {
    tag: String,
    ice: Offer,
}

impl InternetOffer {
    fn tag(&self) -> Option<datagram::Tag> {
        road::RoadOffer {
            port: 1,
            tag: self.tag.clone(),
        }
        .tag()
    }
}

/// This run of Nexus, as the station's journal knows it: made once, at random, never 0.
pub fn run_id() -> u64 {
    static RUN: OnceLock<u64> = OnceLock::new();
    *RUN.get_or_init(|| {
        use ring::rand::SecureRandom;
        let mut bytes = [0; 8];
        if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
            // No randomness: the clock, which no other run of this computer's shares either.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            bytes = (now.as_nanos() as u64).to_le_bytes();
        }
        u64::from_le_bytes(bytes).max(1)
    })
}

tokio::task_local! {
    /// Where [`hearing_build`] keeps the build a station welcomed this computer with.
    static HEARD: Arc<Mutex<Option<String>>>;
}

/// `reaching` (a link being opened, on any road), and the build the station last welcomed this
/// computer with while it ran, whether the link then opened or not: the station's version, which a
/// refusal for another build (`updateStation`, `updateComputer`, `otherBuild`) names beside this
/// computer's. `None` when no station welcomed it.
pub async fn hearing_build<F: std::future::Future>(reaching: F) -> (F::Output, Option<String>) {
    let heard = Arc::new(Mutex::new(None));
    let out = HEARD.scope(Arc::clone(&heard), reaching).await;
    let build = lock(&heard).take();
    (out, build)
}

/// The next call id of this run: rising across every link it opens.
fn next_id() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::SeqCst) + 1
}

/// The call the link writes: its id, the command and its arguments, how many Stops had been pressed
/// here when it was made (a Stop counting itself), and for a whole save the revision of the copy it
/// was made from.
pub fn call_message(id: u64, cmd: &str, args: Value, base: Option<&str>, stops: u64) -> Value {
    let mut message = json!({"type":"call","id":id,"cmd":cmd,"args":args,"stops":stops});
    if let Some(base) = base {
        message["base"] = json!(base);
    }
    message
}

/// What became of a call.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// It ran, and this is what it answered, with the revision of the store it read or saved.
    Value {
        value: Value,
        revision: Option<String>,
    },
    /// It ran, and failed with the command's own error.
    Error(String),
    /// It was refused, at the station or on this computer, and nothing ran.
    Refused(Refusal),
    /// Its id was used before, and this is what became of it then; it did not run again.
    Before(Outcome),
    /// Bigger than the station takes ([`wire::CALL_BYTES`]): nothing was sent.
    TooLarge,
    /// The link ended before its answer came: ask after it on the next link.
    Gone,
}

/// An event raised at the station, as it arrived.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub id: u64,
    pub at_ms: u64,
    pub data: Value,
}

/// One feed as this computer holds it: the number of the last value, its deflate stream, and the
/// value with its number.
#[derive(Default)]
struct Feed {
    last: u64,
    inflate: Inflate,
    latest: Option<(u64, Value)>,
}

/// A question after outcomes, waiting: the ids asked, and where the answer goes.
type Asked = (Vec<u64>, oneshot::Sender<Vec<Outcome>>);

/// What the link's tasks and its callers share.
struct Shared {
    calls: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
    operations: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    asked: Mutex<VecDeque<Asked>>,
    feeds: Mutex<HashMap<&'static str, Feed>>,
    watching: Mutex<Option<Value>>,
    events: broadcast::Sender<Event>,
    ended: AtomicBool,
    end: Notify,
    /// This link's Stop road, told when the link ends.
    road: Option<Arc<StopRoad>>,
    /// This link's audio road, told when the link ends.
    audio: Option<Arc<AudioRoad>>,
    /// Whether this computer's over may be on the air: the status line's rig keyed goes here.
    on_air: Arc<OnAir>,
    /// The internet roads' ICE, by the road's name on the link (`stop`, `audio`): what the
    /// station's candidates go to, and where this computer's come from.
    ice: Vec<(&'static str, Arc<IceRoad>)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A paired station's command link, open.
pub struct Link {
    outbox: Outbox,
    shared: Arc<Shared>,
    tasks: Vec<tokio::task::AbortHandle>,
    pub ids: Ids,
    /// The station's key for the internet road, as its `opened` named it on this link (SPKI,
    /// lowercase hex): `None` from a station with no pairing with the Remote service.
    pub station_key: Option<String>,
}

impl Link {
    /// The link to the station `record` names, at `typed` first when the operator gave an address,
    /// for the run `run` ([`run_id`] in the app), its audio on `headset` when this computer has one:
    /// the link and the address that answered, or the most telling reason it did not open (the
    /// road's, `updateStation`, `updateComputer`, `otherBuild`, or `notStation` for a station that
    /// said something no station says).
    pub async fn connect(
        record: &PairedStation,
        typed: Option<SocketAddrV4>,
        run: u64,
        headset: Option<Arc<dyn Headset>>,
    ) -> Result<(Self, SocketAddrV4), &'static str> {
        Self::connect_at(record, &road::order(record, typed), run, headset).await
    }

    /// [`Link::connect`], at each of `order` in turn.
    pub async fn connect_at(
        record: &PairedStation,
        order: &[SocketAddrV4],
        run: u64,
        headset: Option<Arc<dyn Headset>>,
    ) -> Result<(Self, SocketAddrV4), &'static str> {
        Self::connect_over(record, order, run, headset, None).await
    }

    /// [`Link::connect_at`], with internet roads in place of the LAN's when `internet` asks for
    /// them and the welcome named no road of the LAN's: each road's own socket and ICE, gathering
    /// as `internet` says, its offer in this computer's `open` and the station's in its `opened`.
    /// A station that opens none leaves Stop this link alone, as on the LAN without a road.
    pub async fn connect_over(
        record: &PairedStation,
        order: &[SocketAddrV4],
        run: u64,
        headset: Option<Arc<dyn Headset>>,
        internet: Option<&Internet>,
    ) -> Result<(Self, SocketAddrV4), &'static str> {
        let road::Reached {
            socket,
            ids,
            app,
            contract,
            stop_road,
            audio_road,
            at,
        } = road::reach(record, order).await?;
        let welcomed = road::Welcomed {
            session: ids.session.clone(),
            app,
            contract,
            stop_road,
            audio_road,
        };
        let link = Self::open(socket, ids, welcomed, Some(at), run, headset, internet).await?;
        Ok((link, at))
    }

    /// The link through this computer's end of a pipe through the station's room
    /// ([`crate::tunnel`]): the same TLS, hello, build check and `open` as on this network, with
    /// internet roads beside it as `internet` gathers them. A road of this network's the welcome
    /// names is never opened from here; without a road, Stop has this link alone.
    pub async fn connect_tunnel(
        pipe: tokio::io::DuplexStream,
        record: &PairedStation,
        run: u64,
        headset: Option<Arc<dyn Headset>>,
        internet: &Internet,
    ) -> Result<Self, &'static str> {
        let (socket, ids, welcomed) = road::reach_through(pipe, record).await?;
        Self::open(socket, ids, welcomed, None, run, headset, Some(internet)).await
    }

    /// The link on a road the station welcomed this computer on: this build's check, the `open`,
    /// and the roads beside it, this network's at `at` when it is this network's, or internet roads
    /// as `internet` gathers them.
    async fn open(
        mut socket: Socket,
        ids: Ids,
        welcomed: road::Welcomed,
        at: Option<SocketAddrV4>,
        run: u64,
        headset: Option<Arc<dyn Headset>>,
        internet: Option<&Internet>,
    ) -> Result<Self, &'static str> {
        let road::Welcomed {
            app,
            contract,
            stop_road,
            audio_road,
            ..
        } = welcomed;
        // Only a road on this network has an address for this network's roads.
        let (stop_road, audio_road) = match at {
            Some(_) => (stop_road, audio_road),
            None => (None, None),
        };
        let on_air = Arc::new(OnAir::default());
        let ours = (wire::APP, wire::contract());
        let _ = HEARD.try_with(|heard| *lock(heard) = Some(app.clone()));
        if let Some(reason) = wire::refusal(End::Computer, ours, (&app, &contract)) {
            let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
            return Err(reason);
        }
        let mut open = json!({"type":"open","profile":Profile::Desktop.name(),"app":ours.0,
            "contract":ours.1,"run":run});
        // The internet roads, bound and gathering before the `open` names their offers: a socket
        // of each road's own, the audio road's only with a headset to carry.
        let ice = match (internet, &stop_road) {
            (Some(internet), None) => {
                let bind = |tick| {
                    let at = std::net::SocketAddr::new(internet.bind, 0);
                    IceRoad::bind(at, Role::Computer, internet.gather(), tick).map(Arc::new)
                };
                let stop = bind(super::stop_road::TICK).ok_or("unreachable")?;
                let audio = match &headset {
                    Some(_) => {
                        let audio = bind(super::audio_road::TICK).ok_or("unreachable")?;
                        audio.write_timeout(Some(super::audio_road::SEND_WAIT));
                        Some(audio)
                    }
                    None => None,
                };
                let mut roads = json!({ "stop": stop.offer() });
                if let Some(audio) = &audio {
                    roads["audio"] = json!(audio.offer());
                }
                open["roads"] = roads;
                Some((stop, audio))
            }
            _ => None,
        };
        road::say(&mut socket, open.to_string())
            .await
            .map_err(|_| "unreachable")?;
        let said = tokio::time::timeout(road::HANDSHAKE, first_word(&mut socket, &on_air))
            .await
            .ok()
            .flatten();
        match said.as_ref().and_then(|said| said["type"].as_str()) {
            Some("opened") => {}
            Some("refused") => {
                return Err(
                    match said.as_ref().and_then(|said| said["reason"].as_str()) {
                        Some("updateStation") => "updateStation",
                        Some("updateComputer") => "updateComputer",
                        Some("otherBuild") => "otherBuild",
                        _ => "notStation",
                    },
                )
            }
            _ => return Err("notStation"),
        }
        // The station's key for the internet road, when it has one: a P-256 key, or no station's
        // answer.
        let station_key = match said.as_ref().and_then(|said| said.get("stationKey")) {
            None => None,
            Some(key) => Some(
                key.as_str()
                    .filter(|key| protocol::device_key(key))
                    .ok_or("notStation")?
                    .to_string(),
            ),
        };
        let export = |label: &[u8]| {
            socket
                .get_ref()
                .get_ref()
                .1
                .export_keying_material(
                    [0; datagram::EXPORTED],
                    label,
                    Some(ids.session.as_bytes()),
                )
                .ok()
        };
        // The internet roads the station opened, each on its own ICE: its offer taken, its keys
        // from this session under the internet roads' own labels.
        let offered = |name: &str| -> Option<InternetOffer> {
            let roads = said.as_ref()?.get("roads")?;
            serde_json::from_value(roads.get(name)?.clone()).ok()
        };
        let mut internet_ice = Vec::new();
        let (stop_road, audio_road) = match ice {
            Some((stop_ice, audio_ice)) => {
                let stop = offered("stop").and_then(|offer| {
                    let tag = offer.tag()?;
                    stop_ice.answer(&offer.ice).then_some(())?;
                    internet_ice.push(("stop", stop_ice.clone()));
                    StopRoad::internet(
                        stop_ice,
                        tag,
                        &export(datagram::INTERNET_STOP_LABEL)?,
                        on_air.clone(),
                    )
                });
                let audio = audio_ice
                    .zip(headset.clone())
                    .zip(offered("audio"))
                    .and_then(|((audio_ice, headset), offer)| {
                        let tag = offer.tag()?;
                        audio_ice.answer(&offer.ice).then_some(())?;
                        internet_ice.push(("audio", audio_ice.clone()));
                        AudioRoad::internet(
                            audio_ice,
                            tag,
                            &export(datagram::INTERNET_AUDIO_LABEL)?,
                            headset,
                            on_air.clone(),
                        )
                    });
                (stop, audio)
            }
            None => match at {
                Some(at) => (
                    Self::lan_stop_road(stop_road, at, &export, &on_air),
                    Self::lan_audio_road(audio_road, headset, at, &export, &on_air),
                ),
                None => (None, None),
            },
        };
        let (sink, stream) = socket.split();
        let (outbox, writer) = Outbox::spawn(sink);
        let shared = Arc::new(Shared::new(stop_road, audio_road, on_air, internet_ice));
        let writing = writer.abort_handle();
        let reader = tokio::spawn(read(stream, shared.clone()));
        let keeper = tokio::spawn(keep(outbox.clone(), writer, shared.clone()));
        let tasks = vec![writing, reader.abort_handle(), keeper.abort_handle()];
        Ok(Self {
            outbox,
            shared,
            tasks,
            ids,
            station_key,
        })
    }

    /// Stop's second road on the LAN, from this session's own keys: at the address that welcomed
    /// this computer, at the port and under the tag the welcome named. Without one, Stop has this
    /// link.
    fn lan_stop_road(
        offer: Option<road::RoadOffer>,
        at: SocketAddrV4,
        export: &dyn Fn(&[u8]) -> Option<[u8; datagram::EXPORTED]>,
        on_air: &Arc<OnAir>,
    ) -> Option<Arc<StopRoad>> {
        let offer = offer?;
        StopRoad::open(
            SocketAddrV4::new(*at.ip(), offer.port).into(),
            offer.tag()?,
            &export(datagram::STOP_LABEL)?,
            on_air.clone(),
        )
    }

    /// The audio road on the LAN the same way, under its own label, when this computer has a
    /// headset.
    fn lan_audio_road(
        offer: Option<road::RoadOffer>,
        headset: Option<Arc<dyn Headset>>,
        at: SocketAddrV4,
        export: &dyn Fn(&[u8]) -> Option<[u8; datagram::EXPORTED]>,
        on_air: &Arc<OnAir>,
    ) -> Option<Arc<AudioRoad>> {
        let (offer, headset) = offer.zip(headset)?;
        AudioRoad::open(
            SocketAddrV4::new(*at.ip(), offer.port).into(),
            offer.tag()?,
            &export(datagram::AUDIO_LABEL)?,
            headset,
            on_air.clone(),
        )
    }

    /// One station command, with the revision of the copy a whole save was made from and how many
    /// Stops had been pressed here when it was made (a Stop counting itself: the number its second
    /// road carries too): its id in this run, and what became of it. Routed first, deny by default:
    /// a command that runs on this computer, or is refused here, is answered here and never sent.
    /// The ui-state pair goes as given, already split ([`crate::dispatch::split_ui_state`]).
    pub async fn call(
        &self,
        cmd: &str,
        args: Value,
        base: Option<&str>,
        stops: u64,
    ) -> (u64, Answer) {
        let id = next_id();
        match route(cmd, &args) {
            Route::Shack | Route::Split => {}
            Route::Here => return (id, Answer::Refused(Refusal::RunsHere)),
            Route::Refused(refusal) => return (id, Answer::Refused(refusal)),
        }
        let urgent = is_stop(cmd, &args);
        let text = call_message(id, cmd, args, base, stops).to_string();
        if text.len() > wire::CALL_BYTES {
            return (id, Answer::TooLarge);
        }
        let (answer, answered) = oneshot::channel();
        lock(&self.shared.calls).insert(id, answer);
        let sent = if self.shared.ended.load(Ordering::SeqCst) {
            Err(())
        } else if urgent {
            self.outbox.urgent(Whole::Text(text))
        } else {
            self.outbox.send(Whole::Text(text))
        };
        if sent.is_err() {
            lock(&self.shared.calls).remove(&id);
            return (id, Answer::Gone);
        }
        (id, answered.await.unwrap_or(Answer::Gone))
    }

    /// What became of each of `ids` (calls of this run, from a link that ended), in the order
    /// asked; `None` once this link has ended too, or for more ids than one question may ask.
    pub async fn outcomes(&self, ids: &[u64]) -> Option<Vec<Outcome>> {
        if ids.is_empty() || ids.len() > OUTCOMES {
            return None;
        }
        let (answer, answered) = oneshot::channel();
        lock(&self.shared.asked).push_back((ids.to_vec(), answer));
        if self.shared.ended.load(Ordering::SeqCst) {
            return None;
        }
        let asked = json!({"type":"outcomes","ids":ids});
        self.outbox.text(asked.to_string()).ok()?;
        answered.await.ok()
    }

    /// One of protocol 1's operation requests (the lease's, and Stop): the station's whole
    /// `operationResponse`, or `None` for a request the link does not carry or once it has ended.
    /// A Stop goes ahead of everything waiting.
    pub async fn operation(&self, request: Value) -> Option<Value> {
        let kind = request["type"]
            .as_str()
            .filter(|kind| OPERATIONS.contains(kind))?;
        let urgent = kind == "stopTransmit";
        let request_id = request["requestId"]
            .as_str()
            .filter(|id| protocol::identifier(id))?
            .to_string();
        let (answer, answered) = oneshot::channel();
        lock(&self.shared.operations).insert(request_id.clone(), answer);
        let text = json!({"type":"operationRequest","request":request}).to_string();
        let sent = if self.shared.ended.load(Ordering::SeqCst) {
            Err(())
        } else if urgent {
            self.outbox.urgent(Whole::Text(text))
        } else {
            self.outbox.text(text)
        };
        if sent.is_err() {
            lock(&self.shared.operations).remove(&request_id);
            return None;
        }
        answered.await.ok()
    }

    /// Watch these feeds from now on, in place of the ones before. The station answers with the
    /// ones it has ([`Link::watching`]).
    pub fn watch(&self, feeds: &[&str]) -> bool {
        let watch = json!({"type":"watch","feeds":feeds});
        self.outbox.text(watch.to_string()).is_ok()
    }

    /// The station's last answer to a watch: the feeds it pushes, and the names it has no feed for.
    pub fn watching(&self) -> Option<Value> {
        lock(&self.shared.watching).clone()
    }

    /// The newest value of a feed, with its number.
    pub fn latest(&self, feed: &str) -> Option<(u64, Value)> {
        lock(&self.shared.feeds)
            .get(feed)
            .and_then(|feed| feed.latest.clone())
    }

    /// The station's events from now on.
    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.shared.events.subscribe()
    }

    /// This link's Stop road, when the station opened one.
    pub fn stop_road(&self) -> Option<Arc<StopRoad>> {
        self.shared.road.clone()
    }

    /// This link's audio road, when the station opened one and this computer has a headset.
    pub fn audio_road(&self) -> Option<Arc<AudioRoad>> {
        self.shared.audio.clone()
    }

    /// Whether this link's Stop road goes over the internet, and if it does, whether its ICE has
    /// selected a pair to send on: `None` for a link with no internet Stop road.
    pub fn internet_stop_road(&self) -> Option<bool> {
        self.shared
            .ice
            .iter()
            .find(|(name, _)| *name == "stop")
            .map(|(_, ice)| ice.route().is_some())
    }

    /// Each internet road's pair as its ICE selected it, by the road's name: for a test.
    #[cfg(any(test, feature = "test-util"))]
    pub fn internet_routes(&self) -> Vec<(&'static str, Option<crate::ice::Route>)> {
        self.shared
            .ice
            .iter()
            .map(|(name, ice)| (*name, ice.route()))
            .collect()
    }

    /// Whether the link has ended.
    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::SeqCst)
    }

    /// Waits until the link has ended.
    pub async fn closed(&self) {
        loop {
            let ending = self.shared.end.notified();
            if self.ended() {
                return;
            }
            ending.await;
        }
    }
}

impl Link {
    /// Closes the link: its socket goes with its tasks, and every call still waiting is answered
    /// gone.
    pub fn close(&self) {
        for task in &self.tasks {
            task.abort();
        }
        self.shared.end();
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.close();
    }
}

/// The station's first word after the `open`, the status line kept aside (`on_air`): `None` once
/// it has gone.
async fn first_word(socket: &mut Socket, on_air: &OnAir) -> Option<Value> {
    loop {
        match socket.next().await? {
            Ok(Message::Text(text)) => {
                let said: Value = serde_json::from_str(&text).ok()?;
                if said["type"] != "status" {
                    return Some(said);
                }
                on_air.rig(said["rigKeyed"].as_bool());
            }
            Ok(Message::Ping(_) | Message::Pong(_)) => {}
            _ => return None,
        }
    }
}

/// Everything the station sends, until the link ends.
async fn read(mut stream: SplitStream<Socket>, shared: Arc<Shared>) {
    let mut pieces = Assembly::new(wire::ANSWER_BYTES);
    loop {
        let whole = match stream.next().await {
            Some(Ok(Message::Text(text))) => Whole::Text(text.to_string()),
            Some(Ok(Message::Binary(frame))) if wire::is_piece(&frame) => {
                match pieces.piece(&frame) {
                    Ok(Some(whole)) => whole,
                    Ok(None) => continue,
                    Err(()) => break,
                }
            }
            Some(Ok(Message::Binary(frame))) => Whole::Binary(frame.to_vec()),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => break,
        };
        if shared.heard(whole).is_err() {
            break;
        }
    }
    shared.end();
}

/// The link's keeper: a ping every [`road::PING_EVERY`], so the station never takes this computer
/// for gone, and the link's end when its writing ends.
async fn keep(outbox: Outbox, mut writer: tokio::task::JoinHandle<()>, shared: Arc<Shared>) {
    let mut every = tokio::time::interval(road::PING_EVERY);
    every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    every.tick().await;
    let mut trickle = tokio::time::interval(TRICKLE_EVERY);
    trickle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = &mut writer => break,
            _ = every.tick() => if outbox.ping().is_err() { break },
            // Each internet road's candidates as it finds them, on the link.
            _ = trickle.tick(), if !shared.ice.is_empty() => {
                for (name, ice) in &shared.ice {
                    for line in ice.trickle() {
                        let said = json!({"type":"candidate","road":name,"candidate":line});
                        let _ = outbox.send(Whole::Text(said.to_string()));
                    }
                }
            }
        }
    }
    shared.end();
}

impl Shared {
    fn new(
        road: Option<Arc<StopRoad>>,
        audio: Option<Arc<AudioRoad>>,
        on_air: Arc<OnAir>,
        ice: Vec<(&'static str, Arc<IceRoad>)>,
    ) -> Self {
        Self {
            calls: Default::default(),
            operations: Default::default(),
            asked: Default::default(),
            feeds: Default::default(),
            watching: Default::default(),
            events: broadcast::channel(EVENTS).0,
            ended: AtomicBool::new(false),
            end: Notify::new(),
            road,
            audio,
            on_air,
            ice,
        }
    }

    /// The link has ended: every call still waiting is answered gone, and every other question
    /// is dropped, which its asker reads the same way. Its Stop road stays a minute more; its audio
    /// road goes now.
    fn end(&self) {
        self.ended.store(true, Ordering::SeqCst);
        if let Some(road) = &self.road {
            road.link_ended();
        }
        if let Some(audio) = &self.audio {
            audio.link_ended();
        }
        for (_, waiting) in lock(&self.calls).drain() {
            let _ = waiting.send(Answer::Gone);
        }
        lock(&self.operations).clear();
        lock(&self.asked).clear();
        self.end.notify_waiters();
    }

    /// One message from the station. `Err` for anything no station of this build sends.
    fn heard(&self, whole: Whole) -> Result<(), ()> {
        let text = match whole {
            Whole::Binary(frame) => return self.feed(&frame),
            Whole::Text(text) => text,
        };
        let message: Value = serde_json::from_str(&text).map_err(|_| ())?;
        match message["type"].as_str().ok_or(())? {
            "result" => {
                let id = message["id"].as_u64().ok_or(())?;
                let answer = answer_of(&message).ok_or(())?;
                // An answer nobody waits for any more is let go.
                let waiting = lock(&self.calls).remove(&id);
                if let Some(waiting) = waiting {
                    let _ = waiting.send(answer);
                }
            }
            "outcomes" => {
                let ids: Vec<u64> =
                    serde_json::from_value(message["ids"].clone()).map_err(|_| ())?;
                let outcomes = message["outcomes"]
                    .as_array()
                    .ok_or(())?
                    .iter()
                    .map(outcome_of)
                    .collect::<Option<Vec<_>>>()
                    .filter(|outcomes| outcomes.len() == ids.len())
                    .ok_or(())?;
                let mut asked = lock(&self.asked);
                if let Some(at) = asked.iter().position(|(asked, _)| *asked == ids) {
                    if let Some((_, waiting)) = asked.remove(at) {
                        let _ = waiting.send(outcomes);
                    }
                }
            }
            "operationResponse" => {
                let id = message["requestId"].as_str().ok_or(())?;
                let waiting = lock(&self.operations).remove(id);
                if let Some(waiting) = waiting {
                    let _ = waiting.send(message);
                }
            }
            "watching" => *lock(&self.watching) = Some(message),
            "event" => {
                let name = message["name"]
                    .as_str()
                    .filter(|name| forwards(name))
                    .ok_or(())?;
                let event = Event {
                    name: name.to_string(),
                    id: message["id"].as_u64().ok_or(())?,
                    at_ms: message["at"].as_u64().ok_or(())?,
                    data: message["data"].clone(),
                };
                let _ = self.events.send(event);
            }
            // The rig keyed mutes the station's audio here (M9); `null` when it cannot tell.
            "status" => self.on_air.rig(message["rigKeyed"].as_bool()),
            // One more of the station's candidates for an internet road this link opened. One its
            // ICE reads but will not probe (`crate::ice`'s rule) is dropped there; the link goes on.
            "candidate" => {
                let road = message["road"].as_str().ok_or(())?;
                let line = message["candidate"].as_str().ok_or(())?;
                let (_, ice) = self.ice.iter().find(|(name, _)| *name == road).ok_or(())?;
                if !ice.add_candidate(line) {
                    return Err(());
                }
            }
            _ => return Err(()),
        }
        Ok(())
    }

    /// A feed's value: kept, if it carries the next number of its feed, and the link ended
    /// otherwise, since a gap or a step back is a copy that is no longer the station's.
    fn feed(&self, frame: &[u8]) -> Result<(), ()> {
        let (name, number, deflated) = wire::read_feed(frame).ok_or(())?;
        let feed = wire::feed(name).ok_or(())?;
        let mut feeds = lock(&self.feeds);
        let held = feeds.entry(feed.name).or_default();
        if number != held.last + 1 {
            return Err(());
        }
        let json = held.inflate.value(deflated).ok_or(())?;
        let value: Value = serde_json::from_slice(&json).map_err(|_| ())?;
        held.last = number;
        held.latest = Some((number, value));
        Ok(())
    }
}

/// A call's answer as the station wrote it.
fn answer_of(message: &Value) -> Option<Answer> {
    if message.get("outcome").is_some() {
        return outcome_of(message).map(Answer::Before);
    }
    if let Some(value) = message.get("value") {
        let revision = message["revision"].as_str().map(str::to_string);
        return Some(Answer::Value {
            value: value.clone(),
            revision,
        });
    }
    if let Some(error) = message["error"].as_str() {
        return Some(Answer::Error(error.to_string()));
    }
    Refusal::named(message["refused"].as_str()?).map(Answer::Refused)
}

/// An outcome as the station wrote it.
fn outcome_of(message: &Value) -> Option<Outcome> {
    Some(match message["outcome"].as_str()? {
        "running" => Outcome::Running,
        "done" => Outcome::Done,
        "failed" => Outcome::Failed(message["error"].as_str()?.to_string()),
        "refused" => Outcome::Refused(Refusal::named(message["refused"].as_str()?)?),
        "lost" => Outcome::Lost,
        "unknown" => Outcome::Unknown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Deflate;
    use tokio_tungstenite::tungstenite::protocol::Role;
    use tokio_tungstenite::WebSocketStream;

    /// A link whose writer writes into an in-memory pipe that holds `room` bytes, and the station's
    /// end of the pipe: nothing more is written until the station reads.
    async fn piped(room: usize) -> (Arc<Link>, WebSocketStream<tokio::io::DuplexStream>) {
        let (ours, theirs) = tokio::io::duplex(room);
        let ours = WebSocketStream::from_raw_socket(ours, Role::Client, None).await;
        let theirs = WebSocketStream::from_raw_socket(theirs, Role::Server, None).await;
        let (sink, _stream) = ours.split();
        let (outbox, writer) = Outbox::spawn(sink);
        let link = Link {
            outbox,
            shared: Arc::new(Shared::new(None, None, Arc::default(), Vec::new())),
            tasks: vec![writer.abort_handle()],
            ids: Ids {
                station: String::new(),
                device: String::new(),
                session: String::new(),
            },
            station_key: None,
        };
        (Arc::new(link), theirs)
    }

    /// Lets every task handed something run as far as it can, and the writer stall on the full
    /// pipe.
    async fn stalled() {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    /// ★ Stop first, at this end: a Stop, the off half of a transmit switch and a `stopTransmit`,
    /// each asked for while calls are already waiting to be written, all go out ahead of every one
    /// of them, behind only the frame already being written; among themselves in any order, as
    /// each stops. CONTROL: the calls that waited go out after them, in the order they were made,
    /// the on half of the same switch among them in its turn.
    #[tokio::test]
    async fn a_stop_goes_out_ahead_of_the_calls_already_waiting() {
        let (link, mut station) = piped(64).await;
        let ask = |cmd: &'static str, args: Value| {
            let link = link.clone();
            tokio::spawn(async move { link.call(cmd, args, None, 0).await })
        };
        let _writing = ask("add_radio", json!({"name": "x".repeat(1000)}));
        stalled().await;
        let mut waiting: Vec<_> = (0..4).map(|n| ask("ask_log", json!({"n": n}))).collect();
        waiting.push(ask("set_beacon", json!({"on": true})));
        stalled().await;
        let _halt = ask("halt_tx", json!({}));
        let _off = ask("set_beacon", json!({"on": false}));
        let _stop = {
            let link = link.clone();
            let request =
                json!({"type":"stopTransmit","requestId":"10000000-0000-4000-8000-00000000000a"});
            tokio::spawn(async move { link.operation(request).await })
        };
        stalled().await;
        let mut heard = Vec::new();
        while heard.len() < 9 {
            let next = tokio::time::timeout(Duration::from_secs(5), station.next()).await;
            let Ok(Some(Ok(Message::Text(text)))) = next else {
                panic!("the station heard no more messages after {heard:?}")
            };
            let message: Value = serde_json::from_str(&text).unwrap();
            heard.push(match (message["type"].as_str(), message["cmd"].as_str()) {
                (Some("call"), Some("add_radio")) => format!(
                    "add_radio of {}",
                    message["args"]["name"].as_str().unwrap_or_default().len()
                ),
                (Some("call"), Some(cmd)) => format!("{cmd} {}", message["args"]),
                (Some("operationRequest"), _) => format!("{}", message["request"]["type"]),
                _ => panic!("{message}"),
            });
        }
        assert_eq!(heard[0], "add_radio of 1000", "{heard:?}");
        let mut stops = heard[1..4].to_vec();
        stops.sort();
        assert_eq!(
            stops,
            [
                "\"stopTransmit\"",
                "halt_tx {}",
                "set_beacon {\"on\":false}",
            ],
            "{heard:?}"
        );
        assert_eq!(
            heard[4..],
            [
                "ask_log {\"n\":0}",
                "ask_log {\"n\":1}",
                "ask_log {\"n\":2}",
                "ask_log {\"n\":3}",
                "set_beacon {\"on\":true}",
            ],
            "{heard:?}"
        );
    }

    /// The station's frames for values of `feed` numbered as given, from one deflate stream.
    fn frames(feed: &str, numbers: &[u64]) -> Vec<Vec<u8>> {
        let mut deflate = Deflate::default();
        numbers
            .iter()
            .map(|number| {
                let json = format!("{{\"n\":{number}}}");
                wire::feed_frame(feed, *number, &deflate.value(json.as_bytes()).unwrap())
            })
            .collect()
    }

    /// ★ The feeds' numbers, at this end: each value is kept only when it carries the next number
    /// of its feed, from 1. A gap, a repeat or a step back, a first value numbered past 1, a feed
    /// this build has not, and a value that does not inflate each end the link. CONTROL: 1, 2, 3
    /// are each kept, the newest with its number.
    #[test]
    fn a_feed_value_out_of_its_order_ends_the_link() {
        let shared = Shared::new(None, None, Arc::default(), Vec::new());
        for frame in frames("snapshot/1", &[1, 2, 3]) {
            assert_eq!(shared.heard(Whole::Binary(frame)), Ok(()));
        }
        assert_eq!(
            lock(&shared.feeds)["snapshot/1"].latest,
            Some((3, json!({"n": 3})))
        );
        for (numbers, why) in [
            (&[1, 3][..], "a gap"),
            (&[1, 2, 2][..], "a repeat"),
            (&[1, 2, 1][..], "a step back"),
            (&[2][..], "a first value past 1"),
        ] {
            let shared = Shared::new(None, None, Arc::default(), Vec::new());
            let mut heard: Vec<Result<(), ()>> = frames("meters/1", numbers)
                .into_iter()
                .map(|frame| shared.heard(Whole::Binary(frame)))
                .collect();
            assert_eq!(heard.pop(), Some(Err(())), "{why}");
            assert!(heard.iter().all(Result::is_ok), "{why}");
        }
        let shared = Shared::new(None, None, Arc::default(), Vec::new());
        let unknown = frames("noSuchFeed/1", &[1]).remove(0);
        assert_eq!(
            shared.heard(Whole::Binary(unknown)),
            Err(()),
            "no such feed"
        );
        let garbage = wire::feed_frame("snapshot/1", 1, &[0xff; 16]);
        assert_eq!(
            shared.heard(Whole::Binary(garbage)),
            Err(()),
            "no deflate stream"
        );
    }

    /// ★ Only what a station of this build says is taken: a result of no known shape, a refusal
    /// with an unknown code, an event the dispatcher does not forward and a message of no known
    /// type each end the link. CONTROL: an answer, a repeat's outcome, a forwarded event and the
    /// status line are taken.
    #[test]
    fn a_message_no_station_sends_ends_the_link() {
        let shared = Shared::new(None, None, Arc::default(), Vec::new());
        let said = |value: Value| shared.heard(Whole::Text(value.to_string()));
        assert_eq!(
            said(json!({"type":"result","id":1,"value":{"ok":true}})),
            Ok(())
        );
        assert_eq!(
            said(json!({"type":"result","id":2,"outcome":"done"})),
            Ok(())
        );
        assert_eq!(
            said(json!({"type":"result","id":3,"refused":"busy"})),
            Ok(())
        );
        assert_eq!(
            said(json!({"type":"event","name":"alert","id":1,"at":5,"data":{}})),
            Ok(())
        );
        assert_eq!(said(json!({"type":"status","rigKeyed":null})), Ok(()));
        for wrong in [
            json!({"type":"result","id":4}),
            json!({"type":"result","id":5,"refused":"noSuchRefusal"}),
            json!({"type":"result","id":6,"outcome":"maybe"}),
            json!({"type":"event","name":"update-progress","id":1,"at":5,"data":{}}),
            json!({"type":"event","name":"pounce","id":1,"at":5,"data":{}}),
            json!({"type":"somethingElse"}),
        ] {
            assert_eq!(said(wrong.clone()), Err(()), "{wrong}");
        }
    }

    /// ⛔ A candidate the station trickles for an internet road reaches that road's ICE only as the
    /// road's rule allows: one naming this computer's loopback, for a road on every interface, is
    /// read and dropped, the link is kept, and nothing ever reaches the address. CONTROL: a line no
    /// station sends ends the link, and a test's road on loopback, given the same message, probes
    /// the address.
    #[test]
    fn a_trickled_candidate_off_the_internet_is_dropped_and_the_link_kept() {
        use crate::ice::Counter;
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        let road = |at: &str| {
            let gather = Gather {
                direct: true,
                stun: None,
                relay: None,
            };
            let at = at.parse().unwrap();
            let road = IceRoad::bind(
                at,
                crate::ice::Role::Computer,
                gather,
                Duration::from_millis(5),
            );
            let road = Arc::new(road.unwrap());
            assert!(road.answer(&Offer {
                ufrag: "0011223344556677".into(),
                pwd: "00112233445566778899aabbccddeeff".into(),
                candidates: Vec::new(),
            }));
            road
        };
        let (internet, control) = (road("0.0.0.0:0"), road("127.0.0.1:0"));
        let localhost = std::net::Ipv4Addr::LOCALHOST.into();
        let (refused, probed) = (Counter::on(localhost), Counter::on(localhost));
        let candidate = |line: String| {
            Whole::Text(json!({"type":"candidate","road":"stop","candidate":line}).to_string())
        };
        let on_internet = Shared::new(None, None, Arc::default(), vec![("stop", internet.clone())]);
        let on_control = Shared::new(None, None, Arc::default(), vec![("stop", control.clone())]);
        assert_eq!(
            on_internet.heard(candidate(refused.line())),
            Ok(()),
            "the link is kept"
        );
        assert_eq!(on_control.heard(candidate(probed.line())), Ok(()));
        assert_eq!(
            on_internet.heard(candidate("candidate:no such line".into())),
            Err(()),
            "CONTROL: a line no station sends ends the link"
        );
        // Each road read by a thread of its own, as its road's thread reads it.
        let stopping = Arc::new(AtomicBool::new(false));
        let readers: Vec<_> = [internet, control]
            .into_iter()
            .map(|road| {
                let stopping = stopping.clone();
                std::thread::spawn(move || {
                    let mut buffer = [0u8; 2048];
                    while !stopping.load(SeqCst) {
                        let _ = road.recv(&mut buffer);
                    }
                })
            })
            .collect();
        let started = std::time::Instant::now();
        while probed.heard() == 0 && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(500));
        stopping.store(true, SeqCst);
        for reader in readers {
            reader.join().unwrap();
        }
        assert!(
            probed.heard() > 0,
            "CONTROL: the road on loopback probed nothing"
        );
        assert_eq!(refused.heard(), 0, "the loopback candidate was probed");
    }
}
