// Data layer.
//
// Typed functions over the shared DTO contract. Native calls use Tauri; an
// explicitly installed hosted session or the LAN TV entry supplies its own
// restricted transport. An absent transport throws; there is no mock/demo fallback.

import type { RemoteStationAction, RemoteStationStatus } from './remote-native/types'
import { pollSingleFlight } from './singleFlight'
import { t } from './i18n'
import { isStreamInput } from './remote-native/stream-input'
import { clientWords } from './features/clientWords'
import { noteCallbookAnswer } from './features/callbookAnswers'
import type { ActivationSpotReport } from './selfSpot'
import type { ImportDone, ImportPreview } from './features/logImport'
import type {
  AppSnapshot,
  AudioDevices,
  AwardSummary,
  DxccEntityOption,
  GeoLogStats,
  LogTimeline,
  TimelineRange,
  BandChannel,
  PrivilegeSpans,
  CatTestResult,
  CatProbeResult,
  CwDecodeResult,
  MeterReadout,
  SkimHit,
  PskState,
  KeyboardMacroProfile,
  RttyScope,
  RttyState,
  SstvState,
  ClubLogPushResult,
  Activation,
  LoggedActivation,
  LogExport,
  DetectedRig,
  FdEventBeacon,
  ClubFirewall,
  ClubFirewallPress,
  FdClubLog,
  FdMergeReport,
  OtaSpot,
  DiagnosticsReport,
  FeedHealth,
  ImportStats,
  JourneySummary,
  Js8ComposerPrefill,
  Js8Filter,
  Js8Freq,
  Js8State,
  JttyLogPromptAnswer,
  JttySent,
  JttyState,
  Js8Typeahead,
  Js8InboxState,
  Js8Switch,
  LoggedQso,
  LotwSyncResult,
  UploadReport,
  ModeRequest,
  NeedAlert,
  QrzLookup,
  PointedAt,
  CallBearing,
  RotatorState,
  QrzPushResult,
  RouteMode,
  RadioSwitchPolicy,
  ExperienceLevel,
  RoutingRule,
  Settings,
  SourceKind,
  Spectrum,
  SpectrumFrameWire,
  Tier,
  VoiceMessage,
  OtaMapSpot,
  KpForecast,
  DailySolarIndices,
} from './types'
import type { PropagationSnapshot, PathPrediction, GettingOut, AuroraPoint } from './types'
import type { MufStation, NoaaScalesView, AlertView } from './types'
import type { RepeaterSearchResult, GeoCandidate, RadioProgFileNotice, RadioProgProject, ProgChannel } from './types'
import type { TxHold } from './types'
import type { SliceIntent } from './types'
import type { FieldDayQso } from './types'
import type { AnswerTo, LogQuestion } from './features/logAnswers'
import type { LogColumnsSaved } from './features/logColumns'
import type { SavedLogFilter } from './features/logFilter'
import type { LogQuery } from './features/logQuery'
import type { StreetPack } from './features/streetPack'
import type {
  StreetArea,
  StreetInfo,
  StreetInstalled,
  StreetProgress,
  StreetSize,
  StreetUnfinished,
  StreetUpdate,
} from './features/streetMaps'
import type { WatchFilter } from './watchlist'
import { finishLogStats, type LogStatCounts } from './features/logStats'

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>

declare global {
  interface Window {
    __TAURI__?: {
      /** `Channel` streams a command's progress back (the street map's download). */
      core?: { invoke?: InvokeFn; Channel?: new <T>() => { onmessage: (message: T) => void } }
      invoke?: InvokeFn
      /** Tauri's event bridge (exposed by `withGlobalTauri`). Used by usePounce for the app's
       * one PUSH channel — everything else here polls — and by `useContestScreenWindow`, which
       * hears one window's close (`target` names the window). */
      event?: {
        listen?: <T>(
          event: string,
          handler: (e: { payload: T }) => void,
          options?: { target?: string | { kind: string; label?: string } },
        ) => Promise<() => void>
      }
    }
    /**
     * The low-level IPC bridge. Tauri v2 injects this into EVERY app webview,
     * independent of the `withGlobalTauri` config — so its presence guarantees the
     * real backend is reachable.
     */
    __TAURI_INTERNALS__?: { invoke?: InvokeFn }
  }
}

/** Resolve the Tauri IPC bridge, or THROW. There is no demo fallback: a missing
 *  bridge is a hard error, never silently-fabricated data. */
function bridge(): InvokeFn {
  const internals = window.__TAURI_INTERNALS__
  if (internals?.invoke) return internals.invoke
  const t = window.__TAURI__
  if (t?.core?.invoke) return t.core.invoke.bind(t.core)
  if (t?.invoke) return t.invoke.bind(t)
  throw new Error(
    'Nexus: the Tauri IPC bridge is unavailable — the app must run inside the desktop shell.',
  )
}

/** True when the backend bridge is present (always true inside the installed app). */
export function isTauri(): boolean {
  try {
    bridge()
    return true
  } catch {
    return false
  }
}

/**
 * Fired after any command that could change WHICH credentials are stored.
 *
 * ⚠️ THIS EXISTS SO SETTINGS DOES NOT POLL THE OS KEYCHAIN (#154). Reading whether a password
 * is saved means opening a Secret Service session per connector, and Settings was doing that
 * every 5 seconds — which on Fedora 44 crashed `gnome-keyring-daemon` in a loop
 * (`service_method_open_session` → SIGABRT, restart, repeat) for as long as the app was open.
 * The answer only changes when the operator saves or clears one, so it is an EVENT, not a poll.
 *
 * Raised centrally rather than at each of the ten call sites: a missed one would leave the
 * badge stale, and the whole point is that nothing re-reads the keychain on a timer to correct
 * it.
 */
export const CREDENTIALS_CHANGED = 'nexus-credentials-changed'

/** Commands that add or remove a stored secret. Matched by SHAPE, so a new connector's
 *  `set_x_password` / `clear_x` is covered the day it is added and not the day someone
 *  remembers to extend a list. */
function mutatesCredentials(cmd: string): boolean {
  return /^set_[a-z0-9_]+_(password|key|token|code)$/.test(cmd) || /^clear_[a-z0-9_]+$/.test(cmd)
}

/** Commands that save or forget one stored password, key, token or code: each at the station's
 *  own screen only, never through a stream of it ([`invoke`]). Matched by SHAPE, as
 *  `mutatesCredentials` is, so a new connector's is refused the day it is added. */
function storesACredential(cmd: string): boolean {
  return /^(set|clear)_[a-z0-9_]+_(password|key|token|code)$/.test(cmd)
}

/** The TV entry sets this to its RPC base ('/connect/rpc') before anything invokes.
 *  ⚠️ Deliberately NOT consulted by `bridge()`/`isTauri()`: isTauri() answers "is the
 *  desktop shell here", and desktop-only behaviour (DPI seeding, the external-link
 *  interceptor) must stay off in a browser even when the RPC is reachable. */
declare global {
  interface Window {
    __NEXUS_TV_RPC__?: string
  }
}

/** Invoke over HTTP against the read-only LAN RPC — the TV page's transport. Same
 *  command names and args as the desktop bridge; the server answers only the
 *  allowlisted read-only set and 404s the rest, which surfaces here as a rejection
 *  the callers' existing catch paths treat as "feed unavailable" — the same honest
 *  degradation they already do offline. GET only: the server accepts nothing else. */
async function httpInvoke<T>(base: string, cmd: string, args?: Record<string, unknown>): Promise<T> {
  const qs = args === undefined ? '' : `?args=${encodeURIComponent(JSON.stringify(args))}`
  const r = await fetch(`${base}/${cmd}${qs}`, { cache: 'no-store' })
  if (!r.ok) throw new Error(`${cmd}: ${r.status} ${await r.text().catch(() => '')}`)
  return (await r.json()) as T
}

/** The desktop bridge's rejection, with a client-mode refusal put in words; any other error as
 *  it came. */
function worded(error: unknown): never {
  const code = typeof error === 'string' ? /^nexusClient:([A-Za-z]+)$/.exec(error)?.[1] : undefined
  throw code === undefined ? error : clientWords(code)
}

/** Invoke a backend command. The desktop IPC bridge when present; the TV page's LAN
 *  RPC when its entry declared one; otherwise a hard error — never fabricated data. */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  // Only at the station itself: a credential saved or forgotten through a stream is refused, in
  // words its caller shows. Read before anything awaits, while the stream's dispatch is running.
  if (storesACredential(cmd) && isStreamInput()) throw t('remote.lan.atShackOnly')
  const tv = typeof window !== 'undefined' ? window.__NEXUS_TV_RPC__ : undefined
  const out = isTauri() ? ((await bridge()(cmd, args).catch(worded)) as T)
    : tv ? await httpInvoke<T>(tv, cmd, args)
    : ((await bridge()(cmd, args)) as T) // throws with the bridge's own message
  if (mutatesCredentials(cmd) && typeof window !== 'undefined') {
    window.dispatchEvent(new Event(CREDENTIALS_CHANGED))
  }
  return out
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/** The rolling connectivity log (newest first) — Settings ▸ Connections. */
export async function getConnectionLog(): Promise<import('./types').ConnEvent[]> {
  return invoke('get_connection_log')
}

/** Per-connector status: whether a credential is stored, AND what happened the last time
 *  Nexus actually talked to the service (never the secrets). */
export async function getCredentialsStatus(): Promise<import('./types').CredStatus[]> {
  return invoke('get_credentials_status')
}

export function getRemoteStationStatus(): Promise<RemoteStationStatus> { return invoke('get_remote_station_status') }
/** Whether a stream watches this station's screen now; `remote-stream-watched` tells each change. */
export function getStreamWatched(): Promise<boolean> { return invoke('get_stream_watched') }
/** A press on the shack's Remote settings, stamped with where it came from: the station takes nothing but a
 *  read through a stream. Read here, before anything awaits, while the stream's dispatch is the one running;
 *  every press reaches the station through this one function. */
export function remoteStationAction(action: RemoteStationAction): Promise<RemoteStationStatus> {
  return invoke('remote_station_action', { action, origin: isStreamInput() ? 'stream' : 'shack' })
}

export async function getSnapshot(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('get_snapshot')
}

/** The propagation & opening-intelligence nowcast (adaptive bands, openings,
 *  DXpedition cards, space weather). */
export async function getPropagation(): Promise<PropagationSnapshot> {
  return invoke<PropagationSnapshot>('get_propagation')
}

/** Historical opening episodes (newest first) — the Connect openings-log pane. */
export async function getOpeningsLog(): Promise<import('./types').OpeningEpisode[]> {
  return invoke('get_openings_log')
}

/** Per-path HF outlook to a station's grid (the PathPredictor seam). */
export async function getPathOutlook(grid: string): Promise<PathPrediction> {
  return invoke<PathPrediction>('get_path_outlook', { grid })
}

/** The no-selection "Band outlook (modelled)": modeled per-band workability + MUF to
 *  a ring of representative long-haul DX directions. Needs only the operator's grid. */
export async function getBandOutlook(): Promise<PathPrediction> {
  return invoke<PathPrediction>('get_band_outlook')
}

/** "Am I getting out?" — who is hearing the operator now (observed). */
export async function getGettingOut(): Promise<GettingOut> {
  return invoke<GettingOut>('get_getting_out')
}

/** The current OVATION aurora oval for the map overlay. */
export async function getAurora(): Promise<AuroraPoint[]> {
  return invoke<AuroraPoint[]>('get_aurora')
}

export async function getKc2gMuf(): Promise<MufStation[]> {
  return invoke<MufStation[]>('get_kc2g_muf')
}

/** Polar-cap absorption (GOES protons → D-RAP2). Null = no proton data yet. */
export async function getPca(): Promise<import('./types').PcaView | null> {
  return invoke<import('./types').PcaView | null>('get_pca')
}

/** Magnetic declination (° east-positive) at the QTH (WMM2025); null = no grid. */
export async function getDeclination(): Promise<number | null> {
  return invoke<number | null>('get_declination')
}

/** Amateur satellites: subpoints now + next-24h passes. Null = no elements. */
export async function getSatellites(): Promise<import('./types').SatView | null> {
  return invoke<import('./types').SatView | null>('get_satellites')
}

/** Passes for the ★ favorites over the next `hours` (Satellites section schedule).
 * Empty when the grid is unset or no named bird has elements. */
export async function getSatSchedule(names: string[], hours: number): Promise<import('./types').SatPass[]> {
  return invoke<import('./types').SatPass[]>('get_sat_schedule', { names, hours })
}

/** The needs-aware pass ranking: getSatSchedule's rows (SatNOGS status
 * included) with each pass's `earn` summary stamped (needed grids/entities
 * reachable through the footprint + `score` sort key). AOS-sorted like the
 * schedule; computed on demand in a backend blocking task — the Satellites
 * section fetches it on its slow (5 min) schedule poll, which is the intended
 * cadence: keep it off fast per-second loops. */
export async function getSatPassNeeds(names: string[], hours: number): Promise<import('./types').SatPass[]> {
  return invoke<import('./types').SatPass[]>('get_sat_pass_needs', { names, hours })
}

/** MUTUAL-VISIBILITY WINDOWS with another station — when a ★ bird is above the
 * floor for BOTH of you at once, over the next `days`. `peer` is their grid
 * square (4/6/8 char) or a callsign, which the backend resolves from the most
 * recent logged QSO carrying a grid. Rejects (with a sentence the operator can
 * act on) when neither grid resolves or no elements are loaded; an empty
 * `windows` is a RESULT, not a failure — see SatSked. On demand only: this is a
 * two-observer scan over the whole horizon, never a poll. */
export async function getSatSked(
  names: string[],
  peer: string,
  days: number,
): Promise<import('./types').SatSked> {
  return invoke<import('./types').SatSked>('get_sat_sked', { names, peer, days })
}

/** The ISS's current-or-next pass over the QTH (keyed on NORAD 25544), or null
 * when the grid is unset, no ISS elements are loaded, or no pass falls in the
 * next ~3 h. Drives the SSTV auto-arm opt-in. */
export async function getIssPass(): Promise<import('./types').SatPass | null> {
  return invoke<import('./types').SatPass | null>('get_iss_pass')
}

/** Per-bird detail: SatNOGS status/frequencies (cached weekly; absent offline)
 * + the current/next pass with its polar-plot track. */
export async function getSatDetail(name: string): Promise<import('./types').SatDetail> {
  return invoke<import('./types').SatDetail>('get_sat_detail', { name })
}

/** Arm rotor auto-track for a bird's pass — `aosUnix` picks WHICH schedule row
 * (±3 min tolerance); omitted = current/next. Returns the initial status, or
 * null when there's no rotor/grid/matching pass. */
export async function startSatTrack(
  name: string,
  aosUnix?: number,
): Promise<import('./types').SatTrackStatus | null> {
  return invoke<import('./types').SatTrackStatus | null>('start_sat_track', { name, aosUnix })
}

export async function stopSatTrack(): Promise<void> {
  return invoke('stop_sat_track')
}

/** The live auto-track state (poll while the section is open); null = idle. */
export async function getSatTrackStatus(): Promise<import('./types').SatTrackStatus | null> {
  return invoke<import('./types').SatTrackStatus | null>('sat_track_status')
}

/** Put the radio under Doppler control for one transponder on `name`; `null`
 * clears the selection and hands the dial back to the operator.
 *
 * `index` is a raw index into the list `getSatDetail` returned — every
 * transmitter for that bird, dead ones included. Pass the row index as-is.
 *
 * ⚠️ It did NOT always mean that: the backend used to drop `!alive` entries
 * before indexing, so on any bird listing a dead transmitter first the UI's
 * row index selected a different transponder — a different uplink, silently.
 * Two layers agreeing on a hidden filter is not a contract; the backend now
 * indexes the list the caller was actually shown and REFUSES a dead pick by
 * name instead of shifting everything after it. Do not re-introduce a filter
 * on either side.
 *
 * `auto` says WHO picked: false (the default) = the operator clicked this row,
 * true = the "Work this pass" chain chose it. The backend needs the difference
 * because that chain re-runs, and re-running it mid-pass used to move the
 * operator onto another of the bird's documented pairings — an auto pick is
 * refused while a pass is engaged on a different row, an operator's is not. */
export async function setSatTransponder(
  name: string,
  index: number | null,
  auto = false,
): Promise<void> {
  return invoke('set_sat_transponder', { name, index, auto })
}

/** The transponder the ENGINE holds right now (bird + the raw index into the
 * getSatDetail list), or null when the dial is the operator's. Poll this for
 * read-back rather than trusting the last local click: the hold is released
 * backend-side at LOS and on a live-track stop, and a stale mirror keeps a
 * green Transponder gate — and skips the next "Work this pass" re-pick — for
 * a hold the engine no longer has. */
export async function getSatTransponder(): Promise<import('./types').SatTransponderHeld | null> {
  return invoke<import('./types').SatTransponderHeld | null>('get_sat_transponder')
}

export interface LotwUsersStatus {
  count: number
  fetchedAt: number
}

/** How many LoTW-user calls are loaded + when the ARRL list was last fetched. */
export async function getLotwUsersStatus(): Promise<LotwUsersStatus> {
  return invoke<LotwUsersStatus>('get_lotw_users_status')
}

/** Fetch/refresh ARRL's LoTW user-activity list (manual, WSJT-X-style; the
 * file changes weekly and an unchanged file costs a 304, not 6 MB). */
export async function fetchLotwUsers(): Promise<LotwUsersStatus> {
  return invoke<LotwUsersStatus>('fetch_lotw_users')
}

/** The IOTA island-group list on the station: the operator's answer (`null`: never asked), how many
 *  groups are held, when it was downloaded (Unix seconds, 0: never), and why the last download was
 *  not kept, if it was not. The list is IOTA Ltd's; Nexus downloads it only after a yes. */
export interface IotaListStatus {
  consent: boolean | null
  count: number
  fetchedAt: number
  lastError: string | null
}

export async function getIotaListStatus(): Promise<IotaListStatus> {
  return invoke<IotaListStatus>('get_iota_list_status')
}

/** The operator's answer to "download the IOTA island-group list?". Yes downloads it now (when one
 *  is due); no deletes the copy held. */
export async function setIotaListConsent(consent: boolean): Promise<IotaListStatus> {
  return invoke<IotaListStatus>('set_iota_list_consent', { consent })
}

/** One island group of the list: its reference and IOTA Ltd's name for it. */
export interface IotaGroup {
  reference: string
  name: string
}

/** Every island group the station holds; empty without the list. */
export async function getIotaGroups(): Promise<IotaGroup[]> {
  return invoke<IotaGroup[]>('get_iota_groups')
}

/** An IOTA reference the station OFFERS a contact being logged (`tempo_core::logbook::offer::IotaOffer`),
 *  and where it came from: the spot comments for the call, or a callbook lookup made this session. */
export interface IotaOfferData {
  reference: string
  source: 'spot' | 'callbook'
}

/** What IOTA reference the station offers the contact a form holds for `call`; null for none, and
 *  none while the form holds a reference of its own (`held`). Asking writes nothing. */
export async function getIotaOffer(call: string, held?: string | null): Promise<IotaOfferData | null> {
  return invoke<IotaOfferData | null>('get_iota_offer', { call, held: held ?? null })
}

export interface FccStatesStatus {
  count: number
  fetchedAt: number
  generated: string
}

/** How many FCC callsign→state entries are loaded + when the index was fetched.
 * Drives the WAS "New State" hint for grid-less cluster/CW/SSB spots. */
export async function getFccStatesStatus(): Promise<FccStatesStatus> {
  return invoke<FccStatesStatus>('get_fcc_states_status')
}

/** Download/refresh the FCC callsign→state index (hosted on hamradiotools.io,
 * regenerated weekly from the FCC ULS file). Auto-refreshes on startup when stale;
 * this is the manual button. Resolves with the new status (unchanged if already current). */
export async function fetchFccStates(): Promise<FccStatesStatus> {
  return invoke<FccStatesStatus>('fetch_fcc_states')
}

/** AD1C cty.dat country-file currency — Settings "Country file (DXCC)" fieldset.
 * The resolver is set once at launch, so a downloaded file applies at the NEXT
 * launch: `activeVer` is what is resolving now, `installedVer` what is staged
 * on disk (both AD1C `yyyymmdd` release dates; `installedVer` empty until a
 * download happens). */
export interface CtyStatus {
  /** Entity count of the ACTIVE file. */
  count: number
  fetchedAt: number
  generated: string
  activeVer: string
  installedVer: string
}

export async function getCtyStatus(): Promise<CtyStatus> {
  return invoke<CtyStatus>('get_cty_status')
}

/** Download/refresh the country file if AD1C published a newer release
 * (compared on the content-derived `=VER` date, so an unchanged week is one
 * small manifest fetch). The downloaded file applies at the next launch —
 * never mid-session. */
export async function fetchCty(): Promise<CtyStatus> {
  return invoke<CtyStatus>('fetch_cty')
}

/** Field Day rules-data currency — the "check for rules updates" row in
 * Settings ▸ Contesting ▸ Contest. Same set-once discipline as the
 * country file: `activeGenerated` is the data scoring THIS session (the
 * bundled seed, or a previously downloaded file), `installedGenerated` what is
 * staged on disk (empty until a download happens; newer than active ⇒
 * "applies at next launch"). */
export interface FdRulesStatus {
  /** Newest rules year in the ACTIVE table. */
  rulesYear: number
  activeGenerated: string
  installedGenerated: string
  fetchedAt: number
}

export async function getFdRulesStatus(): Promise<FdRulesStatus> {
  return invoke<FdRulesStatus>('get_fd_rules_status')
}

/** Download the hosted fd-rules.json if its `generated` stamp differs from
 * what we hold (validated with the app loader's own checks before it ever
 * touches disk). Applies at the next launch — never mid-session. */
export async function fetchFdRules(): Promise<FdRulesStatus> {
  return invoke<FdRulesStatus>('fetch_fd_rules')
}

/** The ACTIVE Field Day event's ruleset FACTS for the warn-only advisories
 * (banned-mode chip, assistance advisory). Facts only — the advisory text lives
 * in the catalogs, computed UI-side. `enforcement` ships `'warn'`: nothing is
 * ever removed or disabled by rule (operator ruling). */
export interface FdRulesetDto {
  /** The contest allows spotting only over amateur RF while it runs (Winter Field Day 2027), so
   *  Nexus posts no spot over the internet then. Absent when false. */
  spotsRfOnly?: boolean
  /**
   * The rules-file event id — 'arrlfd' | 'wfd' | 'tnqp' | 'ohqp' | 'cqp' | 'txqp' |
   * 'arrlss_cw' | 'arrlss_ssb' | 'arrlvhf_jan' | 'arrlvhf_jun' | 'arrlvhf_sep' |
   * 'cqww_cw' | 'cqww_ssb' | 'cqww_rtty' | 'cqwpx_cw' | 'cqwpx_ssb' | 'arrlrtty'.
   */
  event: string
  rulesYear: number
  /** On-air modes this event's rules ban outright (uppercase ADIF-style). */
  bannedModes: string[]
  spottingAllowed: boolean
  clusterAllowed: boolean
  enforcement: string
  /** ⭐ The role the SAVED settings put this operator in — `''` for a symmetric contest
   *  (both Field Day events). Derived in Rust by the same constructor mode entry uses;
   *  never re-derived here, so the preview cannot promise a role the session won't give. */
  role?: string
  /** ⭐ The exchange that role would SEND, in send order. The role id names a rule; this
   *  is what goes on the air. */
  exchange?: string[]
  /** Why no session could be built from the saved settings — the sentence mode entry
   *  would refuse with. `''` when the configuration is good. */
  problem?: string
  /** The W/VE warning (never a refusal) these settings would start the contest with. */
  locationWarning?: import('./types').ContestLocationWarning
  /** The sponsor's own entry classes (Cabrillo `ENTRY-CLASS`), in its order: what Settings
   *  offers for `contestEntryClass`. Absent for a contest that declares none. Invariant
   *  tokens: the sponsor's names, never translated. */
  entryClasses?: string[]
  /** ⭐ The entry declarations the picked contest's sponsor names, each absent when it names
   *  none: its multi-operator classes (Cabrillo `CATEGORY-TRANSMITTER`), its overlays
   *  (`CATEGORY-OVERLAY`), the `ADDRESS` headers its template carries, and whether it has a
   *  `SOAPBOX`. Settings offers each only where it is written. Invariant tokens, never
   *  translated. */
  transmitterCategories?: string[]
  overlayCategories?: string[]
  addressHeaders?: string[]
  soapbox?: boolean
  /** ⭐ The bands an entry may declare (Cabrillo `CATEGORY-BAND`, CQ WW's `ALL 160M … 10M`), for
   *  the Band category picker; absent for a contest that lists none. Invariant tokens. */
  bandCategories?: string[]
  /** ⭐ The Cabrillo lines the sponsor requires that a contest started from these settings would
   *  leave out, for the Contest screen to name before Contest mode turns on. Filled by the
   *  preview only. */
  cabrilloMissing?: string[]
  /** ⭐ When the picked contest runs: its running-or-next window (Unix UTC), from the same
   *  rules data the session's banner reads, so the contest screen can show it before any
   *  session exists. Filled by the preview only; absent from the Remote capture. */
  eventStartUnix?: number
  eventEndUnix?: number
  /** ⭐ The settings this contest's exchange sends from, by their Rust names (`fd_class`,
   *  `contest_qth_county`…), so the contest setup asks for this contest's and no other's. Filled by
   *  the preview only. */
  sentSettings?: string[]
  /** The optional Cabrillo header lines the sponsor's template writes (`CATEGORY-POWER`, `EMAIL`,
   *  `CLUB`…), so the contest setup asks only for what this contest's log declares. Preview only. */
  cabrilloHeaders?: string[]
  /** The only `CATEGORY-POWER` values the sponsor takes, when it names some (Winter Field Day's
   *  `QRP` and `LOW`). Preview only. */
  powerCategories?: string[]
}

/** The same preview for settings not saved yet: the saved settings with `patch`'s fields over them.
 *  Read-only — what the contest setup shows while the operator answers its steps. */
export async function previewFdRuleset(patch: Partial<import('./types').Settings>): Promise<FdRulesetDto> {
  return invoke<FdRulesetDto>('preview_fd_ruleset', { patch })
}

/** One contest's running-or-next window (Unix UTC), from the rules data. */
export interface ContestWindow {
  id: string
  startUnix: number
  endUnix: number
}

/** When every contest the rules data holds runs: each one's running-or-next window. */
export async function contestWindows(): Promise<ContestWindow[]> {
  return invoke<ContestWindow[]>('contest_windows')
}

/** Ruleset facts for the CONFIGURED event (`settings.fdEvent`) — independent of
 * the master switch, so Settings can preview an event's rules before it's on. */
export async function getFdRuleset(): Promise<FdRulesetDto> {
  return invoke<FdRulesetDto>('get_fd_ruleset')
}

/** Orbital-element (TLE) currency status — Settings "Orbital elements" fieldset
 * + the Now-Bar `sat` lane. */
export interface TleStatus {
  /** Merged (group + imported) element-set count. */
  count: number
  /** …of which pass the per-bird 30 d usability gate. */
  usableCount: number
  /** …of THOSE, how many are past the 14 d stale line and drifting (the 14–30 d
   * band). A subset of `usableCount`, never a sibling — these birds are still
   * drawn. The median alone hides exactly one shape: half the catalog at 29 d,
   * the other half fetched this morning, every set-wide surface quiet. This is
   * the number that shape cannot hide behind. */
  agingCount: number
  /** …and how many the 30 d gate holds BACK entirely (elements past 30 d).
   * Disjoint from `usableCount` — these sit out — so `usableCount +
   * heldBackCount` is every bird with a readable epoch. Required, never
   * optional and never derived as `count - usableCount`: that difference also
   * swallows unparseable epochs, and an absent counter is the sentinel that
   * makes two surfaces guess differently. Stated beside the age so a current
   * catalog with a few slow-cadence birds excluded cannot read like a set
   * that has gone stale. */
  heldBackCount: number
  /** Unix stamp of the last successful fetch/304; 0 = never. */
  fetchedAt: number
  /** "mirror" | "celestrak" | "import" | "legacy" | "bundled" | "none".
   * "bundled" = the seed snapshot shipped in the installer, serving before
   * this install has ever reached the mirror (its `fetchedAt` is 0). */
  source: string
  /** Operator file-imports riding the snapshot (persist across refreshes). */
  importedCount: number
  /** MEDIAN age (days) of the USABLE (≤30 d) sets — the Satellites badge
   * scalar, and what every currency surface compares against 14 d. Median,
   * never the oldest: the 30 d ceiling bounds the oldest ADMITTED bird just
   * under itself, so a max reads "stale" however fresh the set is. Absent
   * when no set is usable. */
  elementAgeDays?: number | null
  /** Celestrak 403/404 hard stop's end (unix); 0 = not blocked. */
  blockedUntil: number
  /** The last refresh failure, RAW ("HTTP 404", a refused-set reason) —
   * tooltip/debugging material; the operator-voiced headline is composed by
   * `tleRefreshMessage` from `lastErrorKind` + the currency fields, never
   * from this text. Absent = last attempt landed. */
  lastError?: string | null
  /** Which WAY it failed — "mirrorUnreachable" | "celestrakBlocked" |
   * "failed" — the composer's branch key. Present iff `lastError` is. */
  lastErrorKind?: string | null
}

/** Element-currency status (never blocks on the network). Polling this also
 * kicks a due background refresh — the App's Now-Bar poll is what keeps
 * elements current while no satellite surface is open. */
export async function getTleStatus(): Promise<TleStatus> {
  return invoke<TleStatus>('get_tle_status')
}

/** The manual "refresh elements" action: run one refresh attempt NOW and wait
 * for it. Bypasses the TTL and failure backoff, but never the single-flight,
 * the Celestrak 2 h floor, or the 403/404 hard stop (etiquette is not
 * operator-waivable); a mirror that cannot deliver escalates to Celestrak in
 * the same flight when the floor + block allow. Resolves with the
 * post-attempt status whether the attempt landed or failed — compose the
 * result via `tleRefreshMessage`; rejects only when no attempt could run
 * (one already in flight). */
export async function fetchTlesNow(): Promise<TleStatus> {
  return invoke<TleStatus>('fetch_tles_now')
}

/** Import operator-supplied elements (a downloaded Celestrak file, AMSAT keps,
 * a new launch's SupGP set) from 2LE/3LE text. Per-bird integrity only — no
 * count/freshness ratchet; imports persist across refreshes and merge over the
 * fetched group by NORAD, newest epoch winning. */
export async function importTles(text: string): Promise<TleStatus> {
  return invoke<TleStatus>('import_tles', { text })
}

/** The 60 s X-ray fast lane — the freshest GOES long-band flux, so a flare's
 * onset reaches the map + alert in ~1 min instead of the 5-min prop cadence. */
export async function getXrayNow(): Promise<import('./types').XrayNow> {
  return invoke<import('./types').XrayNow>('get_xray_now')
}

/** Modelled best-contact windows for every active + upcoming DXpedition ("Your
 * Window") — computed from YOUR grid with the configured prediction engine and
 * server-cached (climatology; recomputed on day/engine/target changes). */
export async function getDxpedWindows(days?: number): Promise<import('./types').DxpedWindow[]> {
  return invoke<import('./types').DxpedWindow[]>('get_dxped_windows', { days })
}

/** SWPC R/S/G scales + recent alerts (the backend returns a [scales, alerts] tuple). */
export async function getSpaceWxScales(): Promise<{ scales: NoaaScalesView; alerts: AlertView[] }> {
  const [scales, alerts] = await invoke<[NoaaScalesView, AlertView[]]>('get_space_wx_scales')
  return { scales, alerts }
}

export async function sendMessage(peer: string, text: string): Promise<AppSnapshot> {
  // The command returns the post-send snapshot — apply it immediately so the
  // outbound message renders without waiting ~300 ms for the next poll.
  return invoke<AppSnapshot>('send_message', { peer, text })
}

/** One-click resend of a terminal (no-ack / abandoned) chat bubble — re-queues the same
 * text with a fresh cycle budget on the SAME bubble. ackId targets the exact bubble. */
export async function resendChat(peer: string, ackId?: string | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('resend_chat', { peer, ackId: ackId ?? null })
}

/**
 * Send an open broadcast to everyone on frequency (not directed at a peer).
 * Lands in the "*" band-activity feed. Returns the fresh snapshot.
 */
export async function broadcast(text: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('broadcast', { text })
}

export async function selectPeer(peer: string | null): Promise<AppSnapshot> {
  // Round-trip BOTH select and deselect — a null clears the engine's active peer
  // (it used to linger backend-side, leaving stale roster/QSY context).
  return invoke<AppSnapshot>('select_peer', { peer })
}

/** Delete a conversation thread from the recents list, and cancel any messages still
 * queued for that peer so nothing further for it goes on the air. Persists immediately.
 * Returns the fresh snapshot. This deletes history, it does not block a station: the
 * thread re-creates if the peer is heard again (or you broadcast).
 * (Wire name stays `archive_conversation`.) */
export async function archiveConversation(peer: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('archive_conversation', { peer })
}

/**
 * Answer / work a station by callsign: enters QSO mode targeting that DX.
 * `message`/`snr` are the exact decoded line being answered (when the operator
 * double-clicked a decode) so the sequencer jumps to the correct next Tx —
 * WSJT-X double-click semantics — rather than restarting at the grid.
 * Returns the fresh snapshot.
 */
export async function callStation(
  call: string,
  grid?: string,
  message?: string,
  snr?: number,
  freq?: number,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('call_station', {
    call,
    grid: grid ?? null,
    message: message ?? null,
    snr: snr ?? null,
    // The decoded station's audio offset (Hz) — moves our RX/TX onto it (WSJT-X).
    freq: freq ?? null,
  })
}

/** WSJT-X Tx-slot click: force `text` as the next transmission to `call`
 * (starts/retargets the QSO if needed, arms per the double-click-sets-Tx
 * behavior option, fires this period when it still fits). The auto-sequencer
 * resumes normally from whatever step the partner's reply matches. */
export async function overrideNextTx(
  call: string,
  grid: string | null,
  text: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('override_next_tx', { call, grid, text })
}

/** The operator erased a decode pane — mirror it to cooperating apps via the
 * WSJT-X UDP Clear. 0 = Band Activity, 1 = Rx Frequency, 2 = both. */
export async function notifyErase(window: 0 | 1 | 2): Promise<void> {
  await invoke('notify_erase', { window })
}

/** Log a Field Day contact by hand (the CW, Phone, RTTY and PSK cockpits' log strips).
 * Rejects with a message on a band+mode dupe.
 *
 * ⚠️ `mode` IS THE SCORING CLASS, and all THREE belong here. The engine's `log_mode_at`
 * (tempo-core/src/fieldday.rs) has always taken 'DIG' and handled it specially — it stamps
 * the real on-air submode behind the class so exports emit the actual mode — but this
 * signature listed only the two classes its first two callers used. A digital position
 * picking a station up by hand had no way to say so, and 'PH' credits the wrong class.
 *
 * ⚠️ `submode` IS THE MODE THAT WAS ON THE AIR, and 'DIG' contacts outside the FT tiers need
 * it. The engine fills a blank one from `FieldDayLog::current_submode`, which tracks the FT
 * tier alone — so an RTTY or PSK Field Day contact logged without it is stamped "FT8": the
 * wrong ADIF mode, Cabrillo "DG" where ARRL wants "RY", and a mode Winter Field Day bans
 * outright on a QSO that was perfectly legal. Omit it for CW and PH, whose class IS their
 * on-air mode. */
/** ⭐ **Log a contest contact, with the received exchange as a FIELD VECTOR.**
 *
 * `fields` is `[slot id, raw]` per box the entry strip rendered, in the session role's
 * own receive order — Field Day's `CLASS`/`SECTION`, a QSO party's `RST`/`QTH`, CQP's
 * `NR`/`QTH`. The slot ids come from `fieldDay.receives`, so the UI never learns which
 * contest means what by position.
 *
 * ⚠️ `mode` IS THE SCORING CLASS and `submode` IS THE MODE THAT WAS ON THE AIR — see
 * `fdLogManual` below, whose two notes apply here unchanged.
 *
 * `sentRst` is the report typed in the entry's optional "RST sent" box, logged as sent for this
 * contact only (2 digits on phone, 3 on CW and digital, or the engine refuses it). Omitted when
 * nothing was typed there, and then the arguments are exactly what they always were; the same on
 * the two commands below. */
export async function contestLogManual(
  call: string,
  fields: [string, string][],
  mode: 'CW' | 'PH' | 'DIG',
  submode?: string,
  entry?: EntryClaim,
  sentRst?: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('contest_log_manual', {
    call,
    fields,
    mode,
    submode: submode ?? null,
    ...(entry ? { entry } : {}),
    ...(sentRst ? { sentRst } : {}),
  })
}

/** ⭐ **What a strip logging the shared contest entry saw** (`features/contestEntryShare.ts`):
 *  the entry's rev when the strip last agreed with it, and the call and boxes it shows. Passed to
 *  the three contest log commands while the contest logger window is open, so the engine logs
 *  the contact once when Enter is pressed in both windows at the same moment — the second is
 *  refused `contestEntryLogged` or `contestEntryChanged`, and logs nothing. Omitted otherwise, and
 *  then the command's arguments are exactly what they always were. */
export interface EntryClaim {
  rev: number
  call: string
  fields: Record<string, string>
}

/** The contest strip changed in one window while the contest logger window is open: what it now
 *  shows becomes the entry both windows show. `contact` is the contact the strip showed as it
 *  changed (`ContestEntryShared.contact`). Answers the entry's new rev. Rejects with the logger
 *  window closed, and with `contestEntryLogged` when that contact has since been logged. Text in
 *  memory only — nothing is logged, written or sent. */
export async function contestEntryPut(
  call: string,
  fields: Record<string, string>,
  marks: unknown,
  contact: number,
): Promise<number> {
  return invoke<number>('contest_entry_put', { call, fields, marks, contact })
}

/** ⭐ **Log ONE contact as several rows** — a station on a county line, which the Illinois QSO
 * Party counts once per county. `rows` is one field vector per row, each shaped exactly as
 * `contestLogManual`'s `fields`. The engine stamps every row with one time and one band and
 * dupe-checks each on its own key; the answer is, per row, whether it entered the log (`false`:
 * refused as a dupe, as `contestLogManual` refuses one).
 *
 * `mode` and `submode` mean exactly what they mean there. */
export async function contestLogManualRows(
  call: string,
  rows: [string, string][][],
  mode: 'CW' | 'PH' | 'DIG',
  submode?: string,
  entry?: EntryClaim,
  sentRst?: string,
): Promise<boolean[]> {
  return invoke<boolean[]>('contest_log_manual_rows', {
    call,
    rows,
    mode,
    submode: submode ?? null,
    ...(entry ? { entry } : {}),
    ...(sentRst ? { sentRst } : {}),
  })
}

/** A contest contact the operator removed — kept, never deleted, and restorable: the contest
 *  screen's Removed list. `rows` are as the log table shows them (a county line has one per
 *  county); `id` is what `contestRestore` takes. */
export interface RemovedContest {
  id: number
  removedUnix: number
  rows: FieldDayQso[]
}

/** Why a removal or a restore changed nothing. */
export type ContestRemovalRefusal = 'empty' | 'changed' | 'notRemoved' | 'workedAgain'

/** At a club, where a removal or restore goes: `linked` — this laptop is with the host now, so
 *  every laptop drops it (or gets it back) as soon as the host has it; `away` — when the link is
 *  back. Absent without club sync. */
export type ContestClubRoute = 'linked' | 'away'

/** Where a removed contact had already gone — places Nexus cannot take it back from. */
export interface ContestSentTo {
  /** The N3FJP host it was pushed to. */
  n3fjp?: string
  /** The address of the N1MM broadcast it went out on. */
  n1mm?: string
  /** WSJT-X listeners had its "QSO Logged" datagram. */
  wsjtx: boolean
  /** The logbook holds a copy merged from the contest. */
  logbook: boolean
  /** The services holding that copy by its upload marks: 'lotw' | 'qrz' | 'clublog' | 'eqsl'. */
  uploaded: string[]
}

export type ContestRemovalAnswer =
  | { outcome: 'removed'; entry: RemovedContest; sentTo: ContestSentTo; club?: ContestClubRoute }
  | { outcome: 'restored'; entry: RemovedContest; club?: ContestClubRoute }
  | { outcome: 'refused'; refusal: ContestRemovalRefusal }

/** ⭐ **Remove the newest contest contact** — when it is still the one named (`call` and
 *  `whenUnix`, as the strip showed it), else `refused: changed`. It is kept, never deleted: its
 *  serial and club seq stay spent, and `contestRestore` puts it back. At a club the club takes it
 *  back too (`club`). Answers once the contest journal holds the change. Never keys or stops TX. */
export async function contestRemoveLast(call: string, whenUnix: number): Promise<ContestRemovalAnswer> {
  return invoke<ContestRemovalAnswer>('contest_remove_last', { call, whenUnix })
}

/** Put a removed contact back exactly as it was — refused when the station was worked again
 *  since on that band and mode. At a club the club puts it back too (`club`). */
export async function contestRestore(id: number): Promise<ContestRemovalAnswer> {
  return invoke<ContestRemovalAnswer>('contest_restore', { id })
}

/** What a Clear test contacts came to: how many contacts left the contest log (each now under
 *  Removed), where any of them had already gone, and at a club whether the club drops them now —
 *  or why nothing was cleared. */
export type ContestClearAnswer =
  | { outcome: 'cleared'; contacts: number; sentTo: ContestSentTo; club?: ContestClubRoute }
  | { outcome: 'refused'; refusal: ContestRemovalRefusal }

/** ⭐ **Clear test contacts** — every contact of this running out of the contest log, each kept
 *  under Removed as `contestRemoveLast` keeps one, when the log is still what the screen named
 *  (`rowCount` rows, the newest contact `call` at `whenUnix`), else `refused: changed`. Club seqs
 *  stay spent, and the serial run starts again at 1. At a club the club takes each back too (`club`). Answers once the contest
 *  journal holds the change. Never keys or stops TX. */
export async function contestClear(rowCount: number, call: string, whenUnix: number): Promise<ContestClearAnswer> {
  return invoke<ContestClearAnswer>('contest_clear', { rowCount, call, whenUnix })
}

/** The removed contacts, oldest removal first. */
export async function contestRemoved(): Promise<RemovedContest[]> {
  return invoke<RemovedContest[]>('contest_removed')
}

/** Why an edit changed nothing: the contact is no longer as it was shown; it is no longer in the
 *  log at all; no call or no band;
 *  another number of counties than a county line has; a satellite contact's band; club sync,
 *  which cannot carry an edit to the club. */
export type ContestEditRefusal = 'changed' | 'gone' | 'incomplete' | 'rows' | 'satellite' | 'club'

/** What an edit came to: the contact as it was and as it is now (one row, or one per county),
 *  and where it had already gone, which keeps what it said — or why nothing changed. */
export type ContestEditAnswer =
  | { outcome: 'edited'; before: FieldDayQso[]; after: FieldDayQso[]; sentTo: ContestSentTo }
  | { outcome: 'refused'; refusal: ContestEditRefusal }

/** What an edit puts on a contact: its call, what they sent (one `[slot, value]` vector per row,
 *  as `contestLogManualRows` takes them), its band, and its mode on the air (`''` keeps it). */
export interface ContestContactEdit {
  call: string
  rows: [string, string][][]
  band: string
  mode: string
}

/** ⭐ **Edit a contest contact in place** — when it is still the one named (`call` and
 *  `whenUnix`, as the screen showed it) and still reads as `opened`, the rows the form was filled
 *  from, else `refused: changed`. It keeps its time, serial and
 *  number; refused at a club (`club`). Answers once the contest journal holds the edit. Never
 *  keys or stops TX. */
export async function contestEdit(
  call: string,
  whenUnix: number,
  edit: ContestContactEdit,
  opened: FieldDayQso[],
): Promise<ContestEditAnswer> {
  return invoke<ContestEditAnswer>('contest_edit', {
    call,
    whenUnix,
    opened,
    newCall: edit.call,
    rows: edit.rows,
    band: edit.band,
    mode: edit.mode,
  })
}

/** One contest's contacts that the contest journal keeps past its four days and the logbook does
 *  not hold: what the contest screen offers to merge. `event` is the contest's rules-file id;
 *  the times are Unix seconds. */
export interface ContestUnmerged {
  event: string
  count: number
  firstUnix: number
  lastUnix: number
}

/** ⭐ The contest contacts no session loads any more and the logbook does not hold, by contest.
 *  Asking also lets go of every one whose copy the logbook holds on disk, as such a contact
 *  always aged out of the journal. */
export async function contestUnmerged(): Promise<ContestUnmerged[]> {
  return invoke<ContestUnmerged[]>('contest_unmerged')
}

/** ⭐ Merge one contest's contacts that the contest journal keeps past its four days into the
 *  general logbook: the offer above. The end-of-contest merge's own rule, so a contact already
 *  there is skipped, never duplicated. Nothing is queued for upload. */
export async function contestMergeUnmerged(event: string): Promise<FdMergeReport> {
  const [report] = await invoke<[FdMergeReport, AppSnapshot]>('contest_merge_unmerged', { event })
  return report
}

/** ⭐ **`contestLogManual` for a contact worked THROUGH A BIRD** — the Satellites
 * section's strip, and the only caller.
 *
 * ⚠️ **The difference is where the row's band comes from, and it is the whole reason
 * this exists.** The ordinary path stamps the band the radio is on at the moment you
 * type — right for a knob-QSY between contacts, wrong for a pass. An operator turning a
 * rotator by hand logs when their hands are free, which is after LOS with the radio back
 * on the HF run, so a 70 cm pass entered the contest log — and went out to N1MM and
 * N3FJP — on 20 m. Here the band and frequency come from the transponder that was held.
 *
 * With no bird held and none last worked the engine falls back to `contestLogManual`
 * itself: there is no pass, the dial IS where the contact was made, and refusing to log
 * would lose a contact.
 *
 * `mode` and `submode` mean exactly what they mean there. */
export async function contestLogSatellite(
  call: string,
  fields: [string, string][],
  mode: 'CW' | 'PH' | 'DIG',
  submode?: string,
  entry?: EntryClaim,
  sentRst?: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('contest_log_satellite', {
    call,
    fields,
    mode,
    submode: submode ?? null,
    ...(entry ? { entry } : {}),
    ...(sentRst ? { sentRst } : {}),
  })
}

/** ⚠️ ADDING AN EXPORT HERE BREAKS TEST SUITES, AND THE SUITE WILL TELL YOU IT PASSED.
 *
 *  `vi.mock('../api', () => ({ … }))` factories enumerate every export they stand in for and do
 *  NOT fall through to the real module, so the first component that calls a new export throws
 *  inside whatever suite renders it. Adding the two below hit **twelve** test files in four
 *  distinct mock shapes: `vi.fn(() => Promise.resolve({}))`, `vi.fn(() => Promise.resolve(true))`,
 *  `(...args: unknown[]) => localFn(...args)`, and a renamed-local variant — so a single
 *  find-and-replace does not cover it.
 *
 *  ⭐ **The part that will cost you an hour: it is an UNHANDLED ERROR, not a failing assertion.**
 *  `npm test` printed `628 files passed, 6353 tests passed` and exited **1**. The summary line
 *  says green while the exit code says red. Read the exit code. */

/** ⭐ THE CONTEST SERIAL'S PEER DOOR — the operator has committed a callsign on the entry line.
 *
 *  In a contest with a serial the number shown and keyed is bound to THIS station from here
 *  until the contact is logged: a station worked earlier and never logged gets back the number
 *  they already copied, and a different call committed onto the same live entry takes that
 *  exchange over — a corrected call sends what was already sent rather than a second number —
 *  WITHOUT the call it replaced losing the number it copied. Both bindings are kept, because
 *  the gesture is a correction and a move-on in the same keystrokes.
 *
 *  ⚠️ COMMIT ONLY — Enter, blur, a spot click. Never on every keystroke: bound to each edit it
 *  mints a binding per prefix of the call and strands all but the last. `contestPlace` is the
 *  per-keystroke endpoint; this is deliberately not. */
export async function contestWorking(call: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('contest_working', { call })
}

/** The contest entry line was cleared without logging. The number that station copied stays
 *  bound to them; only the live slot is released, so the next call committed is a new contact
 *  rather than a correction of this one. */
export async function contestEntryReset(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('contest_entry_reset')
}

export async function fdLogManual(
  call: string,
  klass: string,
  section: string,
  mode: 'CW' | 'PH' | 'DIG',
  submode?: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('fd_log_manual', {
    call,
    class: klass,
    section,
    mode,
    submode: submode ?? null,
  })
}

/** Where THIS SESSION's merged contacts go (§18.1) — per session, default OFF.
 *
 * `destinations` are connector ids from `fieldDay.upload.available`. Both halves are
 * required: the switch off, or an empty destination list, queues nothing.
 *
 * ⚠️ It does NOT stop ClubLog's catch-up sweep, which re-queues every contact ClubLog
 * never accepted the next time a ClubLog password is saved. That limitation travels as
 * `fieldDay.upload.hint` and must render beside the control. */
export async function fdSetUpload(
  enabled: boolean,
  destinations: string[],
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('fd_set_upload', { enabled, destinations })
}

/** ⭐ Merge this contest session's contacts into the general logbook — the one-click
 *  end-of-contest action (§3.2, §11 item 5).
 *
 *  IDEMPOTENT: every row carries its own merge identity, so a row already in the logbook
 *  is skipped rather than duplicated — safe to press twice, and safe after a restart.
 *  The report says which happened, so a second press reads as a no-op instead of looking
 *  like one that quietly did nothing.
 *
 *  Whether the merged rows are ALSO queued for connector upload is the session's own
 *  control (`fdSetUpload`), off unless the operator turned it on; `queued` reports what
 *  that control was at merge time. It does not override ClubLog's catch-up sweep — see
 *  `fieldDay.upload.hint`.
 *
 *  The command answers with the snapshot too (one engine lock, one logbook sweep); the
 *  poll delivers that anyway, so only the report is handed back. */
export async function fdMergeToGeneral(): Promise<FdMergeReport> {
  const [report] = await invoke<[FdMergeReport, AppSnapshot]>('fd_merge_to_general')
  return report
}

/** ⭐ "I moved" — edit the exchange this session is COMPOSING (§4.1).
 *
 * `values` is `[slot id, value]` for the slots that changed. It takes effect on the
 * NEXT contact: rows already logged keep what they sent, and so does the exchange
 * already in flight to the station being worked.
 *
 * Writes both the live session and the setting a new session starts from. REJECTS with
 * the operator's own message when the new location resolves to a different role — that
 * is a separate entry, not a move. */
export async function contestIMoved(values: [string, string][]): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('contest_i_moved', { values })
}

/** Listen ~2 s for Nexus club-event beacons on the LAN ("Find club events").
 * Empty = nothing announcing (or the Wi-Fi eats broadcast — manual entry stays). */
export async function fdDiscoverEvents(): Promise<FdEventBeacon[]> {
  return invoke<FdEventBeacon[]>('fd_discover_events', {})
}

/** Where Nexus's club sync rules stand in Windows Firewall on this laptop, read with no
 *  administrator rights (`club_firewall_status`). `null` off Windows, and when the firewall
 *  cannot be read. */
export async function clubFirewallStatus(): Promise<ClubFirewall | null> {
  return invoke<ClubFirewall | null>('club_firewall_status')
}

/** Allow Nexus through Windows Firewall (`'allow'`: Nexus's three club sync rules, on Private and
 *  Public networks) or remove its rules (`'remove'`), through one Windows administrator prompt;
 *  resolves with the firewall read back. Rejects with `onAir` while anything is transmitting,
 *  `running` while a press is still waiting on Windows, `path` or `port` when the command cannot
 *  be built, and `unsupported` off Windows. */
export async function clubFirewallPress(action: 'allow' | 'remove'): Promise<ClubFirewallPress> {
  return invoke<ClubFirewallPress>('club_firewall_press', { action })
}

/** Export the merged CLUB log from the host (deduped earliest-wins).
 * Rejects when this instance is not hosting. */
export async function fdClubExport(format: 'cabrillo' | 'adif'): Promise<string> {
  return invoke<string>('fd_club_export', { format })
}

/** The club log for this screen: the newest contacts the club merged, newest first, each with the
 *  position that logged it, and how many the club holds; on a joined laptop, as the host last sent
 *  it. `null` when this instance neither hosts nor has been sent one. A read; never on Remote. */
export async function fdClubLogNewest(): Promise<FdClubLog | null> {
  return invoke<FdClubLog | null>('fd_club_log_newest')
}

/** What the host's Give button did: the position given, or why nothing changed. */
export type FdGiveAnswer =
  | { outcome: 'given' }
  | { outcome: 'refused'; refusal: 'notHosting' | 'stale' }

/** ⭐ **Give a club position to the laptop the host turned away for it** — the Give button on
 *  the host's own turned-away list, by the `handle` that entry carries. The position is pinned
 *  to that laptop, the laptop that held it is closed and turned away by name, and its own entry
 *  can give the position back. Refused when this Nexus is not hosting, and for a handle no
 *  entry on the list now can be given by. Never on Remote. */
export async function fdClubGivePosition(handle: number): Promise<FdGiveAnswer> {
  return invoke<FdGiveAnswer>('fd_club_give_position', { handle })
}

/** What the host's Remove or Allow back did: done, or why nothing changed (`own`: the host's own
 *  position, which it cannot remove; `stale`: no such position on the board now, or, for Allow
 *  back, not removed). */
export type FdRemoveAnswer =
  | { outcome: 'done' }
  | { outcome: 'refused'; refusal: 'notHosting' | 'own' | 'stale' }

/** ⭐ **Remove a position from the club event** — the host's own board, by the position's id. Its
 *  link is closed and it is turned away at every JOIN until the host lets it back
 *  (`fdClubAllowBack`); the contacts it already sent stay in the club's log. Never on Remote. */
export async function fdClubRemovePosition(posid: string): Promise<FdRemoveAnswer> {
  return invoke<FdRemoveAnswer>('fd_club_remove_position', { posid })
}

/** Let a removed position back in: its next try joins, and it sends what it logged meanwhile. */
export async function fdClubAllowBack(posid: string): Promise<FdRemoveAnswer> {
  return invoke<FdRemoveAnswer>('fd_club_allow_back', { posid })
}

/** What the host's "Import a laptop's file…" did (`fd_club_import`): how many of the file's
 *  contacts it added, how many were duplicates or already in, and how many it kept out and why.
 *  Nothing is ever taken out of the club's log. */
export interface FdClubImport {
  added: number
  dupes: number
  alreadyIn: number
  otherContest: number
  otherRunning: number
  otherCall: number
  takenBack: number
  unreadable: number
  /** Kept out where every contact enters the club's log, each with the host screen's sentence. */
  refused: { call: string; reason: string }[]
}

/** ⭐ **Merge a laptop's contest file into the host's club log**, as that laptop's sync would have
 *  merged it, recorded as from position `posid`: the host's own Contest screen, for a host that
 *  died or a laptop that never synced. Importing the same file again adds nothing. Rejects, with
 *  the sentence to show, when this Nexus is not hosting or the position cannot be used. Never on
 *  Remote. */
export async function fdClubImport(text: string, posid: string): Promise<FdClubImport> {
  return invoke<FdClubImport>('fd_club_import', { text, posid })
}

/** The spectator scoreboard's bound state, for the Settings row: running?,
 * the URL a TV on the LAN should open, the last bind error. */
export interface FdScoreboardStatus {
  running: boolean
  url: string | null
  error: string | null
}

export async function fdScoreboardStatus(): Promise<FdScoreboardStatus> {
  return invoke<FdScoreboardStatus>('fd_scoreboard_status', {})
}

/** Callsign + grid only — the TV page's one station fact. Served ONLY by the LAN
 *  RPC (the desktop never needs it); built by hand server-side so the TV page never
 *  touches get_settings. */
export async function getTvStation(): Promise<{ call: string; grid: string }> {
  return invoke('tv_station')
}

/** The Connect LAN page's bound state, for its Settings row. */
export interface ConnectWebStatus {
  running: boolean
  port: number
  /** The address to type into the TV; empty while it is not serving. */
  url: string
  error: string | null
}

export async function connectWebStatus(): Promise<ConnectWebStatus> {
  return invoke<ConnectWebStatus>('connect_web_status', {})
}

/** Test the N3FJP TCP API ("N3FJP's Field Day Contest Log v6.6") — run at the
 * club site before the event. */
export async function n3fjpTestConnection(): Promise<string> {
  return invoke<string>('n3fjp_test_connection', {})
}

/** One-click hunt: remember the activator + park so the next QSO logged with
 * that call auto-tags SIG/SIG_INFO (the hunter-side ADIF credit). */
export async function setHuntTarget(
  call: string,
  program: string,
  reference: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_hunt_target', { call, program, reference })
}

export async function clearHuntTarget(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('clear_hunt_target', {})
}

/** WSJT-X "Decode" / F6: re-run the decoder over the last period's audio with
 * the current settings; only newly-found lines appear. */
export async function redecode(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('redecode', {})
}

/** Start a CQ run; `dir` = a directed token ("DX"/"NA"/"POTA"/…) or null for a
 * plain CQ (clears a sticky directed token). */
export async function startCq(dir: string | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('start_cq', { dir })
}

/** The Operate strip's Call CQ on this computer (`Engine::start_running_with_cq`), and the Tx6 row's
 * while a contest runs (`snap.contestCq`, `snap.contestCqTyped`). `dir` is the direction of the CQ
 * typed in Tx6, which the contest's Running calls where WSJT-X has no activity for the contest
 * (`snap.contestCqTyped`); null for a plain CQ. Outside a contest the row, and the Remote page
 * always, call `startCq`, which sends the CQ they show. */
export async function startRunning(dir: string | null = null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('start_running', { dir })
}

/** The Operate strip's S&P on this computer (`Engine::start_search_and_pounce`). The Remote page's
 * S&P keeps its own path. */
export async function startSearchAndPounce(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('start_search_and_pounce', {})
}

/** Call CQ from Tempo (chat-first): sends one structured `CQ <call> <grid>` frame and
 * arms TX, staying in chat. Rejects if the callsign/grid aren't set. `dir` = optional
 * directed token, or null for a plain CQ. */
export async function callCq(dir: string | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('call_cq', { dir })
}

/** Toggle the chat CQ RUN — keep calling every idle TX slot until answered/stopped. */
export async function setChatCq(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_chat_cq', { on })
}

/** Resume a paused CQ run immediately (skip the idle auto-resume wait). */
export async function resumeChatCq(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('resume_chat_cq')
}

/** Confirm-and-log a QSO held by the prompt-to-log popup (the possibly-edited
 * record). Returns the fresh snapshot.
 *
 * `expectedKey` is the identity of the hold the popup was SHOWING, and it goes on every
 * transport, not only a Remote one (operator ruling 2026-09-19: completed contacts queue). The
 * engine refuses an answer whose key is not the head's, so an edited call can never be filed
 * onto a contact that was promoted while the operator was typing.
 *
 * `iotaOffer` is the IOTA offer the operator took in the popup: the station writes it into an
 * empty IOTA field, held back from uploads until the operator touches it. */
export async function confirmPendingLog(record: LoggedQso, expectedKey?: string | null, iotaOffer?: IotaOfferData | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('confirm_pending_log', { record, expectedKey, ...(iotaOffer ? { iotaOffer } : {}) })
}

/** Discard a QSO held by the prompt-to-log popup without logging it. Names the hold it
 *  answers, exactly as the confirm above does. */
export async function discardPendingLog(expectedKey?: string | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('discard_pending_log', { expectedKey })
}

/** Open (or focus) a standalone OS window for one panel — multi-monitor tear-off. */
export async function openPanelWindow(panel: string): Promise<void> {
  await invoke('open_panel_window', { panel })
}

/** Open (or bring forward) the Stations on this network window: stations paired over the shack's
 *  own network, with no internet, its page in `locale`. Windows only; rejects elsewhere. */
export async function openLanStationsWindow(locale: string): Promise<void> {
  await invoke('open_lan_stations_window', { locale })
}

/** Close a panel's torn-off window if it is open (#263 — re-dock must not leave a second copy
 *  up). A no-op when that window is not open. */
export async function closePanelWindow(panel: string): Promise<void> {
  await invoke('close_panel_window', { panel })
}

/** Snap the current band-map pop-out window to the left/right screen edge as a full-height
 *  vertical strip (or 'none' to un-dock). The dock + geometry persist across launches. */
export async function dockBandmapWindow(side: 'left' | 'right' | 'none'): Promise<void> {
  await invoke('dock_bandmap_window', { side })
}

/** Whether the CALLING window can stay behind other windows (the Connect dashboard, on a
 *  platform that offers it), and whether it does. */
export interface WindowBehind {
  supported: boolean
  on: boolean
}

export async function getWindowBehind(): Promise<WindowBehind> {
  return invoke<WindowBehind>('get_window_behind')
}

/** Keep the calling window behind other windows (or let it come forward): applied now and
 *  remembered for its next open. Rejects where the window or the platform cannot. */
export async function setWindowBehind(on: boolean): Promise<WindowBehind> {
  return invoke<WindowBehind>('set_window_behind', { on })
}

/** Whether the CALLING window offers the pin (the contest logger window), and whether it is
 *  pinned in front of other windows. */
export interface WindowOnTop {
  supported: boolean
  on: boolean
}

export async function getWindowOnTop(): Promise<WindowOnTop> {
  return invoke<WindowOnTop>('get_window_on_top')
}

/** Pin the calling window in front of other windows (or unpin it): applied now and remembered
 *  for its next open. Rejects where the window cannot. */
export async function setWindowOnTop(on: boolean): Promise<WindowOnTop> {
  return invoke<WindowOnTop>('set_window_on_top', { on })
}

/** Bring the calling window to the front with the keyboard: the main window, for itself, when a
 *  contest window sends the operator to a cockpit in it. Rejects where there is no window to move
 *  (a browser). */
export async function focusWindow(): Promise<void> {
  return invoke<void>('focus_window')
}

/** Flash the calling window's taskbar button (a dock bounce on macOS) until the operator looks,
 *  without taking focus. Rejects where there is no window to flash (a browser). */
export async function requestWindowAttention(): Promise<void> {
  return invoke<void>('request_window_attention')
}

/** Switch the Operate mode: 'dx' (FT8/FT4) or 'msg' (Tempo two-way calling).
 * Atomically sets the mode's tier + mode. Returns the fresh snapshot. */
export async function setArea(area: 'dx' | 'msg'): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_area', { area })
}

/** Operator "Resend": re-arm the current QSO message (re-transmit a stalled or
 * uncopied step). No-op outside a QSO. Returns the fresh snapshot. */
export async function qsoResend(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qso_resend', {})
}

/** WSJT-X's Clear DX (F4, the Clear button) and an edit of the DX Call box: the contact with the
 * station in it ends, the QSO start time is reset, and nothing more is sent to that station.
 * Calling CQ with nobody in the contact, only the start time is reset. Returns the fresh snapshot. */
export async function clearDxCall(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('clear_dx_call', {})
}

/** Operator in-QSO free text (WSJT-X Tx5): override the next transmission with
 * `text`, directed to the current DX when known. Returns the fresh snapshot. */
export async function qsoFreetext(text: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qso_freetext', { text })
}

/** Operator "Log QSO": log the active QSO's contact now. `logged` is the engine's verdict
 *  (#100) — false when nothing was loggable (already logged / no QSO / no report yet), and
 *  the UI must not claim success then. Snapshot is fresh either way. */
/** `call` names the station logged or held, which with Auto-log and the popup off can be a
 *  contact set aside behind the station now shown. `alreadyWaiting`: that station's offer was
 *  already waiting in the log popup, so nothing new was held. */
export async function logCurrentQso(): Promise<{ logged: boolean; call?: string | null; alreadyWaiting?: boolean; pending?: boolean; snapshot: AppSnapshot }> {
  return invoke<{ logged: boolean; call?: string | null; alreadyWaiting?: boolean; pending?: boolean; snapshot: AppSnapshot }>('log_current_qso', {})
}

/** Where a log form's STATE came from: typed or picked by the operator, filled in by a callbook
 *  lookup, or placed from the park or summit. The station lets a park's own state outrank a
 *  callbook's, and the operator's outrank both. */
export type StateSource = 'operator' | 'callbook' | 'park'

/** Append a contact to the ADIF logbook. Returns the fresh snapshot. `iotaOffer` is the IOTA offer
 *  the operator took on the form ([`getIotaOffer`]): written into an empty IOTA field, held back
 *  from uploads until the operator touches it. */
export async function logQso(record: LoggedQso, stateSource?: StateSource, iotaOffer?: IotaOfferData | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('log_qso', { record, stateSource, ...(iotaOffer ? { iotaOffer } : {}) })
}

/** One contact "Check park states" lists: a hunted park or summit in ONE state whose contact
 *  holds another state (`state`), or none; `parkState` is the park's. */
export interface ParkStateRow {
  id: string
  call: string
  whenUnix: number
  band: string
  mode: string
  program: string
  reference: string
  state: string | null
  parkState: string
  /** Confirmed by any channel: listed unticked, for the operator to decide. */
  confirmed: boolean
}

/** The Logbook's "Check park states" list. Reads; writes nothing. */
export async function parkStateReview(): Promise<ParkStateRow[]> {
  return invoke<ParkStateRow[]>('park_state_review')
}

/** Give the ticked contacts their park's state: each only while it still holds the state the
 *  check listed. Nothing is uploaded again. How many changed. */
export async function applyParkStates(
  changes: { id: string; state: string | null; parkState: string }[],
): Promise<number> {
  return invoke<number>('apply_park_states', { changes })
}

/** The services "Check confirmations" checks, in the order it lists them. */
export type CheckedService = 'lotw' | 'eqsl' | 'qrz'

/** One line of "Check confirmations": a contact holding a service's confirmation (`lotw`, `eqsl`,
 *  `qrz`), or its LoTW upload mark (`lotwUpload`), that the service's own download gives another
 *  contact, or none. */
export interface ConfirmationLine {
  id: string
  mark: CheckedService | 'lotwUpload'
  /** `moved`: the row it rests on pairs with another contact, the sibling; `contradicted`: the
   *  service holds this contact itself, unconfirmed; `orphan`: the row pairs with no contact. */
  class: 'moved' | 'contradicted' | 'orphan'
  call: string
  whenUnix: number
  timeKnown: boolean
  band: string
  mode: string
  /** The time of the row the line rests on. */
  rowUnix: number
  siblingId: string | null
  siblingUnix: number | null
  /** The service's own unconfirmed record of this contact (`contradicted` only). */
  ownUnix: number | null
  /** The evidence decides it: the line starts ticked. */
  decisive: boolean
  /** Why it starts unticked. */
  unticked: 'dateOnly' | 'notReplayed' | 'tie' | 'orphan' | 'insideWindow' | 'notToTheMinute' | null
  /** The credit codes the change takes off. */
  removeGranted: string[]
  removeSubmitted: string[]
  /** A paper card it keeps, with its award credit. */
  cardHeld: boolean
  /** Owed to LoTW once every line listed for this contact is applied, and not before. */
  owedAfter: boolean
}

/** What "Check confirmations" found for one service: its lines, newest first, and what it leaves
 *  alone. */
export interface ConfirmationCheck {
  /** The check, which its Apply names. */
  session: number
  service: CheckedService
  /** How many records the service's download held (LoTW's confirmations, eQSL's cards, QRZ's
   *  book), as the Connections log says them. */
  records: number
  /** LoTW only: how many of its confirmations are for a call the log holds no contact under (a
   *  club call on the account, say). They change nothing. */
  otherCallRecords: number
  lines: ConfirmationLine[]
  /** The contacts the service confirms that lack its confirmation: Apply adds it. */
  gainIds: string[]
  /** The service's confirmations it leaves alone: no row of it within a day of them, or logged
   *  under another call than the one in Settings. */
  unreached: number
  outOfScope: number
  /** LoTW upload marks it leaves alone, the same two ways (LoTW only). */
  uploadsUnreached: number
  uploadsOutOfScope: number
}

/** What Apply in "Check confirmations" made of one service's check. */
export interface ServiceApplied {
  service: CheckedService
  /** Contacts whose confirmation from this service was taken off. */
  confirmations: number
  /** Contacts that gained the service's confirmation from its download. */
  gained: number
}

/** What Apply in "Check confirmations" made. */
export interface ConfirmationApplied {
  /** The lines ticked. */
  ticked: number
  /** Each service checked, LoTW, eQSL, QRZ: fewer confirmations taken off than ticked when a
   *  contact changed after the check. */
  services: ServiceApplied[]
  /** Contacts whose LoTW upload mark was cleared. */
  uploads: number
  /** The changed contacts as they were: Logbook ▸ Import ADIF puts them back. */
  beforeFile: string | null
}

/** Start the Logbook's "Check confirmations": a new check at the station, which drops one it holds
 *  and any still downloading. Each service is then checked in it. Its session. */
export async function startConfirmationCheck(): Promise<number> {
  return invoke<number>('start_confirmation_check')
}

/** One service's whole history in check `session` (LoTW's confirmations and own-QSO list, eQSL's
 *  InBox, QRZ's book), downloaded once (minutes, for LoTW on a long history) and checked against
 *  the log. It changes nothing: the download is held at the station for the check's Apply, or its
 *  Cancel. */
export async function confirmationCheck(
  session: number,
  service: CheckedService,
): Promise<ConfirmationCheck> {
  return invoke<ConfirmationCheck>('confirmation_check', { session, service })
}

/** Apply check `session`'s ticked lines, each only while its contact still holds what the check
 *  read, after merging each service's held download. Nothing is uploaded. */
export async function applyConfirmationCheck(
  session: number,
  ticked: { id: string; mark: ConfirmationLine['mark'] }[],
): Promise<ConfirmationApplied> {
  return invoke<ConfirmationApplied>('apply_confirmation_check', { session, ticked })
}

/** Drop the check the station holds, and one still downloading. Nothing changes. */
export async function cancelConfirmationCheck(): Promise<void> {
  await invoke<void>('cancel_confirmation_check')
}

/** The cty.dat-resolved DXCC entity for a callsign, or null — the award
 * identity the "new one" badge keys on (never the QRZ country string). */
export async function resolveEntity(call: string): Promise<string | null> {
  return invoke<string | null>('resolve_entity', { call })
}

/** Where the contest `event` places a callsign, or null: the country its scorer counts the call
 *  as (a WAE area is its own only in CQ WW, where IT9 is Sicily; in every other contest IT9 is
 *  Italy) and the CQ zone the country file gives it, out of ONE resolve, the scorer's. The zone is
 *  a HINT for the contest strip's zone box (its placeholder), never a logged value: a station
 *  outside its prefix's zone sends its own. */
export async function contestPlace(call: string, event: string): Promise<{ entity: string; cqZone: number | null } | null> {
  return invoke<{ entity: string; cqZone: number | null } | null>('contest_place', { call, event })
}

/** Every call of the contest log placed for the Stats tab, in one ask: the country the contest
 *  `event` counts each as (`contestPlace`'s) and the country file's point for it; null where the
 *  file cannot place a call. Local: the country file, no network. */
export async function contestStatsPlaces(
  calls: string[],
  event: string,
): Promise<({ entity: string; lat: number; lon: number } | null)[]> {
  return invoke<({ entity: string; lat: number; lon: number } | null)[]>('contest_stats_places', { calls, event })
}

/** What the log last said of `call`'s county or state, for the contest entry's box receiving
 *  `slot`: this contest's received value on a row of this contest (any year), else a row's
 *  STATE, with that contact's time; null when no contact with the call says. Raw as logged: the
 *  strip fills it only if it is one of the box's own codes. Read through the log's per-call
 *  index in the station, so a big log costs one station's rows. */
export async function contestLoggedQth(call: string, slot: string): Promise<{ value: string; whenUnix: number } | null> {
  return invoke<{ value: string; whenUnix: number } | null>('contest_logged_qth', { call, slot })
}

/** Empty the RETIRED wanted list — its one writer — once its entries are safely on the watch list
 *  (`features/watchlistFold`). A whole-struct settings save cannot change the list. */
export async function retireWantedCalls(): Promise<void> {
  await invoke<void>('retire_wanted_calls')
}

/** Send the station the watch list — whole, each entry with its alert gates and label but never
 *  its note — so the Needed board puts its stations first, for this window and every Remote browser
 *  (operator, 2026-09-24: "watched counts as needed"), and the station raises its alerts. The main
 *  window sends it on launch and after every edit; `false` when refused (any other window). */
export async function setWatchList(
  entries: Pick<WatchFilter, 'id' | 'kind' | 'value' | 'cqOnly' | 'minSnr' | 'label'>[],
): Promise<boolean> {
  return invoke<boolean>('set_watch_list', { entries })
}

/** The satellite names LoTW accepts, for the tag picker. The backend owns the table, so what
 *  the operator can choose is exactly what the writer will store — a typed name is a permanent
 *  record of a guess, and TQSL rejects one it does not list ("AO7" for "AO-7"). */
export async function lotwSatNames(): Promise<string[]> {
  return invoke<string[]>('lotw_sat_names')
}

/** One contact, named by its id and the edit key of the version the caller holds (SPEC-2 v2 §3):
 *  what a view that reads the log by id sends to change a contact. The backend hands out both —
 *  with a row it serves, or in a `RowAnswer` — and the UI never builds or parses either. */
export interface RowRef {
  id: string
  editKey: string
}

/** A contact as it now stands, with the key the next change to it must send. */
export interface KeyedRow {
  row: LoggedQso
  editKey: string
}

/** What a change by `RowRef` did. `applied`/`deleted` are made and on disk. `changed`, `gone` and
 *  `busy` changed nothing: the contact changed since the caller read it (`current` is it now — show
 *  it, and retry against its key), or no contact has that id, or the logbook kept changing through
 *  every attempt (`LogBusy`) — the contact is still the version the caller holds, so a retry goes
 *  against the same key. An upload stamp or a confirmation never makes a contact `changed`: the key
 *  covers only what an edit can write. */
export type RowAnswer =
  | { kind: 'applied'; current: KeyedRow }
  | { kind: 'deleted' }
  | { kind: 'changed'; current: KeyedRow }
  | { kind: 'gone' }
  | { kind: 'busy' }

/** The park half of a `QsoEdit`. The form's own rule holds: with both refs empty no park is written
 *  and the stored one is kept whole; a ref carries its programme (the stored one, or POTA). */
export interface OtaEdit {
  myProgram: string | null
  myRef: string | null
  theirProgram: string | null
  theirRef: string | null
  /** The worked station's IOTA reference ("NA-001"). Blank leaves the stored one, as the park
   *  boxes do. */
  iota: string | null
}

/** Exactly what the Logbook's edit form writes to one contact — tempo-core's `QsoEdit`, whose field
 *  list `features/__fixtures__/log-query/qso-edit-fields.json` pins on both sides. Every field is
 *  sent, `null` included. `timeOffUnix: null` leaves the stored end time alone. `qslSentVia` is the
 *  form's own vocabulary: `null` not sent, a method letter, or `'SENT'` for a mark with no method
 *  (which the form shows and never sends as a change). */
export interface QsoEdit {
  call: string
  grid: string | null
  state: string | null
  band: string
  freqMhz: number
  mode: string
  rstSent: string | null
  rstRcvd: string | null
  name: string | null
  qth: string | null
  comment: string | null
  notes: string | null
  txPower: number | null
  whenUnix: number
  timeOffUnix: number | null
  ota: OtaEdit
  myGrid: string | null
  myRig: string | null
  qslSentVia: 'B' | 'D' | 'E' | 'SENT' | null
  qslCard: boolean
}

/** The Logbook form's edit of the contact `target`, as ONE change: the fields, and the QSL-sent
 *  and paper-card marks where the form changed them — one write, where the form used to send three
 *  commands. Confirmation, credit and upload state are kept as they are; a callsign correction
 *  re-queues the uploads (the backend's one edit policy). Returns once it is on disk. */
export async function editQsoById(target: RowRef, edit: QsoEdit, fields?: FieldEdit[]): Promise<RowAnswer> {
  return invoke<RowAnswer>('edit_qso_by_id', fields?.length ? { target, edit, fields } : { target, edit })
}

/** One change the Logbook's contact card makes to a field the log keeps as it was written (`extra`:
 *  a county, a zone, a tag another program wrote) — tempo-core's `FieldEdit`. `old` is the value the
 *  card showed (null: the contact had none); the change is refused as `changed` when the contact no
 *  longer holds it. `new: null` removes the field. Sent beside the edit, in the same change. */
export interface FieldEdit {
  tag: string
  old: string | null
  new: string | null
}

/** One field a change to a contact changed (ADIF tag; `null` where the contact did not carry it). */
export interface QsoFieldChange { tag: string; old: string | null; new: string | null }

/** One entry of a contact's change history. `source` is a token (`operator`, `import`, `fill`,
 *  `lotw`, `eqsl`, `qrz`, `pota`, `contest`, `remote`, `file`, `undo`), never prose. */
export interface QsoHistoryEntry {
  seq: number
  /** `unknown`: a kind a later build recorded — shown, never undone. */
  kind: 'create' | 'edit' | 'delete' | 'merge' | 'fill' | 'unknown'
  source: string
  run: string
  atMs: number
  undoes: number | null
  undoneBy: number | null
  changes: QsoFieldChange[]
}

/** A contact deleted from the log and not put back. */
export interface DeletedQso {
  id: string
  call: string
  band: string
  mode: string
  whenUnix: number
  deletedAtMs: number
  source: string
}

/** What an undo made. `run` is the undo's own run: undoing it is the redo. The counts are the
 *  contacts it could not change, by why: `changedSince` includes a contact an import added that
 *  was edited or confirmed since (it stays), `duplicate` one the log holds already under another
 *  id (it is not put back twice). The calls name those contacts, sorted, each once. `kept`: the
 *  fields a later change moved; `held`: the fields it left because putting them back would send a
 *  contact under a call it was not made under (a blank station call). */
export interface UndoAnswer {
  changed: number
  run: string | null
  kept: string[]
  held: string[]
  /** The call in Settings a held blank station call would have sent the contacts under; `null`
   *  when Settings names none. */
  heldUnder: string | null
  gone: number
  alreadyThere: number
  changedSince: number
  duplicate: number
  unknownKind: number
  changedSinceCalls: string[]
  duplicateCalls: string[]
  /** The contacts whose callsign it changed with nothing known of where the call it replaced was
   *  sent (the session that corrected it has ended): marked not uploaded to every service. */
  owedCalls: string[]
  /** The contacts whose callsign it changed while the call it replaced has gone, or may have
   *  gone, to the services named (their own names): marked not uploaded there. A correction no
   *  service was sent is not here — its undo cancelled the re-send and put the upload marks back. */
  owedAt: { call: string; services: string[] }[]
}

/** One contact's change history, oldest first. */
export async function qsoHistory(id: string): Promise<QsoHistoryEntry[]> {
  return invoke<QsoHistoryEntry[]>('qso_history', { id })
}

/** The contacts deleted and not put back, newest first. */
export async function recentlyDeletedQsos(limit?: number): Promise<DeletedQso[]> {
  return invoke<DeletedQso[]>('recently_deleted_qsos', { limit: limit ?? null })
}

/** Undo one change to a contact. Sends nothing to any service. Returns once it is on disk. */
export async function undoQsoChange(seq: number): Promise<UndoAnswer> {
  return invoke<UndoAnswer>('undo_qso_change', { seq })
}

/** Undo every change of one run, as one transaction. Sends nothing to any service. Only at the
 *  station: a computer driving it is refused, as a purge is. */
export async function undoHistoryRun(run: string): Promise<UndoAnswer> {
  return invoke<UndoAnswer>('undo_history_run', { run })
}

/** Undo a run that changed one contact: the Redo of one change's undo or of one restore. Asked
 *  from afar as the undo is; a run over more contacts is refused (`undoHistoryRun`'s). */
export async function redoQsoChange(run: string): Promise<UndoAnswer> {
  return invoke<UndoAnswer>('redo_qso_change', { run })
}

/** A purge, an import, the one-time contest-column copy or a whole run's undo whose whole-run undo
 *  can still be asked for. */
export interface HistoryRun {
  /** What `undoHistoryRun` takes. */
  run: string
  /** `contestColumns`: merged contest contacts' received grid, name and QTH copied into their own
   *  fields, once. `undo`: a whole run's undo, which undone is the redo. */
  kind: 'purge' | 'import' | 'contestColumns' | 'undo'
  atMs: number
  /** How many contacts it changed. */
  contacts: number
  /** Whether undoing it takes contacts out of the log (else it puts them back). */
  removes: boolean
}

/** The purges, imports and whole undos that can still be undone whole, newest first: the lasting
 *  way to undo one after its toast has gone, through `undoHistoryRun`. */
export async function undoableHistoryRuns(): Promise<HistoryRun[]> {
  return invoke<HistoryRun[]>('undoable_history_runs')
}

/** Put a deleted contact back, as its latest delete took it out — unless it was logged again since
 *  (`duplicate`). `anyway`: put it back all the same, beside the one logged since. */
export async function restoreDeletedQso(id: string, anyway = false): Promise<UndoAnswer> {
  return invoke<UndoAnswer>('restore_deleted_qso', { id, anyway })
}

// ── The Logbook's clean-up tools: duplicates and merge, bulk edit, the check ─────────────────
// Each change is ONE run of the log's history (`run`, undone whole by `undoHistoryRun`). None
// sends anything to a service, and none changes the worked call, so every confirmation stays. The
// one upload mark a tool clears is a bulk STATION_CALLSIGN change's: the next upload sends the
// contact under the new call. The changes are the station's own: a computer driving it is refused,
// as a purge is.

/** What a clean-up tool's change made. `run`: its undo; `null` when it changed nothing. The counts
 *  are what it left alone: gone since shown, changed since shown, a merge whose conflict must be
 *  chosen, a merge of copies that are two contacts. */
export interface ToolRun {
  changed: number
  run: string | null
  gone: number
  changedSince: number
  needsChoice: number
  distinct: number
}

/** A field two copies of a contact disagree on. `guarded`: must be chosen (a confirmed copy's
 *  value, the QSL-sent mark, the contest exchange, a comment or notes, an identity field).
 *  `chosen`: the copy the suggested merge takes. */
export interface DupeConflict {
  tag: string
  values: { id: string; value: string }[]
  guarded: boolean
  chosen: string
}

/** One contact logged more than once: its copies in log order, each with its revision. */
export interface DupeGroup {
  copies: LoggedQso[]
  revisions: string[]
  keep: number
  conflicts: DupeConflict[]
}

export interface DupeScan {
  scanned: number
  groups: DupeGroup[]
}

/** `rule`: the log's own duplicate rule (the same station, band and mode within 5 minutes);
 *  `possible`: within an hour. Changes nothing. */
export async function findLogDuplicates(tier: 'rule' | 'possible'): Promise<DupeScan> {
  return invoke<DupeScan>('find_log_duplicates', { tier })
}

/** One merge: the copy kept, every copy with the revision it was shown with, and for a
 *  conflicting field the copy whose value is taken. */
export interface MergeAsk {
  keep: string
  copies: { id: string; revision: string }[]
  choices: Record<string, string>
}

export async function mergeLogDuplicates(merges: MergeAsk[]): Promise<ToolRun> {
  return invoke<ToolRun>('merge_log_duplicates', { merges })
}

/** A field the clean-up tools set or clear, by its ADIF name. */
export type CleanupField =
  | 'MY_GRIDSQUARE' | 'MY_RIG' | 'OPERATOR' | 'STATION_CALLSIGN' | 'PROP_MODE' | 'SAT_NAME'
  | 'TX_PWR' | 'MY_POTA_REF' | 'MY_SOTA_REF'
  | 'GRIDSQUARE' | 'BAND' | 'DXCC' | 'IOTA' | 'POTA_REF' | 'SOTA_REF' | 'TIME_OFF'
  | AwardStatusField

/** An award's status on a contact, by the award's ADIF Credit code: set to 'SUBMITTED' or
 *  'GRANTED' (one clears the other, in CREDIT_SUBMITTED / CREDIT_GRANTED), cleared for neither.
 *  Only that code moves. */
export type AwardStatusField =
  | 'CREDIT_DXCC' | 'CREDIT_WAS' | 'CREDIT_CQWAZ_MIXED' | 'CREDIT_IOTA' | 'CREDIT_USACA'

/** One field set (`value`) or cleared (`null`). */
export interface FieldChange { field: CleanupField; value: string | null }

/** The contacts a bulk edit acts on: those ticked, or every one the Logbook's filter shows. */
export type BulkTarget = { ids: string[] } | { query: LogQuery }

/** What a bulk edit would make of its contacts. `ids`: the contacts previewed, which the apply is
 *  handed back. `recall`: of those it changes, the ones whose station call it replaces with another.
 *  `resend`: of those, the ones uploaded under the station call it changes. `unconfirm`: the LoTW,
 *  eQSL and QRZ confirmations that change of station call takes off. */
export interface BulkPreview {
  ids: string[]
  selected: number
  change: number
  unchanged: number
  missing: number
  recall: number
  resend: number
  unconfirm: number
  /** Those a filled or cleared station call sends to a service not yet sent them under another
   *  call: `moveTo`, instead of `moveFrom`. Their upload marks stay. */
  moves: number
  moveTo: string | null
  moveFrom: string[]
  /** Those it moves into or out of an activation (a park or summit of yours, or the call it was
   *  worked under); of those, the ones already sent to a service (their marks stay, nothing is sent
   *  again). Of every one it changes, moved or not, the ones in an activation file already exported
   *  — `activationFiles` names those files ("US-1234 2026-10-20 KD9TAW"), which no longer match the
   *  log. */
  activationChanges: number
  activationUploaded: number
  activationExported: number
  activationFiles: string[]
}

export async function bulkEditPreview(target: BulkTarget, changes: FieldChange[]): Promise<BulkPreview> {
  return invoke<BulkPreview>('bulk_edit_preview', { target, changes })
}

/** The change, on exactly the contacts a preview counted (`preview.ids`): refused, changing nothing,
 *  unless they are the `expectCount` the operator confirmed and the contacts it sends under another
 *  call are still the `expectMoves` under `expectMoveTo` the operator acknowledged. */
export async function bulkEditApply(
  ids: string[],
  expectCount: number,
  expectMoves: number,
  expectMoveTo: string | null,
  changes: FieldChange[],
): Promise<ToolRun> {
  return invoke<ToolRun>('bulk_edit_apply', { ids, expectCount, expectMoves, expectMoveTo, changes })
}

/** What "Update from callbook" would do with its contacts: the distinct calls, those the lookup
 *  cache answers (`kept`, no lookup), the rest at the pace (`estimateSecs`), the day's network
 *  lookups (`spent`) under the `cap`, and per ADIF field how many contacts the fill could give it —
 *  exact for a kept answer, at most for a call still to look up. Changes nothing, looks nothing
 *  up. */
export interface CallbookUpdatePreview {
  ids: string[]
  selected: number
  missing: number
  calls: number
  kept: number
  toLookUp: number
  /** What those may cost against the cap: a stroked call is asked again for its base. */
  lookupsNeeded: number
  paceSecs: number
  estimateSecs: number
  spent: number
  cap: number
  /** QRZ's own count of the account's lookups, from its last answer in the last day. */
  qrzCount: number | null
  couldChange: number
  fields: [string, number][]
  configured: boolean
  /** The services each call is asked of, in order, as set up (callook.info: US calls only). */
  services: ('qrz' | 'hamqth' | 'callook')[]
  /** Contacts older than 12 months left without a location field the answer would have given. */
  locationTooOld: number
}

/** How an update ended: `done`, `cancelled`, `limit` (the day's cap), `refused` (QRZ answering
 *  call after call with no record: refusing lookups) or `failed` (`error` says why). `made`: the
 *  fill's change, undone whole by its run — a fill stopped part-way has one too. `notLookedUp`:
 *  calls a later run picks up. */
export interface CallbookUpdateEnd {
  stop: 'done' | 'cancelled' | 'limit' | 'refused' | 'failed'
  error: string | null
  /** QRZ's last Message/Error, in its own words (shown, as QRZ asks). */
  qrzSaid: string | null
  /** QRZ was left out: the account has no XML subscription. */
  qrzSkipped: boolean
  unknown: string[]
  notLookedUp: number
  made: ToolRun | null
}

export interface CallbookUpdateStatus {
  running: boolean
  done: number
  total: number
  fromCache: number
  lookedUp: number
  spent: number
  cap: number
  /** The lookups are done, and the fill waits for the radio to stop transmitting. */
  waitingForTx: boolean
  /** QRZ's last Message/Error this run, in its own words. */
  qrzSaid: string | null
  /** QRZ is left out of this run: the account has no XML subscription. */
  qrzSkipped: boolean
  ended: CallbookUpdateEnd | null
}

export async function callbookUpdatePreview(target: BulkTarget): Promise<CallbookUpdatePreview> {
  return invoke<CallbookUpdatePreview>('callbook_update_preview', { target })
}

/** Start the update on exactly the contacts a preview counted; refused unless they are the
 *  `expectCount` the operator confirmed. Answers at once: the progress is `callbookUpdateStatus`. */
export async function callbookUpdateStart(ids: string[], expectCount: number): Promise<void> {
  return invoke<void>('callbook_update_start', { ids, expectCount })
}

export async function callbookUpdateStatus(): Promise<CallbookUpdateStatus> {
  return invoke<CallbookUpdateStatus>('callbook_update_status')
}

/** Stop after the lookup in hand: what was found is filled as one change. */
export async function callbookUpdateCancel(): Promise<void> {
  return invoke<void>('callbook_update_cancel')
}

/** The last update's end, if it has not been reported yet: answered once. */
export async function callbookUpdateTakeEnd(): Promise<CallbookUpdateEnd | null> {
  return invoke<CallbookUpdateEnd | null>('callbook_update_take_end')
}

/** One problem the check found. `fix`: the proposed change (`value: null` clears the field), or
 *  `null` when none can be proposed. */
export interface LogProblem {
  id: string
  call: string
  whenUnix: number
  kind: 'grid' | 'myGrid' | 'dxcc' | 'noBand' | 'bandFreq' | 'future' | 'endBeforeStart' | 'iota' | 'pota' | 'sota'
  field: CleanupField | null
  current: string | null
  fix: { value: string | null } | null
}

export async function checkLogbook(): Promise<{ checked: number; problems: LogProblem[] }> {
  return invoke('check_logbook')
}

/** One ticked fix, with what the check found in its field: left out when that changed since. */
export interface LogFix { id: string; field: CleanupField; expect: string | null; value: string | null }

export async function applyLogFixes(fixes: LogFix[]): Promise<ToolRun> {
  return invoke<ToolRun>('apply_log_fixes', { fixes })
}

/** A contact logged with no grid, and the grid Nexus knows for its station: `heard`, the one it
 *  sent on the air; `callbook`, its home QTH from a callbook lookup already made (for the distance
 *  and the map only). `uploaded`: a service already has the contact. */
export interface LogGridOffer {
  id: string
  call: string
  whenUnix: number
  band: string
  mode: string
  grid: string
  source: 'heard' | 'callbook'
  uploaded: boolean
}

/** The blank-grid contacts with a grid known (`blank`: how many contacts have no grid). Changes nothing. */
export async function findLogGridOffers(): Promise<{ blank: number; offers: LogGridOffer[] }> {
  return invoke('log_grid_offers')
}

/** Fill the ticked contacts' grids, as ONE change; a contact given a grid since is left out. */
export async function fillLogGrids(
  picks: Array<Pick<LogGridOffer, 'id' | 'grid' | 'source'>>,
): Promise<ToolRun> {
  return invoke<ToolRun>('log_grid_fill', { picks })
}

/** Mark the contact `target` QSL-sent (operator-declared): a card or request went out `via`
 *  "B"(ureau) / "D"(irect) / "E"(lectronic), dated now. A request is NOT a confirmation — this never
 *  moves `confirmed`/`awardConfirmed`. `via: null` WITHDRAWS the mark (#180): sending is once-only,
 *  so without it a mis-click on a send entry was permanent. */
export async function markQslSentById(target: RowRef, via: 'B' | 'D' | 'E' | null): Promise<RowAnswer> {
  return invoke<RowAnswer>('mark_qsl_sent_by_id', { target, via })
}

/** Record whether a PAPER QSL card arrived for the contact `target` (#152). The operator is the
 *  only source — LoTW/eQSL/QRZ report their own confirmations, nothing knows a card landed — and it
 *  is award-eligible, so it moves the awards view. Clearable, for a mis-tick. */
export async function markQslCardById(target: RowRef, received: boolean): Promise<RowAnswer> {
  return invoke<RowAnswer>('mark_qsl_card_by_id', { target, received })
}

/** Put the contact `target` in the cards-to-send queue (ADIF `QSL_SENT=Q`), or take it out. Marking
 *  the card sent (`markQslSentById`) takes it out as well. */
export async function queueQslCardById(target: RowRef, queued: boolean): Promise<RowAnswer> {
  return invoke<RowAnswer>('queue_qsl_card_by_id', { target, queued })
}

/** The contacts whose paper QSL card is still to go out (ADIF `QSL_SENT` `Q` or `R`, not marked
 *  sent), in log order, each with the key a change to it must send. */
export async function qslCardQueue(): Promise<KeyedRow[]> {
  return invoke<KeyedRow[]>('qsl_card_queue')
}

/** Open a sheet of QSL labels (a whole HTML page) in this computer's browser, to print. */
export async function openQslLabels(html: string): Promise<void> {
  await invoke('open_qsl_labels', { html })
}

/** Set — or REMOVE — the satellite tag on `target` (ADIF `PROP_MODE=SAT` + `SAT_NAME`).
 *
 *  ⭐ Its own command, and NOT two boxes on the edit form. That form reads a blank field as
 *  LEAVE ALONE, which is what stops a busted-call fix silently stripping a satellite tag off a
 *  contact that earned it; add the boxes and a blank one would mean "leave it" and "clear it"
 *  at once. Removal has to be an act the operator chooses, like withdrawing a QSL-sent mark.
 *
 *  `satName: null` REMOVES the tag — both fields together, because TQSL validates them as a
 *  pair and a lone member is rejected. Any other value must be a name LoTW accepts (see
 *  `lotwSatNames`); an empty string is an error, never a removal. */
export async function setSatTagById(target: RowRef, satName: string | null): Promise<RowAnswer> {
  return invoke<RowAnswer>('set_sat_tag_by_id', { target, satName })
}

/** Set — or CLEAR — by hand the DXCC entity the contact `target` counts toward (ADIF `DXCC`):
 *  `dxcc` is the entity's number (`getDxccEntities`), and `null` clears it so the contact counts
 *  by its call again. LoTW's number for the contact, where the log keeps one, still counts over
 *  it. Its own command for the satellite tag's reason: a clear has to be an act the operator
 *  chooses, never a blank box on the edit form. */
export async function setDxccById(target: RowRef, dxcc: number | null): Promise<RowAnswer> {
  return invoke<RowAnswer>('set_dxcc_by_id', { target, dxcc })
}

/** Delete the contact `target`. */
export async function deleteQsoById(target: RowRef): Promise<RowAnswer> {
  return invoke<RowAnswer>('delete_qso_by_id', { target })
}

/** What a purge answers: how many contacts it removed, and its run in the log's history — what
 *  its Undo undoes, and nothing else. */
export interface PurgeAnswer { removed: number; run: string }

/** Purge the ENTIRE logbook — delete every contact and truncate the ADIF file. The UI gates this
 * behind a typed confirmation; every contact is kept in the history for the toast's Undo. */
export async function purgeLog(): Promise<PurgeAnswer> {
  return invoke<PurgeAnswer>('purge_log')
}

/** DXCC-first award progress computed from the logbook (cty.dat-resolved). */
export async function getAwards(): Promise<AwardSummary> {
  return invoke<AwardSummary>('get_awards')
}

/** The whole-log DXCC chart: every current entity × every DXCC band, by mode class. */
export async function getDxccChart(): Promise<import('./types').DxccChart> {
  return invoke('get_dxcc_chart')
}

/** An award whose status the log keeps. */
export type AwardKind = 'dxcc' | 'was' | 'waz' | 'iota' | 'usaca'

/** A contact to submit for an entity of an award. `entity`: a DXCC entity's name, a state's code,
 *  a zone's number, an island group's reference, a county as `STATE,Name`. */
export interface ReadyRow {
  entity: string
  id: string
  call: string
  whenUnix: number
  band: string
  mode: string
  /** The channels that confirm it for this award: 'card', 'lotw', 'eqsl'. */
  confirmedBy: string[]
}

/** One award's status over the log, in entities: worked, confirmed, submitted (not yet granted),
 *  granted, and one contact per entity confirmed and neither submitted nor granted. */
export interface AwardStatus {
  award: AwardKind
  creditCode: string
  worked: number
  confirmed: number
  submitted: number
  granted: number
  ready: ReadyRow[]
}

/** One award's status: each ready entity's newest confirmed contact where `newest`, else its
 *  oldest. Read-only. */
export async function getAwardStatus(award: AwardKind, newest: boolean): Promise<AwardStatus> {
  return invoke<AwardStatus>('get_award_status', { award, newest })
}

/** What to QSL: unconfirmed contacts whose confirmation fills a slot an award still needs,
 *  most valuable first. Read-only. */
export async function getWhatToQsl(): Promise<import('./types').QslWant[]> {
  return invoke('get_what_to_qsl')
}

/** The operator's custom awards and their standing in the log. */
export async function getCustomAwards(): Promise<import('./types').CustomAwardsView> {
  return invoke('get_custom_awards')
}

/** Replace the custom award definitions; answers them as kept (normalised, each with an id). */
export async function setCustomAwards(
  awards: import('./types').CustomAward[],
): Promise<import('./types').CustomAward[]> {
  return invoke('set_custom_awards', { awards })
}

/** The definitions a custom awards file holds, checked — not yet kept. */
export async function readCustomAwardsFile(text: string): Promise<import('./types').CustomAward[]> {
  return invoke('read_custom_awards_file', { text })
}

/** The station's custom award definitions as a file another station imports. */
export async function exportCustomAwards(): Promise<string> {
  return invoke('export_custom_awards')
}

/** The Stats pane: what the log holds for `call`, typed in a log entry on `band` in `mode` —
 *  counted by the award fold itself, in the station. */
export async function getCallStats(call: string, band: string, mode: string): Promise<import('./types').CallStats> {
  return invoke('get_call_stats', { call, band, mode })
}

/** Geographic log stats — continent / CQ-zone / DX-vs-domestic (cty.dat-resolved per call). */
export async function getLogStats(): Promise<GeoLogStats> {
  return invoke<GeoLogStats>('get_log_stats')
}

/** The Statistics view's timeline: contacts per mode over the past day, 30 days or year (UTC). */
export async function getLogTimeline(range: TimelineRange): Promise<LogTimeline> {
  return invoke<LogTimeline>('get_log_timeline', { range })
}

/** The Journey snapshot — the in-app, beginner-first achievement layer (firsts,
 * sub-award ladders, collections, feats, personal bests, XP/level, streak). */
export async function getJourney(): Promise<JourneySummary> {
  return invoke<JourneySummary>('get_journey')
}

export async function getConfirmationDiagnostics(): Promise<DiagnosticsReport> {
  return invoke<DiagnosticsReport>('get_confirmation_diagnostics')
}

/** Import an external ADIF logbook (deduped merge → real "needs" + B4). */
export async function importAdif(text: string): Promise<ImportStats> {
  return invoke<ImportStats>('import_adif', { text })
}

/** What importing a log file (ADIF, ADX or Cabrillo) would do, before anything is written. */
export async function previewImport(text: string): Promise<ImportPreview> {
  return invoke<ImportPreview>('preview_import', { text })
}

/** Import a log file (ADIF, ADX or Cabrillo) as one undoable run; `newOnly` leaves every contact the
 *  log already holds as it is. Answers with the no-loss report. */
export async function importLog(text: string, newOnly: boolean): Promise<ImportDone> {
  return invoke<ImportDone>('import_log', { text, newOnly })
}

/** Reconcile a LoTW (or any ADIF) confirmation report INTO the log: upgrade
 * confirmation + credit on already-logged QSOs, return the diff + orphans. */
export async function syncLotwReport(text: string): Promise<LotwSyncResult> {
  return invoke<LotwSyncResult>('sync_lotw_report', { text })
}

/** Store the LoTW website password in the OS keychain (write-only; an empty
 *  string clears it). */
export async function setLotwPassword(password: string): Promise<void> {
  await invoke<void>('set_lotw_password', { password })
}

/** Remove the stored LoTW password from the OS keychain (idempotent). */
export async function clearLotwPassword(): Promise<void> {
  await invoke<void>('clear_lotw_password')
}

/** Store a radio profile's Icom network password in the OS keychain (write-only; an empty string
 *  clears it). The radio takes 16 printable characters at most. */
export async function setIcomLanPassword(radioId: number, password: string): Promise<void> {
  await invoke<void>('set_icom_lan_password', { radioId, password })
}

/** Remove a radio profile's Icom network password from the OS keychain (idempotent). */
export async function clearIcomLanPassword(radioId: number): Promise<void> {
  await invoke<void>('clear_icom_lan_password', { radioId })
}

/** Whether a radio profile has an Icom network password saved (never the password). */
export async function icomLanPasswordSaved(radioId: number): Promise<boolean> {
  return invoke<boolean>('icom_lan_password_saved', { radioId })
}

/** Sync your LoTW state into the log: pull new confirmations AND mark which of your
 *  uploads LoTW now holds on file (own-echo → Pending becomes Accepted). Uses the
 *  stored username + keychain password. */
export async function downloadLotwReport(): Promise<LotwSyncResult> {
  return invoke<LotwSyncResult>('download_lotw_report')
}

/** Empty the LoTW sync cursor, so the next `downloadLotwReport` asks LoTW for the whole
 *  confirmation history once (Settings' Download everything again). Touches no contact or
 *  confirmation. */
export async function resetLotwCursor(): Promise<void> {
  await invoke<void>('reset_lotw_cursor')
}

/** Sign + upload QSOs to LoTW via the operator's installed TQSL. `indices` =
 *  specific log rows, or omit for the default unsent-unconfirmed batch. */
export async function uploadLotwReport(indices?: number[]): Promise<UploadReport> {
  return invoke<UploadReport>('upload_lotw_report', { indices: indices ?? null })
}

/** `uploadLotwReport` for the contacts these ids name (SPEC-2 v2 §3) — ids a view was handed with
 *  the rows, never positions. Refused if an id is not one the backend handed out. */
export async function uploadLotwReportByIds(ids: string[]): Promise<UploadReport> {
  return invoke<UploadReport>('upload_lotw_report', { indices: null, ids })
}

/** Ask the engine one of the UI's log questions (SPEC-2 v3 C17a): a page of the Logbook, where a
 *  row sits in it, one call's history, an entity's slots, a roster's summary, the band map's
 *  worked calls, the squares and bands worked, the statistics, the LoTW backlog. The answer is
 *  what `answerFrom` gives over the whole log; a page carries the query, `revision`, `orderRev`
 *  (moves with the order, not on an upload stamp) and `contentRev`. This is the transport
 *  `createAskingLogSource` takes. Rejects with the engine's reason when it cannot answer.
 *
 *  The statistics arrive COUNTED and are ordered here (`finishLogStats`): their ties are broken
 *  by `localeCompare`, in this webview's locale, which the engine cannot reproduce. */
export async function askLog<Q extends LogQuestion>(q: Q): Promise<AnswerTo<Q>> {
  if (q.kind === 'statistics') {
    return finishLogStats(await invoke<LogStatCounts>('ask_log', { q })) as AnswerTo<Q>
  }
  return invoke<AnswerTo<Q>>('ask_log', { q })
}

/** Mark every currently-unsent QSO as already on LoTW (for an imported legacy log uploaded
 *  through another tool). Returns how many were marked. */
export async function markLotwUploaded(): Promise<number> {
  return invoke<number>('mark_lotw_uploaded')
}

/** Store the eQSL password in the OS keychain (write-only; empty clears it). */
export async function setEqslPassword(password: string): Promise<void> {
  await invoke<void>('set_eqsl_password', { password })
}

/** Remove the stored eQSL password from the OS keychain (idempotent). */
export async function clearEqslPassword(): Promise<void> {
  await invoke<void>('clear_eqsl_password')
}

/** Download new eQSL confirmations and reconcile them into the log (uses the
 *  stored username + keychain password). */
export async function downloadEqslReport(): Promise<LotwSyncResult> {
  return invoke<LotwSyncResult>('download_eqsl_report')
}

/** Two-way QRZ sync: FETCH the online QRZ logbook and merge it into the local log —
 *  pulls new QSOs (logged elsewhere) plus confirmations (uses the stored Logbook API key). */
export async function syncQrz(): Promise<LotwSyncResult> {
  return invoke<LotwSyncResult>('sync_qrz')
}

/** Store the QRZ password in the OS keychain (write-only; empty clears it). */
export async function setQrzPassword(password: string): Promise<void> {
  await invoke<void>('set_qrz_password', { password })
}

/** Remove the stored QRZ password from the OS keychain (idempotent). */
export async function clearQrzPassword(): Promise<void> {
  await invoke<void>('clear_qrz_password')
}

/** Store the HamQTH password in the OS keychain (write-only; empty clears it).
 *  HamQTH is the free fallback used when QRZ isn't configured or has no match. */
export async function setHamqthPassword(password: string): Promise<void> {
  await invoke<void>('set_hamqth_password', { password })
}

/** Remove the stored HamQTH password from the OS keychain (idempotent). */
export async function clearHamqthPassword(): Promise<void> {
  await invoke<void>('clear_hamqth_password')
}

/** Look up a callsign on QRZ.com (uses the stored username + keychain password;
 *  session key cached server-side in memory). The answer is kept for the window
 *  (`features/callbookAnswers`), so the station card shows it whoever asked. */
export async function qrzLookup(callsign: string): Promise<QrzLookup> {
  const r = await invoke<QrzLookup>('qrz_lookup', { callsign })
  noteCallbookAnswer(callsign, r)
  return r
}

/** Look a callsign up again, skipping the callbook cache, and keep the new answer. */
export async function callbookRefresh(callsign: string): Promise<QrzLookup> {
  const r = await invoke<QrzLookup>('callbook_refresh', { callsign })
  noteCallbookAnswer(callsign, r)
  return r
}

/** How many calls the on-disk callbook cache holds. */
export async function callbookCacheCount(): Promise<number> {
  return invoke<number>('callbook_cache_count')
}

/** Forget every kept callbook answer. No contact is touched. */
export async function callbookCacheClear(): Promise<void> {
  await invoke<void>('callbook_cache_clear')
}

/** Store the QRZ Logbook API key in the OS keychain (write-only; empty clears). */
export async function setQrzLogbookKey(key: string): Promise<void> {
  await invoke<void>('set_qrz_logbook_key', { key })
}

/** Remove the stored QRZ Logbook API key from the OS keychain (idempotent). */
export async function clearQrzLogbookKey(): Promise<void> {
  await invoke<void>('clear_qrz_logbook_key')
}

/** Validate the QRZ Logbook API key with a real STATUS round-trip (no insert).
 * Resolves to a human summary ("KD9TAW (My Logbook) — 1234 QSOs…") or rejects
 * with the failure reason. */
export async function qrzTestConnection(): Promise<string> {
  return invoke<string>('qrz_test_connection', {})
}

/** Push one logged QSO to the operator's QRZ logbook. */
export async function qrzPushQso(record: LoggedQso): Promise<QrzPushResult> {
  return invoke<QrzPushResult>('qrz_push_qso', { record })
}

/** What the operator is asked to approve before ONE contact is corrected at QRZ. The
 *  `confirmation` text is composed in Rust and shown verbatim — it names the callsign, the date
 *  and every field that will change. */
export type QrzCorrectPreview = { call: string; date: string; confirmation: string }

/** What happened when a correction was sent. `ok` is true ONLY for QRZ's `RESULT=REPLACE`;
 *  a `RESULT=OK` means QRZ inserted a second copy instead of overwriting, which is a failure,
 *  and `canRecover` says whether Nexus can delete exactly the record QRZ just added. */
export type QrzCorrectResult = { ok: boolean; message: string; canRecover: boolean }

/** Read QRZ's own copy of ONE contact and plan its correction. Writes nothing; rejects with the
 *  refusal sentence when the correction will not be attempted. */
export async function qrzCorrectPreview(
  record: LoggedQso,
  selected: number,
): Promise<QrzCorrectPreview> {
  return invoke<QrzCorrectPreview>('qrz_correct_preview', { record, selected })
}

/** Send the correction for ONE contact (`OPTION=REPLACE`). Re-reads QRZ's copy first. */
export async function qrzCorrectApply(
  record: LoggedQso,
  selected: number,
): Promise<QrzCorrectResult> {
  return invoke<QrzCorrectResult>('qrz_correct_apply', { record, selected })
}

/** Delete the duplicate a missed replace created — the only delete Nexus can send to QRZ, and
 *  only for the record QRZ itself reported adding during this session. Permanent; no undo. */
export async function qrzCorrectUndoMiss(): Promise<string> {
  return invoke<string>('qrz_correct_undo_miss')
}

/** Store the ClubLog Application Password in the OS keychain (write-only; empty
 *  clears). */
export async function setClublogPassword(password: string): Promise<void> {
  await invoke<void>('set_clublog_password', { password })
}

/** Remove the stored ClubLog app-password from the OS keychain (idempotent). */
export async function clearClublogPassword(): Promise<void> {
  await invoke<void>('clear_clublog_password')
}

/** Push one logged QSO to ClubLog (realtime). */
export async function clublogPushQso(record: LoggedQso): Promise<ClubLogPushResult> {
  return invoke<ClubLogPushResult>('clublog_push_qso', { record })
}

/** Upload one logged QSO to eQSL.cc (ImportADIF). */
export async function eqslPushQso(record: LoggedQso): Promise<UploadReport> {
  return invoke<UploadReport>('eqsl_push_qso', { record })
}

/** Store the HRDLog.net upload code in the OS keychain (write-only; empty clears). */
export async function setHrdlogCode(code: string): Promise<void> {
  await invoke<void>('set_hrdlog_code', { code })
}

/** Save the World Radio League API key. Validates against the live service and
 *  resolves the destination logbook BEFORE saving — rejects with a plain message on
 *  a bad key. Write-only: the key is never read back. */
export async function setWrlKey(key: string): Promise<void> {
  await invoke<void>('set_wrl_key', { key })
}

export async function clearWrlKey(): Promise<void> {
  await invoke<void>('clear_wrl_key')
}

/** Push one logged QSO to World Radio League. */
export async function wrlPushQso(q: LoggedQso): Promise<{ result: string; message?: string }> {
  return invoke('wrl_push_qso', { record: q })
}

/** Remove the stored HRDLog.net upload code from the OS keychain (idempotent). */
export async function clearHrdlogCode(): Promise<void> {
  await invoke<void>('clear_hrdlog_code')
}

/** Push one logged QSO to HRDLog.net (NewEntry.aspx). Not an ARRL confirmation
 *  source — this never earns DXCC/WAS credit. */
export async function hrdlogPushQso(
  record: LoggedQso,
): Promise<import('./types').HrdLogPushResult> {
  return invoke<import('./types').HrdLogPushResult>('hrdlog_push_qso', { record })
}

/** Store the Cloudlog/Wavelog instance API key in the OS keychain (write-only; empty clears). */
export async function setCloudlogKey(key: string): Promise<void> {
  await invoke<void>('set_cloudlog_key', { key })
}

/** Remove the stored Cloudlog/Wavelog API key from the OS keychain (idempotent). */
export async function clearCloudlogKey(): Promise<void> {
  await invoke<void>('clear_cloudlog_key')
}

/** Need-aware spotting: the stations heard now, ranked by award value. */
export async function getNeedAlerts(): Promise<NeedAlert[]> {
  return invoke<NeedAlert[]>('get_need_alerts')
}

/** Raw spot firehose for the Spots panel — every recent spot (all modes/sources),
 * newest first, NOT needs-gated. The panel filters client-side. */
export async function getAllSpots(): Promise<import('./types').SpotRow[]> {
  return invoke<import('./types').SpotRow[]>('get_all_spots')
}

/** The spots programs on Nexus's TCI server sent ("Share by TCI"), for the band map: each a spot
 *  row whose `tci` names the program. Kept apart from the cluster's (`getAllSpots`), and asked only
 *  when `radio.tciServer.spots` says they changed (`features/tciSpots.ts`). */
export async function getTciSpots(): Promise<import('./types').SpotRow[]> {
  return invoke<import('./types').SpotRow[]>('get_tci_spots')
}

/** A click on a program's spot on the band map: the station tunes there and tells every program
 *  on its TCI server, and does nothing else (never a section change, an arm, a QSO). Rejects with
 *  the station's reason, refused while anything is on the air. */
export async function clickTciSpot(call: string, freqMhz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('click_tci_spot', { call, freqMhz })
}

/** Check the release feed (hamradiotools.io, SourceForge as fallback) for a newer Nexus
 * release (Phase 1 update check). Rejects offline / on a
 * fetch error — callers treat that as a silent no-op. */
export async function checkForUpdate(): Promise<import('./types').UpdateInfo> {
  return invoke<import('./types').UpdateInfo>('check_for_update')
}

/** Open the SourceForge download page in the operator's default browser. */
export async function openDownloadPage(): Promise<void> {
  return invoke('open_download_page')
}

/** How this install takes an update (Rust `update_route`), decided by the package it was
 * installed from. Ask it only where the updater plugin's API exists: a page without one (the
 * Remote page, a plain browser) has nothing to ask. */
export interface UpdateRoute {
  /** True for the packages the signed self-updater can replace: the Windows setup, the AppImage
   *  and the macOS app. False for the .deb packages and a build run straight from cargo, which
   *  keep the notice that opens the download page. */
  selfUpdate: boolean
  /** The download page, for the way out when a self-update fails. */
  downloadPage: string
}

export async function updateRoute(): Promise<UpdateRoute> {
  return invoke<UpdateRoute>('update_route')
}

/** Liveness of the background live feeds (cluster/RBN + PSK Reporter MQTT) for the
 *  Now-Bar connector pills. */
export async function getFeedHealth(): Promise<FeedHealth> {
  return invoke<FeedHealth>('get_feed_health')
}

/** Each human DX-cluster node's standing, for Settings ▸ Spot Sources. Desktop only: nothing on
 *  the Remote page reads it, and the Remote transport does not carry it. */
export async function getClusterNodes(): Promise<import('./types').ClusterNodes> {
  return invoke('get_cluster_nodes')
}

/** Export the general logbook as ADIF or CSV text. Optional `from`/`to` are UTC
 *  "YYYY-MM-DD" dates bounding the QSO time inclusively (#98); empty/absent = all. */
export async function exportGeneralLog(
  format: 'adif' | 'csv',
  from?: string,
  to?: string,
): Promise<LogExport> {
  return invoke<LogExport>('export_general_log', { format, from: from || null, to: to || null })
}

/** Export the contacts selected in the Logbook as ADIF, by their ids (`LoggedQso.id`), each record
 *  as the whole-log export writes it. */
export async function exportSelectedLog(ids: string[]): Promise<LogExport> {
  return invoke<LogExport>('export_selected_log', { ids })
}

/** The absolute path where the ALL.TXT decode log is written (to show in Settings). */
export async function allTxtLocation(): Promise<string> {
  return invoke<string>('all_txt_location')
}

/** Where `nexus-diag.log` lives — the file we ask an operator to send when something goes
 * wrong. Always written, no toggle. */
export async function diagLogLocation(): Promise<string> {
  return invoke<string>('diag_log_location')
}

/** Reveal `nexus-diag.log` in the file manager (falls back to opening its folder). */
export async function revealDiagLog(): Promise<void> {
  return invoke<void>('reveal_diag_log')
}

/** The absolute folder where per-QSO recordings land (to show in Settings). Per-PROFILE: a second
 * radio records under its own config dir, which is the whole of why "it is not writing the file"
 * gets reported when nothing has failed. */
export async function recordingsLocation(): Promise<string> {
  return invoke<string>('recordings_location')
}

/** The app version string (e.g. "0.15.8") from tauri.conf.json — shown under the wordmark. */
export async function appVersion(): Promise<string> {
  return invoke<string>('app_version')
}

/** Why installing a downloaded update is refused right now, or null when it is allowed.
 * Asked at the moment of the press, never cached — the radio can go busy between a poll and a
 * click, and installing restarts the app. */
export async function updateInstallBlock(): Promise<string | null> {
  return invoke<string | null>('update_install_block')
}

/** Flush the conversations, the Field Day log, the open propagation episodes and the window
 * geometry to disk before a self-update hands off to the installer. Called immediately BEFORE
 * the plugin's `install()`, because on Windows that call ends the process outright
 * (`ShellExecuteW` then `exit(0)`) and the ordinary quit cleanup never runs. On Windows it
 * first waits for the logbook to be saved — showing the saving line and, if the save cannot
 * finish, asking the operator (see `LogbookSaving.tsx`) — so it resolves when the logbook is on
 * disk or the operator chose to go on without the rest. A no-op on macOS/Linux, where
 * `restartApp()` takes the normal exit path. */
export async function prepareUpdateInstall(): Promise<void> {
  return invoke<void>('prepare_update_install')
}

/** Restart Nexus after a self-update install — through the backend's ordinary quit cleanup
 * (TX unkey, journal flushes, window geometry), never a hard kill. The backend stops the radio,
 * then saves the logbook with the window still up, then restarts, so this resolves before the
 * restart happens. The updater plugin's `install()` restarts nothing on macOS/Linux, so this
 * call is what makes "Nexus will restart…" true there; on Windows the installer already exited
 * the process before `install()` resolves, so this is never reached. */
export async function restartApp(): Promise<void> {
  return invoke<void>('restart_app')
}

/** The operator's answer when a quit could not save the logbook on its own: keep waiting for
 * the changes still on their way to disk, or quit without them. The backend waits for exactly
 * one answer per question and ignores a second. */
export async function logbookSaveChoice(keepTrying: boolean): Promise<void> {
  return invoke<void>('logbook_save_choice', { keepTrying })
}

/** A newer BETA build the opt-in channel found, or null when up to date / offline. */
export interface BetaUpdateInfo {
  version: string
  notes: string | null
}

/** Check the opt-in BETA channel for a newer build. Resolves the newest release (pre-releases
 * included) from the GitHub API, points the updater at its manifest, and stashes it for
 * `installBetaUpdate()` if it's newer than the running build. Returns null when up to date, and
 * throws on a fetch error (the caller treats that silently, like the stable check). Call only when
 * the operator has turned beta updates on. */
export async function checkBetaUpdate(): Promise<BetaUpdateInfo | null> {
  return invoke<BetaUpdateInfo | null>('check_beta_update')
}

/** Download and install the beta build the last `checkBetaUpdate()` stashed. Call the SAME guards
 * the stable install uses first — `updateInstallBlock()` then `prepareUpdateInstall()` — and, on
 * macOS/Linux, `restartApp()` after it resolves (on Windows the installer exits the process here).
 * No progress is reported; the banner shows an indeterminate installing state. */
export async function installBetaUpdate(): Promise<void> {
  return invoke<void>('install_beta_update')
}

/** One selectable radio in the launch picker. */
export interface RadioLaunchOption {
  id: number
  name: string
  /** Already open in another window → the picker greys it out. */
  inUse: boolean
}
/** Whether to show the "which radio?" launch picker, and the radios to offer. */
export interface RadioLaunchInfo {
  showPicker: boolean
  radios: RadioLaunchOption[]
}
/** Ask the backend whether this window should show the radio picker (simultaneous-radios on,
 *  ≥2 radios, launched without a profile). Single-radio stations get showPicker=false. */
export async function radioLaunchInfo(): Promise<RadioLaunchInfo> {
  return invoke<RadioLaunchInfo>('radio_launch_info')
}
/** Operator picked a radio: the backend relaunches this window bound to that radio and exits. */
export async function chooseRadio(radioId: number): Promise<void> {
  await invoke('choose_radio', { radioId })
}

/** Picker escape: turn simultaneous-radios off (in the base config the picker reads) and proceed
 *  as the single band-following window — the pre-picker behavior. */
export async function useSingleRadio(): Promise<void> {
  await invoke('use_single_radio')
}

/** Reveal ALL.TXT (or its folder) in the OS file manager. */
/** Stamp POTA/SOTA park refs from a pota.app export onto matching logged QSOs. */
export interface PotaStampResult {
  stamped: number
  already: number
  unmatched: number
}
export async function importPotaLog(text: string): Promise<PotaStampResult> {
  return invoke<PotaStampResult>('import_pota_log', { text })
}

/** The native title bar's theme on Windows: the operator's pick, or null for System. */
export async function setTitleBarTheme(theme: 'light' | 'dark' | null): Promise<void> {
  await invoke('set_title_bar_theme', { theme })
}

/** Open a station's QRZ.com profile in the system browser (roster/logbook affordance). */
export async function openQrzPage(call: string): Promise<void> {
  await invoke('open_qrz_page', { call })
}

/** Open a DXpedition's own webpage in the system browser, falling back to the
 * callsign's QRZ page when the calendar source published no site. The backend
 * re-validates the URL's scheme — it is third-party data. */
export async function openDxpedPage(call: string, url?: string | null): Promise<void> {
  await invoke('open_dxped_page', { call, url: url ?? null })
}

export async function revealAllTxt(): Promise<void> {
  await invoke('reveal_all_txt')
}

export async function revealRecordings(): Promise<void> {
  await invoke('reveal_recordings')
}

/** Where the station's data + log folder is (#289). */
export async function getDataFolder(): Promise<import('./types').DataFolderInfo> {
  return invoke<import('./types').DataFolderInfo>('get_data_folder')
}

/** Choose the data + log folder. `copy` carries the log and data across, verified, first; the
 *  original is never moved or deleted, and the choice applies at the next launch. */
export async function setDataFolder(path: string, copy: boolean): Promise<import('./types').DataCopyReport> {
  return invoke<import('./types').DataCopyReport>('set_data_folder', { path, copy })
}

/** Go back to the default data folder (clears the choice). Applies at the next launch. */
export async function clearDataFolder(): Promise<void> {
  await invoke('clear_data_folder')
}

/** Ask the OS for a folder, so nobody has to type a filesystem path (#289). `null` means the
 *  operator cancelled — not an error, and not a change: this only fills the field in, and
 *  `setDataFolder` is still what adopts the folder. Desktop only; call it behind `isTauri()`,
 *  because a browser has no filesystem to pick from. */
export async function pickDataFolder(): Promise<string | null> {
  return invoke<string | null>('pick_data_folder')
}

/** Open the SSTV gallery folder (Pictures/Nexus SSTV) in the OS file manager — #130. */
export async function revealSstvGallery(): Promise<void> {
  await invoke('reveal_sstv_gallery')
}

/** Delete one received SSTV image — the file AND its gallery entry, in one action so the two
 * cannot drift. Irreversible: the caller confirms first. */
export async function sstvDeleteImage(path: string): Promise<void> {
  await invoke('sstv_delete_image', { path })
}

/** Toggle Skip Tx1 — the station's session-only flag, resets each launch.
 *  Returns the snapshot, whose `radio.skipTx1` the box draws. */
export async function setSkipTx1(enabled: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_skip_tx1', { enabled })
}

/** Write text to the operator's Downloads folder; returns the full saved path. Reliable in a
 *  WebView2 window where a browser `<a download>` blob may silently fail. */
export async function saveTextToDownloads(filename: string, text: string): Promise<string> {
  return invoke<string>('save_text_to_downloads', { filename, text })
}

/** Save As: the OS save dialog, opening in Downloads with `filename` filled in, then the text
 *  written where the operator picked. The saved path, or null when the dialog was cancelled. */
export async function saveTextAs(filename: string, text: string): Promise<string | null> {
  return invoke<string | null>('save_text_as', { filename, text })
}

/** Binary sibling of saveTextToDownloads for the share-card PNG (base64-encoded bytes) —
 *  on macOS wry cancels `<a download>` navigations outright, so a blob anchor saves nothing. */
export async function savePngToDownloads(filename: string, base64: string): Promise<string> {
  return invoke<string>('save_png_to_downloads', { filename, base64 })
}

/** The station folders a computer driving it from afar may fetch from (features/shackFiles.ts). */
export type ShackFolder = 'recordings' | 'allTxt' | 'sstv'

export interface ShackFile {
  name: string
  bytes: number
  modifiedMs: number
}

/** The files in one of the station's three shared folders, newest first. */
export async function shackFileList(folder: ShackFolder): Promise<ShackFile[]> {
  return invoke<ShackFile[]>('shack_file_list', { folder })
}

/** One bounded piece of a station file from `offset` (base64), with the file's whole size. */
export async function shackFileRead(
  folder: ShackFolder,
  name: string,
  offset: number,
): Promise<{ data: string; offset: number; size: number }> {
  return invoke('shack_file_read', { folder, name, offset })
}

/** Where a saved piece went: the file's name in THIS computer's Downloads folder, which the later
 *  pieces of the same save name, and its full path. */
export interface ShackSaved {
  name: string
  path: string
}

/** One piece of a station file, written into THIS computer's Downloads folder at `offset`. The
 *  piece at offset 0 starts a new file, under the next free name (`name (1).ext`) when one of that
 *  name is there already; each later piece names the file it started. */
export async function shackFileSave(filename: string, base64: string, offset: number): Promise<ShackSaved> {
  return invoke<ShackSaved>('shack_file_save', { filename, base64, offset })
}

/** One received SSTV picture from the station, whole (base64), by its gallery path. */
export async function shackGalleryImage(path: string): Promise<string> {
  return invoke<string>('shack_gallery_image', { path })
}

/** Open an external http(s) link in the system browser. Backing for the app-wide
 *  `target="_blank"` anchor interceptor (externalLinks.ts) — raw `_blank` anchors are dead
 *  in the Tauri webview (the opener plugin's injected handler is ACL-denied). */
export async function openExternalUrl(url: string): Promise<void> {
  await invoke('open_external_url', { url })
}

/** Fire an OS notification through the Rust notification plugin — WKWebView has no web
 *  Notification API, so this is the only path that exists on macOS. Rejects when the OS
 *  refuses; the caller decides what a miss means (Pounce: nothing — sound is primary). */
export async function osNotify(title: string, body: string): Promise<void> {
  await invoke('os_notify', { title, body })
}

/**
 * Switch the top-level operating mode (and operator role). Returns the fresh
 * snapshot so callers can render the new mode immediately.
 */
export async function setMode(mode: ModeRequest): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_mode', { mode })
}

/**
 * Switch the link tier (TempoFast fast / TempoDeep robust). Returns the fresh snapshot so
 * the UI reflects the authoritative `link.tier` rather than local state.
 */
export async function setTier(tier: Tier): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tier', { tier })
}

/** Switch the RX signal source: 'native' (decode local audio) or 'companion'
 * (ride an upstream WSJT-X/JTDX/MSHV decode stream over UDP). */
export async function setSource(kind: SourceKind): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_source', { kind })
}

/** Fetch the band-plan channel presets (grouped HF / VHF / UHF). */
export async function getBandPlan(): Promise<BandChannel[]> {
  return invoke<BandChannel[]>('get_band_plan')
}

/**
 * Tune the rig: set the dial frequency (MHz), band label, and phone mode.
 * Returns the fresh snapshot so the readout reflects the authoritative state.
 */
export async function setFrequency(
  dialMhz: number,
  band: string,
  mode: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_frequency', { dialMhz, band, mode })
}

/** An SSTV strip pick / dial commit: claims the Phone section (SSTV rides Phone) and QSYs
 * atomically, so the previous section's mode policy can't keep commanding the rig at the new
 * dial (picking 20 m SSTV from RTTY used to land DATA-L at 14.230). */
export async function sstvTune(
  dialMhz: number,
  band: string,
  mode: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('sstv_tune', { dialMhz, band, mode })
}

/** Park the dial on one of the APP's OWN fixed channels (the ISS SSTV auto-arm's 145.800 FM).
 * An FM channel is held in FM, as the SSTV strip's pick of it is, and routed on FM, as a
 * repeater tune is (neither while a satellite pass owns the dial). The per-(band, mode) dial
 * memory never takes it — an app-driven auto-tune is not the operator's dial in that mode, so
 * it is never remembered, and the dial it displaces is banked. Restoring the operator's OWN
 * saved dial afterwards uses setFrequency, because that one IS theirs. */
export async function tuneChannel(
  dialMhz: number,
  band: string,
  mode: string,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('tune_channel', { dialMhz, band, mode })
}

/** Band pick by NAME — the CW/Phone cockpit band dropdowns. The engine lands on the last
 * dial you used on that band in that mode THIS SESSION, else the same licensed default the
 * dropdown always used. Every explicit-frequency path keeps calling setFrequency: a typed
 * MHz, a spot's frequency or a band-plan channel stays authoritative, with no memory
 * overlay. `mode` = the cockpit's mode (race-safe on entry, like getLicensedBandPlan);
 * omit for the engine's current operating mode. */
export async function pickBand(band: string, mode?: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('pick_band', { band, mode: mode ?? null })
}

/** Set the per-section operating mode (the rig-mode policy): "digital" obeys the rig,
 * "phone" forces USB/LSB by band, "cw" forces CW. `followFreq` = true when the operator
 * clicks an actual operating-section tab — then the rig QSYs to that mode's home frequency
 * on the current band (phone segment / CW segment / FT8 watering hole). Pass false for
 * incidental nav and the Needed click (which sets the spot's exact frequency itself).
 * "keyboard" and "jtty" are the one Keyboard section with PSK or JTTY as its modem, which
 * picks the band plan the section's entry lands on.
 * `keepDial`: the Phone tab pressed on the SSTV screen. With `followFreq` the engine keeps the
 * dial where Phone may transmit there and homes it where it may not; without, nothing homes.
 * Either way an FM channel the SSTV screen holds stays FM in Phone. Sent only when true, so every
 * other entry's wire is unchanged. */
export async function setOperatingMode(
  mode: 'digital' | 'phone' | 'cw' | 'rtty' | 'keyboard' | 'jtty',
  followFreq: boolean,
  keepDial = false,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>(
    'set_operating_mode',
    keepDial ? { mode, followFreq, keepDial } : { mode, followFreq },
  )
}

/** Work a spotted station (the Needed click): set the operating mode AND QSY to the spot's
 * exact frequency atomically — one round-trip, so the rig can't end up in the new mode at the
 * old dial and the UI never sees a half-applied state. `mode` is the section, and comes back as
 * the snapshot's `workView`: 'keyboard' is PSK's, and 'js8' is Digital named for the JS8 screen
 * (`WORK_SECTION`, features/needs.ts). */
export async function workSpot(
  mode: 'digital' | 'phone' | 'cw' | 'rtty' | 'keyboard' | 'js8' | 'jtty',
  freqMhz: number,
  band: string,
  call?: string,
  tier?: 'FT8' | 'FT4' | 'JS8',
  park?: { program: string; reference: string } | null,
): Promise<AppSnapshot> {
  // `call` lets the backend look up the spot's pile-up split ("UP 2") and
  // configure rig split automatically. `tier` is the digital
  // spot's protocol: passing it here makes the tier switch and the QSY ONE atomic
  // backend call — as two calls, the radio loop could command the tier's default
  // dial to the rig in the gap before the spot's exact frequency ("hitting a
  // default first, then switching" — operator report, 2026-08-09). `park` is a
  // Needed row's park, which rides the work hint (`workPark`) to the log line.
  return invoke<AppSnapshot>('work_spot', {
    mode,
    freqMhz,
    band,
    call: call ?? null,
    tier: tier ?? null,
    // Only when there is one: the hosted page's control transport refuses a work_spot carrying
    // any other key, and a browser's Work never carries a park.
    ...(park ? { park } : {}),
  })
}

/** Set (`txMhz`) or clear (`null`) manual rig split — the TX dial when working split
 * (e.g. "up 5"). `null` returns to simplex. The radio loop applies it to the rig. */
export async function setSplit(txMhz: number | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_split', { txMhz })
}

/** Set ('USB'|'LSB'|'FM') or clear (null = AUTO) the transient Phone mode override. The radio
 * loop applies it next cycle; a band change reverts to the band-auto sideband. */
export async function setSidebandOverride(mode: 'USB' | 'LSB' | 'FM' | 'AM' | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_sideband_override', { mode })
}

/** Set the rig RX filter / passband width (Hz); the radio loop applies it via set_mode. */
export async function setFilterWidth(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_filter_width', { hz })
}

/** Set the native Icom scope SPAN — the rig's real panadapter ± half-width in Hz. */
export async function setScopeSpan(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_scope_span', { hz })
}
/** Set the FT-710 scope POSITION — 'center' | 'cursor' | 'fix'.
 *
 * The position travels by name and the rig's display family (3DSS / W-F EXPAND / W-F NORMAL) is
 * resolved next to the radio from what it reports, so centring the sweep never drags a 3DSS
 * operator out of 3DSS. */
export async function setYaesuScopeMode(position: 'center' | 'cursor' | 'fix'): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_yaesu_scope_mode', { position })
}
/** Set the native Icom scope REFERENCE level in tenths of a dB (−200..+200). */
export async function setScopeRef(tenthsDb: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_scope_ref', { tenthsDb })
}
/** Set the FlexRadio native-panadapter BANDWIDTH (span) in Hz. */
export async function setFlexPanSpan(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_flex_pan_span', { hz })
}
/** Set the FlexRadio panadapter REFERENCE level (dBm); null = auto. */
export async function setFlexPanRef(refDbm: number | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_flex_pan_ref', { refDbm })
}
/** Set the native Icom scope center/fixed mode (true = fixed band-edge view). */
export async function setScopeFixed(fixed: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_scope_fixed', { fixed })
}

/** Set the RIT (receive incremental tuning) offset in Hz — 0 turns RIT off. */
export async function setRit(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_rit', { hz })
}
/** Set the XIT (transmit incremental tuning) offset in Hz — 0 turns XIT off. */
export async function setXit(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_xit', { hz })
}
/** Select the active VFO ("A" / "B"). */
export async function setVfo(vfo: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_vfo', { vfo })
}
/** Swap the active VFO (A↔B). */
export async function swapVfo(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('swap_vfo')
}

/** Toggle a rig CAT function on/off; the radio loop applies it. The returned snapshot
 * reflects the request optimistically (the loop's read-back reconciles). */
export async function setRigFunc(
  // 'notch' is the AUTOMATIC notch (ANF); 'manualNotch' is the one you place (MN). Two
  // different rig functions — see engine.rs func_index, where they are indices 2 and 5.
  // 'monitor' is the TRANSMIT monitor (MON) — your own audio played back while you talk,
  // not a receive-side DSP toggle like the rest. Its volume is `setMonitorGain`.
  func: 'nb' | 'nr' | 'notch' | 'comp' | 'vox' | 'manualNotch' | 'monitor',
  on: boolean,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_rig_func', { func, on })
}

/** Queue CW to transmit (CAT keyer). `text` is an F-key macro template or literal
 * type-ahead — the engine expands the tokens and the rig keys it. `reply`: the message answers a
 * station, so CW speed match may key it at that station's speed (`cwSendsAnswer`). */
export async function sendCw(text: string, reply = false): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('send_cw', { text, reply })
}

/** Queue CW for Enter Sends Message. Unlike `sendCw` it NEVER turns TX on: it rejects, with the
 *  reason, while TX is off or any transmit gate refuses, and then nothing was taken. */
export async function sendCwArmed(text: string, reply = false): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('send_cw_armed', { text, reply })
}

/** CW speed match on or off: the contest strip's switch, saved alone (never a settings-form save).
 *  It changes only the speed an answer to a station keys at. */
export async function setCwSpeedMatch(on: boolean): Promise<void> {
  await invoke('set_cw_speed_match', { on })
}

/** The ESM switch in a cockpit's TX dock, saved alone (never a settings-form save). */
export async function setContestEsm(cockpit: 'cw' | 'rtty' | 'phone', on: boolean): Promise<void> {
  await invoke('set_contest_esm', { cockpit, on })
}

/** Record the worked station's QRZ name + state for the {HISNAME}/{HISSTATE} CW-macro tokens,
 *  keyed to `call` (pass an empty call to clear). Fire-and-forget. */
export async function setCwPeerInfo(call: string, name: string, peerState: string): Promise<void> {
  await invoke('set_cw_peer_info', { call, name, peerState })
}

/** Set the CW keyer speed in WPM (5–50). */
/** Set CW sending speed. `commit` persists it to settings — pass false for the continuous
 * paths (slider drag, decoder auto speed-match) and true once the operator settles, so a
 * drag can't fsync the settings file ~45 times. */
export async function setCwWpm(wpm: number, commit = false): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_cw_wpm', { wpm, commit })
}

/** Set FT8/FT4 decode depth (1=Fast, 2=Normal, 3=Deep) live from the Operate cockpit; persists. */
export async function setDecodeDepth(depth: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_decode_depth', { depth })
}

/** Abort CW in progress (Esc) — stops the rig keyer + clears the queue. */
export async function stopCw(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('stop_cw')
}

/** Choose the CW keyer back-end ("cat" = rig send_morse / "soundcard" = keyed tone)
 * and tone pitch in Hz (<=0 keeps the current pitch). */
export async function setCwKeyer(
  backend: 'cat' | 'soundcard' | 'winkeyer' | 'serial',
  pitch = 0,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_cw_keyer', { backend, pitch })
}

/** Manual PTT for live phone — key (true) / unkey (false) the rig. */
export async function setPtt(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_ptt', { on })
}

/** Phone's Lock, re-asserted from a computer driving a station: one hold of its held press on the
 *  station's Stop road (`src-tauri/src/client_mode.rs`), `stillTalking` the operator's answer to
 *  "Still talking?". Resolves whether that is asked, and how long is left before the Lock's cap;
 *  fails once the press has ended (`lockCapped` at the cap). */
export async function holdPttLock(stillTalking: boolean): Promise<{ ask: boolean; leftMs?: number }> {
  return invoke<{ ask: boolean; leftMs?: number }>('hold_ptt_lock', { stillTalking })
}

/** A Phone PTT press made through the Remote stream's picture: ARMS the streamed operator's
 *  microphone over, which only their audio keys. Resolves whether an over is armed. */
export async function armStreamMic(): Promise<boolean> {
  return invoke<boolean>('arm_stream_mic')
}

/** Let go of the microphone over a streamed press armed. */
export async function releaseStreamMic(): Promise<void> {
  await invoke<void>('release_stream_mic')
}

/** Set RF output power as a 0.0–1.0 fraction. */
export async function setRfPower(power: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_rf_power', { power })
}

/** Set mic gain as a 0.0–1.0 fraction. */
export async function setMicGain(gain: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_mic_gain', { gain })
}
/** Set the noise-reduction level as a 0.0–1.0 fraction. */
export async function setNrLevel(level: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_nr_level', { level })
}
/** Pick the radio's SSB transmit-bandwidth step by its name in `radio.txBwSteps`. Refused while
 *  transmitting, and for a step this radio does not offer; the radio's read-back is the truth. */
export async function setTxBw(step: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tx_bw', { step })
}
/** Set the speech-processor depth as a 0.0–1.0 fraction (#95). */
export async function setCompLevel(level: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_comp_level', { level })
}

/** Set the MANUAL-notch frequency in HZ (#95). The engine clamps it to the audio passband. */
export async function setNotchFreq(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_notch_freq', { hz })
}

/** Set AF gain (the rig's volume) as a 0.0–1.0 fraction.
 *
 * ⚠️ On a station whose soundcard is fed from the rig's speaker or headphone jack this is
 * also the DECODER's audio level — zero here stops FT8/RTTY/PSK, not just the speaker. */
export async function setAfGain(gain: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_af_gain', { gain })
}

/** Set RF gain (RECEIVE front-end gain, not transmit power) as a 0.0–1.0 fraction. */
export async function setRfGain(gain: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_rf_gain', { gain })
}

/** Set a level on the SUB receiver of a dual-receiver radio — RF gain, AF gain or squelch, as
 * a 0.0–1.0 fraction. The radio loop sends it at receive time to the Sub alone (`L Sub …`),
 * never to Main.
 *
 * REJECTS, with the reason, where the Sub cannot be reached: no Sub offered for this radio, a
 * stage no vendor statement credits to it, or a connection that cannot name it. On the Remote
 * page this same call becomes one `radio.subLevel` station intent (capability
 * `subReceiverLevels`), refused for the same reasons by the same engine verb. */
export async function setSubLevel(level: 'rf' | 'af' | 'sql', value: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_sub_level', { level, value })
}

/** Ask for a change to one of a FlexRadio's slices through Nexus's own Flex client — a slice of
 * ours that is not the transmit slice (the transmit slice follows the radio's dial). `index` is
 * the radio's slice number (0 is slice A). The radio's own report shows the result in a later
 * snapshot's `radio.receivers.set`.
 *
 * REJECTS, with the reason, where it could not or must not reach the slice: no Flex client
 * serving this radio, no such slice, another program's slice, the transmit slice, or a value a
 * slice cannot take. */
export async function setSlice(index: number, intent: SliceIntent): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_slice', { index, intent })
}

/** Set the TRANSMIT-MONITOR gain (0.0–1.0) — how loud the rig plays your own audio back
 * while you are talking. Its on/off half is `setRigFunc('monitor', …)`.
 *
 * ⚠️ NOT `setAfGain`: the monitor is heard only while transmitting, so unlike AF gain it
 * can never silence the audio a decoder is listening to. */
export async function setMonitorGain(gain: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_monitor_gain', { gain })
}

/** Set the ATTENUATOR to a whole number of dB (0 = off).
 *
 * ⚠️ `db` MUST be one of `radio.attStepsDb` — the pads this radio declares. Anything else
 * REJECTS (the promise rejects with a message naming the step), because the rig would NAK
 * it or silently substitute a neighbour and attenuate by an amount nobody chose. Render the
 * control from `attStepsDb`, and render none at all when it is absent. */
export async function setAttDb(db: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_att_db', { db })
}

/** Set the PREAMP by its dB LABEL (0 = off). Same list rule and same rejection as
 * `setAttDb`; the labels come from `radio.preampStepsDb`. On an Icom they are `1`/`2`
 * (P.AMP1/P.AMP2) rather than decibels, so never treat one as a gain figure. */
export async function setPreampDb(db: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_preamp_db', { db })
}

/** Set the squelch threshold as a 0.0–1.0 fraction. */
export async function setSquelch(level: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_squelch', { level })
}

/** Set the AGC speed — `Engine::AGC_SPEEDS`, in the order the cockpits show them. */
export async function setAgc(speed: 'auto' | 'fast' | 'mid' | 'slow' | 'off'): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_agc', { speed })
}

// --- phone voice keyer ---
/** Play the recorded WAV bound to a voice-keyer slot (PTT + audio via the radio loop). */
export async function playVoiceMessage(slot: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('play_voice_message', { slot })
}
/** Abort voice playback in progress (Esc). */
export async function stopVoice(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('stop_voice')
}
/** Begin recording a voice message from the input device. */
export async function startVoiceRecording(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('start_voice_recording')
}
/** Cancel an in-progress recording, discarding the captured audio (e.g. on unmount). */
export async function cancelVoiceRecording(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('cancel_voice_recording')
}
/** Stop recording, save the slot's WAV, and return the updated message list. */
export async function stopVoiceRecording(slot: number, label: string): Promise<VoiceMessage[]> {
  return invoke<VoiceMessage[]>('stop_voice_recording', { slot, label })
}
/** Import a `.wav` (raw bytes) into a slot, normalized to 12 kHz mono. */
export async function importVoiceMessage(
  slot: number,
  label: string,
  bytes: number[],
): Promise<VoiceMessage[]> {
  return invoke<VoiceMessage[]>('import_voice_message', { slot, label, bytes })
}
/** Rename a voice-keyer slot's label. */
export async function setVoiceLabel(slot: number, label: string): Promise<VoiceMessage[]> {
  return invoke<VoiceMessage[]>('set_voice_label', { slot, label })
}
/** Clear the recording bound to a slot (keeps the label). */
export async function clearVoiceMessage(slot: number): Promise<VoiceMessage[]> {
  return invoke<VoiceMessage[]>('clear_voice_message', { slot })
}
/** The configured voice-keyer message slots. */
export async function getVoiceMessages(): Promise<VoiceMessage[]> {
  return invoke<VoiceMessage[]>('get_voice_messages')
}

// --- license class + licensed band plan ---
/** Set the operator's amateur license class (novice/technician/general/advanced/extra/open). */
export async function setLicenseClass(licenseClass: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_license_class', { class: licenseClass })
}
/** Bands the operator may use in `mode` ('phone' | 'cw' | 'digital'), parked at the licensed
 * segment start. Mode is passed explicitly (not read from the engine) to avoid a mount race. */
export async function getLicensedBandPlan(mode: string): Promise<BandChannel[]> {
  return invoke<BandChannel[]>('get_licensed_band_plan', { mode })
}
/** The licence-class band edges a scope tints: where the station's class may transmit the section
 * `mode`'s emission ('digital' | 'phone' | 'cw' | 'rtty' | 'keyboard', the snapshot's
 * `radio.operatingMode`), from the transmit gate's own table. Display only. */
export async function getPrivilegeSpans(mode: string): Promise<PrivilegeSpans | null> {
  return invoke<PrivilegeSpans>('get_privilege_spans', { mode })
}

/** Every current DXCC entity name (sorted) — the full table behind the country-hide
 *  picker's "Other country…" search (F4MQS). */
export async function getDxccEntityNames(): Promise<string[]> {
  return invoke<string[]>('dxcc_entity_names')
}

/** Every DXCC entity a contact can be set to count toward, current and deleted, in number
 *  order — the Logbook's country picker. The station owns the table, so the picker offers
 *  exactly the numbers `setDxccById` accepts. */
export async function getDxccEntities(): Promise<DxccEntityOption[]> {
  return invoke<DxccEntityOption[]>('dxcc_entities')
}

/** Every cty.dat entity name with its continent code (`[entity, 'EU']`), for band activity's
 *  hide-by-continent (#229) — a decode row carries the entity name, never the continent. */
export async function getDxccEntityContinents(): Promise<[string, string][]> {
  return invoke<[string, string][]>('dxcc_entity_continents')
}

/** #226: one station location offered by the Cloudlog/Wavelog picker. */
export interface CloudlogStation {
  stationId: string
  profileName: string
  callsign: string
  gridsquare: string
  active: boolean
}

/** #226: ask the operator's OWN Cloudlog/Wavelog for its station locations, so Settings can fill
 *  the station profile id. ⛔ The request carries the API key — call this ONLY from an explicit
 *  operator press, never on mount and never on a timer. */
export async function getCloudlogStations(): Promise<CloudlogStation[]> {
  return invoke<CloudlogStation[]>('cloudlog_station_info')
}

/** Every entity's cty.dat representative location as `[name, lat, lon]` — the azimuth
 *  fallback for a station that never sent a grid. Keyed by the same `country`/`entity`
 *  string a decode/spot row carries. WAE/CQ-only entities are included: `resolve()`
 *  returns those as a country too. */
export async function getDxccEntityLocations(): Promise<[string, number, number][]> {
  return invoke<[string, number, number][]>('dxcc_entity_locations')
}

// --- QSO recording (audio bridge) ---
/** Start streaming the live RX audio to a timestamped WAV on disk. */
export async function startQsoRecording(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('start_qso_recording')
}
/** Stop the in-progress QSO recording (finalizes the WAV). */
export async function stopQsoRecording(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('stop_qso_recording')
}

/** Enumerate available audio input + output devices. */
export async function getAudioDevices(): Promise<AudioDevices> {
  return invoke<AudioDevices>('get_audio_devices')
}

/** What the OS can say about this display's physical density — the input to the first-launch
 *  UI-scale seed. `physicalDpi` is null wherever the platform already sizes CSS pixels
 *  correctly (Windows, macOS) or the panel reports no physical size; that is a "change
 *  nothing" answer, not a failure. See `display_metrics` in src-tauri and `dpiSeedCap`. */
export interface DisplayMetrics {
  physicalDpi: number | null
  scaleFactor: number
}

export async function getDisplayMetrics(): Promise<DisplayMetrics> {
  return invoke<DisplayMetrics>('display_metrics')
}

/**
 * Enable / disable transmit (the Monitor toggle). Enabling also clears a tripped
 * TX watchdog. Returns the fresh snapshot.
 */
export async function setTxEnabled(enabled: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tx_enabled', { enabled })
}

/** Set the TX audio drive level (0.0–1.0) — the "Pwr" slider. Returns the snapshot. */
export async function setTxLevel(level: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tx_level', { level })
}

/** Set the RX capture gain (≥1.0 multiplier on received audio before decode). Returns the snapshot. */
export async function setRxGain(gain: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_rx_gain', { gain })
}

/** Switch the active radio (dual-radio). The rig loop swaps rigs on the next tick (carrier
 * dropped first); Mode/TX-queues are untouched. Returns the fresh snapshot. */
export async function setActiveRadio(id: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_active_radio', { id })
}

/** Peg-lock the active radio (dual-radio): band selection won't auto-switch. Returns the snapshot. */
export async function setPegLock(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_peg_lock', { on })
}

/** Add a radio to the roster (dual-radio). Distinct daemon ports auto-assigned; active unchanged. */
export async function addRadio(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('add_radio')
}

/** Remove a radio from the roster (no-op on the active or last radio). Returns the snapshot. */
export async function removeRadio(id: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('remove_radio', { id })
}

/** Rename a radio profile (its switcher label). Returns the snapshot. */
export async function renameRadio(id: number, name: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('rename_radio', { id, name })
}

/** Disable (`enabled: false`) or enable a radio. A disabled radio keeps its profile and Nexus lets go
 * of its port. Disabling the operating radio is refused. Returns the snapshot. */
export async function setRadioEnabled(id: number, enabled: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_radio_enabled', { id, enabled })
}

/** Set a radio's band-coverage set (empty = covers everything) for auto-routing. Returns snapshot. */
export async function setRadioBands(id: number, bands: string[]): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_radio_bands', { id, bands })
}

/** Replace the band+mode routing rules (list order IS the first-match-wins precedence). A live verb,
 * not part of the settings form — like the roster, so a stale-form Save can't revert it. */
export async function setRoutingRules(rules: RoutingRule[]): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_routing_rules', { rules })
}

/** Set (or clear, with `null`) the fallback radio for band+modes no rule and no coverage claims. */
export async function setDefaultRadio(id: number | null): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_default_radio', { id })
}

/** Whether a radio switch Nexus makes by itself is said on screen — the ONE writer of
 * `radioSwitchPolicy` (a settings save keeps the live value, as for `setBetaUpdates`). */
export async function setRadioSwitchPolicy(policy: RadioSwitchPolicy): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_radio_switch_policy', { policy })
}

/** The first-run wizard's experience answer (also Settings ▸ Station) — the ONE writer of
 * `experienceLevel` (a settings save keeps the live value, as for `setRadioSwitchPolicy`). */
export async function setExperienceLevel(level: ExperienceLevel): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_experience_level', { level })
}

/** The first-run setup wizard was finished or skipped (`Settings.setupWizardSeen`'s one writer):
 *  recorded beside the station's settings, so it never opens by itself again. */
export async function markSetupWizardSeen(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('mark_setup_wizard_seen')
}

/** Where a (band, mode class) WOULD route right now — the rule editor's "test" affordance.
 * Read-only: it never moves a rig. */
export async function routePreview(
  band: string,
  mode: RouteMode,
): Promise<{ radio: number; name: string }> {
  return invoke<{ radio: number; name: string }>('route_preview', { band, mode })
}

/** The editable CAT/audio/PTT/rotator/native subset of a radio profile — the per-radio Settings
 * page edits ONE radio with this, without making it active. Field names match the flat rig form. */
export interface RadioProfilePatch {
  pttMethod: string
  rigModel: number
  rigModelName: string
  serialPort: string
  /** Dedicated RTS/DTR PTT port for THIS radio; empty = key on the CAT port. An SO2R box
   * (U2R/MK2R) gives each radio its own keying port, so this must travel per-radio — it was
   * flat-only until 2026-07-25 and PTT did not follow a radio switch. */
  pttSerialPort: string
  baud: number
  /** This radio's RATED OUTPUT in watts — full scale for the Phone analog meter's PO arc.
   *  Per-radio: a station with a QRP set and a 100 W rig has two different meters.
   *  ⚠️ NOT the power cap, which is a FRACTION of this and rides on `Settings`. */
  ratedWatts: number
  rigConn: string
  rigAddr: string
  /** Which OmniRig slot this radio drives when rigConn === "omnirig" (1 = RIG 1, 2 = RIG 2).
   * On the patch because the per-radio Edit flow saves through it — a per-radio field missing
   * here is silently dropped on Save (the 2026-08-17 Flex-three data loss, exactly). */
  omnirigSlot: number
  rigctldPort: number
  icomNativeCat: boolean
  dataModesPlainSsb: boolean
  /** Hold the FM DATA submode (FM-D / PKTFM) for as long as the SSTV receiver is running,
   * instead of only around a send (#130). Per radio, so it must ride the patch — a per-radio
   * field missing here is silently dropped on Save (the 2026-08-17 Flex-three data loss). */
  sstvHoldDataSubmode: boolean
  /** APRS in this radio's DATA-FM mode instead of plain FM (#393). Per radio, off by default;
   * obeyed only on a radio that has one (`APRS_DATA_FM_MODELS`). Rides the patch like its
   * neighbours — a per-radio field missing here is blanked on Save. */
  aprsDataFm: boolean
  audioIn: string
  audioOut: string
  txLevel: number
  rxGain: number
  rotatorModel: number
  rotatorPort: string
  rotatorBaud: number
  rotatorHost: string
  rotctldPort: number
  nativeScope: string
  /** Which Icom DATA submode THIS radio uses. Rust carries a `#[serde(default = "one")]`, so
   * omitting it here did not fail the save — it silently RESET the operator's choice to DATA1
   * on every edit of the rig form. A default on the backend hides drift instead of catching it,
   * which is why the guard below now reads this interface directly. */
  icomDataMode: number
  /** "Transmit audio source (CAT PTT)" for THIS radio (#381): 'front' keys as every release has
   * (`T 1`), 'rear' keys the DATA input (`T 3`). Rust defaults it for an old settings file but NOT
   * on the patch, so a form that dropped it would fail the Save loudly rather than quietly put a
   * SignaLink radio back on Front/Mic. */
  txAudioSource: string
  /** THIS radio's amplifier, per-radio because the amp is wired to a radio, not to the station.
   * Absent here these had no serde default, so the patch did not silently drop them — it failed
   * to deserialize at all and took the whole Save with it. */
  ampModel: string
  ampPort: string
  ampFollowBand: boolean
  /** THIS radio's FlexRadio LAN IP (SmartSDR API, port 4992) for the native panadapter/DAX
   * workers. Per-radio since 2026-08-18: it was flat-only, so the per-radio Edit flow — which
   * saves through THIS patch — silently dropped it, and two Flexes could not both be configured
   * (2026-08-17 Flex audit). */
  flexRadioIp: string
  /** This radio's native-panadapter opt-in (per-radio, as above). */
  flexNativePan: boolean
  /** See RadioProfile.yaesuRfScope — the FT-710's USB-SPI spectrum, per radio. */
  yaesuRfScope: boolean
  /** See RadioProfile.yaesuFixStarts — the operator's own FIX-window start per band, kept
   *  because the derived band-edge start CAN be wrong and the radio reports neither the
   *  change nor the new start. OPTIONAL and currently never sent: nothing writes it yet, and
   *  omitting it means "leave alone" (the Rust side is an `Option`). It is declared here
   *  because the Rust patch carries it, and the guard that compares the two sides is right to
   *  demand they agree — a field on one side only is how a save silently resets a value. */
  yaesuFixStarts?: Record<string, number>
  /** This radio's native-DAX-audio opt-in (per-radio, as above). */
  flexNativeAudio: boolean
  /** This radio's opt-in to Nexus's own Flex client as its CAT (Beta; per-radio, as above). */
  flexNativeCat: boolean
  /** This radio's Beta setting that lets the Flex client send CW, Tune and ATU before a bench has
   *  confirmed them (off by default; per-radio, as above). */
  flexNativeUnbenched: boolean
  /** This radio's address, network user and control port for the Icom network connection (Beta;
   *  per-radio, as above). Its password is never here: the OS keychain holds it. */
  icomLanHost: string
  icomLanUser: string
  icomLanPort: number
  /** This radio's TCI connection (the "TCI" Connection choice): the SDR program's server address
   *  and port, which of its transceivers (from 0), its dialect ('auto' | 'expertsdr3' | 'thetis' |
   *  'pihpsdr'), Nexus's audio over it (both ways), its spectrum and its spots. Rust takes each as
   *  optional and leaves a stored value alone when absent (a form from before them); this form
   *  states every one. */
  tciHost: string
  tciPort: number
  tciTrx: number
  tciDialect: string
  tciAudio: boolean
  tciPan: boolean
  tciSpots: boolean
}

/** Edit one radio's CAT/audio/PTT/rotator/native config IN PLACE without changing the active radio
 * (no live rig swap / dropped carrier). Returns the updated snapshot. */
export async function updateRadioProfile(
  id: number,
  patch: RadioProfilePatch,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('update_radio_profile', { id, patch })
}

/** Key / unkey a tune carrier. Returns the fresh snapshot. */
export async function setTune(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tune', { on })
}

/** Run the radio's OWN built-in antenna tuner. Rejects with the reason when the rig has no ATU or
 * a TX gate is down (TX off, outside privileges, transmitter busy) — it keys the transmitter. */
export async function atuTune(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('atu_tune')
}

/** Switch the radio's built-in tuner OUT of line — the click on ATU ON (#344). Keys nothing;
 * rejects with the reason while anything is on the air or when the tuner is not in line. */
export async function atuBypass(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('atu_bypass')
}

/** Emergency stop: halt any transmit immediately. Returns the fresh snapshot. */
export async function haltTx(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('halt_tx')
}

/** Repair clock: run the repair the clock check found, once, through Windows' administrator
 *  prompt. Resolves `true` when it took and `false` when it did not (the prompt was declined, or
 *  a step failed). Rejects with `onAir` while anything is transmitting, `midMessage` while a JS8
 *  or Tempo message of several overs is part-way through, `repairRunning` while a repair is
 *  already running, and `nothingToRepair` when none is on offer. */
export async function repairClock(): Promise<boolean> {
  return invoke<boolean>('repair_clock')
}

/** Dismiss one transmitter alarm: exactly the one with this id, never one raised since. */
export async function dismissTxAlarm(id: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('dismiss_tx_alarm', { id })
}

/**
 * Test the rig/CAT connection (WSJT-X-style). The radio loop (re)opens + probes
 * the rig from the current settings; this returns whether it connected and a
 * detail line (read frequency, or a specific error). Save settings first.
 */
export async function testCat(): Promise<CatTestResult> {
  return invoke<CatTestResult>('test_cat')
}

/** Auto-test which serial port drives the rig: probes each USB port read-only and
 * returns the working (port, baud, model) to auto-select, or found=false. */
export async function probeCatPorts(radioId?: number): Promise<CatProbeResult> {
  return invoke<CatProbeResult>('probe_cat_ports', { radioId })
}

/** Point the antenna rotator at an absolute azimuth (degrees) via rotctld. An az/el rotator's
 *  elevation stays where it is, unless `elDeg` gives the one to go with it (the Rotor pane, while
 *  an elevation it set is still on its way). Without it the call is exactly `{ azDeg }`, the only
 *  shape the Remote transport takes. */
export async function pointRotator(azDeg: number, elDeg?: number): Promise<void> {
  return invoke('point_rotator', elDeg === undefined ? { azDeg } : { azDeg, elDeg })
}

/** Point an az/el rotator at an elevation (degrees, inside the range its backend declares). The
 *  azimuth stays where it is, unless `azDeg` gives the one to go with it. Desktop only. */
export async function pointRotatorElevation(elDeg: number, azDeg?: number): Promise<void> {
  return invoke('point_rotator_elevation', azDeg === undefined ? { elDeg } : { elDeg, azDeg })
}

/** Point the rotator at a callsign's station: its own grid or callbook position when Nexus
 *  knows one, else the centre of its country. Resolves to the bearing and what it was taken to
 *  (a browser gets nothing back: the station resolves it). `longPath` takes the reciprocal — the
 *  same great circle the other way. */
export async function pointRotatorAtCall(call: string, longPath = false): Promise<PointedAt> {
  return invoke<PointedAt>('point_rotator_at_call', { call, longPath })
}

/** The bearing `pointRotatorAtCall(call)` turns the antenna to, and the distance to the same point,
 *  without turning it: the station's own resolver, read only (the rotor box's line 2). Refuses with
 *  `noGrid` or `unknownStation` when there is no bearing to give. The desktop's alone: a browser is
 *  never offered it. */
export async function rotatorBearingToCall(call: string): Promise<CallBearing> {
  return invoke<CallBearing>('rotator_bearing_to_call', { call })
}

/** The grid the log form holds for the call it is logging — typed, or filled in from the
 *  callbook — so pointing the rotator at that call aims at the station. `''` forgets it for
 *  that call. Fire-and-forget, like `setCwPeerInfo`. */
export async function setLogFormGrid(call: string, grid: string): Promise<void> {
  await invoke('set_log_form_grid', { call, grid })
}

/** Current rotator azimuth (degrees), or null if rotctld is unset/unreachable. */
/** Scan ~3s for FlexRadio LAN discovery broadcasts (the Find-my-Flex button). */
export async function discoverFlex(): Promise<{ model: string; nickname: string; ip: string }[]> {
  return invoke('discover_flex')
}

/** What one port answered when Find TCI server tried it: an SDR program's TCI server
 *  (`found`), Nexus's own TCI server (`nexus`, skipped), nothing (`noAnswer`), something that is
 *  not a TCI server (`notTci`), or a WebSocket server that never finished the TCI handshake
 *  (`noReady`). */
export type TciPortOutcome = 'found' | 'nexus' | 'noAnswer' | 'notTci' | 'noReady'

/** The SDR program's TCI server Find TCI server found: what it said about itself before `ready;`,
 *  as sent. `dialect` is how Nexus will read it: 'expertsdr3', 'expertsdr2', 'thetis', or
 *  'unknown' for a program this build does not know. */
export interface TciServerFound {
  port: number
  program: string | null
  version: string | null
  device: string | null
  trxCount: number | null
  dialect: string
}

/** Find TCI server's answer for one address. `resolved` is false when this computer cannot find
 *  the address at all (nothing was tried). */
export interface TciProbeResult {
  host: string
  resolved: boolean
  tried: { port: number; outcome: TciPortOutcome }[]
  found: TciServerFound | null
}

/** Look for an SDR program's TCI server at `host`: `port` first, then 50001 and 40001, the ports
 *  the programs come set to. It reads each server's handshake and closes, writing nothing to it: it
 *  never keys and never changes the radio. A few seconds at most. At the shack only. */
export async function tciProbe(host: string, port: number): Promise<TciProbeResult> {
  return invoke('tci_probe', { host, port })
}

/** "Share by TCI": the operator's Disconnect for program `id` (its number in
 *  `radio.tciServer.programs`). What the program keyed ends first, then its connection closes. A
 *  number no connected program has does nothing. At the shack only. */
export async function tciServerDisconnect(id: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('tci_server_disconnect', { id })
}

/** Stop the rotator immediately (rotctld S). */
export async function stopRotator(): Promise<void> {
  return invoke('stop_rotator')
}

export async function readRotator(): Promise<number | null> {
  return invoke<number | null>('read_rotator')
}

/** The Rotor pane's poll: where the rotator is and what the read found, `null` with no rotator
 *  configured. Desktop only: a browser reads the station's heading through its own collection. */
export async function readRotatorState(): Promise<RotatorState | null> {
  return invoke<RotatorState | null>('read_rotator_state')
}

/** Single-signal CW decode of the recent RX audio (live readout: text + estimated WPM).
 * `sensitivity` (0..1, 0.5 = default gates) scales the decoder's presence + SNR gates. */
export async function cwDecode(sensitivity: number): Promise<CwDecodeResult> {
  return invoke<CwDecodeResult>('cw_decode', { sensitivity })
}

/** Toggle the AI CW decoder (beta). Persisted; returns the fresh snapshot. */
export async function setAiCw(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_ai_cw', { on })
}

/** Declare (or end) an UNASSISTED contest entry — one switch that suppresses every
 *  QSO-finding assistance source (AI CW decoder, cluster/RBN, PSK Reporter needs) and
 *  journals the change. The operator's own settings are overridden, never overwritten. */
export async function setUnassistedMode(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_unassisted_mode', { on })
}

/** The assistance journal, newest first — what was running, and when. */
export async function getAssistanceJournal(): Promise<import('./types').AssistanceEvent[]> {
  return invoke('get_assistance_journal')
}

/** Super Check Partial: download the list, or check for a newer one, when the site's rules for
 *  logging software allow it now, and say what is held. The station decides: nothing is sent
 *  while SCP is off or Unassisted mode is declared, and never more than once a day unless
 *  `manual` (the operator's "Update now"). */
export async function scpEnsure(manual: boolean): Promise<import('./types').ScpStatus> {
  return invoke('scp_ensure', { manual })
}

/** What is held of the Super Check Partial list, without asking the site anything. */
export async function getScpStatus(): Promise<import('./types').ScpStatus> {
  return invoke('get_scp_status')
}

/** The Super Check Partial calls, once per strip mount; empty while SCP is effectively off. */
export async function getScpCalls(): Promise<string[]> {
  return invoke('get_scp_calls')
}

/** Import an N1MM-format call-history file for one contest (a `FieldDayStatus.event` id). A file
 *  that does not read is refused and the one held is kept. */
export async function importCallHistory(
  text: string,
  fileName: string,
  contest: string,
): Promise<import('./types').CallHistoryStatus> {
  return invoke('import_call_history', { text, fileName, contest })
}

/** What call history is imported, if any. */
export async function getCallHistoryStatus(): Promise<import('./types').CallHistoryStatus | null> {
  return invoke('get_call_history_status')
}

/** The imported call history for the strip; null when none is imported or it is effectively off. */
export async function getCallHistory(): Promise<import('./types').CallHistoryFile | null> {
  return invoke('get_call_history')
}

/** Forget the imported call history. */
export async function clearCallHistory(): Promise<void> {
  return invoke('clear_call_history')
}

/** Clear the streaming CW decoder's accumulated transcript. */
export async function cwClear(): Promise<void> {
  return invoke('cw_clear')
}

/** Expand a CW macro to the exact text it will send, without sending (reply preview). */
export async function previewCw(text: string): Promise<string> {
  return invoke<string>('preview_cw', { text })
}

/** Wideband CW skim of the recent RX audio: every distinct keyed signal across the band. */
export async function cwSkim(): Promise<SkimHit[]> {
  return invoke<SkimHit[]>('cw_skim')
}

/** One decoded APRS packet for the cockpit list (from `get_aprs_heard`). */
export interface AprsHeard {
  source: string
  dest: string
  path: string[]
  lat: number | null
  lon: number | null
  symbolTable: string
  symbolCode: string
  /** "position" | "mice" | "message" | "status" | "other". */
  kind: string
  text: string
  speedKnots: number | null
  courseDeg: number | null
  /** For a `message` kind: who it was addressed to (base call). */
  addressee: string | null
  /** For a `message` kind: the sender's line number, if any. */
  msgId: string | null
  atUnix: number
  /** WHERE this packet came from. `rf` = this station's own receiver decoded it; `inet` = the
   * APRS-IS feed reported it; `both` = the same packet arrived each way. Never inferred — an RF
   * sighting proves the antenna hears the station, an internet one proves nothing about range. */
  sourceKind: AprsSource
  /** The packet as a TNC2 monitor line, for the raw readout. */
  raw: string
}

/** Where an APRS packet reached us from. See `AprsHeard.sourceKind`. */
export type AprsSource = 'rf' | 'inet' | 'both'

/** One STATION, accumulated from every packet it has sent (from `get_aprs_stations`).
 *
 * Distinct from `AprsHeard`, which is one PACKET. The map and the station list read stations; the
 * packet pane and the message list read packets. Conflating them is what made the map flash — the
 * packet log is capped by count with no age expiry, so a busy internet feed evicted stations that
 * were still active, and they blinked off until their next beacon. */
export interface AprsStation {
  /** Callsign-SSID — the station identity and the store key. */
  call: string
  /** Last known position. Sticky: a later packet without one does not erase it. */
  lat: number | null
  lon: number | null
  symbolTable: string
  symbolCode: string
  kind: string
  text: string
  speedKnots: number | null
  courseDeg: number | null
  path: string[]
  raw: string
  /** Most recent packet from this station, either source. Drives the age column and the fade. */
  lastHeardUnix: number
  /** When this receiver last decoded it off the air, if ever. */
  lastRfUnix: number | null
  /** When it last arrived via APRS-IS, if ever. */
  lastInetUnix: number | null
  /** Derived from the two above, so it cannot drift from the evidence. */
  sourceKind: AprsSource
  packets: number
  firstHeardUnix: number
  /** Weather readings, when this station sends them. Sticky like the position. */
  wx: AprsWx | null
  /** Where the station has been, oldest first, ending at its current position — one point per
   * distinct position, held to the age window. Absent until the station has moved. */
  track?: AprsTrackPoint[]
}

/** One place a station reported, and when (the latest packet that put it there). */
export interface AprsTrackPoint {
  lat: number
  lon: number
  atUnix: number
}

/** APRS weather readings — the common core of the WX format (APRS 1.0.1 ch. 12). Every field is
 * optional: real stations omit what they have no sensor for, and the wire's placeholder for
 * "no reading" must never be shown as a measurement of zero. */
export interface AprsWx {
  windDirDeg: number | null
  windMph: number | null
  gustMph: number | null
  tempF: number | null
  /** Hundredths of an inch. */
  rain1hIn100: number | null
  rain24hIn100: number | null
  rainMidnightIn100: number | null
  humidityPct: number | null
  /** Tenths of a hectopascal — 10156 is 1015.6 hPa. */
  pressureTenthHpa: number | null
}

/** The station roster plus the aging thresholds that produced it, so the UI's fade can never
 * disagree with the backend's retention. */
export interface AprsStationsView {
  stations: AprsStation[]
  /** Minutes of silence after which a station is dropped. */
  ttlMin: number
  /** Minutes of silence after which it starts to fade (derived from ttlMin). */
  fadeAfterMin: number
}

/** Poll the APRS station roster — what the map and station list draw. */
export async function getAprsStations(): Promise<AprsStationsView> {
  return invoke<AprsStationsView>('get_aprs_stations')
}

/** Arm/disarm the APRS RX decoder (session-only; RX decode). Returns the current heard list. */
export async function aprsArm(on: boolean): Promise<AprsHeard[]> {
  return invoke<AprsHeard[]>('aprs_arm', { on })
}

/** The decoded-APRS list, newest last (poll while the APRS cockpit is visible). */
export async function getAprsHeard(): Promise<AprsHeard[]> {
  return invoke<AprsHeard[]>('get_aprs_heard')
}

/** What the APRS decoder is hearing (from `get_aprs_health`) — lets an empty map say WHY it is
 * empty instead of looking identical whether the app is deaf, mistuned, or the band is quiet. */
export interface AprsHealth {
  /** Armed-ness AND how it was armed. `auto` (armed by entering the view) is RECEIVE-ONLY and
   * never auto-acks; only `explicit` (the operator armed it themselves) can, and even then only
   * with TX enabled. One field so "is it running" and "may it ack" cannot drift apart. */
  arm: 'off' | 'auto' | 'explicit'
  /** Peak |sample| of the last drain that carried audio (empty drains do not clobber it). */
  audioPeak: number
  /** When audio last ARRIVED at the tap, at any level. Distinct from the level on purpose: a
   * squelched codec streams digital zeros, so this stays fresh while `audioPeak` is 0. */
  lastAudioUnix: number | null
  /** Drains reported since arming, carrying audio or not — lets "never heard anything" be told
   * apart from "have not looked yet", so arming does not flash a capture alarm. */
  drains: number
  /** HDLC frames recovered since arming, BEFORE the AX.25 FCS check. */
  framesSeen: number
  /** Of those, how many passed the FCS. Seen climbing while this does not = heard, but not cleanly. */
  framesDecoded: number
  lastDecodeUnix: number | null
  /** When a frame candidate was last recovered. The counters are cumulative since arming and the
   * level is live, so without this the UI cannot tell "failing right now" from "two turned up
   * six minutes ago". */
  lastFrameSeenUnix: number | null
  /** Peak of the drain that carried the most recent frame candidate — the BURST-TIME level.
   * `audioPeak` between packets is the gap/hiss level, which is what misled the field report. */
  framePeak: number
  /** Largest burst-time peak this session. */
  maxFramePeak: number
  /** Near-full-scale samples in the drain that carried the last frame candidate. */
  frameClippedSamples: number
  /** Name of the radio the decoder is listening to (the ACTIVE radio — the tap follows it). */
  radioName: string
  /** Enabled radios explicitly covering the band in use. >1 means the activation had a choice, and
   * the operator cannot tell from the radio which way it went. */
  bandRadioCount: number
}

/** Poll the APRS decoder's health beside the heard list. */
export async function getAprsHealth(): Promise<AprsHealth> {
  return invoke<AprsHealth>('get_aprs_health')
}

/** Why what was queued for APRS was last dropped instead of sent (TX off, or outside the licence
 *  privileges): the engine's own sentence for the cockpit's status line, or null once a frame
 *  keys. The desktop's alone — the Remote has no such read. */
export async function getAprsTxNotice(): Promise<string | null> {
  return invoke<string | null>('get_aprs_tx_notice')
}

/** What the APRS-IS internet feed is doing (from `get_aprs_is_status`) — the counterpart to
 * `AprsHealth` for the other inlet. The two fail independently, and that is the point: internet
 * stations arriving while the RF chip stays silent proves the fault is in the radio chain. */
export interface AprsIsStatus {
  /** The operator has the feed switched on. Independent of the RF decoder's arm state. */
  enabled: boolean
  /** A session is up and the server answered the login. */
  connected: boolean
  /** The login was accepted as VERIFIED — required to upload. A read-only feed (`pass -1`) is
   * unverified by design and works perfectly. */
  verified: boolean
  /** Packet lines received this session. */
  packets: number
  /** When the last line arrived — tells "connected but quiet" from "connected". */
  lastPacketUnix: number | null
  /** The operator has the receive-only iGate on. */
  uplinkEnabled: boolean
  /** Packets contributed to APRS-IS this session. */
  uploaded: number
  /** RF-heard packets the iGate rules refused (mostly the loop guard doing its job). */
  gateRejected: number
  /** The most recent refusal reason, for the tooltip. */
  lastReject: string | null
  /** The session is up for JS8's @APRSIS gateway: what JS8 stations address to @APRSIS goes up
   * through it. With the feed off, that is why it is up. Desktop only: the Remote page's sample
   * keeps the nine fields above, which its validator requires exactly. */
  forJs8?: boolean
  /** …and JS8's relay listens on it for APRS messages to stations heard on JS8. Desktop only. */
  forJs8Relay?: boolean
}

/** Poll the APRS-IS feed's status beside the decoder health. */
export async function getAprsIsStatus(): Promise<AprsIsStatus> {
  return invoke<AprsIsStatus>('get_aprs_is_status')
}

/** Arm the decoder because the operator ENTERED the APRS view — receive-only, never ack-capable.
 * Only upgrades from off, and refuses once the operator has explicitly stopped it this session
 * (the engine owns that policy). Returns whether this call armed it. */
export async function aprsAutoArm(): Promise<boolean> {
  return invoke<boolean>('aprs_auto_arm')
}

/** Queue an APRS position beacon — an explicit operator send (the engine validates TX-enable /
 * privileges / no other over and rejects with the reason). Symbols are single chars. */
export async function aprsSendBeacon(
  lat: number,
  lon: number,
  symbolTable: string,
  symbolCode: string,
  comment: string,
  path: string[],
): Promise<void> {
  return invoke<void>('aprs_send_beacon', {
    lat,
    lon,
    symbolTable,
    symbolCode,
    comment,
    path,
  })
}

/** Queue an APRS text message to `addressee` — an explicit operator send (the engine validates
 * TX-enable / privileges / no other over and rejects with the reason). Capped at 67 chars. */
export async function aprsSendMessage(addressee: string, text: string): Promise<void> {
  return invoke<void>('aprs_send_message', { addressee, text })
}

/** Tune the rig for APRS: QSY to `dialMhz` on 2 m FM simplex, auto-routing to the 2 m-capable radio
 * (dual-radio hand-off). Unlike a plain frequency dropdown this establishes the FM context APRS
 * needs — a 2 m packet demodulated as USB/DATA never decodes. Returns the updated snapshot. */
export async function aprsTune(dialMhz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('aprs_tune', { dialMhz })
}

/** Tune to an FM repeater in ONE atomic step: its output frequency on FM carrying the machine's
 * exact shift / offset (Hz, 0 = band convention) / CTCSS tone (0 = none), auto-routed to the radio
 * mapped for that band on FM. Replaces the old write-settings-then-QSY pair, which routed on the
 * mode class of the section being left and briefly left the rig on the new dial with the previous
 * machine's tone. Tuning only — nothing here arms TX. */
export async function repeaterTune(
  outputMhz: number,
  shift: 'simplex' | 'plus' | 'minus',
  offsetHz: number,
  toneHz: number,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('repeater_tune', { outputMhz, shift, offsetHz, toneHz })
}

/** Arm/disarm the RTTY RX decoder (session-only; RX decode, never TX). */
export async function rttyArm(on: boolean): Promise<RttyState> {
  return invoke<RttyState>('rtty_arm', { on })
}

/** Arm the decoder because the operator ENTERED the RTTY view (the PSK/APRS/SSTV auto-arm
 *  doctrine). Receive-only by construction; the ENGINE owns the policy — it only upgrades
 *  from off, honours the session decline memory and honours the Settings opt-out
 *  (`rttyRxAutoArm`). Deliberately not reimplemented in the UI: four cockpits ask this same
 *  question and there must be one answer.
 *
 *  ⚠️ THE BACKEND HALF IS NOT LANDED YET. `rtty_auto_arm` is a wave-2 engine.rs + lib.rs
 *  contract; until it exists this rejects at runtime and the caller's `.catch` swallows it,
 *  leaving RTTY exactly as it is today (armed by hand). A green typecheck is NOT evidence
 *  this works. */
export async function rttyAutoArm(): Promise<RttyState> {
  return invoke<RttyState>('rtty_auto_arm')
}

/** Live RTTY state (poll while the RTTY cockpit is visible). */
export async function getRttyState(): Promise<RttyState> {
  return invoke<RttyState>('get_rtty_state')
}

/** The RTTY tuning scope's newest frame (poll while its pane is shown). Receive-only. */
export async function getRttyScope(): Promise<RttyScope> {
  return invoke<RttyScope>('get_rtty_scope')
}

/** Queue RTTY text to transmit — an explicit operator send (the engine validates
 * TX-enable / privileges / RTTY-section ownership and rejects with the reason). */
export async function rttySend(text: string): Promise<RttyState> {
  return invoke<RttyState>('rtty_send', { text })
}

/** Continuous TX on/off (the "TX" latch): stay keyed and type into a live
 * transmission instead of one keyed over per Enter. ON runs the same gate a send
 * runs; OFF lets what was already typed finish keying, then unkeys — it is a mode
 * toggle, not the emergency stop (Stop TX and the TX-enable latch are). */
export async function rttySetLatched(on: boolean): Promise<RttyState> {
  return invoke<RttyState>('rtty_set_latched', { on })
}

/** Feed typed characters into the live latched transmission. One insertion at a
 * time — RTTY has no un-send, so what reaches here has already gone on the air. */
export async function rttyType(text: string): Promise<RttyState> {
  return invoke<RttyState>('rtty_type', { text })
}

/** Stop RTTY now: abort the over in progress, drop the queue, unkey. */
export async function rttyStop(): Promise<RttyState> {
  return invoke<RttyState>('rtty_stop')
}

/** Clear the decoded-RTTY transcript (display only). */
export async function rttyClear(): Promise<RttyState> {
  return invoke<RttyState>('rtty_clear')
}

/** Drop + rebuild the RTTY demodulator (fresh AFC acquire — the wrong-neighbor
 * freeze recovery). RX only. */
export async function rttyAfcReset(): Promise<RttyState> {
  return invoke<RttyState>('rtty_afc_reset')
}

/** Net the RTTY decoder onto a new audio center (Hz) — a waterfall click. Rebuilds
 * the demodulator around the new mark/space pair. RX only. */
export async function rttyNet(hz: number): Promise<RttyState> {
  return invoke<RttyState>('rtty_net', { hz })
}

/** Turn the RTTY auto-sequencer on/off (on builds the sequencer from your identity +
 * active exchange, off aborts any live session and stops TX). NEVER transmits — a
 * session only ever starts from an explicit CQ/Answer (the human-initiate gate). */
export async function rttySetAuto(on: boolean): Promise<RttyState> {
  return invoke<RttyState>('rtty_set_auto', { on })
}

/** Start an auto CQ run (a human-initiate gate; rejected with the reason if a TX
 * gate is down). */
export async function rttyAutoCq(): Promise<RttyState> {
  return invoke<RttyState>('rtty_auto_cq')
}

/** Answer a surfaced CQ — search & pounce (a human-initiate gate). */
export async function rttyAutoAnswer(call: string): Promise<RttyState> {
  return invoke<RttyState>('rtty_auto_answer', { call })
}

/** Kill the live auto session: abort the sequencer, drop the queue, unkey. */
export async function rttyAutoAbort(): Promise<RttyState> {
  return invoke<RttyState>('rtty_auto_abort')
}

/** Arm/disarm the PSK31 RX decoder (session-only; RX decode — arming never
 * keys, TX starts only from an explicit send). Stopping it is remembered for
 * the session, so the view-entry auto-arm cannot restart it behind the
 * operator. */
export async function pskArm(on: boolean): Promise<PskState> {
  return invoke<PskState>('psk_arm', { on })
}

/** Arm the decoder because the operator ENTERED the PSK view (the APRS/SSTV
 * auto-arm doctrine). Receive-only by construction; the engine owns the policy
 * (only upgrades from off, honours the session decline + the Settings opt-out). */
export async function pskAutoArm(): Promise<PskState> {
  return invoke<PskState>('psk_auto_arm')
}

/** Live PSK state (poll while the PSK cockpit is visible). */
export async function getPskState(): Promise<PskState> {
  return invoke<PskState>('get_psk_state')
}

/** Clear the decoded-PSK transcript (display only). */
export async function pskClear(): Promise<PskState> {
  return invoke<PskState>('psk_clear')
}

/** Drop + rebuild the PSK demodulator (a fresh slew-limited AFC pull from the
 * netted center). RX only. */
export async function pskAfcReset(): Promise<PskState> {
  return invoke<PskState>('psk_afc_reset')
}

/** Net the PSK decoder onto a new audio center (Hz) — a waterfall click, the
 * single-signal click-to-tune. Moves the DECODER, never the rig. RX only. */
export async function pskNet(hz: number): Promise<PskState> {
  return invoke<PskState>('psk_net', { hz })
}

/** Select the PSK sub-mode ('psk31' | 'qpsk31') + the QPSK sideband-reverse
 * polarity — the cockpit's selector and Reverse toggle. The engine refuses a
 * switch while any PSK transmission is active (returns why); a change
 * re-acquires the RX demodulator on the new mode. */
export async function pskSetMode(mode: string, reverse: boolean): Promise<PskState> {
  return invoke<PskState>('psk_set_mode', { mode, reverse })
}

/** Queue PSK31 text to transmit — an explicit operator send, the only way PSK
 * TX starts. The engine re-validates every gate (TX-enable, privileges, the
 * Keyboard section) and returns why a send was refused. While continuous TX is
 * latched a send types into the live stream (the RTTY macro semantic). */
export async function pskSend(text: string): Promise<PskState> {
  return invoke<PskState>('psk_send', { text })
}

/** Continuous TX on/off — the PSK cockpit's TX button (the same latch RTTY carries).
 * ON runs the same gate a send runs; OFF lets what was typed finish keying.
 * NOT the emergency stop: Stop TX / Esc / the TX-enable latch cut instantly. */
export async function pskSetLatched(on: boolean): Promise<PskState> {
  return invoke<PskState>('psk_set_latched', { on })
}

/** Feed typed characters into the live latched transmission (one insertion at
 * a time — PSK has no un-send). Refused unless continuous TX is latched. */
export async function pskType(text: string): Promise<PskState> {
  return invoke<PskState>('psk_type', { text })
}

/** Stop PSK now: abort the over in progress, drop the queue, unkey. */
export async function pskStop(): Promise<PskState> {
  return invoke<PskState>('psk_stop')
}

// ---- JTTY (WSJT-X's keyboard mode). Every command answers the whole JttyState (a send, with what
// it did). The engine checks every TX gate on each transmit command and refuses with its reason. ----

/** Live JTTY state (poll while the JTTY cockpit is visible). */
export async function getJttyState(): Promise<JttyState> {
  return invoke<JttyState>('get_jtty_state')
}

/** Arm or stop the JTTY decoder by an explicit operator act (session-only; a stop is remembered,
 * so entering the view does not restart it behind the operator). */
export async function jttyArm(on: boolean): Promise<JttyState> {
  return invoke<JttyState>('jtty_arm', { on })
}

/** Arm the decoder because the operator ENTERED the JTTY view. Receive-only; the engine owns the
 * policy (only upgrades from off, honours the session stop and the Settings opt-out). */
export async function jttyAutoArm(): Promise<JttyState> {
  return invoke<JttyState>('jtty_auto_arm')
}

/** WSJT-X's Rx frequency and F Tol: the audio channel always searched. F Tol is kept between
 * sessions, as WSJT-X keeps it; Rx is not. */
export async function jttySetRx(rxHz: number, ftolHz: number): Promise<JttyState> {
  return invoke<JttyState>('jtty_set_rx', { rxHz, ftolHz })
}

/** The JTTY waterfall's visible range: WSJT-X searches its fixed channels inside it. */
export async function jttySetBand(lowHz: number, highHz: number): Promise<JttyState> {
  return invoke<JttyState>('jtty_set_band', { lowHz, highHz })
}

/** Empty the JTTY receive window (WSJT-X's Erase). */
export async function jttyClear(): Promise<JttyState> {
  return invoke<JttyState>('jtty_clear')
}

/** Decode again (WSJT-X's Decode button, and a double-click on a waterfall row with that row's
 * time): the receive audio still kept, or 40 s of it from 5 s before the row. Receive only. */
export async function jttyDecodeAgain(atUtcMs: number | null = null): Promise<JttyState> {
  return invoke<JttyState>('jtty_decode_again', { atUtcMs })
}

/** Answer the JTTY Log QSO prompt `id` (Log QSO or Not now). `taken` for the first answer of the
 * prompt still asked, which takes it for every screen; log the contact only on `taken`. Any other
 * answer says why it took nothing. */
export async function jttyLogPromptAnswer(id: number): Promise<JttyLogPromptAnswer> {
  return invoke<JttyLogPromptAnswer>('jtty_log_prompt_answer', { id })
}

/** Give back JTTY Log QSO prompt `id`, taken by a Log QSO that then logged nothing: every screen
 * asks it again. Logs and sends nothing. */
export async function jttyLogPromptRelease(id: number): Promise<void> {
  return invoke<void>('jtty_log_prompt_release', { id })
}

/** Send typed text (WSJT-X's Send): all of it, its macros expanded, ending the message (and a live
 * session). `hisCall` is the dock's Call (`%H`); `hisSnr` the SNR of the decode it was picked
 * from (`%R`), null when it was typed (WSJT-X's -10). */
export async function jttySend(text: string, hisCall: string, hisSnr: number | null = null): Promise<JttySent> {
  return invoke<JttySent>('jtty_send', { text, hisCall, hisSnr })
}

/** F-key `n` (1–8): its template in the message style in force, compiled as WSJT-X compiles it. */
export async function jttyFkey(n: number, hisCall: string, hisSnr: number | null = null): Promise<JttySent> {
  return invoke<JttySent>('jtty_fkey', { n, hisCall, hisSnr })
}

/** Start live entry with what is typed so far (WSJT-X's Alt+J). */
export async function jttyLiveStart(typed: string, hisCall: string, hisSnr: number | null = null): Promise<JttySent> {
  return invoke<JttySent>('jtty_live_start', { typed, hisCall, hisSnr })
}

/** Commit live typing: `flush` after a pause (all of it), otherwise once five words are complete
 * beyond the three WSJT-X holds back. */
export async function jttyLiveCommit(
  typed: string,
  hisCall: string,
  flush: boolean,
  hisSnr: number | null = null,
): Promise<JttySent> {
  return invoke<JttySent>('jtty_live_commit', { typed, hisCall, flush, hisSnr })
}

/** Sign off (WSJT-X's Alt+K): `DE <my call> K` after what is typed; the session ends. */
export async function jttySignOff(typed: string, hisCall: string, hisSnr: number | null = null): Promise<JttySent> {
  return invoke<JttySent>('jtty_sign_off', { typed, hisCall, hisSnr })
}

/** Stop JTTY now: everything queued dropped, a live session ended, what is on the air cut. */
export async function jttyStop(): Promise<JttyState> {
  return invoke<JttyState>('jtty_stop')
}

/** JTTY's Tx frequency (Hz): refused while JTTY transmits. */
export async function jttySetTx(txHz: number): Promise<JttyState> {
  return invoke<JttyState>('jtty_set_tx', { txHz })
}

/** The JTTY contest serial number (`%E`, `%N`), kept in Settings as WSJT-X keeps it. */
export async function jttySetSerial(serial: number): Promise<JttyState> {
  return invoke<JttyState>('jtty_set_serial', { serial })
}

// ---- JS8 (interfaces.md §3.6 — every command answers the whole Js8State, the PSK shape) ----

/** The operator ENTERED the JS8 view: `set_tier(JS8)` + retune to the JS8 watering hole for
 * the current band. RX ONLY by construction — it confers neither TX-enable nor any
 * auto-reply arm; nothing keys. */
export async function js8Enter(): Promise<Js8State> {
  return invoke<Js8State>('js8_enter')
}

/** Live JS8 state (poll ~500 ms while the JS8 cockpit is visible). */
export async function getJs8State(): Promise<Js8State> {
  return invoke<Js8State>('get_js8_state')
}

/** Why the JS8 gate would refuse the locator in Settings (the gate's own sentence), or null when
 *  it would take it. "Send my grid" follows this rather than a copy of the gate's rule. */
export async function js8LocatorRefusal(): Promise<string | null> {
  return invoke<string | null>('js8_locator_refusal')
}

/** Select the TRANSMIT speed (0 Slow | 1 Normal | 2 Fast | 3 JS8 40 | 4 JS8 60). Persisted; the slot
 * clock and the boundary decode window follow. Never touches the TX latch. */
export async function js8SetSpeed(speed: number): Promise<Js8State> {
  return invoke<Js8State>('js8_set_speed', { speed })
}

/** Select which speeds the receiver decodes (bitmask slow 1 · normal 2 · fast 4 · turbo 8 · ultra 16).
 * Persisted; a mask that decodes nothing is refused (the engine returns why). */
export async function js8SetRxSpeeds(mask: number): Promise<Js8State> {
  return invoke<Js8State>('js8_set_rx_speeds', { mask })
}

/** JS8Call's decode filter (on/off, centre and width in Hz): the waterfall controls and the row
 *  menus' "Centre filter at …Hz". Receive only. Persisted. */
export async function js8SetFilter(filter: Js8Filter): Promise<Js8State> {
  return invoke<Js8State>('js8_set_filter', { filter })
}

/** JS8Call 3.0.3's own frequency table as rows of the operator's list: what Settings shows while
 *  no list of the operator's own is kept, and what Reset returns to. */
export async function js8DefaultFrequencies(): Promise<Js8Freq[]> {
  return invoke<Js8Freq[]>('js8_default_frequencies')
}

/** The passband centre "QSY to centre" puts the transmit offset on, Hz. Moves nothing by itself. */
export async function js8SetQsyCenter(hz: number): Promise<Js8State> {
  return invoke<Js8State>('js8_set_qsy_center', { hz })
}

/** JS8Call's QSY: move the dial so the transmit offset sits on the passband centre. MOVES THE RIG;
 *  refused while anything transmits or a JS8 message is going out (the engine says why). */
export async function js8QsyToCentre(): Promise<Js8State> {
  return invoke<Js8State>('js8_qsy_to_centre')
}

/** Answer a scheduled frequency change's notice: `go` = change now, otherwise cancel (stay). */
export async function js8HopAnswer(go: boolean): Promise<Js8State> {
  return invoke<Js8State>('js8_hop_answer', { go })
}

/** Queue a message to `to` (a callsign, an @GROUP, or null for @ALLCALL). An explicit
 * operator send — the engine re-validates every gate and returns why a send was refused.
 * Refused outright in the receive-only build. */
export async function js8Send(to: string | null, text: string): Promise<Js8State> {
  return invoke<Js8State>('js8_send', { to, text })
}

/** Queue a directed command (`cmd` = the 32-entry table id) with its argument. */
export async function js8SendCommand(to: string, cmd: number, arg: string): Promise<Js8State> {
  return invoke<Js8State>('js8_send_command', { to, cmd, arg })
}

/** Call CQ with the CQ message from Settings (`js8CqText`, JS8Call's CQ message), its macros
 * replaced by the station. */
export async function js8CallCq(): Promise<Js8State> {
  return invoke<Js8State>('js8_call_cq')
}

/** The SECOND act of the two-act rule: autoreply | relay | hback | hbnet (persisted) or hb
 * (session-only). Turning a switch on never keys by itself — the session TX latch is the
 * first act, re-checked at plan time on every slot. */
export async function js8Arm(which: Js8Switch, on: boolean): Promise<Js8State> {
  return invoke<Js8State>('js8_arm', { which, on })
}

/** Arm/disarm JS8Call's repeating CQ, which sends the CQ message from Settings. Session-only
 * and never persisted, like the HB toggle; the interval is the persisted half (js8CqIntervalMin).
 * Arming keys nothing — the session TX latch is the first act. */
export async function js8CqRepeat(on: boolean): Promise<Js8State> {
  return invoke<Js8State>('js8_cq_repeat', { on })
}

/** The native cockpit's compose box, both ways: whether it holds text, and the id of the reply
 * it took into the box since it last asked; answered with the reply waiting for the box (AUTO
 * off, as JS8Call fills its compose box), or null. Keys nothing. */
export async function js8Composer(composing: boolean, taken: number | null): Promise<Js8ComposerPrefill | null> {
  return invoke<Js8ComposerPrefill | null>('js8_composer', { composing, taken })
}

/** JS8Call's typeahead: what is still to go of the operator's message going out, `sentSeen` the
 * characters of it the box had seen go. Taken just before the next frame; answered with what the box
 * shows, or null once nothing of the box's is going out. Native only. */
export async function js8Typeahead(sentSeen: number, unsent: string): Promise<Js8Typeahead | null> {
  return invoke<Js8Typeahead | null>('js8_typeahead', { sentSeen, unsent })
}

/** What the compose box shows of the operator's message going out, or null once it has gone. */
export async function js8TypeaheadState(): Promise<Js8Typeahead | null> {
  return invoke<Js8Typeahead | null>('js8_typeahead_state')
}

/** The compose box's words JS8Call underlines (its word check): one answer per word, in order,
 * true where the JSC dictionary codes the word in pieces. The dictionary only; nothing keys. */
export async function js8WordCheck(words: string[]): Promise<boolean[]> {
  return invoke<boolean[]>('js8_word_check', { words })
}

/** What JS8Call's right-click offers for an underlined word. */
export interface Js8WordSuggestions {
  /** Up to five cheaper words, cheapest first. */
  words: string[]
  /** The dictionary holds some prefix of the word: "No suggestions" when `words` is empty. */
  found: boolean
}
export async function js8WordSuggestions(word: string): Promise<Js8WordSuggestions> {
  return invoke<Js8WordSuggestions>('js8_word_suggestions', { word })
}

/** The operator's Yes or No to the automatic reply the dock asks about (JS8Call's
 * AutoreplyConfirmation box), named by the `display` and `firesAtMs` it was shown with so an
 * answer can never reach a different reply. A Yes to one no longer waiting is refused. */
export async function js8AnswerReply(yes: boolean, display: string, firesAtMs: number): Promise<Js8State> {
  return invoke<Js8State>('js8_answer_reply', { yes, display, firesAtMs })
}

/** Drop the outbox — a SENDER-class control, not a stop (Stop TX is haltTx). */
export async function js8DropQueue(): Promise<Js8State> {
  return invoke<Js8State>('js8_drop_queue')
}

/** Mark an inbox row (unread | read | store | delivered). Journaled. */
export async function js8InboxMark(id: number, state: Js8InboxState): Promise<Js8State> {
  return invoke<Js8State>('js8_inbox_mark', { id, state })
}

/** Delete an inbox row. Journaled. */
export async function js8InboxDelete(id: number): Promise<Js8State> {
  return invoke<Js8State>('js8_inbox_delete', { id })
}

/** "Send heartbeat now" (JS8Call's HB button menu): one heartbeat in the next period, the repeat
 * left as it is. Refused with heartbeat networking off or at a speed with no heartbeats; keys only
 * with TX on. */
export async function js8HeartbeatNow(): Promise<Js8State> {
  return invoke<Js8State>('js8_heartbeat_now')
}

/** The call in the To box (null when empty): JS8Call's selected call, which the QSO pause reads.
 * Keys nothing. */
export async function js8SelectCall(call: string | null): Promise<void> {
  return invoke<void>('js8_select_call', { call })
}

/** JS8Call's "Store message…": leave `text` at this station for `to`, handed over when `to` asks
 * (QUERY MSGS, then QUERY MSG n). Keys nothing. Refused with the reason. Journaled. */
export async function js8StoreMessage(to: string, text: string): Promise<Js8State> {
  return invoke<Js8State>('js8_store_message', { to, text })
}

/** Clear the Inbox's notes of what a cap dropped. Journaled. */
export async function js8InboxDroppedClear(): Promise<Js8State> {
  return invoke<Js8State>('js8_inbox_dropped_clear')
}

/** The JS8 call list's "Block this station": the RX block list (Settings `js8RxBlocklist`), that one
 *  field, never the settings form. Resolves with the list as saved, for the UI's settings mirror. */
export async function js8SetRxBlocklist(calls: string[]): Promise<string[]> {
  return invoke<string[]>('js8_set_rx_blocklist', { calls })
}

/** The JS8 call list's group joins and leaves (Settings `js8Groups`), that one field, never the
 *  settings form; refused for @APRSIS and @JS8NET. Resolves with the groups as saved. */
export async function js8SetGroups(groups: string[]): Promise<string[]> {
  return invoke<string[]>('js8_set_groups', { groups })
}

/** The JS8 call list's Remove on @ALLCALL and Add of @ALLCALL (Settings `js8AvoidAllcall`, JS8Call's
 *  "Do not participate in the @ALLCALL group"), that one field, never the settings form. Resolves
 *  with the setting as saved. */
export async function js8SetAvoidAllcall(avoid: boolean): Promise<boolean> {
  return invoke<boolean>('js8_set_avoid_allcall', { avoid })
}

/** Arm/disarm the SSTV RX decoder by an EXPLICIT operator act (session-only; RX
 * decode, never TX). Stopping it here is remembered for the session — opening the
 * view again will not restart it behind the operator. */
export async function sstvArm(on: boolean): Promise<SstvState> {
  return invoke<SstvState>('sstv_arm', { on })
}

/** Start decoding an SSTV picture NOW in the named mode, without waiting for a VIS
 *  header (#202) — for tuning into a transmission already in progress, or one whose
 *  header was lost. Arms the receiver if it was off. RX only: this cannot key anything.
 *  `mode` is a `short_name` slug from `SSTV_RX_MODES`. */
export async function sstvManualRx(mode: string): Promise<SstvState> {
  return invoke<SstvState>('sstv_manual_rx', { mode })
}

/** Start the SSTV receiver because the operator OPENED the SSTV view.
 *
 * ⭐ Without this, the ordinary way to use SSTV — open the view, tune 14.230, wait
 * for a picture — decoded nothing, because arming was manual, default-off and lost
 * on every restart. RX only: this cannot key anything. Only ever an upgrade from
 * off, and refused after an explicit Stop (the policy lives in the engine so it
 * survives a remount). */
export async function sstvAutoArm(): Promise<SstvState> {
  return invoke<SstvState>('sstv_auto_arm')
}

/** Stop the SSTV receiver as part of an AUTOMATIC sequence — the ISS pass unwind at
 * LOS, which disarms what it armed.
 *
 * ⚠️ NEVER `sstvArm(false)` for that: an explicit Stop is remembered for the session,
 * so an automatic one would leave every later entry to the SSTV view refusing to
 * start the receiver — the decoding bug again, one ISS pass later. */
export async function sstvAutoDisarm(): Promise<SstvState> {
  return invoke<SstvState>('sstv_auto_disarm')
}

/** Live SSTV RX state: in-flight progress + preview + the saved-image gallery
 * (poll while the SSTV view is visible). */
export async function getSstvState(): Promise<SstvState> {
  return invoke<SstvState>('get_sstv_state')
}

/** Transmit an SSTV image: the webview cover-crops the picture to the mode's exact
 * dimensions and passes raw row-major RGB (base64). `mode` is the stable slug
 * ("pd120", "scottie1", "scottiedx", "martin1", …). Resolves to the fresh state
 * (sending=true); rejects — with the reason — on a dimension mismatch or any TX gate
 * being down. Human-initiated only: nothing keys until this is called. */
export async function sstvSend(
  rgbB64: string,
  width: number,
  height: number,
  mode: string,
  /** #50: the operator affirms THIS picture already shows their callsign (a pre-made QSO
   *  card), so the ID plate is skipped rather than drawn over it. Per-image, never sticky. */
  idInImage?: boolean,
): Promise<SstvState> {
  return invoke<SstvState>('sstv_send', {
    mode,
    width,
    height,
    rgbBase64: rgbB64,
    idInImage: idInImage ?? false,
  })
}

/** Stop the SSTV transmission now: abort the image in progress, drop the queued job,
 * and unkey. */
export async function sstvStop(): Promise<SstvState> {
  return invoke<SstvState>('sstv_stop')
}

/** Set the TX period: true = even/"1st" slots, false = odd/"2nd". */
export async function setTxCycleAuto(auto: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tx_cycle_auto', { auto })
}

export async function setBeacon(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_beacon', { on })
}

export async function setTxEven(even: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tx_even', { even })
}

/** Set the receive audio offset (Hz) — the green marker; TX stays unchanged. */
export async function setRxOffset(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_rx_offset', { hz })
}

/** Set the transmit audio offset (Hz) — the red marker. */
export async function setTxOffset(hz: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_tx_offset', { hz })
}

/** Explicit combined-marker gesture: the TX offset, then the RX offset. */
export async function setBothOffsets(hz: number): Promise<AppSnapshot> {
  await setTxOffset(hz)
  return setRxOffset(hz)
}

/** Hold the TX offset fixed when selecting a decoded station ("Hold Tx Freq"). */
export async function setHoldTxFreq(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_hold_tx_freq', { on })
}

/** The Spots view's "Spotted from" chips, which Pounce follows (#174): continent codes and
 * DXCC countries, as the chips carry them. Empty lists = every spot. Kept for the session. */
export async function setPounceSpotterFilter(conts: string[], entities: string[]): Promise<void> {
  return invoke<void>('set_pounce_spotter_filter', { conts, entities })
}

/** Replace the blocked-callsigns list — the ONE write path (Alt-double-click gesture and
 * the Settings editor). Narrow write: never routes through the heavyweight settings save,
 * so it is safe mid-QSO. The auto-responder honors the list on the next slot. */
export async function setBlockedCalls(calls: string[]): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_blocked_calls', { calls })
}

/** Turn the BETA update channel on or off — the ONE write path for Settings ▸ App updates,
 * and the reason `betaUpdates` is not written by the settings form. A settings payload is a
 * snapshot from whenever the sending surface last read the settings, and some surfaces keep
 * theirs for the life of the window (the APRS cockpit is mounted permanently and reloads only
 * after its own writes) — so a whole-struct save could post a stale value over the live one.
 * Doing that to this field is invisible: it returns a beta tester to the stable channel with
 * no error, no toast and no log line, and the betas simply stop arriving. The backend keeps
 * the live value across every settings save, so this verb is the only thing that moves it. */
export async function setBetaUpdates(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_beta_updates', { on })
}

/** Save the Logbook's column choice (features/logColumns.ts) — the ONE write path for
 * `logbookColumns`, for the same stale-snapshot reason as `setBetaUpdates`: the backend keeps the
 * live value across every settings save. Narrow, never the settings form, which would also move
 * the transmit gate's generation for a change that has nothing to do with transmitting. `null`
 * forgets the choice: the default table. */
export async function setLogbookColumns(columns: LogColumnsSaved | null): Promise<void> {
  return invoke<void>('set_logbook_columns', { columns })
}

/** Save the Logbook's named filters (features/logFilter.ts) — the ONE write path for
 * `logbookFilters`, narrow for the same stale-snapshot reason as `setLogbookColumns`. The whole list
 * each time: saving, renaming and deleting a filter are each one write of the list it leaves. */
export async function setLogbookFilters(filters: SavedLogFilter[]): Promise<void> {
  return invoke<void>('set_logbook_filters', { filters })
}

/** Start Nexus when the operator signs in to this computer — the ONE write path for Settings ▸
 * Start at sign-in, for the same stale-snapshot reason as `setBetaUpdates`. The backend changes
 * the operating system's login entry first and persists the choice only when that succeeded, so
 * a platform that refuses rejects here with the reason, and the switch never shows a state the
 * computer is not in. Starting at sign-in changes nothing else: transmit stays off at launch. */
export async function setLaunchAtLogin(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_launch_at_login', { on })
}

/** Record that the operator answered Remote's one-time "start Nexus at sign-in?" offer, either
 * way, so it is never asked again. A separate verb from `setLaunchAtLogin` because "No thanks"
 * must be remembered without touching the login entry. */
export async function answerRemoteAutostartOffer(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('answer_remote_autostart_offer')
}

/** Set (or clear, with '') who is at the key — the ONE write path for the seat-swap chip,
 * the Field Day panel's Operator field and the pop-out scoreboard. Narrow write: never the
 * heavyweight settings save, which clears the TX queue and re-derives the TX cycle from the
 * struct the caller happened to be holding (#54). A seat swap is a mid-QSO act by
 * definition. (Since #100 that save no longer resets the operating mode — the other two
 * effects are reason enough.) The engine trims + uppercases. */
export async function setFdOperator(call: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_fd_operator', { call })
}

/** Save the RTTY cockpit's macro sets and which one it shows — the ONE write path for
 * `macros.rttyProfiles` + `macros.activeRttyProfile`, from the dock's editor and its
 * Everyday/Contest switch. Narrow and atomic, NEVER the settings form: `set_settings` advances
 * the TX gate generation (an over just queued would not key) and revokes Remote actuation. The
 * payload must round-trip exactly or the save is refused, and a Settings-panel save keeps the
 * live value. Resolves with the saved `macros`, for the caller's settings mirror. */
export async function setRttyMacros(
  profiles: KeyboardMacroProfile[],
  active: string,
): Promise<Settings['macros']> {
  return invoke<Settings['macros']>('set_rtty_macros', { profiles, active })
}

/** Save the PSK cockpit's macro sets and which one it shows (#316) — `setRttyMacros`' twin over
 * `macros.pskProfiles` + `macros.activePskProfile`, with every word of its contract: narrow,
 * atomic and never the settings form. Separate storage, so a PSK edit cannot rewrite the RTTY
 * dock's keys. Resolves with the saved `macros`, for the caller's settings mirror. */
export async function setPskMacros(
  profiles: KeyboardMacroProfile[],
  active: string,
): Promise<Settings['macros']> {
  return invoke<Settings['macros']>('set_psk_macros', { profiles, active })
}

/** On a computer driving a station, each settings read comes with the station's revision of what
 *  it read (`src-tauri/src/client_mode.rs` puts it beside the settings, under this key). It is
 *  taken off and kept on the copy under a symbol: no field, no JSON and no comparison sees it, yet
 *  every copy spread from the read carries it, so a save made from that read goes back with its
 *  revision (and a change made at the station since refuses it, "Changed at the station"),
 *  whatever else this window read meanwhile. */
const READ_REVISION = 'nexusRevision'
const MADE_FROM: unique symbol = Symbol('the station revision this settings copy was read at')
type Read = Settings & { [MADE_FROM]?: string }

/** Load persisted operator + radio settings. */
export async function getSettings(): Promise<Settings> {
  const read = await invoke<Settings & { [READ_REVISION]?: unknown }>('get_settings')
  if (typeof read?.[READ_REVISION] !== 'string') return read
  const { [READ_REVISION]: revision, ...settings } = read
  return { ...settings, [MADE_FROM]: revision as string } as Read
}

/** Persist operator + radio settings; returns the updated snapshot. */
export async function setSettings(settings: Settings): Promise<AppSnapshot> {
  const base = (settings as Read)[MADE_FROM]
  return invoke<AppSnapshot>('set_settings', base === undefined ? { settings } : { settings, base })
}

/** THE writer of the satellite uplink consent pair (VFO mapping + per-radio
 * confirmation) — the pass rail's confirm/select and Settings ▸ Radio's
 * mapping select both go through it. The pair is backend-owned live state
 * (`set_settings` payloads cannot carry it), so a stale form snapshot can
 * never resurrect a pruned consent. `radioId` = the RadioProfile.id the UI's
 * copy named (the DTO's uplinkRadioId); omitted, the backend records the
 * radio that is ACTIVE at write time — never a form snapshot's. `map`
 * omitted = confirm the mapping IN FORCE, resolved by the backend at write
 * time under the same rule: the rail's confirm for a mapping already in
 * force sends none, because its DTO copy is poll-time state (round 4). An
 * explicit pick (select, derived offer) always sends its map. */
export async function confirmSatUplink(
  map?: NonNullable<Settings['satVfoMap']>,
  radioId?: number,
): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('confirm_sat_uplink', { map, radioId })
}

/** Enumerate available serial / COM ports for rig control. */
export async function getSerialPorts(): Promise<string[]> {
  return invoke<string[]>('get_serial_ports')
}

export interface SerialPortInfo {
  name: string
  /** USB product string, e.g. "USB-Enhanced-SERIAL-B CH342" ("" for non-USB ports). */
  label: string
  /**
   * Which interface of a multi-interface bridge this is. A CP2105 is DUAL and only interface 0
   * carries CAT on the rigs this targets; interface 1 answers nothing and looks exactly like a
   * dead radio. `undefined`/`null` means UNKNOWN (one interface, or no topology source) — never
   * read it as 0.
   */
  interfaceIndex?: number | null
  /**
   * How many serial interfaces this USB device exposes in total.
   *
   * ⚠️ `interfaceIndex` is not usable without this. Plenty of single-interface devices number
   * their one interface something other than 0 — an LG monitor's control port enumerates as
   * interface 2 — so "index > 0" alone would tell an operator their only port is the wrong one.
   * The advice only means something when there IS another port to have picked.
   */
  siblingPorts?: number | null
  /**
   * A sound card on the same physical USB device — i.e. inside the same radio. `null` for a plain
   * serial adapter, which is correct, and `undefined` wherever topology is unavailable; both mean
   * "nothing proven", so nothing may be refused on it.
   *
   * ⚠️ Weaker than `siblingPorts`: a rig's CAT bridge and its codec are separate USB devices
   * behind the rig's own internal hub, so they can only be related by their PARENT — and two
   * unrelated things in one external hub share a parent too. Warning-only for that reason.
   */
  pairedAudio?: string | null
}

/** Serial ports with a descriptive USB-product label (to tell dual-serial rigs apart). */
export async function getSerialPortsDetailed(): Promise<SerialPortInfo[]> {
  return invoke<SerialPortInfo[]>('get_serial_ports_detailed')
}

/** Post your own DX spot to the connected human cluster (rejects if none connected). */
export async function postSpot(freqMhz: number, call: string, comment: string): Promise<void> {
  return invoke('post_spot', { freqMhz, call, comment })
}

/** "Spot me": your own activation to pota.app and the DX cluster, once, with each result — each park
 *  of a two-fer to pota.app on its own. The call, parks, dial and mode sent are the station's;
 *  `reference` and `dialHz` are what the confirm showed, `comment` the operator's ("QRV" when
 *  absent or blank). Rejects with `contextChanged` if either has moved since, `noActivation` with
 *  none on, `invalidComment` for one neither target takes. */
export async function selfSpot(reference: string, dialHz: number, comment?: string): Promise<ActivationSpotReport> {
  return invoke<ActivationSpotReport>('self_spot_activation', { reference, dialHz, comment })
}

/** What the contest screen's Spot me posted (`contest_spot.rs`). */
export interface ContestSpotPosted {
  freqKhz: number
  call: string
  comment: string
  cooldownSecs: number
}

/** The contest screen's Spot me: ONE DX cluster spot of the station's own call on its dial, with
 *  `comment`. Rejects with a token (`spotNoCall`, `spotNoCluster`, `spotCooldown:<secs>`, …);
 *  `spotSelfUnconfirmed` for a contest not known to allow self-spotting until `confirmed`. */
export async function contestSelfSpot(comment: string, confirmed: boolean): Promise<ContestSpotPosted> {
  return invoke<ContestSpotPosted>('contest_self_spot', { comment, confirmed })
}

/** The Spots board's Spot me, in any cockpit: the contest screen's press and its ONE cooldown. Outside a
 *  contest it posts on one press; with Contest mode on it rejects exactly as `contestSelfSpot` does. */
export async function clusterSelfSpot(comment: string, confirmed: boolean): Promise<ContestSpotPosted> {
  return invoke<ContestSpotPosted>('cluster_self_spot', { comment, confirmed })
}

/** A band map frequency marker (`freq_markers.rs`): local to the station, never posted. */
export interface FreqMarker {
  id: number
  dialHz: number
  /** Uppercase band label, e.g. "20M". */
  band: string
  /** Uppercase; empty for a marker made with an empty entry. */
  call: string
}

/** This contest session's markers; empty with Contest mode off. */
export async function getFreqMarkers(): Promise<FreqMarker[]> {
  return invoke<FreqMarker[]>('get_freq_markers')
}

/** Mark the station's dial now with `call` (may be empty). Rejects `markerNoContest` with Contest
 *  mode off. */
export async function addFreqMarker(call: string): Promise<FreqMarker[]> {
  return invoke<FreqMarker[]>('add_freq_marker', { call })
}

export async function removeFreqMarker(id: number): Promise<FreqMarker[]> {
  return invoke<FreqMarker[]>('remove_freq_marker', { id })
}

/** Jump back to a marker (an operator QSY). Rejects `markerTransmitting` while anything transmits. */
export async function freqMarkerQsy(id: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('freq_marker_qsy', { id })
}

/** Upcoming contests from the WA7BNM calendar (rejects if the feed is unreachable). */
export async function getContests(): Promise<import('./types').ContestEvent[]> {
  return invoke<import('./types').ContestEvent[]>('get_contests')
}

/** Enumerate the CURATED (verified) Hamlib rig models as [modelNumber, name] pairs. */
export async function getRigModels(): Promise<[number, string][]> {
  return invoke<[number, string][]>('get_rig_models')
}

/** Enumerate the FULL Hamlib catalog (verified + extended) — for the Settings
 *  "show all models" toggle and resolving a typed-in model number's name. */
export async function getAllRigModels(): Promise<[number, string][]> {
  return invoke<[number, string][]>('get_all_rig_models')
}

/** Models that need NO serial port — Dummy/NET/FLRig and the software-CAT profiles.
 *  The rule lives in Rust (`rigmodels::portless_rig_models`) and is fetched rather than
 *  duplicated here: its membership has changed before, and a stale copy would block saves that
 *  are in fact correct. An empty array means the rule could not be read. */
export async function getPortlessRigModels(): Promise<number[]> {
  return invoke<number[]>('get_portless_rig_models')
}

/** Models whose CAT CW keyer is UNPROVEN and cannot report its own failure (today: the Yaesu
 *  FTX-1). Drives a caution on the CW settings page — never a block, the keyer stays selectable.
 *  The rule lives in Rust (`rigmodels::cat_cw_unproven_rig_models`) and is fetched rather than
 *  duplicated here: membership changes as backends are fixed upstream, and a stale copy would
 *  keep warning about a radio that had started working. An empty array means the rule could not
 *  be read, and the caution is simply not shown. */
export async function getCatCwUnprovenRigModels(): Promise<number[]> {
  return invoke<number[]>('get_cat_cw_unproven_rig_models')
}

/** Models whose Hamlib CAT PTT can choose the radio's MIC or DATA input — the only ones Settings
 *  offers "Transmit audio source (CAT PTT): Rear/Data" on (#381). The set is measured from the
 *  bundled Hamlib and lives in Rust (`rigmodels::PTT_MIC_DATA_RIGS`); fetched, not copied here.
 *  An empty array means the rule could not be read, and no radio is offered the choice. */
export async function getPttMicDataRigModels(): Promise<number[]> {
  return invoke<number[]>('get_ptt_mic_data_rig_models')
}

/** One keystroke to a configured SPE amplifier. The set is closed at the Rust boundary; an
 *  unrecognised name is refused there rather than reaching an opcode.
 *
 *  Resolves false when the queue is full — surface that rather than swallowing it, because a
 *  keystroke the operator watched themselves make and that vanished reads as a broken control.
 *
 *  The transmit interlock lives in the poll thread, not here: it holds a status frame from a
 *  moment earlier and this layer has no reading of its own. Disabling the buttons while keyed
 *  is a courtesy to the operator, never the thing that protects the amplifier. */
export async function ampCommand(which: 'bandDown' | 'bandUp' | 'operate'): Promise<boolean> {
  return invoke<boolean>('amp_command', { which })
}

/** Zero-config: scan connected USB radios → suggested model + port + paired audio. */
export async function detectRigs(): Promise<DetectedRig[]> {
  return invoke<DetectedRig[]>('detect_rigs')
}

/** Activators on the air now for the program ("POTA" | "SOTA") — the hunter feed. */
export async function getOtaSpots(program: string, cached = false): Promise<OtaSpot[]> {
  // `cached`: re-derive what the LOG says over the rows the last fetch left in the shared cache,
  // instead of fetching. The board asks for that when the log changes — which is several times
  // per contact once the upload stamps land — so it must never reach pota.app or SOTAwatch.
  // Rejects when nothing is cached for the programme yet; the caller keeps what it is showing.
  return invoke<OtaSpot[]>('get_ota_spots', cached ? { program, cached } : { program })
}

/** The NOAA planetary-K outlook (three days ahead). Cached 15 min server-side; an
 *  EMPTY series means we have never had one, which the panel must say rather than
 *  draw as a quiet sky. */
export async function getKpForecast(): Promise<KpForecast> {
  return invoke<KpForecast>('get_kp_forecast')
}

/** NOAA's daily solar indices: the last thirty days of SFI and sunspot number. Cached an hour
 *  server-side, and the last good copy is served when a fetch fails — every row carries its own
 *  date, so the caller can say how old the newest day is. EMPTY = never had the file. */
export async function getSolarIndices(): Promise<DailySolarIndices> {
  return invoke<DailySolarIndices>('get_solar_indices')
}

/** Activators placed for the Connect map's parks layer. Served from a shared cache
 *  with its own TTL, so polling this does not add load to the POTA feed — POTA only,
 *  because a SOTA spot carries no position to plot. */
export async function getOtaMapSpots(): Promise<OtaMapSpot[]> {
  return invoke<OtaMapSpot[]>('get_ota_map_spots')
}

/** One park's activator standing under one callsign. */
export interface ParkStanding {
  reference: string
  /** From the park directory; empty when it has none. */
  name: string
  /** UTC days with ten or more distinct contacts there. */
  activations: number
  /** UTC days that fell short of ten. */
  attempts: number
  /** Distinct contacts there, every day's (Kilo's count). */
  qsos: number
  lastDate: string
}

/** One callsign's POTA activator standing, counted from the log by the activation counter's rule. */
export interface ActivatorStats {
  callsign: string | null
  activations: number
  attempts: number
  qsos: number
  parksActivated: number
  /** The next "different parks" achievement; null past the last. */
  nextParkTier: number | null
  roverBest: number
  roverBestDate: string | null
  /** UTC days reaching 5, 10, 15, 20, 25 and 30 parks. */
  roverDays: number[]
  parks: ParkStanding[]
}

/** A park of yours for the map: the running activation's, or one you have activated. */
export interface MyPark {
  reference: string
  name: string
  lat: number
  lon: number
  /** Placed by a grid square rather than the park's own pin. */
  approx: boolean
  activating: boolean
  activations: number
}

export interface PotaActivator {
  calls: ActivatorStats[]
  parks: MyPark[]
}

/** The operator's POTA activator standing per callsign, and the parks of theirs the map shows. */
export async function getPotaActivator(): Promise<PotaActivator> {
  return invoke<PotaActivator>('get_pota_activator')
}

/** The street-map packs installed in the maps folder. Empty until the operator downloads one. */
export async function streetMapPacks(): Promise<StreetPack[]> {
  return invoke<StreetPack[]>('street_map_packs')
}

/** `length` bytes of an installed pack from `offset`, raw (an ArrayBuffer): exactly `length`, or
 *  fewer only at the end of the file. Rust refuses a read over 4 MiB. The pack reader
 *  (features/streetPack.ts) is the only caller, and it checks every answer. */
export async function streetMapRead(packId: string, offset: number, length: number): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('street_map_read', { packId, offset, length })
}

/** One file from the maps folder's `assets/`, raw: a glyph range or a sprite sheet. */
export async function streetMapAsset(path: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('street_map_asset', { path })
}

/** The maps folder, and whether this run benches the street map (NEXUS_STREET_MAP=1). Reads neither
 *  the disk nor the network. */
export async function streetMapInfo(): Promise<StreetInfo> {
  return invoke<StreetInfo>('street_map_info')
}

/** The exact size of an area against the host's current build, and whether it fits on disk: the
 *  host's index and a few MB of its directories, no tile data. Only the download sheet asks. */
export async function streetMapSize(area: StreetArea): Promise<StreetSize> {
  return invoke<StreetSize>('street_map_size', { area })
}

/** Download an area, resuming an unfinished download of it. Progress arrives on `onProgress`
 *  through a Tauri channel, from the same global bridge as `invoke`. Refusals are `{kind, message}`
 *  (features/streetMaps.ts `StreetError`). */
export async function streetMapDownload(area: StreetArea, onProgress: (p: StreetProgress) => void): Promise<StreetPack> {
  const Channel = window.__TAURI__?.core?.Channel
  if (!Channel) throw new Error('Nexus: the Tauri channel is unavailable — the app must run inside the desktop shell.')
  const channel = new Channel<StreetProgress>()
  channel.onmessage = onProgress
  return invoke<StreetPack>('street_map_download', { area, onProgress: channel })
}

/** Stop the running street-map download; it keeps what it has. False when nothing runs. */
export async function streetMapCancel(): Promise<boolean> {
  return invoke<boolean>('street_map_cancel')
}

/** Remove a pack, or what an unfinished download of it left. Resolves with the bytes freed. */
export async function streetMapRemove(packId: string): Promise<number> {
  return invoke<number>('street_map_remove', { packId })
}

/** Downloads that stopped before they finished, for Resume. */
export async function streetMapUnfinished(): Promise<StreetUnfinished[]> {
  return invoke<StreetUnfinished[]>('street_map_unfinished')
}

/** Installed packs a newer build on the host could replace. Reads the host's index; downloads
 *  nothing. */
export async function streetMapUpdates(): Promise<StreetUpdate[]> {
  return invoke<StreetUpdate[]>('street_map_updates')
}

/** THE BENCH AID: the OS file picker opens in the shell (the webview never names the file), and the
 *  file is installed as a pack or as the fonts and icons. Null when the picker is cancelled. */
export async function streetMapInstallFile(): Promise<StreetInstalled | null> {
  return invoke<StreetInstalled | null>('street_map_install_file')
}

/** Begin an activation (validates + normalizes the reference); returns the state. */
export async function setActivation(program: string, reference: string): Promise<Activation> {
  return invoke<Activation>('set_activation', { program, reference })
}
/** Pick the state you are activating from, of a park on a state line ("ND"): its contacts carry
 *  it as MY_STATE. The station refuses a state the park is not in. */
export async function setActivationState(myState: string): Promise<Activation> {
  return invoke<Activation>('set_activation_state', { myState })
}

/** The grid the activation's contacts carry as MY_GRIDSQUARE (blank clears it); never Settings'. */
export async function setActivationGrid(grid: string): Promise<Activation> {
  return invoke<Activation>('set_activation_grid', { grid })
}

/** End the current activation. */
export async function clearActivation(): Promise<Activation> {
  return invoke<Activation>('clear_activation')
}

/** "Still activating": an activation taken up at launch, paused until now, tags contacts again. */
export async function resumeActivation(): Promise<Activation> {
  return invoke<Activation>('resume_activation')
}

/** Read the current activation state. */
export async function getActivation(): Promise<Activation> {
  return invoke<Activation>('get_activation')
}

/** A park directory entry from the local searchable list. */
export interface Park {
  reference: string
  name: string
  grid: string
  location: string
  /** The US states, DC and Canadian provinces in `location` ("US-ND"; each, for a park on a state
   *  line), read by the station with the hunter feed's own rule. */
  states?: string[]
  /** Coordinates — only the live lookup carries these. */
  latitude?: number | null
  longitude?: number | null
}
/** Search the local (offline) POTA park directory by reference prefix or name substring. */
export async function searchParks(query: string, limit?: number): Promise<Park[]> {
  return invoke<Park[]>('search_parks', { query, limit })
}
/** Exact local lookup by reference (offline, instant). null = malformed ref or not in the list. */
export async function lookupPark(reference: string): Promise<Park | null> {
  return invoke<Park | null>('lookup_park', { reference })
}
/** Live lookup of one park's details (name/grid/location + coordinates) from the POTA directory. */
export async function lookupParkLive(reference: string): Promise<Park> {
  return invoke<Park>('lookup_park_live', { reference })
}
/** How many parks are loaded locally (0 = not downloaded/imported yet). */
export async function parksCount(): Promise<number> {
  return invoke<number>('parks_count')
}
/** How many parks the operator imported from their Hunted Parks.CSV (0 = none). */
export async function huntedParksCount(): Promise<number> {
  return invoke<number>('hunted_parks_count')
}
/** Import a park directory from CSV text the operator downloaded. Returns the park count. */
export async function importParksCsv(csv: string): Promise<number> {
  return invoke<number>('import_parks_csv', { csv })
}
/** Download + cache the current POTA all-parks list for offline search. Returns the park count. */
export async function downloadParks(): Promise<number> {
  return invoke<number>('download_parks')
}
/** Import the operator's POTA "Hunted Parks.CSV" so those parks count as worked (drives the NEW
 * PARK badge, including CW hunts the log can't know). Returns the imported park count. */
export async function importHuntedParksCsv(csv: string): Promise<number> {
  return invoke<number>('import_hunted_parks_csv', { csv })
}

/**
 * Arm or disarm the native CI-V bus diagnostic log. When enabled, returns the path
 * of the log file (in Downloads) that captures the raw CI-V traffic; when disabled,
 * returns an empty string. A support tool for hardware-only faults like the IC-9700
 * PTT flicker — off by default, not persisted.
 */
export async function civDiagnosticLog(enable: boolean): Promise<string> {
  return invoke<string>('civ_diagnostic_log', { enable })
}

/**
 * The active CI-V diagnostic-log path, or '' when logging is off. The Settings toggle
 * queries this on mount so it reflects the real backend state — logging keeps running while
 * you leave Settings to transmit, so the switch must not appear to reset (re-arming would
 * truncate the capture).
 */
export async function civDiagnosticStatus(): Promise<string> {
  return invoke<string>('civ_diagnostic_status', {})
}

/**
 * Export the contest/contact log in the given format. Returns the serialized
 * text (the caller saves it via a browser download). Rejects if there is no
 * log to export (e.g. not in Field Day mode).
 */
export async function exportLog(format: 'cabrillo' | 'adif'): Promise<string> {
  return invoke<string>('export_log', { format })
}

/**
 * Subscribe to live snapshot updates. Returns an unsubscribe function. Polls the
 * core a few times a second (a real build can swap this for a Tauri event
 * listener; polling keeps the contract dependency-free).
 */
export function subscribeSnapshot(fn: (snap: AppSnapshot) => void): () => void {
  // 300 ms: the dial-lag fix's real lever is the backend's fast ~180 ms dial read-back (was 750 ms),
  // so a knob turn now tracks in well under a second even at this UI cadence, and wheel-tuning
  // updates the readout instantly via the flushed set_frequency snapshot. Kept at 300 ms (not
  // faster) because get_snapshot still does an O(roster×log) worked-before scan under the engine
  // mutex — Wave 1 optimizes that scan, after which this can safely drop to ~150 ms.
  //
  // SINGLE-FLIGHT (#335). `get_snapshot` waits for the engine mutex on a tokio worker, and the
  // mutex is held across blocking CAT I/O and log saves. A bare interval stacked a new waiter
  // every tick through such a stall — from each window that polls — until no worker was left for
  // the lock-free waterfall row. A tick now skips while the last snapshot is still out, so the
  // cadence changes only while the backend is stalled; the watchdog gives up a call that never
  // answers (see `pollSingleFlight`), and a late answer it gave up on is never delivered.
  return pollSingleFlight(
    'snapshot',
    300,
    (owns) =>
      invoke<AppSnapshot>('get_snapshot')
        .then((snap) => {
          if (owns()) fn(snap)
        })
        .catch(() => {}),
    { leading: false },
  )
}

/** Fetch the next waterfall row (a real Spectrum from the core). */
export async function getSpectrumRow(_transmitting: boolean): Promise<Spectrum> {
  return invoke<Spectrum>('get_spectrum_row')
}

/**
 * Fetch one RIG SCOPE row, spread across the window the scope is actually drawing.
 *
 * Same 512 bins as `getSpectrumRow`, same bytes on the wire — but over `loHz..hiHz` instead of
 * the full 0-4000 Hz capture, so the CW cockpit's 800 Hz pitch-centered view gets 1.5625 Hz
 * bins rather than 7.8125. The span request rides this call; there is nothing to set up and
 * nothing to tear down. Ask for a span the backend cannot honour (a native RF panadapter is live, or the numbers
 * are not a sane audio window) and it returns exactly what `getSpectrumRow` would have — so a
 * caller never has to branch on which row it got, only read the `loHz`/`hiHz` it came back with.
 */
export async function getScopeRow(
  _transmitting: boolean,
  loHz: number,
  hiHz: number,
  window?: ScopeAnalysis,
): Promise<Spectrum> {
  return invoke<Spectrum>('get_scope_row', { loHz, hiHz, window })
}

/**
 * The rig scope's newest FRAME over the window it is drawing, or null when the source has not
 * advanced past `lastSeq`, the number of the frame the scope drew last (0 = none yet).
 *
 * `get_scope_row`'s twin: the same span request, window and fallback, plus the frame's number, time
 * and scale. A scope that commits a waterfall row only when the number advances no longer scrolls
 * copies of a source that sweeps slower than it polls. Two answers come back whatever `lastSeq`
 * says, because neither means "nothing new": a source that went quiet answers its EMPTY frame, and
 * a row nobody numbered (the Companion path) comes back unsequenced (`seq` 0) on every poll.
 */
export async function getScopeFrame(
  loHz: number,
  hiHz: number,
  window: ScopeAnalysis | undefined,
  lastSeq: number,
): Promise<SpectrumFrameWire | null> {
  return invoke<SpectrumFrameWire | null>('get_scope_frame', { loHz, hiHz, window, lastSeq })
}

/**
 * The RF scope pane's poll: the radio's own panadapter frame newer than `lastSeq` (0 = none drawn
 * yet), null when the radio has not swept since, or an EMPTY frame (no bins) when no panadapter is
 * streaming. Never the audio FFT — the pane sits beside the cockpit's audio waterfall, and a copy of
 * it there would say nothing.
 *
 * THE POLL IS THE PANE'S REQUEST. In a data mode the station streams the Icom CI-V scope only while
 * this is being polled (the request lapses two seconds after the last poll), because that stream
 * shares the CAT link with the PTT of every over.
 */
export async function getRfFrame(lastSeq: number): Promise<SpectrumFrameWire | null> {
  return invoke<SpectrumFrameWire | null>('get_rf_frame', { lastSeq })
}

/**
 * Analysis window length for the rig scope — a genuine time-versus-frequency trade, and the one
 * scope control with no right answer for everybody.
 *
 *   fast     1024 —  85 ms — 46.9 Hz lobe — 25 WPM keying is VISIBLE
 *   balanced 2048 — 171 ms — 23.4 Hz lobe — the shipped default; dits are never resolved
 *   sharp    4096 — 341 ms — 11.7 Hz lobe — half the width, double the smear
 *
 * ⚠️ Mirrors `tempo_core::spectrum::WindowN` — the tags are the wire contract and an unknown one
 * is treated as `balanced` by the backend, so an out-of-step UI degrades to today's picture
 * rather than to a blank scope.
 */
export type ScopeWindow = 'fast' | 'balanced' | 'sharp'

/**
 * What the rig scope's row is computed with: one of the windows above, or `hidef`, the CW scope's
 * zoom FFT of its own passband (a steady carrier ~4–5 Hz wide, ~256 ms behind the audio). Mirrors
 * `tempo_core::spectrum::ScopeAnalysis`; a backend that does not know `hidef` draws `balanced`.
 */
export type ScopeAnalysis = ScopeWindow | 'hidef'

/** Set the MSK144 T/R period (5/10/15/30 s) — the cockpit's narrow write. Deliberately not a
 * full settings save, which clears the TX queue and re-derives the TX cycle (#54; the mode
 * reset it also used to do was narrowed to Field Day by #100). */
export async function setMsk144Period(secs: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('set_msk144_period', { secs })
}

/** One Fast Graph power sample: raw 20 ms RMS, monotonic seq for delta polling. */
export interface FastPowerSample {
  seq: number
  unixMs: number
  rms: number
}

/** Fast-mode (MSK144) power trace since `sinceSeq` — the Fast Graph's poll. Meter bus only,
 * no engine mutex, so pings keep rendering while the engine is busy decoding. */
export async function getFastPower(sinceSeq: number): Promise<FastPowerSample[]> {
  return invoke<FastPowerSample[]>('get_fast_power', { sinceSeq })
}

/** Fetch the live meters (RX audio level + CAT S-meter). Lock-free backend-side (no engine
 * mutex), so it is safe to poll fast and a CAT stall cannot freeze it — one shared ~100 ms
 * poll feeds every meter widget instead of riding the 300 ms snapshot (see `LiveMeters`). */
export async function getMeters(): Promise<MeterReadout> {
  return invoke<MeterReadout>('get_meters')
}

// ---------------------------------------------------------------------------
// Coordinated QSY ("move together") — a separate, opt-in feature. No-ops when
// disabled; everything announced in the clear (NOT private / NOT encrypted).
// ---------------------------------------------------------------------------

/** Enable / disable coordinated QSY (captures home + partner / returns home). */
export async function qsySetEnabled(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qsy_set_enabled', { on })
}

/** Set the QSY channel set (band-plan tokens) + announce cadence (overs/hop). */
export async function qsyConfigure(channels: string[], cadence: number): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qsy_configure', { channels, cadence })
}

/** Manual override: announce a move on the next over (initiator). */
export async function qsyMoveNow(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qsy_move_now')
}

/** Manual override: hold the current channel (pause) or resume hopping. */
export async function qsyPause(on: boolean): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qsy_pause', { on })
}

/** Manual override: stop and return to the home channel. */
export async function qsyStop(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('qsy_stop')
}

// ── Program section (radio programming) ──────────────────────────────────────

/** Search repeaters within a radius (km) of a point. Source (RepeaterBook with
 * a stored token on US ground, else hearham) is resolved backend-side. */
export async function repeaterSearch(
  lat: number,
  lon: number,
  radiusKm: number,
  /** A route's other end: the search is then the machines within `radiusKm` of the line from
   * (lat, lon) to it, in order along it. */
  to?: { lat: number; lon: number },
): Promise<RepeaterSearchResult> {
  return invoke<RepeaterSearchResult>(
    'repeater_search',
    to ? { lat, lon, radiusKm, toLat: to.lat, toLon: to.lon } : { lat, lon, radiusKm },
  )
}

/** City-name → candidates via OSM Nominatim (explicit Search click only). */
export async function geocodeCity(query: string): Promise<GeoCandidate[]> {
  return invoke<GeoCandidate[]>('geocode_city', { query })
}

/** Store (or clear, with '') the operator's RepeaterBook rbuapp_… token. */
export async function setRepeaterbookToken(token: string): Promise<void> {
  return invoke<void>('set_repeaterbook_token', { token })
}

/** All saved programming projects (radioprog.json beside settings.json). */
export async function radioprogListProjects(): Promise<RadioProgProject[]> {
  return invoke<RadioProgProject[]>('radioprog_list_projects')
}

/** This run's notice about the saved-projects file, when Program could not read it: where the
 * file is kept, and whether it could be moved aside (if not, saving is refused). */
export async function radioprogFileNotice(): Promise<RadioProgFileNotice | null> {
  return invoke<RadioProgFileNotice | null>('radioprog_file_notice')
}

/** Create/update one programming project (upsert by id). */
export async function radioprogSaveProject(project: RadioProgProject): Promise<void> {
  return invoke<void>('radioprog_save_project', { project })
}

/** Delete one programming project. */
export async function radioprogDeleteProject(id: string): Promise<void> {
  return invoke<void>('radioprog_delete_project', { id })
}

/** Render channels to an export format ('chirp' | 'csv'); returns the file text. */
export async function exportChannels(
  channels: ProgChannel[],
  format: 'chirp' | 'csv',
  nameCap: number,
  attribution: string,
): Promise<string> {
  return invoke<string>('export_channels', { channels, format, nameCap, attribution })
}

/** This computer's over, as the station last said it on Stop's road: on the air with its holes so
 *  far (silences where its audio did not reach the station in time), ended and why, or a press
 *  held with no word from the station. */
export type ClientOver =
  | { phase: 'onAir'; holes: number; holeMs: number }
  | {
      phase: 'ended'
      why:
        | 'audioGap'
        | 'pttGap'
        | 'stopped'
        | 'presence'
        | 'ceiling'
        | 'watchdog'
        | 'routeChanged'
        | 'refused'
    }
  | { phase: 'noWord' }

/** How this computer drives a paired station, when it does (client mode): where the station is,
 *  how the link stands, why it was refused, and this computer's over. `null` on a computer
 *  running its own station. */
export interface ClientStatus {
  station: string
  phase: 'connecting' | 'live' | 'notLive' | 'busy' | 'refused'
  reason: string | null
  over?: ClientOver | null
  /** The link runs over the internet road (Beta), through the station's room. */
  internet?: boolean
  /** "Still there?": nothing from the operator here for a quarter of an hour on the internet. */
  stillThere?: boolean
  /** On the internet, this network lets no datagram road through: watch, tune and log only. */
  udpBlocked?: boolean
  /** A test build's road switch: where it is. A shipped build's status never names it. */
  bench?: BenchPosition
  /** This computer's Nexus version. */
  computerBuild?: string
  /** The station's Nexus version, as its last welcome said it: null until one has. */
  stationBuild?: string | null
  /** The day the station stops taking this computer on its network (seconds since 1970), while that
   *  is within a week and this computer has not signed in again. */
  signInBy?: number | null
  /** Operate's launch is signing in again for the station: the sign-in waits in the browser. */
  signingAgain?: boolean
  /** The station is kept by its service id until this computer first reaches it on its network
   *  (signed in to from elsewhere): the internet road is its only road from here. */
  away?: boolean
  /** Why a press of PTT on this computer would not key the station, or `null`/absent when a press
   *  would (or did) key. Recorded by the station's own arming path — display only; the UI never
   *  decides whether a press keys. Absent on a station build older than the one that added it. */
  pttRefusal?: PttRefusal | null
  /** The station holds this computer from transmit, in any mode, as its snapshot says it: with
   *  `leftSecs` only when the station would lift the hold by itself. Display only. Absent/null when
   *  not held, and on a station build older than the one that added it. */
  txHold?: TxHold | null
}

/** The reasons a client PTT press does not key, as the station's arming path records them. The
 *  names are a backend contract (`client_status`), matched one-for-one by `ClientBanner`. */
export type PttRefusal =
  | 'notPhone'
  | 'txDisabled'
  | 'outsideLicence'
  | 'tuning'
  | 'otherOwner'
  | 'clockRepair'
  | 'onHold'
  | 'noPresence'
  | 'noFeed'
  | 'noAudio'

/** The test build's road switch: both roads as they are, or one of them blocked. */
export type BenchPosition = 'normal' | 'blockLink' | 'blockStopRoad'

/** One of this computer's audio devices: the name kept, and the label shown. */
export interface ClientAudioDevice {
  name: string
  label: string
}

/** This computer ▸ Audio (client mode): the output the station's audio plays on and the
 *  microphone, `''` for the system's default, how loud the station is heard here (dB over its own
 *  level, the hosted page's volume), this computer's devices, whether its headset started at all,
 *  and why a device would not open. Kept on this computer only. */
export interface ClientAudio {
  output: string
  input: string
  volumeDb: number
  outputs: ClientAudioDevice[]
  inputs: ClientAudioDevice[]
  available: boolean
  trouble: { output: string | null; input: string | null }
}

export async function clientAudio(): Promise<ClientAudio> {
  return invoke<ClientAudio>('client_audio')
}

/** Picks this computer's headset for the station's audio; kept in this computer's own file. */
export async function setClientAudio(output: string, input: string, volumeDb?: number): Promise<ClientAudio> {
  return invoke<ClientAudio>('set_client_audio', { output, input, volumeDb })
}

/** The microphone's loudest moment since the last ask, `0` to `1`, keeping it open a moment. */
export async function clientAudioLevel(): Promise<{
  level: number
  open: boolean
  trouble: string | null
}> {
  return invoke('client_audio_level')
}

/** A test note in this computer's output. */
export async function clientAudioTest(): Promise<{ played: boolean; trouble?: string | null }> {
  return invoke('client_audio_test')
}

/** The operator did something here (a click, a key, the wheel): it answers the internet road's
 *  "Still there?", and after a link nobody answered it for, connects again. */
export async function clientStillHere(): Promise<void> {
  return invoke<void>('client_still_here')
}

export async function clientStatus(): Promise<ClientStatus | null> {
  return invoke<ClientStatus | null>('client_status')
}

/** Moves a test build's road switch: where it is now. Blocking one road while the other is
 *  blocked is refused (`oneRoadAtATime`); a shipped build has no such command. */
export async function benchRoadSwitch(to: BenchPosition): Promise<BenchPosition> {
  return invoke<BenchPosition>('bench_road_switch', { to })
}

/** Take control from the device holding the station, once the operator confirmed it: that
 *  device's over ends as its Stop would, and this computer starts with TX off. */
export async function clientTakeControl(): Promise<void> {
  return invoke<void>('client_take_control')
}

/** Leave: this computer's Nexus starts again as itself, the station handed back first. With
 *  `pairAgain`, while the station refuses this computer in a way only a new pairing ends, the start
 *  that follows opens Stations on this network offering Pair again for it. */
export async function leaveClientMode(pairAgain?: boolean): Promise<void> {
  return invoke<void>('leave_client_mode', pairAgain ? { pairAgain } : undefined)
}

/** Load the durable UI-state store (`ui-state.json`, beside settings.json). See
 *  `features/durableStore.ts` for why some browser-storage keys are mirrored there. `strict`:
 *  a file that is there and does not read rejects, instead of reading as an empty store — for a
 *  flush about to merge its own changes into the file. */
export async function uiStateLoad(strict?: boolean): Promise<Record<string, string>> {
  return invoke<Record<string, string>>('ui_state_load', strict ? { strict: true } : undefined)
}

/** Replace the durable UI-state store. Returns whether the write landed — the caller keeps
 *  `localStorage` as a live fallback rather than trusting a save that did not happen. */
export async function uiStateSave(state: Record<string, string>): Promise<boolean> {
  return invoke<boolean>('ui_state_save', { state })
}

/** Distinct operators present in the log (#25). Empty for a single-op station — the Logbook
 *  uses that to decide whether a per-operator export is worth offering at all. */
export async function logOperators(): Promise<string[]> {
  return invoke<string[]>('log_operators')
}

/** ADIF for ONE operator's contacts (#25). POTA and Field Day both require each operator to
 *  submit their own log. */
export async function exportLogForOperator(operator: string): Promise<LogExport> {
  return invoke<LogExport>('export_log_for_operator', { operator })
}

/** Activations present in the log — your park × UTC day × the callsign you signed, newest
 *  first. Empty for a station that has never activated, which is how the Logbook decides
 *  whether to offer the per-activation export at all. */
export async function logActivations(): Promise<LoggedActivation[]> {
  return invoke<LoggedActivation[]>('log_activations')
}

/** ADIF for ONE activation — the file POTA wants: one park, one UTC day, one callsign.
 *  Takes NO date range on purpose; see the Logbook export area for why an activation
 *  SUPERSEDES the range instead of intersecting with it. */
export async function exportLogForActivation(
  reference: string,
  dayStartUnix: number,
  callsign: string | null,
): Promise<LogExport> {
  return invoke<LogExport>('export_log_for_activation', { reference, dayStartUnix, callsign })
}

/** The activation file `exportLogForActivation` made was saved: the station remembers it, so a later
 *  change to those contacts can say the file no longer matches the log. Asked only once the save
 *  succeeded. */
export async function noteActivationExported(
  reference: string,
  dayStartUnix: number,
  callsign: string | null,
): Promise<void> {
  return invoke<void>('note_activation_exported', { reference, dayStartUnix, callsign })
}

/** Everything that makes this station THIS station, as one JSON file (#28) — settings plus the
 *  durable UI state. Carries NO secrets: passwords and API keys live in the OS keychain, and the
 *  ClubLog key is redacted because ClubLog auto-revokes one that becomes public. */
export async function exportSettingsBundle(): Promise<string> {
  return invoke<string>('export_settings_bundle')
}

/**
 * Reset the configuration to factory defaults.
 *
 * Keeps the logbook (it lives outside the settings) and keeps stored credentials (they live in
 * the OS keychain — `clear*` verbs forget those, deliberately separately). Applies through the
 * same path a restore uses, so a running app reconfigures rather than writing the old config back.
 */
export async function resetSettings(): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('reset_settings')
}

/** Restore a bundle written by `exportSettingsBundle`. Refuses anything that is not one, by
 *  name and schema — a partial restore is worse than a refusal. */
export async function importSettingsBundle(text: string): Promise<AppSnapshot> {
  return invoke<AppSnapshot>('import_settings_bundle', { text })
}
