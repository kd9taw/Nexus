import { useEffect, useRef, useState } from 'react'
import { SAT_VFO_MAPS } from '../features/satVfo'
import { confirmSatUplink } from '../api'
import type {
  AudioDevices,
  BandChannel,
  CatTestResult,
  DetectedRig,
  RadioStatus,
  RouteMode,
  RoutingRule,
  Settings,
} from '../types'
import {
  clearCloudlogKey,
  clearClublogPassword,
  clearEqslPassword,
  clearHamqthPassword,
  clearHrdlogCode,
  clearLotwPassword,
  clearQrzLogbookKey,
  clearQrzPassword,
  detectRigs,
  downloadEqslReport,
  downloadLotwReport,
  getAllRigModels,
  getAudioDevices,
  getBandPlan,
  getRigModels,
  getSerialPortsDetailed,
  getSettings,
  setCloudlogKey,
  setClublogPassword,
  setEqslPassword,
  setHamqthPassword,
  setHrdlogCode,
  setLotwPassword,
  setQrzLogbookKey,
  setQrzPassword,
  setRepeaterbookToken,
  setRxGain,
  setSettings,
  setTxLevel,
  addRadio,
  removeRadio,
  renameRadio,
  setActiveRadio,
  setRadioBands,
  setRoutingRules,
  setDefaultRadio,
  routePreview,
  updateRadioProfile,
  type RadioProfilePatch,
  testCat,
  probeCatPorts,
  qrzTestConnection,
  syncQrz,
  n3fjpTestConnection,
} from '../api'
import { pushToast, withErrorToast } from '../toast'
import { loadProfiles, mergeProfile, saveProfile, deleteProfile, type Profile } from '../profiles'
import {
  getAssistanceJournal,
  getConnectionLog,
  getCredentialsStatus,
  setUnassistedMode,
} from '../api'
import { AssistanceNote } from './AssistanceNote'
import { fetchLotwUsers, getLotwUsersStatus, type LotwUsersStatus } from '../api'
import { fetchFccStates, getFccStatesStatus, type FccStatesStatus } from '../api'
import { fetchTlesNow, getTleStatus, importTles, type TleStatus } from '../api'
import { tleRefreshMessage } from '../features/tleMessages'
import { elementBandParts } from '../features/elementBands'
import { discoverFlex } from '../api'
import { civDiagnosticLog, civDiagnosticStatus } from '../api'
import { allTxtLocation, revealAllTxt } from '../api'
import { findDaxDevices, isDaxPaired } from '../features/dax'
import type { AssistanceEvent, ConnEvent, CredStatus } from '../types'
import { FrequencyControl } from './FrequencyControl'
import { rxLevelDb } from './LevelMeter'
import { LiveLevelMeter, LiveRxLevelDb } from './LiveMeters'
import { WatchlistPanel } from './WatchlistPanel'
import { MiniSpectrum } from './MiniSpectrum'
import { SettingsGroup } from './SettingsGroup'
import type { Scale, ScaleMode } from '../useScale'
import { SCALE_STEPS, fitScale } from '../useScale'
import type { Density } from '../useDensity'
import type { FeaturesApi } from '../useFeatures'
import { FEATURES, featureById, type FeatureCategory, type FeatureDef, type FeatureId } from '../features/registry'
import { PROFILE_LIST } from '../features/profiles'
import { checkForUpdateManual } from '../features/updateCheck'
import { ARRL_SECTIONS_BY_DIVISION } from '../features/arrlSections'

interface Props {
  /** Called after a successful save so the shell can refresh its snapshot. */
  onSaved?: () => void
  /** Live radio status, so the Audio section can show the real RX meter. */
  radio?: RadioStatus
  /** The live active radio id (dual-radio). The form reloads when this changes so a switch made from
   * the always-visible TopBar pills while Settings is open can't leave the Rig form stale. */
  activeRadioId?: number
  /** Prove the TX path — keys a bounded tune carrier (behind a confirm dialog) for the Setup Health
   * strip's "Prove TX" button. */
  onProveTx?: () => void
  /** Workspace scale prefs (UI-only — applied live, not via setSettings). */
  scale: Scale
  scaleMode: ScaleMode
  scaleCap: Scale
  onScaleModeChange: (m: ScaleMode) => void
  onScaleCapChange: (c: Scale) => void
  density: Density
  onDensityChange: (d: Density) => void
  onResetLayout: () => void
  /** Modular-features API (toggles + profiles). */
  features: FeaturesApi
  /** Re-open the first-run setup wizard. */
  onRerunWizard?: () => void
}

/** Display order for the Features section's category groups. */
const FEATURE_CATEGORY_ORDER: FeatureCategory[] = [
  'Operate',
  'DX & Awards',
  'Propagation',
  'Contesting',
  'POTA/SOTA',
  'Logging',
  'System',
]

type FieldKey = keyof Settings

// Flat ARRL/RAC section list + validity set for the Field Day section picker
// (spec §6). Derived once from the grouped universe; the datalist shows every
// section (code + name + division) and any typed value is validated against
// this set so an operator can only ship a real section in the Cabrillo log.
const FD_SECTION_OPTIONS = ARRL_SECTIONS_BY_DIVISION.flatMap((d) => d.sections)
const FD_SECTION_CODES = new Set(FD_SECTION_OPTIONS.map((s) => s.code))

// TODO (spec §6 grid-seed, deferred): pre-suggest the section from the operator's
// Maidenhead grid for single-section US states. This needs grid → lat/lon → state
// polygon lookup (point-in-polygon against us-atlas) → state → section, which is not
// trivially derivable on the frontend today (no `state_for_grid` mirror exists here);
// the honest single-section states are also a subset that a naive map would get wrong.
// Skipping rather than fabricating — the datalist + validation above already make the
// section fast and correct to pick by hand.

interface FieldDef {
  key: FieldKey
  label: string
  type: 'text' | 'number'
  placeholder: string
  hint?: string
}

// Operator basics (band / dial / sideband are handled by FrequencyControl).
const BASIC_FIELDS: FieldDef[] = [
  { key: 'mycall', label: 'Callsign', type: 'text', placeholder: 'KD9TAW', hint: 'Your station callsign (required).' },
  {
    key: 'mygrid',
    label: 'Grid',
    type: 'text',
    placeholder: 'EN52xa',
    hint: 'Maidenhead locator. All 6 characters — 4 measures every distance and bearing from the middle of a ~100-mile square.',
  },
  { key: 'opName', label: 'Operator name', type: 'text', placeholder: 'Seth', hint: 'Used by the CW {NAME} macro and logging.' },
  { key: 'opState', label: 'State', type: 'text', placeholder: 'WI', hint: 'Your US state/province — the CW {MYSTATE} macro (ragchew QTH).' },
]

const PTT_METHODS: { value: string; label: string }[] = [
  { value: 'cat', label: 'CAT (via rigctld)' },
  { value: 'rts', label: 'Serial RTS' },
  { value: 'dtr', label: 'Serial DTR' },
  { value: 'vox', label: 'VOX (no keying)' },
]

// Standard EIA CTCSS (PL) tones, Hz — for the FM repeater-access tone picker.
const CTCSS_TONES = [
  67.0, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2,
  110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 162.2,
  167.9, 173.8, 179.9, 186.2, 192.8, 203.5, 210.7, 218.1, 225.7, 233.6, 241.8, 250.3,
]

const NUMERIC_KEYS: FieldKey[] = ['dialMhz', 'baud', 'rigctldPort', 'rigModel', 'txWatchdogMin', 'catBrokerPort', 'tuneTimeoutSecs', 'aprsIsPort', 'aprsIsRadiusKm', 'aprsStationTtlMin']

// Standard serial CAT baud rates offered in the Rig baud picker. A rig's manual lists its
// supported rate(s); most modern rigs run 38400 or 115200. Auto-detect may set a value outside
// this list — the picker keeps it as an extra option so it's never silently dropped.
const STANDARD_BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200]

/** WSJT-X Split Operation choices (Settings ▸ Radio parity). */
const SPLIT_MODES: { value: NonNullable<Settings['splitMode']>; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'rig', label: 'Rig' },
  { value: 'fakeit', label: 'Fake It' },
]

// SAT_VFO_MAPS moved to features/satVfo.ts — the Satellites readiness rail
// mirrors this setting live, and the two surfaces must share ONE label list.

/** What the rotator does when a pass ends. */
const ROT_POST_PASS: { value: string; label: string }[] = [
  { value: 'stop', label: 'Stop — leave the antenna where the pass ended' },
  { value: 'park', label: 'Park — drive to the park position' },
  { value: 'ready', label: 'Ready — drive to the ready position' },
]

/** One operator override of the working-frequency table. */
type WorkingFrequency = NonNullable<Settings['workingFrequencies']>[number]

/** The stock WSJT-X working-frequency table, shown read-only for reference.
 * An override replaces the matching band+mode row; no overrides = stock. */
const STOCK_WORKING_FREQUENCIES: WorkingFrequency[] = [
  { band: '160m', mode: 'FT8', mhz: 1.84 },
  { band: '80m', mode: 'FT8', mhz: 3.573 },
  { band: '60m', mode: 'FT8', mhz: 5.3715 },
  { band: '40m', mode: 'FT8', mhz: 7.074 },
  { band: '30m', mode: 'FT8', mhz: 10.136 },
  { band: '20m', mode: 'FT8', mhz: 14.074 },
  { band: '17m', mode: 'FT8', mhz: 18.1 },
  { band: '15m', mode: 'FT8', mhz: 21.074 },
  { band: '12m', mode: 'FT8', mhz: 24.915 },
  { band: '10m', mode: 'FT8', mhz: 28.074 },
  { band: '6m', mode: 'FT8', mhz: 50.313 },
  { band: '4m', mode: 'FT8', mhz: 70.154 },
  { band: '2m', mode: 'FT8', mhz: 144.174 },
  { band: '70cm', mode: 'FT8', mhz: 432.065 },
  { band: '23cm', mode: 'FT8', mhz: 1296.174 },
  { band: '80m', mode: 'FT4', mhz: 3.575 },
  { band: '40m', mode: 'FT4', mhz: 7.0475 },
  { band: '30m', mode: 'FT4', mhz: 10.14 },
  { band: '20m', mode: 'FT4', mhz: 14.08 },
  { band: '17m', mode: 'FT4', mhz: 18.104 },
  { band: '15m', mode: 'FT4', mhz: 21.14 },
  { band: '12m', mode: 'FT4', mhz: 24.919 },
  { band: '10m', mode: 'FT4', mhz: 28.18 },
  { band: '6m', mode: 'FT4', mhz: 50.318 },
  { band: '2m', mode: 'FT4', mhz: 144.17 },
]

/** Bands offered in the override editor AND in the two band-routing surfaces (per-radio
 * coverage chips, routing-rule bands, the routing test) — the four lists share this one.
 * So it is not "the stock table's coverage": it must cover every stock row (or a row is
 * unreachable to override — 4 m was) plus every band a station might route a rig to.
 * 1.25 m is here for the routing half — no FT8/FT4 row exists upstream, but the band is
 * fully supported (`privileges::VHF` 222–225 all-mode from Technician up, `band_for_dial`,
 * `cw_activity_mhz`, native + Q65/JT65 channels) and a 1.25 m rig could not be declared.
 * 33 cm is deliberately absent: `band_for_dial` cannot name a 902 MHz dial and the
 * privilege table has no 902–928 segment, so offering it would produce an unlabellable
 * frequency. Modes stay FT8/FT4 — the override table's own scope. */
const FREQ_BANDS = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '1.25m', '70cm', '23cm']
const FREQ_MODES = ['FT8', 'FT4']

/** The mode classes radio ROUTING decides on, with the operator-facing labels (must match the Rust
 * `RouteMode::label`). Coarser than the submode list on purpose: five rules cover a whole station. */
const ROUTE_MODES: [RouteMode, string][] = [
  ['digital', 'Weak-signal digital'],
  ['fm', 'FM & APRS'],
  ['ssb', 'SSB phone'],
  ['cw', 'CW'],
  ['rtty', 'RTTY'],
]
const ROUTE_MODE_LABEL: Record<RouteMode, string> = Object.fromEntries(ROUTE_MODES) as Record<
  RouteMode,
  string
>

/** Settings is split into tabbed sections: only the active one renders, so a
 * keystroke re-renders ~one section's worth of inputs instead of the whole panel
 * (fixes typing lag) — and it tames the single-giant-scroll wall. */
// Consolidated 8-tab IA (0.17.0): identity → radio (CAT+audio) → the operating modes → frequencies
// → what-am-I-told (spots+alerts) → where-QSOs-go (logging+connectors) → contesting → app prefs.
// Old 14-tab ids fold into these: rig+audio→'radio', phone/digital/cw/rtty→'modes',
// alerts→'spots', confirmations+connections→'logging', fieldday→'contesting',
// workspace+features→'appearance'. The render blocks are grouped by tab, not moved.
type SettingsTab =
  | 'station'
  | 'radio'
  | 'modes'
  | 'frequencies'
  | 'spots'
  | 'logging'
  | 'contesting'
  | 'appearance'

const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  { id: 'station', label: 'Station' },
  { id: 'radio', label: 'Radio' },
  { id: 'modes', label: 'Modes' },
  { id: 'frequencies', label: 'Frequencies' },
  { id: 'spots', label: 'Spots & Alerts' },
  { id: 'logging', label: 'Logging & Connectors' },
  { id: 'contesting', label: 'Contesting' },
  { id: 'appearance', label: 'Appearance' },
]

/** Pick just the per-radio CAT/audio/PTT/rotator/native fields — the flat rig form and a radio
 * profile share these exact field names, so this serves BOTH directions: build the save patch
 * from the form, and load a radio's profile into the form (`{...form, ...radioPatch(profile)}`). */
function radioPatch(s: Partial<RadioProfilePatch>): RadioProfilePatch {
  // `??` only fills genuinely-absent (null/undefined) fields — 0 / '' legit values are preserved.
  return {
    pttMethod: s.pttMethod ?? 'vox',
    rigModel: s.rigModel ?? 0,
    rigModelName: s.rigModelName ?? '',
    serialPort: s.serialPort ?? '',
    pttSerialPort: s.pttSerialPort ?? '',
    baud: s.baud ?? 38400,
    rigConn: s.rigConn ?? 'serial',
    rigAddr: s.rigAddr ?? '',
    rigctldPort: s.rigctldPort ?? 4532,
    icomNativeCat: s.icomNativeCat ?? false,
    dataModesPlainSsb: s.dataModesPlainSsb ?? false,
    audioIn: s.audioIn ?? '',
    audioOut: s.audioOut ?? '',
    txLevel: s.txLevel ?? 1,
    rxGain: s.rxGain ?? 1,
    rotatorModel: s.rotatorModel ?? 0,
    rotatorPort: s.rotatorPort ?? '',
    rotatorBaud: s.rotatorBaud ?? 9600,
    rotatorHost: s.rotatorHost ?? '',
    rotctldPort: s.rotctldPort ?? 4533,
    nativeScope: s.nativeScope ?? 'auto',
  }
}

/** Setup Health — "is the station actually working?" made visible, so setup stops running on
 * faith (0.17.0). Reads live snapshot state: Rig (CAT responding), RX audio (level/error), and
 * whether TX is armed. A live Test-CAT result, when present, overrides the passive CAT state.
 * Reusable — the wizard finale and a cockpit indicator can render the same strip later. */
function SetupHealth({
  radio,
  catResult,
  onProveTx,
}: {
  radio?: {
    catOk?: boolean | null
    catDetail?: string
    rxLevel: number
    audioError?: string | null
    txEnabled: boolean
    tuning?: boolean
    txPower?: number | null
  }
  catResult: CatTestResult | null
  /** Key a bounded tune carrier to prove the CAT→PTT→RF path (behind a confirm dialog). */
  onProveTx?: () => void
}) {
  const rigOk = catResult ? catResult.ok : radio?.catOk
  const rigDetail = catResult ? catResult.detail : radio?.catDetail
  const rxDb = radio ? Math.round(rxLevelDb(radio.rxLevel)) : null
  const rxLive = rxDb != null && rxDb > -60 && !radio?.audioError
  const cls = (ok?: boolean | null) => (ok === true ? 'ok' : ok === false ? 'bad' : 'unknown')
  const tuning = !!radio?.tuning
  const watts = radio?.txPower ?? null
  // While keying: green once forward power registers (RF is being made → CAT/PTT/rig all work).
  const txClass = tuning ? (watts != null && watts > 0 ? 'ok' : 'bad') : 'unknown'
  return (
    <div className="setup-health" role="status" aria-label="Setup health">
      <span className="setup-health-title">Setup health</span>
      <span
        className={`health-item ${cls(rigOk)}`}
        title={rigDetail || 'CAT not tested yet — use Test CAT below'}
      >
        <span className="health-dot" /> Rig{' '}
        {rigOk === true ? 'responding' : rigOk === false ? 'not answering' : 'untested'}
      </span>
      <span
        className={`health-item ${radio?.audioError ? 'bad' : rxLive ? 'ok' : 'unknown'}`}
        title={
          radio?.audioError || (rxLive ? 'Receiving audio' : 'No RX audio — check the audio device below')
        }
      >
        <span className="health-dot" /> RX audio{' '}
        {radio?.audioError ? 'error' : rxDb != null ? `${rxDb} dB` : '—'}
      </span>
      <span
        className={`health-item ${txClass}`}
        title={
          tuning
            ? 'Keying a tune carrier — forward power confirms the CAT → PTT → RF path'
            : radio?.txEnabled
              ? 'Transmit is enabled'
              : 'Transmit is off'
        }
      >
        <span className="health-dot" /> TX{' '}
        {tuning
          ? `keying${watts != null ? ` · ${watts.toFixed(0)} W` : '…'}`
          : radio?.txEnabled
            ? 'on'
            : 'off'}
      </span>
      {onProveTx && !tuning && (
        <button
          type="button"
          className="np-chip health-prove"
          onClick={() => {
            if (
              window.confirm(
                'Prove the transmit path?\n\nThis keys your transmitter for ~2 seconds at your tune ' +
                  'power. Make sure an antenna or dummy load is connected.',
              )
            )
              onProveTx()
          }}
          title="Key a 2 s tune carrier to verify CAT → PTT → RF (asks first, every time)"
        >
          Prove TX
        </button>
      )}
    </div>
  )
}

// Public human DX-cluster nodes (SSB/phone + human spots) — the RBN CW + digital skimmer
// feeds connect automatically, so these are for the phone/human spots RBN doesn't carry.
// Researched, community-trusted, callsign-only login. (NOT RBN ports — those are wired
// separately; the global human-spot mesh means any well-connected node has the same spots.)
const CLUSTER_PRESETS: { label: string; host: string }[] = [
  { label: 'VE7CC-1 — human SSB/CW, clean (recommended)', host: 've7cc.net:23' },
  { label: 'WA9PIE-2 — port 8000 (use if port 23 is blocked)', host: 'dxc.wa9pie.net:8000' },
  { label: 'W1NR — DXSpider, phone-rich', host: 'dx.w1nr.net:23' },
  { label: 'W3LPL — firehose (skimmer-heavy)', host: 'w3lpl.net:7373' },
]

export function SettingsPanel({
  onSaved,
  radio,
  activeRadioId,
  onProveTx,
  scale,
  scaleMode,
  scaleCap,
  onScaleModeChange,
  onScaleCapChange,
  density,
  onDensityChange,
  onResetLayout,
  features,
  onRerunWizard,
}: Props) {
  const [form, setForm] = useState<Settings | null>(null)
  // Highest scale the CURRENT window can auto-fit (Auto never upscales past what
  // fits). Cap chips above this are dead — they all yield this same scale — so we
  // disable them and say why (operator report: "150 isn't bigger than 175"). Tracks
  // live: a bigger window / monitor re-enables the higher chips.
  const MAX_STEP = SCALE_STEPS[SCALE_STEPS.length - 1]
  const [autoCeil, setAutoCeil] = useState<Scale>(() =>
    fitScale(window.innerWidth, window.innerHeight, MAX_STEP),
  )
  useEffect(() => {
    const onResize = () =>
      setAutoCeil(fitScale(window.innerWidth, window.innerHeight, MAX_STEP))
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [MAX_STEP])
  const [allTxtPath, setAllTxtPath] = useState('')
  const [status, setStatus] = useState<'idle' | 'loading' | 'saving' | 'saved'>('loading')
  const [error, setError] = useState<string | null>(null)
  const [rigModels, setRigModels] = useState<[number, string][]>([])
  // Full Hamlib catalog (thousands of entries) — fetched lazily only when the
  // operator checks "Show all models", so the common case (curated ~50) stays fast.
  const [allRigModels, setAllRigModels] = useState<[number, string][]>([])
  const [allRigModelsLoading, setAllRigModelsLoading] = useState(false)
  const [showAllRigModels, setShowAllRigModels] = useState(false)
  const [serialPorts, setSerialPorts] = useState<string[]>([])
  // Port -> USB product label ("USB-Enhanced-SERIAL-B CH342"), so the picker can tell a
  // dual-serial rig's two interfaces apart (Xiegu CAT is on SERIAL-B).
  const [portLabels, setPortLabels] = useState<Record<string, string>>({})
  const applyPorts = (infos: { name: string; label: string }[]) => {
    setSerialPorts(infos.map((i) => i.name))
    setPortLabels(Object.fromEntries(infos.map((i) => [i.name, i.label])))
  }
  // Native CI-V bus diagnostic log: null = off, string = the log file path while capturing.
  // Transient (not persisted) — a support tool the operator arms to capture a fault. The
  // backend keeps logging while Settings is closed (so the operator can go transmit), so the
  // toggle reads its real state on mount instead of resetting to "off" on every return here.
  const [civLogPath, setCivLogPath] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    civDiagnosticStatus()
      .then((p) => {
        if (alive) setCivLogPath(p === '' ? null : p)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])
  useEffect(() => {
    let alive = true
    allTxtLocation()
      .then((p) => {
        if (alive) setAllTxtPath(p)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])
  const [profiles, setProfiles] = useState<Profile[]>(() => loadProfiles())
  const [selectedProfile, setSelectedProfile] = useState('')
  const [newProfileName, setNewProfileName] = useState('')
  const [bandPlan, setBandPlan] = useState<BandChannel[]>([])
  // Device NAMES (the values stored in settings and resolved at open time) kept separate
  // from their display labels — the same split `applyPorts` uses for serial ports above.
  // Every option builder, `includes()` guard and DAX matcher below therefore keeps working
  // on plain strings; only what the operator READS comes from the label map.
  const [audio, setAudio] = useState<{ input: string[]; output: string[] }>({ input: [], output: [] })
  // Device name -> human label, kept PER DIRECTION. On Windows/macOS they are the same
  // string; on Linux the name is an ALSA PCM name and the label is the card description —
  // and one PCM can legitimately read differently in the two lists, because the label
  // shortens only where its card is unambiguous within that list. On the reported machine
  // `plughw:CARD=Generic,DEV=0` is the card's only capture entry ("HD-Audio Generic") but
  // one of two playback entries ("HD-Audio Generic, ALC1220 Analog"). A single merged map
  // showed the output's label in the input picker (caught by the new picker test).
  const [audioLabels, setAudioLabels] = useState<{
    input: Record<string, string>
    output: Record<string, string>
  }>({ input: {}, output: {} })
  const applyAudio = (d: AudioDevices) => {
    setAudio({ input: d.input.map((x) => x.name), output: d.output.map((x) => x.name) })
    setAudioLabels({
      input: Object.fromEntries(d.input.map((x) => [x.name, x.label])),
      output: Object.fromEntries(d.output.map((x) => [x.name, x.label])),
    })
  }
  const [portsLoading, setPortsLoading] = useState(false)
  const [audioLoading, setAudioLoading] = useState(false)
  const [detected, setDetected] = useState<DetectedRig[]>([])
  const [detectedFlex, setDetectedFlex] = useState<{ model: string; nickname: string; ip: string }[]>([])
  const [detecting, setDetecting] = useState(false)
  const [catTesting, setCatTesting] = useState(false)
  const [catResult, setCatResult] = useState<CatTestResult | null>(null)
  // Connections visibility: stored-credential status + the rolling event log —
  // the answer to "I hit save and couldn't tell anything happened".
  const [creds, setCreds] = useState<CredStatus[]>([])
  // ARRL LoTW user-activity list (the decode/roster LoTW marks).
  // Rotator "Other model" entry: UI mode + text live in LOCAL state so a
  // sentinel can never leak into the form (review catch: -1 in the payload
  // failed serde's u32 and rejected the ENTIRE settings save).
  // Find-my-Flex discovery (network rig section).
  const [rotOther, setRotOther] = useState(false)
  const [rotCustom, setRotCustom] = useState('')
  const [lotwUsers, setLotwUsers] = useState<LotwUsersStatus | null>(null)
  const [lotwFetching, setLotwFetching] = useState(false)
  useEffect(() => {
    getLotwUsersStatus()
      .then(setLotwUsers)
      .catch(() => {})
  }, [])
  // FCC callsign→state index (drives New-State on grid-less cluster/CW/SSB spots).
  const [fccStates, setFccStates] = useState<FccStatesStatus | null>(null)
  const [fccFetching, setFccFetching] = useState(false)
  useEffect(() => {
    getFccStatesStatus()
      .then(setFccStates)
      .catch(() => {})
  }, [])
  // Orbital elements (TLE snapshot) — the satellite currency pipeline's
  // operator surface: status + manual refresh + the file-import escape hatch.
  const [tleStatus, setTleStatus] = useState<TleStatus | null>(null)
  const [tleFetching, setTleFetching] = useState(false)
  const [tleImporting, setTleImporting] = useState(false)
  const tleFileRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    getTleStatus()
      .then(setTleStatus)
      .catch(() => {})
  }, [])
  // "Saved" must not linger forever (it read as a stale artifact) — fade it out.
  // QRZ connection test: a real STATUS round-trip (validates the Logbook API
  // key without inserting anything). idle | testing | the result line.
  const [qrzTest, setQrzTest] = useState<{ state: 'idle' | 'testing' | 'ok' | 'fail'; msg: string }>({ state: 'idle', msg: '' })
  const runQrzTest = () => {
    setQrzTest({ state: 'testing', msg: 'testing…' })
    qrzTestConnection()
      .then((msg) => setQrzTest({ state: 'ok', msg }))
      .catch((e) => setQrzTest({ state: 'fail', msg: String(e) }))
  }
  const [n3fjpTest, setN3fjpTest] = useState<{ state: 'idle' | 'testing' | 'ok' | 'fail'; msg: string }>({ state: 'idle', msg: '' })
  const runN3fjpTest = () => {
    setN3fjpTest({ state: 'testing', msg: 'testing…' })
    n3fjpTestConnection()
      .then((msg) => setN3fjpTest({ state: 'ok', msg }))
      .catch((e) => setN3fjpTest({ state: 'fail', msg: String(e) }))
  }
  useEffect(() => {
    if (status !== 'saved') return
    const id = window.setTimeout(() => setStatus('idle'), 2500)
    return () => window.clearTimeout(id)
  }, [status])
  const [connLog, setConnLog] = useState<ConnEvent[]>([])
  // The assistance journal is the operator's EVIDENCE of what was running during an event, so
  // it is shown next to the switch rather than hidden in a file. Same poll as the conn log.
  const [assistLog, setAssistLog] = useState<AssistanceEvent[]>([])
  useEffect(() => {
    let live = true
    const load = () => {
      getCredentialsStatus().then((c) => live && setCreds(c)).catch(() => {})
      getConnectionLog().then((l) => live && setConnLog(l)).catch(() => {})
      getAssistanceJournal().then((l) => live && setAssistLog(l ?? [])).catch(() => {})
    }
    load()
    const id = window.setInterval(load, 5_000)
    return () => {
      live = false
      window.clearInterval(id)
    }
  }, [])
  // LoTW/eQSL passwords are write-only (kept in the OS keychain, never read back),
  // so they live in local state — not in `form`/Settings.
  const [lotwPw, setLotwPw] = useState('')
  const [lotwSyncing, setLotwSyncing] = useState(false)
  const [eqslPw, setEqslPw] = useState('')
  const [eqslSyncing, setEqslSyncing] = useState(false)
  const [qrzPw, setQrzPw] = useState('')
  const [qrzKey, setQrzKey] = useState('')
  const [qrzSyncing, setQrzSyncing] = useState(false)
  const [hamqthPw, setHamqthPw] = useState('')
  const [clublogPw, setClublogPw] = useState('')
  const [hrdlogCode, setHrdlogCodeField] = useState('')
  const [rbToken, setRbTokenField] = useState('')
  const [cloudlogKey, setCloudlogKeyField] = useState('')
  const [tab, setTab] = useState<SettingsTab>('station')
  // Which radio the Radio-tab form is currently EDITING — decoupled from which radio is operating
  // (activeRadioId). Editing a non-active radio writes just that profile (no live rig swap).
  const [editingRadioId, setEditingRadioId] = useState<number | undefined>(activeRadioId)
  // In-progress MHz text for the override row being edited — committed only when
  // it parses as a positive number, so a half-typed "14." never corrupts the form.
  const [mhzDraft, setMhzDraft] = useState<{ idx: number; text: string } | null>(null)
  // The routing "test" probe: which (band, mode) the operator asked about, and the radio name the
  // backend resolved it to. `null` result = not asked yet / the probe failed.
  const [routeTest, setRouteTest] = useState<{ band: string; mode: RouteMode }>({
    band: '2m',
    mode: 'fm',
  })
  const [routeTestResult, setRouteTestResult] = useState<string | null>(null)

  // Dual-radio: if the active radio changes underneath us (the operator used the always-visible
  // TopBar switcher pills while Settings is open), the flat Rig/CAT form now describes the WRONG
  // radio. Reload the form from the live settings so a Save can't fold edits into — or command —
  // the wrong rig. Skip the first observation (the mount effect already loads the form).
  const lastActiveRef = useRef<number | undefined>(undefined)
  useEffect(() => {
    if (activeRadioId == null) return
    if (lastActiveRef.current === undefined) {
      lastActiveRef.current = activeRadioId
      return
    }
    if (lastActiveRef.current === activeRadioId) return
    lastActiveRef.current = activeRadioId
    void getSettings()
      .then((s) => {
        setForm(s)
        dirtyRef.current = false
        setEditingRadioId(activeRadioId) // form now mirrors the (new) active radio
      })
      .catch(() => {})
  }, [activeRadioId])

  useEffect(() => {
    let mounted = true
    setStatus('loading')
    getSettings()
      .then((s) => {
        if (mounted) {
          setForm(s)
          dirtyRef.current = false
          setStatus('idle')
        }
      })
      .catch(() => mounted && setStatus('idle'))
    getRigModels()
      .then((m) => mounted && setRigModels(m))
      .catch(() => {})
    getSerialPortsDetailed()
      .then((infos) => mounted && applyPorts(infos))
      .catch(() => {})
    getBandPlan()
      .then((b) => mounted && setBandPlan(b))
      .catch(() => {})
    getAudioDevices()
      .then((d) => mounted && applyAudio(d))
      .catch(() => {})
    return () => {
      mounted = false
    }
  }, [])

  const refreshPorts = () => {
    setPortsLoading(true)
    getSerialPortsDetailed()
      .then(applyPorts)
      .catch(() => {})
      .finally(() => setPortsLoading(false))
  }

  const refreshAudio = () => {
    setAudioLoading(true)
    getAudioDevices()
      .then(applyAudio)
      .catch(() => {})
      .finally(() => setAudioLoading(false))
  }

  const updateNum = (key: FieldKey, value: number) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, [key]: value } : prev))
  }

  // Per-mode RF-power ceiling: shown as a PERCENT (0–100), stored as a 0–1 fraction; empty = no
  // cap (null). Clamped 0–100 so a fat-finger can't store a >100% "cap"; junk input is ignored.
  const updatePowerCap = (key: 'maxPowerPhone' | 'maxPowerCw' | 'maxPowerDigital', pct: string) => {
    markDirty()
    const t = pct.trim()
    const frac = t === '' ? null : Math.min(100, Math.max(0, Number(t))) / 100
    setForm((prev) =>
      prev
        ? { ...prev, [key]: t !== '' && !Number.isFinite(frac as number) ? prev[key] : frac }
        : prev,
    )
  }
  const capPct = (v: number | null | undefined): string => (v == null ? '' : String(Math.round(v * 100)))

  // Apply RX capture gain to the LIVE audio stream (not just the form). Called on
  // release — `set_rx_gain` persists + returns a snapshot, so we commit once the
  // operator lets go of the slider rather than on every drag tick (which would
  // disk-thrash). The RX Level meter then responds within a poll, so the control
  // visibly "works" instead of appearing dead until a full Save.
  const applyRxGainLive = (value: number) => {
    void setRxGain(value).catch(() => pushToast('Could not apply RX gain', 'error'))
  }

  // Apply TX drive (Pwr) to the LIVE radio, the SAME value as the cockpit "Pwr" slider —
  // set_tx_level persists + updates the snapshot, so the cockpit reflects it immediately.
  // Commit on release (not per drag tick) to avoid disk-thrash.
  const applyTxLevelLive = (value: number) => {
    void setTxLevel(value).catch(() => pushToast('Could not set TX power', 'error'))
  }

  // Keep the Tx Power slider in step with the cockpit "Pwr" control — both are the SAME value
  // (radio.txLevel). When the cockpit changes it, mirror the live value into the form so the slider
  // tracks it (previously the form was a stale copy, so cockpit→Settings never showed). setForm
  // directly (not updateNum) so this live-sync never spuriously marks the form dirty.
  useEffect(() => {
    const live = radio?.txLevel
    if (live == null) return
    setForm((prev) => (prev && Math.abs(prev.txLevel - live) > 1e-6 ? { ...prev, txLevel: live } : prev))
  }, [radio?.txLevel])

  // Tracks unsaved flat-form edits, so switching the active radio (which reloads the form) can warn
  // before discarding them. A ref (not state) — read synchronously in the switch handler, no re-render.
  const dirtyRef = useRef(false)
  const markDirty = () => {
    dirtyRef.current = true
    setStatus('idle')
    setError(null)
  }

  const update = (key: FieldKey, raw: string) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      if (NUMERIC_KEYS.includes(key)) {
        const num = raw === '' ? 0 : Number(raw)
        return { ...prev, [key]: Number.isNaN(num) ? (prev[key] as number) : num }
      }
      return { ...prev, [key]: raw }
    })
  }

  const updateBool = (key: FieldKey, value: boolean) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, [key]: value } : prev))
  }

  // Per-alert band-scope selects (new DXCC / new grid / rare grid). The three selects ARE the
  // truth: the stored `alertNew` master is DERIVED from them (on = any scope not Off), never an
  // independent hidden toggle — which previously stuck `true` even after every scope was set to
  // Off (only ever set true, never back to false), silently keeping the alert engine "armed" with
  // nothing selected. When re-materializing from an old master-off state, show only the scope just
  // changed so enabling one type can't resurrect the rest.
  const ALERT_SCOPE_KEYS = ['alertDxccBands', 'alertGridBands', 'alertRareGridBands'] as const
  const changeAlertScope = (key: (typeof ALERT_SCOPE_KEYS)[number], value: string) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const next = { ...prev, [key]: value }
      if (!prev.alertNew) {
        for (const k of ALERT_SCOPE_KEYS) if (k !== key) next[k] = 'off'
      }
      next.alertNew = ALERT_SCOPE_KEYS.some((k) => (next[k] ?? 'off') !== 'off')
      return next
    })
  }

  // The DX-cluster node list (SSB/phone aggregator). Functional update so rapid edits
  // (add/remove/edit a row) never race on a stale `form` capture.
  const mutateClusterHosts = (fn: (hosts: string[]) => string[]) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, clusterHosts: fn(prev.clusterHosts ?? []) } : prev))
  }

  // The APRS-IS budlist. Edited as one comma-separated field rather than a row editor: it is a
  // short list of callsigns, and typing them is faster than adding rows one at a time.
  const setWatchCalls = (calls: string[]) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, aprsIsWatchCalls: calls } : prev))
  }

  // Optional numeric fields ('' = null = feature off) — `update` coerces '' to 0,
  // which would silently mean "cap at 0" instead of "no cap".
  const updateNullableNum = (key: FieldKey, raw: string, min: number) => {
    markDirty()
    const v = raw === '' ? null : Math.max(min, Number(raw))
    setForm((prev) =>
      prev ? { ...prev, [key]: Number.isNaN(v as number) ? null : v } : prev,
    )
  }

  // Macros are edited as comma-separated text per context; commit on change.
  // CW F-key macro editor. Empty/unset = the cockpit's built-in defaults; "Customize"
  // seeds the editor from them so the operator tweaks rather than starts blank.
  // The Guided copilot recommends the next F-key by ROLE (CQ → answer → report → 73):
  // customization changes the TEXT each key sends, never the key's job — so the
  // recommended-key highlight and the auto call fill (!) keep working for everyone.
  const CW_MACRO_ROLES: Record<string, string> = {
    F1: 'CQ',
    F2: 'Answer a station',
    F3: 'Send report',
    F4: 'Sign off (73)',
    F5: 'My call',
    F6: 'His call',
    F7: 'Ask repeat',
    F8: 'Query',
  }
  const CW_MACRO_DEFAULTS: { key: string; label: string; text: string }[] = [
    { key: 'F1', label: 'CQ', text: 'CQ CQ DE {MYCALL} {MYCALL} K' },
    { key: 'F2', label: 'Call', text: '! DE {MYCALL} {MYCALL} K' },
    { key: 'F3', label: 'Reply', text: '! DE {MYCALL} UR {RST} {RST} NAME {NAME} {NAME} HW? KN' },
    { key: 'F4', label: '73', text: '! DE {MYCALL} TU 73 SK' },
    { key: 'F5', label: 'My Call', text: '{MYCALL}' },
    { key: 'F6', label: 'His Call', text: '! ' },
    { key: 'F7', label: 'AGN', text: 'AGN AGN' },
    { key: 'F8', label: '?', text: '? ' },
  ]
  // --- CW macro profiles (named F-key sets; the active one drives the cockpit) ---
  // The backend migrates the legacy flat `cw` list into a "Default" profile on load, so
  // `cwProfiles` always has at least one entry. The macro grid below edits the ACTIVE
  // profile's macros; an empty macro list means the cockpit's built-in F1–F8 defaults.
  const cwProfiles = form?.macros.cwProfiles ?? []
  const activeCwIdx = form?.macros.activeCwProfile ?? 0
  const activeCwMacros = cwProfiles[activeCwIdx]?.macros ?? []

  const selectCwProfile = (i: number) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, macros: { ...prev.macros, activeCwProfile: i } } : prev))
  }
  const addCwProfile = () => {
    const name = window.prompt('New CW macro profile name:', `Profile ${cwProfiles.length + 1}`)?.trim()
    if (!name) return
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const list = prev.macros.cwProfiles ?? []
      const cur = list[prev.macros.activeCwProfile ?? 0]?.macros ?? []
      // Seed from the current profile's macros (a copy) so a new set starts editable, or
      // the built-in defaults when the current profile is still on the built-ins (empty).
      const seed = cur.length ? cur.map((m) => ({ ...m })) : CW_MACRO_DEFAULTS.map((m) => ({ ...m }))
      const next = [...list, { name, macros: seed }]
      return { ...prev, macros: { ...prev.macros, cwProfiles: next, activeCwProfile: next.length - 1 } }
    })
  }
  const renameCwProfile = () => {
    const cur = cwProfiles[activeCwIdx]
    if (!cur) return
    const name = window.prompt('Rename CW macro profile:', cur.name)?.trim()
    if (!name) return
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const idx = prev.macros.activeCwProfile ?? 0
      const list = (prev.macros.cwProfiles ?? []).map((p, i) => (i === idx ? { ...p, name } : p))
      return { ...prev, macros: { ...prev.macros, cwProfiles: list } }
    })
  }
  const deleteCwProfile = () => {
    if (cwProfiles.length <= 1) return // never delete the last profile
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const idx = prev.macros.activeCwProfile ?? 0
      const list = (prev.macros.cwProfiles ?? []).filter((_, i) => i !== idx)
      return {
        ...prev,
        macros: { ...prev.macros, cwProfiles: list, activeCwProfile: Math.min(idx, list.length - 1) },
      }
    })
  }
  // Seed / clear the ACTIVE profile's macros (empty list = the built-in defaults).
  const setCwMacros = (macros: { key: string; label: string; text: string }[]) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const idx = prev.macros.activeCwProfile ?? 0
      const list = (prev.macros.cwProfiles ?? []).map((p, i) => (i === idx ? { ...p, macros } : p))
      return { ...prev, macros: { ...prev.macros, cwProfiles: list } }
    })
  }
  const updateCwMacro = (i: number, field: 'label' | 'text', value: string) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const idx = prev.macros.activeCwProfile ?? 0
      const list = (prev.macros.cwProfiles ?? []).map((p, pi) => {
        if (pi !== idx) return p
        return { ...p, macros: p.macros.map((m, mi) => (mi === i ? { ...m, [field]: value } : m)) }
      })
      return { ...prev, macros: { ...prev.macros, cwProfiles: list } }
    })
  }

  const updateMacros = (ctx: keyof Settings['macros'], raw: string) => {
    markDirty()
    const list = raw
      .split(',')
      .map((x) => x.trim())
      .filter((x) => x.length > 0)
    setForm((prev) =>
      prev ? { ...prev, macros: { ...prev.macros, [ctx]: list } } : prev,
    )
  }

  // Wanted watch list: comma-separated exact calls or trailing-* wildcard prefixes
  // (e.g. "VP8*, 3Y0J") that raise a loud alert even on a worked station.
  /** WSJT-X Split Operation (none | rig | fakeit). */
  const setSplitMode = (m: NonNullable<Settings['splitMode']>) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, splitMode: m } : prev))
  }

  /** Choosing a VFO mapping IS confirming it — for the OPERATING (active)
   * radio, resolved by the BACKEND at write time: the pick invokes the
   * `confirmSatUplink` verb with no radio id, so a form snapshot that went
   * stale while the panel sat open (the active radio can change elsewhere)
   * can never record consent for the wrong rig. Write-through, not
   * form-buffered — the consent pair is engine-owned live state that the
   * Save payload cannot carry, which is what keeps a stale snapshot from
   * resurrecting a pruned consent. The select still disables itself in the
   * per-radio Edit flow: a live pick there would confirm the operating
   * radio while the panel shows another rig's card. A second radio is
   * confirmed on the pass rail. */
  const setSatVfoMap = (m: NonNullable<Settings['satVfoMap']>) => {
    // Mirror into the form for display only — the backend owns the value.
    setForm((prev) => (prev ? { ...prev, satVfoMap: m } : prev))
    void confirmSatUplink(m).catch(() =>
      pushToast('Could not confirm the VFO mapping', 'error'),
    )
  }

  // --- working-frequency overrides (Frequencies tab) ---
  const updateOverride = (idx: number, patch: Partial<WorkingFrequency>) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const list = [...(prev.workingFrequencies ?? [])]
      if (!list[idx]) return prev
      list[idx] = { ...list[idx], ...patch }
      return { ...prev, workingFrequencies: list }
    })
  }

  const addOverride = () => {
    markDirty()
    setForm((prev) =>
      prev
        ? {
            ...prev,
            workingFrequencies: [
              ...(prev.workingFrequencies ?? []),
              { band: '20m', mode: 'FT8', mhz: 14.074 },
            ],
          }
        : prev,
    )
  }

  const removeOverride = (idx: number) => {
    markDirty()
    setMhzDraft(null)
    setForm((prev) =>
      prev
        ? { ...prev, workingFrequencies: (prev.workingFrequencies ?? []).filter((_, i) => i !== idx) }
        : prev,
    )
  }

  const resetOverrides = () => {
    if (
      (form?.workingFrequencies?.length ?? 0) > 0 &&
      !window.confirm('Clear all working-frequency overrides and go back to the stock WSJT-X table?')
    ) {
      return
    }
    markDirty()
    setMhzDraft(null)
    setForm((prev) => (prev ? { ...prev, workingFrequencies: [] } : prev))
  }

  /** Commit a typed MHz only when valid (positive, finite); otherwise keep prior. */
  const commitMhz = (idx: number, raw: string) => {
    const num = Number(raw)
    if (raw.trim() !== '' && Number.isFinite(num) && num > 0) updateOverride(idx, { mhz: num })
  }

  // Resolve a model's friendly name from whichever list(s) are loaded; an unrecognized
  // number (e.g. typed directly) still commits — Hamlib may support it even unnamed here.
  const findRigModelName = (modelNum: number): string =>
    rigModels.find((m) => m[0] === modelNum)?.[1] ?? allRigModels.find((m) => m[0] === modelNum)?.[1] ?? ''

  // Rigs whose CAT never runs at the app's 38400 default, so picking one auto-sets a baud
  // that actually answers (rigctld connects regardless of baud; the radio just stays silent
  // on a mismatch — the classic "CAT dead on defaults" trap):
  // - Xiegu CI-V (G90/X6100/X6200/X5105/X108G): fixed 19200, no baud menu on the radio.
  // - Vintage Kenwood (IC-10/IF-232C era — TS-140S/440S/450S/690S/790/850/940S/950SDX and
  //   the TS-50S): the serial interface is FIXED at 4800 (8N2, handled by the Hamlib
  //   backend; -s only overrides the speed).
  // - 1990s Kenwood with a baud menu (TS-870S, TS-570D/S): factory default 9600.
  // Model numbers mirror crates/tempo-audio/src/rigmodels.rs (verified vs riglist.h).
  const BAUD_BY_MODEL = new Map<number, number>([
    // Xiegu → 19200
    [3088, 19200], [3087, 19200], [3091, 19200], [3089, 19200], [3076, 19200],
    // Vintage Kenwood → 4800 fixed
    [2001, 4800], [2002, 4800], [2003, 4800], [2005, 4800], [2007, 4800],
    [2009, 4800], [2011, 4800], [2013, 4800], [2025, 4800],
    // Kenwood with 9600 factory default
    [2010, 9600], [2004, 9600], [2016, 9600],
  ])
  const recommendedBaud = (modelNum: number): number | null => BAUD_BY_MODEL.get(modelNum) ?? null

  const selectRig = (modelNum: number) => {
    markDirty()
    const baud = recommendedBaud(modelNum)
    setForm((prev) =>
      prev
        ? { ...prev, rigModel: modelNum, rigModelName: findRigModelName(modelNum), ...(baud ? { baud } : {}) }
        : prev,
    )
  }

  // --- Dual-radio roster (P2). These are LIVE verbs: they persist immediately and return a fresh
  // snapshot. We then re-pull the full settings and merge ONLY the radios[]/active/peg fields back
  // into the form, so the roster reflects the change without discarding unsaved flat-form edits.
  const reloadRadios = () => {
    void getSettings().then((s) => {
      setForm((prev) =>
        prev
          ? {
              ...prev,
              radios: s.radios,
              activeRadio: s.activeRadio,
              radioPegged: s.radioPegged,
              // The routing table is live state too (own verbs, restored across a stale-form Save),
              // so it must come back from the backend here rather than from the form.
              routingRules: s.routingRules,
              defaultRadio: s.defaultRadio,
            }
          : s,
      )
      onSaved?.()
    })
  }
  const handleAddRadio = () => {
    void withErrorToast(() => addRadio(), 'Could not add a radio').then((s) => s && reloadRadios())
  }
  const handleRemoveRadio = (id: number) => {
    // Destructive + immediate + unrecoverable (drops the radio's CAT/audio config, its unique
    // rigctld port, and band coverage) — confirm before removing.
    const r = form?.radios?.find((p) => p.id === id)
    if (!window.confirm(`Remove ${r?.name ?? 'this radio'}? This deletes its CAT/audio config and can't be undone.`)) {
      return
    }
    void withErrorToast(() => removeRadio(id), 'Could not remove the radio').then((s) => s && reloadRadios())
  }
  const handleRenameRadio = (id: number, name: string) => {
    void withErrorToast(() => renameRadio(id, name), 'Could not rename the radio').then((s) => s && reloadRadios())
  }
  const handleToggleRadioBand = (id: number, band: string) => {
    const radio = form?.radios?.find((r) => r.id === id)
    if (!radio) return
    const bands = radio.bands.includes(band)
      ? radio.bands.filter((b) => b !== band)
      : [...radio.bands, band]
    void withErrorToast(() => setRadioBands(id, bands), 'Could not set band coverage').then(
      (s) => s && reloadRadios(),
    )
  }
  // --- Band+mode routing rules. Live verbs like the roster above: each edit persists immediately
  // and re-pulls, so the rules survive a later stale-form Save (and an edit made while the rig form
  // is pointed at a non-active radio, where Save goes through update_radio_profile and drops the
  // form entirely).
  const mutateRules = (next: RoutingRule[]) => {
    void withErrorToast(() => setRoutingRules(next), 'Could not save the routing rules').then(
      (s) => s && reloadRadios(),
    )
  }
  const handleAddRule = () => {
    const first = form?.radios?.[0]?.id ?? 0
    mutateRules([...(form?.routingRules ?? []), { bands: [], mode: null, radio: first }])
  }
  const handleRemoveRule = (i: number) => {
    mutateRules((form?.routingRules ?? []).filter((_, n) => n !== i))
  }
  const handlePatchRule = (i: number, patch: Partial<RoutingRule>) => {
    mutateRules((form?.routingRules ?? []).map((r, n) => (n === i ? { ...r, ...patch } : r)))
  }
  const handleToggleRuleBand = (i: number, band: string) => {
    const rule = form?.routingRules?.[i]
    if (!rule) return
    handlePatchRule(i, {
      bands: rule.bands.includes(band)
        ? rule.bands.filter((b) => b !== band)
        : [...rule.bands, band],
    })
  }
  // Order IS the precedence (first match wins), so the operator needs to reorder rules.
  const handleMoveRule = (i: number, delta: number) => {
    const rules = [...(form?.routingRules ?? [])]
    const to = i + delta
    if (to < 0 || to >= rules.length) return
    ;[rules[i], rules[to]] = [rules[to], rules[i]]
    mutateRules(rules)
  }
  const handleSetDefaultRadio = (id: number | null) => {
    void withErrorToast(() => setDefaultRadio(id), 'Could not set the default radio').then(
      (s) => s && reloadRadios(),
    )
  }
  // The "test" affordance: ask the backend where a (band, mode) resolves, so the operator can check
  // the table without QSYing a rig. Answered by the same resolver the radio loop uses.
  const runRouteTest = (band: string, mode: RouteMode) => {
    setRouteTest({ band, mode })
    void routePreview(band, mode)
      .then((r) => setRouteTestResult(r.name))
      .catch(() => setRouteTestResult(null))
  }

  // EDIT a radio's config without touching what you're operating on: load that radio's profile
  // into the rig form LOCALLY (no backend call, no live rig swap, no dropped carrier). Save then
  // writes just this radio (via update_radio_profile) when it isn't the active one.
  const handleConfigureRadio = (id: number) => {
    if (id === editingRadioId) return
    if (dirtyRef.current && !window.confirm('Discard unsaved changes to the radio you were editing?')) {
      return
    }
    const r = form?.radios?.find((p) => p.id === id)
    if (!r) return
    setForm((prev) => (prev ? { ...prev, ...radioPatch(r) } : prev))
    setEditingRadioId(id)
    dirtyRef.current = false
  }

  // MAKE ACTIVE — the operating radio swap (carrier dropped first). Separate from Edit now, so
  // configuring a second rig no longer forces you to start operating on it.
  const handleMakeActive = (id: number) => {
    if (dirtyRef.current && !window.confirm('Discard unsaved changes and switch the operating radio?')) {
      return
    }
    void withErrorToast(() => setActiveRadio(id), 'Could not switch radios').then((s) => {
      if (!s) return
      void getSettings().then((full) => {
        setForm(full)
        dirtyRef.current = false
        setEditingRadioId(id)
      })
      onSaved?.()
    })
  }

  // Lazily fetch the full Hamlib catalog only the first time it's requested.
  const onToggleShowAllRigModels = (checked: boolean) => {
    setShowAllRigModels(checked)
    if (checked && allRigModels.length === 0 && !allRigModelsLoading) {
      setAllRigModelsLoading(true)
      getAllRigModels()
        .then(setAllRigModels)
        .catch(() => {})
        .finally(() => setAllRigModelsLoading(false))
    }
  }

  // Zero-config: scan connected USB radios.
  const onDetectRigs = async () => {
    setDetecting(true)
    // ONE detect for every radio kind (operator request: the USB-only scan
    // could never see a Flex): USB enumeration + LAN discovery in parallel;
    // either probe may fail without killing the other's results.
    const [rigs, flexes] = await Promise.all([
      withErrorToast(() => detectRigs(), 'USB radio detection failed'),
      discoverFlex().catch((e) => {
        pushToast(`Flex LAN scan: ${e instanceof Error ? e.message : e}`, 'info', 6000)
        return []
      }),
    ])
    setDetectedFlex(flexes)
    setDetecting(false)
    if (rigs) {
      setDetected(rigs)
      if (rigs.length === 0 && flexes.length === 0)
        pushToast('No radios found — USB: plug in + power on; Flex: must be on this network.', 'info')
    }
  }

  // One-click apply a detected rig: fill model (if identified) + port + paired audio.
  const applyDetectedRig = (r: DetectedRig) => {
    if (!form) return
    markDirty()
    const baud = r.suggestedModel != null ? recommendedBaud(r.suggestedModel) : null
    const applied = {
      ...form,
      ...(r.suggestedModel != null
        ? { rigModel: r.suggestedModel, rigModelName: r.suggestedModelName ?? '' }
        : {}),
      serialPort: r.portName,
      // Pair RX from the capture list, TX from the OUTPUT list — the rig's CODEC enumerates under
      // different names per direction on Windows, so reusing the input name for audioOut sent TX
      // audio to the PC speakers. Fall back to the input name only if no output paired.
      ...(r.suggestedAudio ? { audioIn: r.suggestedAudio } : {}),
      ...(r.suggestedAudioOut || r.suggestedAudio
        ? { audioOut: r.suggestedAudioOut ?? r.suggestedAudio ?? '' }
        : {}),
      ...(baud ? { baud } : {}),
      // A recognised interface cable keys a serial line, so pre-fill that much. The PTT PORT is
      // only filled when we actually know the answer: `interfaceSharesCatPort === true` means
      // one cable, so blank is correct. `null` means it varies by model — leave whatever the
      // operator already had rather than guessing, because a wrong keying port keys the wrong
      // radio, which is a TX-path error, not a cosmetic one.
      ...(r.interfacePttMethod ? { pttMethod: r.interfacePttMethod } : {}),
      ...(r.interfaceSharesCatPort === true ? { pttSerialPort: '' } : {}),
    }
    setForm(applied)
    if (r.interfaceName) {
      // Do NOT chain into Auto-test the way an unidentified rig does. We know exactly what this
      // device is — a cable — and the sweep would be looking for a radio that this port may not
      // even have a CAT link to yet. Tell the operator the one thing still missing.
      pushToast(
        `Applied ${r.interfaceName} on ${r.portName} — now pick your Rig Model, then Save`,
        'success',
      )
    } else if (r.suggestedModel == null) {
      // Unidentified rig (bridge chip only, no model) — instead of making the operator pick a model
      // and Test CAT by hand, chain straight into the port Auto-test, which sweeps COMMON_CAT_MODELS
      // + bauds to find the one that actually answers. Pass the freshly-applied form (state is async).
      pushToast(`Applied ${r.product || 'radio'} on ${r.portName} — identifying via Auto-test…`, 'info')
      void handleAutoTestPorts(applied)
    } else {
      pushToast(
        `Applied ${r.suggestedModelName ?? (r.product || 'radio')} on ${r.portName} — review + Save settings`,
        'success',
      )
    }
  }

  // One-click apply a discovered Flex: network conn via SmartSDR CAT's default
  // slice-A TCP port + the FLEX-6xxx dialect model (the WSJT-X-proven path).
  const applyDetectedFlex = (f: { model: string; nickname: string; ip: string }) => {
    markDirty()
    setForm((prev) =>
      prev
        ? {
            ...prev,
            rigConn: 'network',
            rigAddr: '127.0.0.1:5002',
            rigModel: 2036,
            rigModelName: 'FlexRadio FLEX-6xxx (SmartSDR CAT)',
            // Keep the discovered radio IP — the native panadapter / DAX path connects to the rig
            // directly over VITA-49 at this address (CAT still rides the localhost SmartSDR proxy
            // above). Discovery already knows it; dropping it left the native features unreachable.
            flexRadioIp: f.ip || (prev.flexRadioIp ?? ''),
          }
        : prev,
    )
    pushToast(
      `Applied ${f.model}${f.nickname ? ` "${f.nickname}"` : ''} at ${f.ip} — SmartSDR CAT (slice A, port 5002); native panadapter/DAX ready to enable below. Review + Save, then Test CAT. Second slice? Use port 60001.`,
      'success',
    )
  }

  // FrequencyControl edits the in-form band/dial/sideband; it's persisted on Save.
  const setFreq = (dialMhz: number, band: string, mode: string) => {
    markDirty()
    setForm((prev) =>
      prev ? { ...prev, dialMhz, band: band || prev.band, sideband: mode } : prev,
    )
  }

  // Persist the rig form to the radio it actually describes.
  //
  // The backend contract is that a settings payload's `activeRadio` names the radio whose
  // config the flat fields describe (engine.rs apply_settings → sync_active_from_flat). The
  // per-radio Edit flow breaks that contract by design, so every write of the rig form has to
  // route through the per-radio verb instead. Only Save was taught this; Test CAT and Auto-test
  // were not, and a whole-settings save from either one stamped the EDITED radio's COM port,
  // model and audio devices onto the ACTIVE radio's profile — persisted, silent, and
  // unrecoverable. Operator report, 2026-07-25: both radios ended up on one set of comm ports.
  //
  // Takes the form explicitly: setForm is async, so a caller that just built a new form must
  // hand it over rather than let this read a stale closure.
  const persistRadioForm = async (next: NonNullable<typeof form>) => {
    if (editingRadioId != null && editingRadioId !== next.activeRadio) {
      const edited = next.radios?.find((r) => r.id === editingRadioId)
      await updateRadioProfile(
        editingRadioId,
        radioPatch({
          ...next,
          rotctldPort: edited?.rotctldPort ?? next.rigctldPort + 1,
          nativeScope: edited?.nativeScope ?? 'auto',
        }),
      )
    } else {
      await setSettings({ ...next, mycall: next.mycall.trim().toUpperCase() })
    }
  }

  // A station-wide save (LoTW/eQSL credentials, config profiles) that must NOT carry the rig
  // form's radio fields — while editing a non-active radio those describe the wrong radio, and
  // the backend would fold them into the active profile. Restore the active radio's own config
  // into the payload so the fold is a no-op; everything station-wide still persists.
  const withActiveRadioConfig = (next: NonNullable<typeof form>) => {
    const active = next.radios?.find((r) => r.id === next.activeRadio)
    return active ? { ...next, ...radioPatch(active) } : next
  }

  // Test CAT (WSJT-X-style): save the form first so the radio loop reconfigures
  // (launching rigctld for CAT) from these exact values, then probe the rig and
  // show a green/red result with the read frequency or a specific error.
  const handleTestCat = async () => {
    if (!form) return
    if (!form.mycall.trim()) {
      setError('Callsign is required.')
      return
    }
    setCatTesting(true)
    setCatResult(null)
    setError(null)
    try {
      await persistRadioForm(form)
      onSaved?.()
      if (editingRadioId != null && editingRadioId !== form.activeRadio) {
        // test_cat reports the ACTIVE radio's CAT state — it has no radio argument. Running it
        // here would save this radio's config and then hand back a green tick earned by the
        // OTHER radio. A false green is worse than no test: it's what hid the CAT-flip bug
        // through a whole review. Say what was actually done instead.
        const name = form.radios?.find((r) => r.id === editingRadioId)?.name ?? `radio ${editingRadioId}`
        setCatResult({
          ok: true,
          detail: `Saved to ${name}. CAT can only be tested on the radio you're operating — make ${name} active to test it.`,
        })
      } else {
        const result = await testCat()
        setCatResult(result)
      }
    } catch {
      setCatResult({ ok: false, detail: 'Could not run the CAT test.' })
    } finally {
      setCatTesting(false)
    }
  }

  // Auto-test ports: probe each USB port (read-only) for the one that actually drives
  // the rig, then auto-fill + save the winning port/baud/model so CAT just works — no
  // guessing which COM port among a rig's several is the control port.
  const handleAutoTestPorts = async (base?: typeof form) => {
    const f = base ?? form
    if (!f) return
    setCatTesting(true)
    setCatResult(null)
    setError(null)
    try {
      // Probe on behalf of the radio being CONFIGURED, not the one being operated — its Hamlib
      // model is what seeds a bridge-chip port, and an Icom answers only at its own CI-V address.
      const r = await probeCatPorts(editingRadioId ?? f.activeRadio)
      if (r.found) {
        // Apply port + baud (confirmed working). Only trust the MODEL when it wasn't a guess — a
        // seeded common-rig probe can be answered by a same-family sibling (FT-991A on the FTDX10
        // probe), so keep the operator's Rig Model rather than persisting a wrong one.
        const next = {
          ...f,
          serialPort: r.portName,
          baud: r.baud,
          pttMethod: 'cat',
          ...(r.modelSeeded
            ? {}
            : { rigModel: r.model, rigModelName: r.modelName }),
        }
        setForm(next)
        await persistRadioForm(next)
        onSaved?.()
        setCatResult({ ok: true, detail: `✓ ${r.detail}` })
      } else {
        setCatResult({ ok: false, detail: r.detail })
      }
    } catch {
      setCatResult({ ok: false, detail: 'Could not run the port auto-test.' })
    } finally {
      setCatTesting(false)
    }
  }

  // Config profiles: snapshot the current settings under a name, then switch the whole
  // rig/antenna/CAT/band setup in one move (loading applies via the normal Save path).
  const handleSaveProfile = async () => {
    if (!form || !newProfileName.trim()) return
    // Snapshot the last-SAVED settings, never the dirty form — a half-edited
    // form must not become a named configuration.
    const saved = await getSettings()
    setProfiles(saveProfile(newProfileName, saved))
    pushToast(`Profile "${newProfileName.trim()}" saved`, 'success')
    setNewProfileName('')
  }
  const handleLoadProfile = async () => {
    const p = profiles.find((x) => x.name === selectedProfile)
    if (!p) return
    // MERGE onto the current settings (see profiles.mergeProfile): identity,
    // license class, the radio roster and sync cursors never import, and a key
    // the profile predates keeps its current value — the raw replay this
    // replaces silently removed the per-mode power ceilings from any profile
    // saved before those fields existed.
    const merged = mergeProfile(await getSettings(), p.settings)
    setForm(merged)
    // A profile is a whole-station config, so the form no longer describes whichever radio was
    // being edited — drop back to the active radio (the MACHINE's, never the profile's).
    setEditingRadioId(merged.activeRadio)
    await setSettings(merged)
    onSaved?.()
    pushToast(`Loaded profile "${p.name}"`, 'success')
  }
  const handleDeleteProfile = () => {
    if (!selectedProfile) return
    setProfiles(deleteProfile(selectedProfile))
    setSelectedProfile('')
  }

  const onSaveLotwPassword = async () => {
    if (!lotwPw) return
    const ok = await withErrorToast(async () => {
      await setLotwPassword(lotwPw)
      return true
    }, 'Could not save the LoTW password')
    if (ok) {
      setLotwPw('')
      pushToast('LoTW password saved to the system keychain', 'success')
    }
  }

  const onForgetLotwPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearLotwPassword()
      return true
    }, 'Could not clear the LoTW password')
    if (ok) {
      setLotwPw('')
      pushToast('LoTW password cleared from the keychain', 'success')
    }
  }

  const onSyncLotw = async () => {
    if (!form) return
    setLotwSyncing(true)
    // Persist the form first so the download runs against the username the user
    // sees — the backend reads SAVED settings, not the in-form draft (and a
    // username change resets the sync cursor). Mirrors how Test CAT saves first.
    // This is a station-wide save, so it must not carry the rig form's radio fields.
    const r = await withErrorToast(async () => {
      await setSettings({
        ...withActiveRadioConfig(form),
        mycall: form.mycall.trim().toUpperCase(),
      })
      return downloadLotwReport()
    }, 'LoTW sync failed')
    setLotwSyncing(false)
    if (r) {
      const orphans = r.orphans.length ? ` · ${r.orphans.length} unmatched` : ''
      const promoted = r.promoted ? ` · ${r.promoted} upload${r.promoted === 1 ? '' : 's'} now on file` : ''
      pushToast(
        `LoTW: ${r.newlyConfirmed} newly confirmed, ${r.newlyCredited} credited${promoted}${orphans}`,
        r.orphans.length ? 'info' : 'success',
      )
      onSaved?.()
    }
  }

  const onSaveEqslPassword = async () => {
    if (!eqslPw) return
    const ok = await withErrorToast(async () => {
      await setEqslPassword(eqslPw)
      return true
    }, 'Could not save the eQSL password')
    if (ok) {
      setEqslPw('')
      updateBool('eqslUpload', true)
      pushToast('eQSL password saved — auto-upload to eQSL is ON', 'success')
    }
  }

  const onForgetEqslPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearEqslPassword()
      return true
    }, 'Could not clear the eQSL password')
    if (ok) {
      setEqslPw('')
      updateBool('eqslUpload', false)
      pushToast('eQSL password cleared — auto-upload to eQSL is off', 'success')
    }
  }

  const onSyncEqsl = async () => {
    if (!form) return
    setEqslSyncing(true)
    // Save the form first so the download uses the username the user sees (the
    // backend reads SAVED settings; a username change resets the cursor).
    // Station-wide save: must not carry the rig form's radio fields (see onSyncLotw).
    const r = await withErrorToast(async () => {
      await setSettings({
        ...withActiveRadioConfig(form),
        mycall: form.mycall.trim().toUpperCase(),
      })
      return downloadEqslReport()
    }, 'eQSL sync failed')
    setEqslSyncing(false)
    if (r) {
      const orphans = r.orphans.length ? ` · ${r.orphans.length} unmatched` : ''
      // eQSL is non-award-grade, so report newlyConfirmedAny (newlyConfirmed is
      // award-only and always 0 for eQSL).
      pushToast(
        `eQSL: ${r.newlyConfirmedAny} newly confirmed (not DXCC/WAS credit)${orphans}`,
        r.orphans.length ? 'info' : 'success',
      )
      onSaved?.()
    }
  }

  const onSaveQrzPassword = async () => {
    if (!qrzPw) return
    const ok = await withErrorToast(async () => {
      await setQrzPassword(qrzPw)
      return true
    }, 'Could not save the QRZ password')
    if (ok) {
      setQrzPw('')
      pushToast('QRZ password saved to the system keychain', 'success')
    }
  }

  const onForgetQrzPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearQrzPassword()
      return true
    }, 'Could not clear the QRZ password')
    if (ok) {
      setQrzPw('')
      pushToast('QRZ password cleared from the keychain', 'success')
    }
  }

  const onSaveHamqthPassword = async () => {
    if (!hamqthPw) return
    const ok = await withErrorToast(async () => {
      await setHamqthPassword(hamqthPw)
      return true
    }, 'Could not save the HamQTH password')
    if (ok) {
      setHamqthPw('')
      pushToast('HamQTH password saved to the system keychain', 'success')
    }
  }

  const onForgetHamqthPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearHamqthPassword()
      return true
    }, 'Could not clear the HamQTH password')
    if (ok) {
      setHamqthPw('')
      pushToast('HamQTH password cleared from the keychain', 'success')
    }
  }

  const onSaveQrzLogbookKey = async () => {
    if (!qrzKey) return
    const ok = await withErrorToast(async () => {
      await setQrzLogbookKey(qrzKey)
      return true
    }, 'Could not save the QRZ Logbook key')
    if (ok) {
      setQrzKey('')
      updateBool('qrzLogbookUpload', true)
      pushToast('QRZ Logbook key saved — auto-upload to QRZ is ON', 'success')
    }
  }

  const onForgetQrzLogbookKey = async () => {
    const ok = await withErrorToast(async () => {
      await clearQrzLogbookKey()
      return true
    }, 'Could not clear the QRZ Logbook key')
    if (ok) {
      setQrzKey('')
      updateBool('qrzLogbookUpload', false)
      pushToast('QRZ Logbook key cleared — auto-upload to QRZ is off', 'success')
    }
  }

  const onSyncQrz = async () => {
    setQrzSyncing(true)
    // Two-way pull: FETCH the online QRZ logbook and merge it in (new QSOs +
    // confirmations). Uses the saved Logbook API key, so no form save is needed.
    const r = await withErrorToast(syncQrz, 'QRZ sync failed')
    setQrzSyncing(false)
    if (r) {
      const added = r.added ?? 0
      const orphans = r.orphans.length ? ` · ${r.orphans.length} unmatched` : ''
      // QRZ-native confirmations are non-award-grade, so report newlyConfirmedAny.
      pushToast(
        `QRZ: ${added} new QSO${added === 1 ? '' : 's'}, ${r.newlyConfirmedAny} newly confirmed${orphans}`,
        'success',
      )
      onSaved?.()
    }
  }

  const onSaveClublogPassword = async () => {
    if (!clublogPw) return
    const ok = await withErrorToast(async () => {
      await setClublogPassword(clublogPw)
      return true
    }, 'Could not save the ClubLog password')
    if (ok) {
      setClublogPw('')
      updateBool('clublogUpload', true)
      pushToast('ClubLog app-password saved — auto-upload to ClubLog is ON', 'success')
    }
  }

  const onForgetClublogPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearClublogPassword()
      return true
    }, 'Could not clear the ClubLog password')
    if (ok) {
      setClublogPw('')
      updateBool('clublogUpload', false)
      pushToast('ClubLog password cleared — auto-upload to ClubLog is off', 'success')
    }
  }

  const onSaveHrdlogCode = async () => {
    if (!hrdlogCode) return
    const ok = await withErrorToast(async () => {
      await setHrdlogCode(hrdlogCode)
      return true
    }, 'Could not save the HRDLog.net upload code')
    if (ok) {
      setHrdlogCodeField('')
      updateBool('hrdlogUpload', true)
      pushToast('HRDLog.net code saved — auto-upload to HRDLog.net is ON', 'success')
    }
  }

  const onForgetHrdlogCode = async () => {
    const ok = await withErrorToast(async () => {
      await clearHrdlogCode()
      return true
    }, 'Could not clear the HRDLog.net upload code')
    if (ok) {
      setHrdlogCodeField('')
      updateBool('hrdlogUpload', false)
      pushToast('HRDLog.net code cleared — auto-upload to HRDLog.net is off', 'success')
    }
  }

  const onSaveRbToken = async () => {
    if (!rbToken) return
    const ok = await withErrorToast(async () => {
      await setRepeaterbookToken(rbToken)
      return true
    }, 'Could not save the RepeaterBook token')
    if (ok) {
      setRbTokenField('')
      pushToast('RepeaterBook token saved — the Program section now uses RepeaterBook', 'success')
    }
  }

  const onForgetRbToken = async () => {
    const ok = await withErrorToast(async () => {
      await setRepeaterbookToken('')
      return true
    }, 'Could not clear the RepeaterBook token')
    if (ok) {
      setRbTokenField('')
      pushToast('RepeaterBook token cleared — the Program section falls back to hearham.com', 'success')
    }
  }

  const onSaveCloudlogKey = async () => {
    if (!cloudlogKey) return
    const ok = await withErrorToast(async () => {
      await setCloudlogKey(cloudlogKey)
      return true
    }, 'Could not save the Cloudlog API key')
    if (ok) {
      setCloudlogKeyField('')
      pushToast('Cloudlog API key saved to the keychain', 'success')
    }
  }

  const onForgetCloudlogKey = async () => {
    const ok = await withErrorToast(async () => {
      await clearCloudlogKey()
      return true
    }, 'Could not clear the Cloudlog API key')
    if (ok) {
      setCloudlogKeyField('')
      pushToast('Cloudlog API key cleared from the keychain', 'success')
    }
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!form) return
    if (!form.mycall.trim()) {
      // Don't dead-end on another tab: route the operator to where the fix is instead of a
      // silently-greyed Save button with a context-free "required" error.
      setTab('station')
      setError('Enter your callsign on the Station tab before saving.')
      return
    }
    setStatus('saving')
    setError(null)
    try {
      // Editing a NON-active radio writes ONLY that radio's CAT/audio/PTT/rotator/native config;
      // the active radio, its flat mirror and station-wide settings are untouched (the backend
      // re-syncs the flat mirror from the still-active radio). No live rig swap.
      await persistRadioForm(form)
      dirtyRef.current = false
      setStatus('saved')
      onSaved?.()
    } catch (err) {
      setStatus('idle')
      // Surface the backend's actual message (Tauri rejects with the Err string) — e.g. the
      // dual-radio port-collision rejection tells the operator exactly which ports clash.
      const msg = typeof err === 'string' ? err : err instanceof Error ? err.message : ''
      setError(msg || 'Could not save settings.')
    }
  }

  if (!form) {
    return (
      <section className="panel settings-panel">
        <div className="panel-header">
          <h2>Settings</h2>
        </div>
        <p className="empty">Loading settings…</p>
      </section>
    )
  }

  // One feature row. Core features are always on and can't be switched, so they
  // show a static "always on" badge instead of a toggle — a locked switch next to
  // the real, toggleable settings just reads as broken.
  const featureRow = (f: FeatureDef) => {
    if (f.core) {
      return (
        <div className="settings-field" key={f.id}>
          <div className="settings-toggle">
            <span className="settings-label">{f.label}</span>
            <span className="feature-always-on">always on</span>
          </div>
          <span className="settings-hint">{f.oneLine}</span>
        </div>
      )
    }
    const on = features.enabled[f.id] !== false
    const depOff = f.dependsOn.find((d) => features.enabled[d] === false)
    return (
      <div className="settings-field" key={f.id}>
        <label className="settings-toggle">
          <span className="settings-label">{f.label}</span>
          <button
            type="button"
            role="switch"
            aria-checked={on}
            className={`toggle${on ? ' on' : ''}`}
            onClick={() => features.toggle(f.id)}
            aria-label={`${on ? 'Disable' : 'Enable'} ${f.label}`}
          >
            <span className="toggle-knob" />
          </button>
        </label>
        <span className="settings-hint">
          {f.oneLine}
          {depOff && ` Turning on also enables ${featureById(depOff as FeatureId)?.label ?? depOff}.`}
        </span>
      </div>
    )
  }

  // serial-port options include the current value even if not in the enumerated
  // list (e.g. a port that has since disappeared), so it stays selectable.
  const portOptions = form.serialPort && !serialPorts.includes(form.serialPort)
    ? [form.serialPort, ...serialPorts]
    : serialPorts

  // include the current selection even if it's not in the enumerated list
  const audioInOptions = form.audioIn && !audio.input.includes(form.audioIn)
    ? [form.audioIn, ...audio.input]
    : audio.input
  const audioOutOptions = form.audioOut && !audio.output.includes(form.audioOut)
    ? [form.audioOut, ...audio.output]
    : audio.output
  // Headphone-monitor device picker: same enumerated-output list, keeping the
  // saved selection visible even if it's since disappeared.
  const monitorOutOptions = form.monitorDevice && !audio.output.includes(form.monitorDevice)
    ? [form.monitorDevice, ...audio.output]
    : audio.output
  // Voice-mic device picker: the enumerated INPUT list (it's a microphone), keeping the
  // saved selection visible even if it's since disappeared.
  const voiceMicOptions = form.voiceMicDevice && !audio.input.includes(form.voiceMicDevice)
    ? [form.voiceMicDevice, ...audio.input]
    : audio.input

  // What one option READS. An enumerated device shows its human label; a stored value that is
  // not one of the entries we offer says so — a saved routing can go stale (rig off, USB port
  // moved, ALSA card renumbered) and it used to look exactly like a working choice.
  //
  // ⚠️ IT SAYS "not in the list", NOT "not detected", AND THE DIFFERENCE IS HONESTY. This flag
  // is computed against the list we OFFER — on Linux the PRUNED ALSA hint set (one entry per
  // card, `plughw` preferred, `dmix`/`dsnoop`/`front`/`iec958` dropped). A device is OPENED
  // through cpal, whose enumeration is its own probe-open sweep: a DIFFERENT set, and neither
  // is a subset of the other. So a device that works perfectly but that our pruning does not
  // offer was being told it was missing (proven live: a saved `dsnoop` PCM,
  // offered_in_picker=false, opens_at_runtime=true). We cannot know from here whether a name
  // will open — so we state the thing we do know and leave the verdict to the open, which is
  // already a visible error naming the device.
  const audioLabel = (name: string, kind: 'input' | 'output') =>
    audio[kind].includes(name)
      ? (audioLabels[kind][name] ?? name)
      : `${name} — saved, not in the list`

  // Frequencies tab: last-wins override lookup for the stock table, plus
  // duplicate band+mode keys (flagged in the editor — the last row wins).
  const overrides = form.workingFrequencies ?? []
  const overrideByKey = new Map<string, number>()
  const dupKeys = new Set<string>()
  for (const o of overrides) {
    const k = `${o.band}|${o.mode}`
    if (overrideByKey.has(k)) dupKeys.add(k)
    overrideByKey.set(k, o.mhz)
  }

  // Field Day section validity: a non-empty value that isn't a known ARRL/RAC
  // section is flagged inline so it never silently reaches the Cabrillo log.
  const fdSectionInvalid =
    form.fdSection.trim() !== '' && !FD_SECTION_CODES.has(form.fdSection.trim().toUpperCase())

  return (
    <section className="panel settings-panel">
      <div className="panel-header">
        <h2>Settings</h2>
        <span className="settings-sub">operator, rig &amp; network</span>
        <span className="settings-build" title="This install's build stamp — confirm a fresh install actually took">
          build {__BUILD_ID__}
        </span>
        <button
          type="button"
          className="settings-update-btn"
          onClick={() => void checkForUpdateManual()}
          title="Check SourceForge for a newer Nexus release"
        >
          Check for updates
        </button>
      </div>

      <form className="settings-form" onSubmit={handleSubmit}>
        <div className="settings-tabs" role="tablist" aria-label="Settings sections">
          {/* Contesting is always visible now (0.17.0 decision) — capability, not config, gates
              tabs; the Field Day master switch lives inside the Contesting tab. */}
          {SETTINGS_TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              className={`settings-tab${tab === t.id ? ' active' : ''}`}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="settings-scroll">
          {/* ---- Workspace (UI-only prefs, applied live like the theme) ---- */}
          {tab === 'appearance' && (
          <fieldset className="settings-section">
            <legend>Workspace</legend>
            <div className="settings-grid">
              <div className="settings-field">
                <span className="settings-label">UI scale</span>
                <div className="theme-switcher" role="group" aria-label="UI scale mode">
                  <button
                    type="button"
                    className={`theme-chip${scaleMode === 'auto' ? ' active' : ''}`}
                    aria-pressed={scaleMode === 'auto'}
                    onClick={() => onScaleModeChange('auto')}
                  >
                    Auto (fit)
                  </button>
                  <button
                    type="button"
                    className={`theme-chip${scaleMode !== 'auto' ? ' active' : ''}`}
                    aria-pressed={scaleMode !== 'auto'}
                    onClick={() => onScaleModeChange(scale)}
                  >
                    Manual
                  </button>
                </div>
                {scaleMode === 'auto' ? (
                  <>
                    <span className="settings-hint">Max scale (auto won&apos;t exceed)</span>
                    <div className="theme-switcher" role="group" aria-label="Maximum UI scale">
                      {SCALE_STEPS.filter((s) => s >= 100).map((s) => {
                        // Disable chips this window can't reach: Auto never upscales
                        // past the fit, so every chip > autoCeil yields the SAME scale
                        // (the operator's "150 == 175" — a dead option). Re-enables
                        // when the window/monitor grows.
                        const unreachable = s > autoCeil
                        return (
                          <button
                            key={s}
                            type="button"
                            className={`theme-chip${scaleCap === s ? ' active' : ''}`}
                            aria-pressed={scaleCap === s}
                            disabled={unreachable}
                            title={
                              unreachable
                                ? `This window only fits up to ${autoCeil}% — a larger window or monitor unlocks ${s}%.`
                                : undefined
                            }
                            onClick={() => onScaleCapChange(s)}
                          >
                            {s}%
                          </button>
                        )
                      })}
                    </div>
                    <span className="settings-hint">
                      Fits the whole interface to the window so nothing is cut off (currently {scale}%).
                      {autoCeil < 100
                        ? ` This window maxes out at ${autoCeil}% — raising the cap can't help until you enlarge the window, or switch to Manual to force a bigger scale.`
                        : autoCeil < MAX_STEP
                          ? ` This window fits up to ${autoCeil}%; bigger caps need a larger window or monitor. The waterfall stays sharp.`
                          : ' The waterfall stays sharp. Raise the max for big monitors.'}
                    </span>
                  </>
                ) : (
                  <>
                    <div className="theme-switcher" role="group" aria-label="UI scale">
                      {SCALE_STEPS.map((s) => (
                        <button
                          key={s}
                          type="button"
                          className={`theme-chip${scale === s ? ' active' : ''}`}
                          aria-pressed={scale === s}
                          onClick={() => onScaleModeChange(s)}
                        >
                          {s}%
                        </button>
                      ))}
                    </div>
                    <span className="settings-hint">
                      Fixed scale. Switch to Auto to fit the interface to the window automatically.
                    </span>
                  </>
                )}
              </div>

              <div className="settings-field">
                <span className="settings-label">Density</span>
                <div className="theme-switcher" role="group" aria-label="Information density">
                  <button
                    type="button"
                    className={`theme-chip${density !== 'dense' ? ' active' : ''}`}
                    aria-pressed={density !== 'dense'}
                    onClick={() => onDensityChange('standard')}
                  >
                    Comfortable
                  </button>
                  <button
                    type="button"
                    className={`theme-chip${density === 'dense' ? ' active' : ''}`}
                    aria-pressed={density === 'dense'}
                    onClick={() => onDensityChange('dense')}
                  >
                    Compact
                  </button>
                </div>
                <span className="settings-hint">
                  How tightly rows and controls pack. Compact fits more on screen.
                </span>
              </div>

              <div className="settings-field">
                <span className="settings-label">Pane sizes</span>
                <button type="button" className="settings-refresh" onClick={onResetLayout}>
                  Reset pane sizes
                </button>
                <span className="settings-hint">Restore the default left/right pane widths.</span>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- Features (modular toggles + goal profiles) ---- */}
          {tab === 'appearance' && (
          <fieldset className="settings-section">
            <legend>Features</legend>
            <div className="settings-field">
              <span className="settings-label">Profile</span>
              <div className="theme-switcher settings-profiles" role="group" aria-label="Feature profile">
                {PROFILE_LIST.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    className={`theme-chip${features.profile === p.id ? ' active' : ''}`}
                    aria-pressed={features.profile === p.id}
                    title={p.blurb}
                    onClick={() => {
                      // Switching from a hand-tuned set discards it — confirm first.
                      if (
                        features.profile !== 'custom' ||
                        window.confirm(`Switch to “${p.label}”? This replaces your custom feature set.`)
                      ) {
                        features.applyProfile(p.id)
                      }
                    }}
                  >
                    {p.label}
                  </button>
                ))}
                {features.profile === 'custom' && (
                  <span className="theme-chip active" aria-disabled="true" title="Custom — a blended feature set (manual toggles or multiple goals)">
                    Custom
                  </span>
                )}
              </div>
              <span className="settings-hint">
                {features.profile === 'custom'
                  ? 'Custom — a blended feature set. Pick a single goal above to reset to its defaults.'
                  : 'Pick a goal to set sensible defaults — every feature stays toggleable below. Switching profiles re-applies its set.'}
                {onRerunWizard && (
                  <>
                    {' '}
                    <button type="button" className="settings-linkbtn" onClick={onRerunWizard}>
                      Re-run setup…
                    </button>
                  </>
                )}
              </span>
            </div>

            {/* Core spine first, as a locked group (spec §4.4). */}
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Core — always on</span>
              <div className="settings-grid">{FEATURES.filter((f) => f.core).map(featureRow)}</div>
            </div>

            {/* Optional features, grouped by category. Field Day is NOT a plain feature
                toggle: its visibility is driven by the persisted master switch
                (settings.fdActive), so the Contesting group hosts that master toggle in
                place of the old standalone fieldDay flag. */}
            {FEATURE_CATEGORY_ORDER.map((cat) => {
              const inCat = FEATURES.filter((f) => f.category === cat && !f.core && f.id !== 'fieldDay')
              const isContesting = cat === 'Contesting'
              if (inCat.length === 0 && !isContesting) return null
              return (
                <div className="settings-featgroup" key={cat}>
                  <span className="settings-featgroup-title">{cat}</span>
                  <div className="settings-grid">
                    {isContesting && (
                      <div className="settings-field">
                        <label className="settings-toggle">
                          <span className="settings-label">Field Day mode</span>
                          <button
                            type="button"
                            role="switch"
                            aria-checked={!!form.fdActive}
                            className={`toggle${form.fdActive ? ' on' : ''}`}
                            onClick={() => {
                              const next = !form.fdActive
                              updateBool('fdActive', next)
                              // Turning on with no Class/Section yet → jump to the FD setup
                              // tab (now visible) so they're filled; the backend won't enter
                              // Field Day until both are set.
                              if (next && (!form.fdClass.trim() || !form.fdSection.trim())) setTab('contesting')
                            }}
                            aria-label={`${form.fdActive ? 'Disable' : 'Enable'} Field Day mode`}
                          >
                            <span className="toggle-knob" />
                          </button>
                        </label>
                        <span className="settings-hint">
                          Turn on for Field Day weekend — reveals the Field Day workspace, the
                          Class/Section exchange across all modes, and the setup tab. Off the rest of
                          the year (nothing shows). Stays on across restarts until you turn it off;
                          Save settings to apply.
                        </span>
                      </div>
                    )}
                    {inCat.map(featureRow)}
                  </div>
                </div>
              )
            })}
          </fieldset>
          )}

          {/* ---- Operator & radio ---- */}
          {tab === 'station' && (
          <fieldset className="settings-section">
            <legend>Operator &amp; Radio</legend>
            <div className="settings-grid">
              {BASIC_FIELDS.map((f) => {
                const value = form[f.key]
                const invalid = f.key === 'mycall' && !String(value).trim()
                return (
                  <label className="settings-field" key={f.key}>
                    <span className="settings-label">{f.label}</span>
                    <input
                      className={`settings-input${invalid && error ? ' invalid' : ''}`}
                      type={f.type}
                      value={String(value)}
                      placeholder={f.placeholder}
                      onChange={(e) => update(f.key, e.target.value)}
                      aria-invalid={invalid && !!error}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    {f.hint && <span className="settings-hint">{f.hint}</span>}
                  </label>
                )
              })}
              <label className="settings-field">
                <span className="settings-label">License Class</span>
                <select
                  className="settings-input"
                  value={String(form.licenseClass ?? 'open')}
                  onChange={(e) => update('licenseClass', e.target.value)}
                >
                  <option value="technician">Technician (US)</option>
                  <option value="general">General (US)</option>
                  <option value="extra">Amateur Extra (US)</option>
                  <option value="open">Open — no transmit limits</option>
                </select>
                <span className="settings-hint">
                  Sets your transmit privileges + the licensed-segment band dropdown. Open = no
                  limits (outside the US).
                </span>
              </label>
            </div>
            <div className="settings-freq">
              <span className="settings-label">Band &amp; Frequency</span>
              <FrequencyControl
                channels={bandPlan}
                dialMhz={form.dialMhz}
                band={form.band}
                mode={form.sideband}
                variant="full"
                onSet={setFreq}
              />
              <span className="settings-hint">
                Pick a band-plan channel, or type a dial frequency in MHz.
              </span>
            </div>
          </fieldset>
          )}

          {/* ---- Rig control ---- */}
          {tab === 'radio' && (
          <>
          <SetupHealth radio={radio} catResult={catResult} onProveTx={onProveTx} />
          {/* Dual-radio roster (P2). Always shown — the "+ Add radio" button is the discovery
              affordance a single-radio operator sees; the per-radio list + band coverage only
              matter once there's a 2nd radio. */}
          <fieldset className="settings-section">
            <legend>Radios</legend>
            {editingRadioId != null && editingRadioId !== form.activeRadio && (
              <p className="settings-note radio-editing-note">
                <strong>
                  Editing {form.radios?.find((r) => r.id === editingRadioId)?.name ?? 'another radio'}
                </strong>{' '}
                — not your operating radio. <strong>Save</strong> writes only this radio&apos;s CAT /
                audio config; your active radio and station-wide settings are untouched.
              </p>
            )}
            <div className="radios-manager">
              {(form.radios ?? []).map((r) => {
                const isActive = r.id === form.activeRadio
                const isEditing = r.id === editingRadioId
                const multi = (form.radios?.length ?? 1) > 1
                return (
                  <div
                    key={r.id}
                    className={`radio-card${isActive ? ' active' : ''}${isEditing ? ' editing' : ''}`}
                  >
                    <div className="radio-card-head">
                      <input
                        className="settings-input radio-name-input"
                        type="text"
                        defaultValue={r.name}
                        placeholder="Radio name"
                        onBlur={(e) => {
                          const v = e.target.value.trim()
                          if (v && v !== r.name) handleRenameRadio(r.id, v)
                        }}
                        autoComplete="off"
                        spellCheck={false}
                      />
                      {isActive && (
                        <span className="radio-active-badge" title="Your operating radio.">
                          Active
                        </span>
                      )}
                      {isEditing && !isActive && (
                        <span
                          className="radio-editing-badge"
                          title="The Rig / CAT + Audio form below is editing this radio."
                        >
                          Editing
                        </span>
                      )}
                      {!isEditing && (
                        <button
                          type="button"
                          className="settings-refresh"
                          onClick={() => handleConfigureRadio(r.id)}
                          title="Edit this radio's CAT / audio below — WITHOUT changing your operating radio (no swap, no dropped carrier)."
                        >
                          Edit
                        </button>
                      )}
                      {!isActive && (
                        <button
                          type="button"
                          className="settings-refresh"
                          onClick={() => handleMakeActive(r.id)}
                          title="Make this your operating radio (swaps rigs; drops any carrier first)."
                        >
                          Make active
                        </button>
                      )}
                      {multi && !isActive && (
                        <button
                          type="button"
                          className="settings-refresh danger"
                          onClick={() => handleRemoveRadio(r.id)}
                          title="Remove this radio from the roster"
                        >
                          Remove
                        </button>
                      )}
                    </div>
                    <div className="radio-card-meta">
                      {r.rigModelName && r.rigModelName !== 'None / VOX'
                        ? r.rigModelName
                        : 'No rig model set'}
                      {' · '}
                      {r.rigConn === 'network' ? r.rigAddr || 'no address' : r.serialPort || 'no COM port'}
                      {' · audio '}
                      {r.audioIn ? (audioLabels.input[r.audioIn] ?? r.audioIn) : 'default'}
                      {' · rigctld :'}
                      {r.rigctldPort}
                    </div>
                    {multi && (
                      <div className="radio-band-coverage">
                        <span className="settings-hint">
                          Covers bands (for auto band-routing; none = covers all):
                        </span>
                        <div className="band-chip-row">
                          {FREQ_BANDS.map((b) => {
                            const on = r.bands.includes(b)
                            return (
                              <button
                                key={b}
                                type="button"
                                className={`band-chip${on ? ' on' : ''}`}
                                aria-pressed={on}
                                onClick={() => handleToggleRadioBand(r.id, b)}
                              >
                                {b}
                              </button>
                            )
                          })}
                        </div>
                      </div>
                    )}
                  </div>
                )
              })}
            </div>
            <div className="radios-actions">
              <button type="button" className="settings-refresh" onClick={handleAddRadio}>
                + Add radio
              </button>
              <span className="settings-hint">
                {(form.radios?.length ?? 1) > 1
                  ? `The Rig / CAT + Audio settings below edit “${
                      form.radios?.find((r) => r.id === editingRadioId)?.name ?? 'the selected radio'
                    }”. Each radio has its OWN CAT + audio — click “Edit” on any radio to configure it WITHOUT changing the one you're operating on; “Make active” swaps your operating radio.`
                  : 'Run two rigs at once — e.g. an HF radio plus a VHF/UHF radio on a different antenna? Add a second radio; you can then Edit either one without interrupting the one you are operating on. Newcomers can ignore this.'}
              </span>
            </div>
            {(form.radios?.length ?? 1) > 1 && (
              <div className="routing-rules">
                <span className="settings-hint">
                  <strong>Route by band AND mode</strong> — band coverage above sends a whole band to
                  one radio. Add rules here when TWO radios share a band and the MODE decides which
                  one: 2 m FT8 to the digital rig, 2 m FM and APRS to the FM rig. Rules are checked
                  top to bottom and the FIRST match wins; anything no rule matches falls back to band
                  coverage, then to the default radio.
                </span>
                {(form.routingRules ?? []).length === 0 && (
                  <p className="settings-note">
                    No rules — routing is by band only (today&apos;s behavior).
                  </p>
                )}
                {(form.routingRules ?? []).map((rule, i) => (
                  <div className="routing-rule" key={i}>
                    <div className="routing-rule-head">
                      <span className="routing-rule-n">{i + 1}.</span>
                      {/* 'satellite' is a CONTEXT designation, not a sixth mode class: it rides
                          the same dropdown (that is where the operator looks for it) but is
                          stored as `context` with the mode cleared. A satellite rule is matched
                          only by transponder picks, at a tier above the mode rules. */}
                      <select
                        className="settings-input"
                        value={rule.context === 'satellite' ? 'satellite' : (rule.mode ?? '')}
                        onChange={(e) =>
                          handlePatchRule(
                            i,
                            e.target.value === 'satellite'
                              ? { mode: null, context: 'satellite' }
                              : {
                                  mode:
                                    e.target.value === '' ? null : (e.target.value as RouteMode),
                                  context: null,
                                },
                          )
                        }
                        aria-label={`Rule ${i + 1} mode`}
                      >
                        <option value="">Any mode</option>
                        {ROUTE_MODES.map(([v, label]) => (
                          <option key={v} value={v}>
                            {label}
                          </option>
                        ))}
                        <option
                          value="satellite"
                          title="Satellite passes only: picking a transponder checks Satellite rules before the mode rules, so the FM & APRS rule can keep terrestrial packet while satellites go to the sat rig. Terrestrial tunes never match this rule."
                        >
                          Satellite
                        </option>
                      </select>
                      <span className="routing-rule-arrow">→</span>
                      <select
                        className="settings-input"
                        value={rule.radio}
                        onChange={(e) => handlePatchRule(i, { radio: Number(e.target.value) })}
                        aria-label={`Rule ${i + 1} radio`}
                      >
                        {(form.radios ?? []).map((r) => (
                          <option key={r.id} value={r.id}>
                            {r.name}
                          </option>
                        ))}
                      </select>
                      <button
                        type="button"
                        className="settings-refresh"
                        onClick={() => handleMoveRule(i, -1)}
                        disabled={i === 0}
                        title="Check this rule earlier (first match wins)"
                        aria-label={`Move rule ${i + 1} up`}
                      >
                        ↑
                      </button>
                      <button
                        type="button"
                        className="settings-refresh"
                        onClick={() => handleMoveRule(i, 1)}
                        disabled={i === (form.routingRules?.length ?? 0) - 1}
                        title="Check this rule later"
                        aria-label={`Move rule ${i + 1} down`}
                      >
                        ↓
                      </button>
                      <button
                        type="button"
                        className="settings-refresh danger"
                        onClick={() => handleRemoveRule(i)}
                        aria-label={`Remove rule ${i + 1}`}
                      >
                        ✕
                      </button>
                    </div>
                    <div className="band-chip-row">
                      {FREQ_BANDS.map((b) => {
                        const on = rule.bands.includes(b)
                        return (
                          <button
                            key={b}
                            type="button"
                            className={`band-chip${on ? ' on' : ''}`}
                            aria-pressed={on}
                            onClick={() => handleToggleRuleBand(i, b)}
                          >
                            {b}
                          </button>
                        )
                      })}
                    </div>
                    <span className="settings-hint">
                      {rule.bands.length === 0 ? 'Any band' : rule.bands.join(', ')}
                      {' · '}
                      {rule.context === 'satellite'
                        ? 'Satellite'
                        : rule.mode
                          ? ROUTE_MODE_LABEL[rule.mode]
                          : 'any mode'}
                      {' → '}
                      {form.radios?.find((r) => r.id === rule.radio)?.name ?? `Radio ${rule.radio}`}
                    </span>
                  </div>
                ))}
                <div className="radios-actions">
                  <button type="button" className="settings-refresh" onClick={handleAddRule}>
                    + Add routing rule
                  </button>
                  <label className="settings-input-row routing-default">
                    <span className="settings-label">Everything else</span>
                    <select
                      className="settings-input"
                      value={form.defaultRadio ?? ''}
                      onChange={(e) =>
                        handleSetDefaultRadio(e.target.value === '' ? null : Number(e.target.value))
                      }
                      aria-label="Default radio"
                    >
                      <option value="">Stay on the current radio</option>
                      {(form.radios ?? []).map((r) => (
                        <option key={r.id} value={r.id}>
                          {r.name}
                        </option>
                      ))}
                    </select>
                  </label>
                </div>
                {/* Test affordance: check the table without QSYing a rig. Answered by the same
                    resolver the radio loop uses, so it can't drift from real behavior. */}
                <div className="routing-test">
                  <span className="settings-label">Test a band + mode</span>
                  <div className="settings-input-row">
                    <select
                      className="settings-input"
                      value={routeTest.band}
                      onChange={(e) => runRouteTest(e.target.value, routeTest.mode)}
                      aria-label="Test band"
                    >
                      {FREQ_BANDS.map((b) => (
                        <option key={b} value={b}>
                          {b}
                        </option>
                      ))}
                    </select>
                    <select
                      className="settings-input"
                      value={routeTest.mode}
                      onChange={(e) => runRouteTest(routeTest.band, e.target.value as RouteMode)}
                      aria-label="Test mode"
                    >
                      {ROUTE_MODES.map(([v, label]) => (
                        <option key={v} value={v}>
                          {label}
                        </option>
                      ))}
                    </select>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={() => runRouteTest(routeTest.band, routeTest.mode)}
                    >
                      Where would this go?
                    </button>
                  </div>
                  {routeTestResult && (
                    <p className="settings-note routing-test-result">
                      {routeTest.band} {ROUTE_MODE_LABEL[routeTest.mode]} →{' '}
                      <strong>{routeTestResult}</strong>
                    </p>
                  )}
                </div>
              </div>
            )}
            {(form.radios?.length ?? 1) > 1 && (
              <label className="settings-field settings-simul-radios">
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!!form.simultaneousRadios}
                    onChange={(e) => updateBool('simultaneousRadios', e.target.checked)}
                    aria-label="Run both radios at the same time"
                  />
                  <span className="settings-hint">
                    <strong>Run both radios at the same time</strong> — launch Nexus and it asks
                    which radio this window drives; open a second window for the other. Both share
                    one logbook. Leave off if you only ever use one radio at a time (you can still
                    switch between them from the top bar).
                  </span>
                </span>
              </label>
            )}
          </fieldset>
          <fieldset className="settings-section">
            <legend>Profiles</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Saved profiles</span>
                <div className="settings-input-row">
                  <select
                    className="settings-input"
                    value={selectedProfile}
                    onChange={(e) => setSelectedProfile(e.target.value)}
                  >
                    <option value="">— Select a profile —</option>
                    {profiles.map((p) => (
                      <option key={p.name} value={p.name}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={handleLoadProfile}
                    disabled={!selectedProfile}
                    title="Apply this profile — merged onto your current settings. Your callsign, license class, radio roster and sync history never come from a profile, and anything the profile predates keeps its current value."
                  >
                    Load
                  </button>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={handleDeleteProfile}
                    disabled={!selectedProfile}
                  >
                    Delete
                  </button>
                </div>
                <span className="settings-hint">
                  Switch a whole rig / antenna / CAT / band setup in one move.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Save current as</span>
                <div className="settings-input-row">
                  <input
                    className="settings-input"
                    type="text"
                    value={newProfileName}
                    placeholder="e.g. Portable VHF"
                    onChange={(e) => setNewProfileName(e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={handleSaveProfile}
                    disabled={!newProfileName.trim()}
                  >
                    Save
                  </button>
                </div>
                <span className="settings-hint">Snapshots the current settings under a name.</span>
              </label>
            </div>
          </fieldset>

          <fieldset className="settings-section">
            <legend>Rig Control</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">PTT Method</span>
                <select
                  className="settings-input"
                  value={form.pttMethod}
                  onChange={(e) => update('pttMethod', e.target.value)}
                >
                  {PTT_METHODS.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">How transmit is keyed.</span>
              </label>

              {(form.pttMethod === 'rts' || form.pttMethod === 'dtr') && (
                <label className="settings-field">
                  <span className="settings-label">PTT Serial Port</span>
                  <input
                    className="settings-input"
                    list="serial-port-list"
                    value={form.pttSerialPort}
                    placeholder="e.g. COM16 — blank = use the CAT port"
                    onChange={(e) => update('pttSerialPort', e.target.value)}
                  />
                  <span className="settings-hint">
                    COM port your RTS/DTR keying line is on — e.g. an SO2R controller (u2R/MK2R)
                    that routes PTT on its own port, separate from CAT. Leave blank if keying
                    shares the CAT port, which is how a single-cable interface like a Digirig
                    Mobile is wired; CAT keeps working either way. <strong>Per radio</strong>:
                    each rig on an SO2R box has its own keying port, and this one follows the
                    radio you switch to.
                  </span>
                </label>
              )}

              <div className="settings-field">
                <span className="settings-label">Zero-config setup</span>
                <div className="settings-input-row">
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={onDetectRigs}
                    disabled={detecting}
                  >
                    {detecting ? 'Scanning…' : 'Detect my radio'}
                  </button>
                </div>
                {(detected.length > 0 || detectedFlex.length > 0) && (
                  <ul className="rig-detect-list">
                    {detectedFlex.map((f, i) => (
                      <li className="rig-detect" key={`flex-${f.ip}-${i}`}>
                        <div className="rig-detect-main">
                          <span className="rig-detect-name">
                            {f.model}
                            {f.nickname ? ` “${f.nickname}”` : ''} — network
                          </span>
                          <span className="rig-detect-meta">
                            {f.ip} · via SmartSDR CAT on this PC (slice A, TCP 5002)
                          </span>
                        </div>
                        <button type="button" className="settings-save" onClick={() => applyDetectedFlex(f)}>
                          Use this
                        </button>
                      </li>
                    ))}
                    {detected.map((r, i) => (
                      <li className="rig-detect" key={`${r.portName}-${i}`}>
                        <div className="rig-detect-main">
                          <span className="rig-detect-name">
                            {r.interfaceName ??
                              r.suggestedModelName ??
                              (r.product || 'Unknown radio')}
                          </span>
                          <span className="rig-detect-meta">
                            {r.portName} · {r.chip}
                            {/* Dual-UART Icoms (IC-7610/9700) show up as TWO rows that both
                                say the rig's name — the A/B tag is the only tie-breaker. */}
                            {r.civSide === true
                              ? ' · CI-V port — use this one'
                              : r.civSide === false
                                ? ' · second port, not CI-V'
                                : ''}
                            {/* The suggestion is a device NAME (that is what gets stored);
                                show its human label so the Linux row reads
                                "USB AUDIO CODEC", not "plughw:CARD=CODEC,DEV=0". */}
                            {r.suggestedAudio
                              ? ` · ${audioLabels.input[r.suggestedAudio] ?? r.suggestedAudio}`
                              : ''}
                          </span>
                          {/* A recognised interface is a CABLE, not a radio. Say so plainly and
                              tell the operator what is still theirs to choose — the rig — rather
                              than showing the generic "couldn't identify the model" warning,
                              which reads as a failure when nothing actually went wrong. */}
                          {r.interfaceName && (
                            <span className="rig-detect-interface">
                              This is an interface cable, not a radio — pick your rig in{' '}
                              <em>Rig Model</em> below. {r.interfaceNote}
                            </span>
                          )}
                          {!r.suggestedModel && !r.interfaceName && (
                            <span className="rig-detect-nomodel">
                              ⚠ Found the port but not the exact model — normal when the rig sits
                              behind a generic USB bridge chip that reports only its own name (common
                              on Icom, Yaesu, Kenwood, Elecraft, and Xiegu). Pick your rig in{' '}
                              <em>Rig Model</em> below, or click <em>Auto-test</em> (it tries the
                              common rigs to find the right port + baud for you).
                            </span>
                          )}
                          {r.driverNote && !r.driverBundled && (
                            <span className="rig-detect-driver">
                              {r.driverNote}
                              {r.driverUrl && (
                                <>
                                  {' '}
                                  <a href={r.driverUrl} target="_blank" rel="noreferrer">
                                    driver ↗
                                  </a>
                                </>
                              )}
                            </span>
                          )}
                        </div>
                        <button type="button" className="settings-save" onClick={() => applyDetectedRig(r)}>
                          Use this
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                <span className="settings-hint">
                  One scan for everything: USB radios (fills model, port, sound device)
                  AND FlexRadios on the network (fills the SmartSDR CAT config). Review, then Save.
                </span>
              </div>

              <label className="settings-field">
                <span className="settings-label">Rig Model</span>
                <div className="settings-input-row">
                  <select
                    className="settings-input"
                    value={String(form.rigModel)}
                    onChange={(e) => selectRig(Number(e.target.value))}
                  >
                    <option value="0">— None —</option>
                    {(showAllRigModels ? allRigModels : rigModels).map(([num, name]) => (
                      <option key={num} value={String(num)}>
                        {name} ({num})
                      </option>
                    ))}
                  </select>
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="numeric"
                    min="0"
                    placeholder="or enter model #"
                    onChange={(e) => {
                      const raw = e.target.value
                      const n = Number(raw)
                      if (raw.trim() !== '' && Number.isInteger(n) && n >= 0) {
                        markDirty()
                        setForm((prev) =>
                          prev ? { ...prev, rigModel: n, rigModelName: findRigModelName(n) } : prev,
                        )
                      }
                    }}
                    aria-label="Enter a Hamlib rig model number directly"
                  />
                </div>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={showAllRigModels}
                    onChange={(e) => onToggleShowAllRigModels(e.target.checked)}
                    aria-label="Show all Hamlib rig models"
                  />
                  <span className="settings-hint">
                    Show all models{allRigModelsLoading ? ' (loading…)' : ''} — the list above
                    defaults to ~50 curated common rigs; check this for the full Hamlib catalog.
                  </span>
                </span>
                <span className="settings-hint">
                  Hamlib rig model. Not listed? Type its model number directly — Hamlib may
                  still support it even without a friendly name here.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Connection</span>
                <select
                  className="settings-input"
                  value={form.rigConn || 'serial'}
                  onChange={(e) => update('rigConn', e.target.value)}
                >
                  <option value="serial">Serial (USB / COM)</option>
                  <option value="network">Network (FlexRadio / remote)</option>
                </select>
                <span className="settings-hint">
                  Serial for a USB/COM rig (most, incl. Xiegu); Network for a FlexRadio via
                  SmartSDR or a remote rigctld over TCP.
                </span>
              </label>

              {form.rigConn === 'network' && (
                <label className="settings-field">
                  <span className="settings-label">Network Address</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.rigAddr}
                    placeholder="127.0.0.1:5002"
                    onChange={(e) => update('rigAddr', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  {(() => {
                    const dax = findDaxDevices(audio.input, audio.output)
                    // Bootstrapper, not enforcer: once BOTH sides are any DAX
                    // device (auto or hand-picked), stop offering to "fix" them
                    // — re-pairing over a manual endpoint choice was reverting
                    // the operator's working config (real-6400M report).
                    const paired = isDaxPaired(form.audioIn, form.audioOut)
                    return dax && !paired ? (
                      <button
                        type="button"
                        className="settings-test-btn"
                        onClick={() => {
                          update('audioIn', dax.input)
                          update('audioOut', dax.output)
                          pushToast(`DAX paired: ${dax.input} → in, ${dax.output} → out`, 'success', 6000)
                        }}
                        title="SmartSDR's DAX virtual audio devices were detected — one click sets them as Nexus's audio in/out (bit-clean digital audio, no sound card)"
                      >
                        ⚡ Pair DAX audio ({dax.input})
                      </button>
                    ) : null
                  })()}
                  <span className="settings-hint">
                    host:port. For a Flex: the WSJT-X-proven path is the SmartSDR CAT app
                    on THIS PC — its DEFAULT TCP port 5002 is directed at slice A, so
                    127.0.0.1:5002 with the FLEX-6xxx model works out of the box; audio
                    rides DAX. Multi-slice: SmartSDR CAT's per-slice ports are B=60001,
                    C=60002, D=60003 — Nexus drives ONE slice, so enter the port of the
                    slice you run digital on. (Direct-to-radio :4992 needs Hamlib's
                    experimental native model and failed on real hardware.) Other rigs:
                    a remote rigctld's host:port with their normal model.
                  </span>
                </label>
              )}

              {form.rigConn !== 'network' && (
                <>
              <label className="settings-field">
                <span className="settings-label">Serial Port</span>
                <div className="settings-input-row">
                  {/* Combobox, not a bare <select>: some driver setups (virtual/SO2R COM
                      ports) make enumeration come back empty, so the operator must be able
                      to TYPE a port (e.g. COM16) — the datalist just offers the found ports. */}
                  <input
                    className="settings-input"
                    list="serial-port-list"
                    value={form.serialPort}
                    placeholder="Select or type, e.g. COM16"
                    onChange={(e) => update('serialPort', e.target.value)}
                  />
                  {/* Shared by the CAT + PTT port inputs. The label text shows the USB product
                      so two identical-looking ports (e.g. a Xiegu's SERIAL-A vs SERIAL-B) are
                      distinguishable in the suggestion list. */}
                  <datalist id="serial-port-list">
                    {portOptions.map((p) => (
                      <option key={p} value={p}>
                        {portLabels[p] || ''}
                      </option>
                    ))}
                  </datalist>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={refreshPorts}
                    disabled={portsLoading}
                    title="Re-scan serial ports"
                  >
                    {portsLoading ? '…' : 'Refresh'}
                  </button>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={() => handleAutoTestPorts()}
                    disabled={catTesting}
                    title="Probe each USB port (read-only — never transmits) and auto-select the one that drives your rig"
                  >
                    {catTesting ? '…' : 'Auto-test'}
                  </button>
                </div>
                <span className="settings-hint">
                  COM / tty device for rig control — or Auto-test to find it.
                  {[3088, 3087, 3091, 3089, 3076].includes(form.rigModel) && (
                    <>
                      {' '}
                      <strong>Xiegu:</strong> the radio makes two serial ports — CAT is on the{' '}
                      <strong>SERIAL-B</strong> one (often the higher COM number).
                    </>
                  )}
                  {[3078, 3081].includes(form.rigModel) && (
                    <>
                      {' '}
                      <strong>Icom:</strong> this radio makes two COM ports and only one speaks
                      CI-V — in Device Manager it is the CP210x port marked{' '}
                      <strong>Enhanced</strong> (Icom's driver: “Serial Port A (CI-V)”). The
                      “Standard” / “Serial Port B” one never answers.
                    </>
                  )}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Baud</span>
                <select
                  className="settings-input"
                  value={String(form.baud)}
                  onChange={(e) => updateNum('baud', Number(e.target.value))}
                >
                  {(STANDARD_BAUDS.includes(form.baud)
                    ? STANDARD_BAUDS
                    : [form.baud, ...STANDARD_BAUDS]
                  ).map((b) => (
                    <option key={b} value={b}>
                      {b.toLocaleString()}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  Serial baud rate — match your rig's CAT setting (most modern rigs: 38,400 or 115,200).
                  Native Icom CI-V scope needs 115,200 here <em>and</em> on the rig.
                </span>
              </label>
                </>
              )}

              <div className="settings-field">
                <span className="settings-label">Antenna rotator</span>
                {(() => {
                  const CURATED = [
                    '0', '601', '603', '602', '901', '902', '202', '204', '401',
                    '403', '405', '1001', '1102', '1701', '1',
                  ]
                  const modelStr = String(form.rotatorModel ?? 0)
                  const isOther = rotOther || !CURATED.includes(modelStr)
                  return (
                    <>
                      <select
                        value={isOther ? 'other' : modelStr}
                        onChange={(e) => {
                          const v = e.target.value
                          if (v === 'other') {
                            setRotOther(true)
                            setRotCustom(
                              (form.rotatorModel ?? 0) > 0 ? String(form.rotatorModel) : '',
                            )
                          } else {
                            setRotOther(false)
                            updateNum('rotatorModel', Number(v))
                          }
                        }}
                        aria-label="Rotator model"
                      >
                        <option value="0">None</option>
                        <option value="601">Yaesu GS-232A</option>
                        <option value="603">Yaesu GS-232B</option>
                        <option value="602">GS-232 (generic)</option>
                        <option value="901">SPID Rot2Prog</option>
                        <option value="902">SPID Rot1Prog</option>
                        <option value="202">EasyComm II</option>
                        <option value="204">EasyComm III</option>
                        <option value="401">Hy-Gain Rotor-EZ</option>
                        <option value="403">Hy-Gain DCU</option>
                        <option value="405">Green Heron RT-21</option>
                        <option value="1001">M2 RC2800</option>
                        <option value="1102">EA4TX ARS (az)</option>
                        <option value="1701">Prosistel D (az)</option>
                        <option value="1">Dummy (testing — no hardware)</option>
                        <option value="other">Other Hamlib model #…</option>
                      </select>
                      {isOther && (
                        <input
                          className="settings-input"
                          type="number"
                          min="1"
                          placeholder="Hamlib rotator model number (rotctl -l lists them)"
                          value={rotCustom}
                          onChange={(e) => {
                            setRotCustom(e.target.value)
                            const n = Number(e.target.value)
                            // Only ever commit a REAL model; an incomplete
                            // entry leaves the last valid value in the form.
                            if (Number.isInteger(n) && n > 0) updateNum('rotatorModel', n)
                          }}
                          aria-label="Hamlib rotator model number"
                        />
                      )}
                    </>
                  )
                })()}
                {(form.rotatorModel ?? 0) > 1 && (
                  <div className="settings-inline-pair">
                    <input
                      className="settings-input"
                      type="text"
                      value={form.rotatorPort ?? ''}
                      placeholder="COM7 / /dev/ttyUSB1"
                      onChange={(e) => update('rotatorPort', e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                      aria-label="Rotator serial port"
                    />
                    <input
                      className="settings-input"
                      type="number"
                      value={form.rotatorBaud ?? 9600}
                      onChange={(e) => {
                        const n = Number(e.target.value)
                        if (!Number.isNaN(n)) updateNum('rotatorBaud', n)
                      }}
                      aria-label="Rotator baud rate"
                      title="Baud rate (GS-232 default 9600)"
                    />
                  </div>
                )}
                <span className="settings-hint">
                  Pick your rotator and its COM port — Nexus runs the control daemon for you
                  (same as the rig). Then use the Rotor pane in Connect, ↗ on Needed rows,
                  or the compass anywhere.
                </span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.rotatorHost}
                  placeholder="Advanced: external rotctld host:port (overrides the above)"
                  onChange={(e) => update('rotatorHost', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                  aria-label="External rotctld address (advanced)"
                />
              </div>

              <label className="settings-field">
                <span className="settings-label">ISS SSTV auto-arm</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!!form.issSstvAutoArm}
                    onChange={(e) => updateBool('issSstvAutoArm', e.target.checked)}
                    aria-label="Auto-arm SSTV for ISS passes"
                  />
                  <span className="settings-hint">
                    Auto-arm SSTV for ISS passes — tunes 145.800 FM and arms the decoder when
                    the ISS is overhead, restores your dial at LOS. Off by default.
                  </span>
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">Split operation</span>
                <div className="theme-switcher" role="group" aria-label="Split operation">
                  {SPLIT_MODES.map((m) => (
                    <button
                      key={m.value}
                      type="button"
                      className={`theme-chip${(form.splitMode ?? 'none') === m.value ? ' active' : ''}`}
                      aria-pressed={(form.splitMode ?? 'none') === m.value}
                      onClick={() => setSplitMode(m.value)}
                    >
                      {m.label}
                    </button>
                  ))}
                </div>
                <span className="settings-hint">
                  Keeps your transmitted audio between 1500–2000 Hz by shifting the TX dial in
                  500 Hz steps, so audio harmonics fall outside the transmit filter — cleaner
                  signal. Rig = uses VFO B split. Fake It = retunes the VFO around each over (works
                  on any CAT rig). None = stock WSJT-X default, transmits at the raw audio offset.
                </span>
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  Wheel tuning sensitivity{' '}
                  <span className="settings-value">×{(form.wheelTuneSensitivity ?? 1).toFixed(2)}</span>
                </span>
                <input
                  className="settings-slider"
                  type="range"
                  min="0.25"
                  max="2"
                  step="0.05"
                  value={String(form.wheelTuneSensitivity ?? 1)}
                  onChange={(e) => updateNum('wheelTuneSensitivity', Number(e.target.value))}
                  aria-label="Mouse-wheel tuning sensitivity"
                />
                <span className="settings-hint">
                  How far the dial moves per mouse-wheel notch. Lower it if a high-resolution or
                  free-spin mouse tunes too far per flick; raise it to tune faster. Applies to the
                  frequency readout and the Phone/CW scope wheel.
                </span>
              </label>
            </div>
            <SettingsGroup title="Advanced" defaultOpen={false}>
              <label className="settings-field">
                <span className="settings-label">rigctld TCP Port</span>
                <input
                  className="settings-input"
                  type="number"
                  inputMode="numeric"
                  value={String(form.rigctldPort)}
                  placeholder="4532"
                  onChange={(e) => update('rigctldPort', e.target.value)}
                  autoComplete="off"
                />
                <span className="settings-hint">Port Nexus launches rigctld on.</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Data modes use plain SSB</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={form.dataModesPlainSsb ?? false}
                  className={`toggle${form.dataModesPlainSsb ? ' on' : ''}`}
                  onClick={() => updateBool('dataModesPlainSsb', !form.dataModesPlainSsb)}
                >
                  <span className="toggle-knob" />
                </button>
                <span className="settings-hint">
                  <strong>Leave this off unless you know you need it.</strong> Nexus normally puts
                  the radio in its DATA submode (DATA-U / USB-D / PKTUSB) for FT8, FT4, RTTY-AFSK
                  and SSTV, because on most rigs that is the only mode where the USB codec reaches
                  the transmitter. Turn this on and Nexus commands plain{' '}
                  <strong>USB/LSB</strong> for those modes instead, and stays there — through band
                  changes and when you call a station.
                  {' '}
                  Only correct if your transmit audio goes in the <strong>microphone</strong> path,
                  as with an interface wired to the mic jack (some RIGblaster models). On a rig
                  where the codec feeds only the data port, plain SSB takes audio from the mic and
                  the radio transmits <strong>no RF at all</strong> — a red TX light and nothing on
                  the air. <strong>Per radio</strong>, since it depends on how that rig is cabled.
                  True FSK RTTY is unaffected — it keeps the rig's own RTTY mode.
                </span>
              </label>

              {form.rigConn !== 'network' &&
                /IC-?\s?(7300|7610|9700|705|905)\b/i.test(form.rigModelName ?? '') && (
                  <label className="settings-field">
                    <span className="settings-label">Native Icom CI-V (early access)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.icomNativeCat ?? false}
                      className={`toggle${form.icomNativeCat ? ' on' : ''}`}
                      onClick={() => updateBool('icomNativeCat', !form.icomNativeCat)}
                    >
                      <span className="toggle-knob" />
                    </button>
                    <span className="settings-hint">
                      Nexus drives this Icom's CI-V directly instead of launching rigctld —
                      unlocking the rig's real spectrum scope in the waterfall ("CI-V RF") and
                      instant dial tracking. The scope needs{' '}
                      <strong>115200 baud, set the same on BOTH the radio and Nexus</strong>:
                      (1) on the rig, Menu ▸ SET ▸ Connectors ▸ CI-V ▸ "CI-V USB Baud Rate" ={' '}
                      <strong>115200</strong>; (2) on the rig, same menu, "CI-V USB Port" =
                      "Unlink from [REMOTE]"; (3) the <strong>Baud</strong> field above ={' '}
                      <strong>115200</strong> to match. Below that the rig refuses to stream the
                      scope (CAT still works; the panadapter just stays off). Save to apply; turn
                      off any time to return to the classic Hamlib path.
                    </span>
                  </label>
                )}

              {form.rigConn === 'network' &&
                (/flex/i.test(form.rigModelName ?? '') || (form.flexRadioIp ?? '').trim() !== '') && (
                  <label className="settings-field">
                    <span className="settings-label">Flex native panadapter (early access)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.flexNativePan ?? false}
                      className={`toggle${form.flexNativePan ? ' on' : ''}`}
                      onClick={() => updateBool('flexNativePan', !form.flexNativePan)}
                    >
                      <span className="toggle-knob" />
                    </button>
                    <span className="settings-hint">
                      Stream this FlexRadio's real SmartSDR panadapter (VITA-49 FFT) into the
                      cockpit scope — the RF spectrum around your dial, with the Flex-pan span/ref
                      controls. <strong>Unverified on hardware</strong>, so it's opt-in: needs the
                      Flex IP set (from Find Radios) and SmartSDR reachable on this network. If the
                      scope stays blank or the app hitches, turn it back off. Save to apply.
                    </span>
                  </label>
                )}

              {form.rigConn === 'network' &&
                (/flex/i.test(form.rigModelName ?? '') || (form.flexRadioIp ?? '').trim() !== '') && (
                  <label className="settings-field">
                    <span className="settings-label">Flex native DAX audio (early access)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.flexNativeAudio ?? false}
                      className={`toggle${form.flexNativeAudio ? ' on' : ''}`}
                      onClick={() => updateBool('flexNativeAudio', !form.flexNativeAudio)}
                    >
                      <span className="toggle-knob" />
                    </button>
                    <span className="settings-hint">
                      Take this FlexRadio's RX audio straight off the network (VITA-49 DAX) instead of
                      the "DAX Audio RX" sound device — which is <strong>invisible under Remote
                      Desktop</strong>. Decoders then read the rig's audio directly.{' '}
                      <strong>Unverified on hardware</strong>, RX-only, opt-in: needs the Flex IP set
                      and SmartSDR reachable. If decodes stop, turn it back off. Save to apply.
                    </span>
                  </label>
                )}

              {form.rigConn !== 'network' &&
                /IC-?\s?(7300|7610|9700|705|905)\b/i.test(form.rigModelName ?? '') &&
                form.icomNativeCat && (
                  <label className="settings-field">
                    <span className="settings-label">CI-V bus diagnostic log</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={civLogPath !== null}
                      className={`toggle${civLogPath !== null ? ' on' : ''}`}
                      onClick={() =>
                        void withErrorToast(
                          () => civDiagnosticLog(civLogPath === null),
                          'Could not toggle the CI-V diagnostic log',
                        ).then((path) => {
                          if (path === undefined) return // error already toasted
                          setCivLogPath(path === '' ? null : path)
                        })
                      }
                    >
                      <span className="toggle-knob" />
                    </button>
                    <span className="settings-hint">
                      {civLogPath !== null ? (
                        <>
                          <strong>Recording</strong> to <code>{civLogPath}</code> — this keeps
                          running while you're on other screens, so go to the FT8 or Phone cockpit
                          and reproduce the issue (Tune or transmit) now. Come back and turn it off
                          when done, then send that file. It shows exactly what's on the bus during
                          the fault.
                        </>
                      ) : (
                        <>
                          Records every byte to/from the radio on the native CI-V path to a file in
                          your Downloads — a support tool for hardware-only issues like the IC-9700
                          PTT flicker. Turn on, reproduce the problem, turn off, then send the file.
                        </>
                      )}
                    </span>
                  </label>
                )}

              {(form.rigModel === 2036 || form.rigModel === 23005) && (
                <label className="settings-field">
                  <span className="settings-label">Flex radio IP (native panadapter)</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.flexRadioIp}
                    placeholder="e.g. 192.168.1.50"
                    onChange={(e) => update('flexRadioIp', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">
                    Your FlexRadio's LAN IP (SmartSDR API, port 4992) — turns on the native RF
                    panadapter. This is the <em>radio's</em> address, not the SmartSDR-CAT port above.
                  </span>
                </label>
              )}

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Share my radio (CAT broker)</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.catBroker}
                    className={`toggle${form.catBroker ? ' on' : ''}`}
                    onClick={() => updateBool('catBroker', !form.catBroker)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Run a rigctld-compatible server so WSJT-X / N1MM / loggers share this radio THROUGH Nexus
                  (point them at Hamlib NET rigctl, localhost:{form.catBrokerPort}). Takes effect right away
                  — no restart. Works even when Nexus is sharing an external rigctld.
                </span>
              </div>

              {form.catBroker && (
                <label className="settings-field">
                  <span className="settings-label">CAT broker port</span>
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="numeric"
                    value={String(form.catBrokerPort)}
                    placeholder="4532"
                    onChange={(e) => update('catBrokerPort', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">Other apps connect here (Hamlib NET rigctl default 4532).</span>
                </label>
              )}
              {form.catBroker && (
                <div className="settings-field">
                  <span className="settings-label">Broker PTT</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.catBrokerPtt ?? false}
                    className={`toggle${form.catBrokerPtt ? ' on' : ''}`}
                    onClick={() => updateBool('catBrokerPtt', !form.catBrokerPtt)}
                  >
                    <span className="knob" />
                  </button>
                  <span className="settings-hint">
                    Let the connected app key transmit when Nexus is idle. Off = other apps
                    control the rig but never key it (Nexus owns TX).
                  </span>
                </div>
              )}
            </SettingsGroup>
            <div className="settings-cat-test">
              <button
                type="button"
                className="settings-testcat"
                onClick={handleTestCat}
                disabled={catTesting}
                title="Save settings, connect to the rig, and read its frequency"
              >
                {catTesting ? 'Testing…' : 'Test CAT'}
              </button>
              {(() => {
                // Show the just-run test result, else the live CAT status from the snapshot.
                const ok = catResult ? catResult.ok : radio?.catOk
                const detail = catResult ? catResult.detail : radio?.catDetail
                if (detail == null || detail === '') return null
                const cls = ok === true ? 'ok' : ok === false ? 'fail' : 'na'
                const mark = ok === true ? '✓ ' : ok === false ? '✗ ' : ''
                return (
                  <span className={`cat-result ${cls}`} role="status">
                    {mark}
                    {detail}
                  </span>
                )
              })()}
            </div>
            {/* Band-edge tones live with the RIG, not with Digital: useBandEdgeTones is
                called at App top level off snap.radio.txAllowed (App.tsx:739), so the cue
                fires on phone and CW exactly as it does on FT8. It was only ever filed
                under Digital by accident. */}
            <div className="settings-field">
              <label className="settings-toggle">
                <span className="settings-label">Band-edge tones</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={form.bandEdgeTones !== false}
                  className={`toggle${form.bandEdgeTones !== false ? ' on' : ''}`}
                  onClick={() => updateBool('bandEdgeTones', form.bandEdgeTones === false)}
                >
                  <span className="toggle-knob" />
                </button>
              </label>
              <span className="settings-hint">
                A short audio cue when the dial crosses your license privileges — a rising
                "ding" back in band, a falling "dong" past an edge. Applies on every mode.
              </span>
            </div>

            <div className="settings-field">
              <span className="settings-label">Max power by mode (safety)</span>
              <div className="settings-power-caps">
                {(
                  [
                    ['Phone', 'maxPowerPhone'],
                    ['CW', 'maxPowerCw'],
                    ['Digital', 'maxPowerDigital'],
                  ] as const
                ).map(([label, key]) => (
                  <label key={key} className="settings-power-cap">
                    <span>{label}</span>
                    <input
                      type="number"
                      min={0}
                      max={100}
                      inputMode="numeric"
                      placeholder="—"
                      value={capPct(form[key])}
                      onChange={(e) => updatePowerCap(key, e.target.value)}
                    />
                    <span className="settings-power-cap-unit">%</span>
                  </label>
                ))}
              </div>
              <span className="settings-hint">
                A ceiling on RF output per mode — leave blank for full power. FT8/FT4/RTTY run
                ~100% duty cycle, so capping the Digital modes (e.g. 30%) protects your finals and
                any amplifier. The rig is brought down to the cap the moment you enter a capped
                mode, not only when you touch the power slider.
              </span>
            </div>

            <p className="settings-note">
              Saving applies your rig settings live (no restart). <strong>Test CAT</strong> saves,
              launches the bundled <code>rigctld</code> (Hamlib ships with Nexus on Windows — no
              separate install), and reads your rig&apos;s frequency to confirm CAT. For CAT, pick
              your <em>Rig Model</em> and <em>Serial Port</em>; serial RTS/DTR and VOX need no model.
            </p>
          </fieldset>

          {/* ---- Satellite Doppler + rotator manners (Phase 1 sat station) ---- */}
          <fieldset className="settings-section">
            <legend>Satellite Doppler</legend>
            <p className="settings-note">
              Corrects both legs of a pass: the downlink you listen on and the uplink you
              transmit on. Nexus tunes only while auto-track is following a pass and you have
              picked a transponder in the Satellites section. The downlink needs no setup here;
              the uplink is confirmed once per radio, on the pass itself.
            </p>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Doppler correction</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!form.satDopplerOff}
                    onChange={(e) => updateBool('satDopplerOff', !e.target.checked)}
                    aria-label="Enable satellite Doppler correction"
                  />
                  <span className="settings-hint">
                    Retunes the radio through a pass so you stay on the station you are working.
                    On: the downlink follows the bird as soon as you arm a pass and hold a
                    transponder. Clearing this stops both legs.
                  </span>
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">VFO mapping</span>
                {/* DISABLED while the panel is editing a radio that is not the
                    operating one: the mapping is a flat (station-level) field
                    whose pick is a LIVE write confirming the uplink for the
                    OPERATING radio (the backend resolves it at write time) —
                    in the per-radio Edit flow that would confirm the active
                    rig while the panel shows another radio's card. A control
                    that consents a radio the operator is not looking at,
                    under a hint naming the wrong one, is how a wrong-uplink
                    consent gets minted — refuse with the reason instead.
                    The guard keys on the LIVE activeRadioId prop — the same
                    live state the verb resolves against — never the form
                    snapshot's activeRadio, which goes stale while the panel
                    sits open and would disable the select for the very radio
                    the operator is operating (round 4). */}
                <select
                  className="settings-input"
                  value={form.satVfoMap ?? 'off'}
                  disabled={
                    editingRadioId != null && editingRadioId !== (activeRadioId ?? form.activeRadio)
                  }
                  title={
                    editingRadioId != null && editingRadioId !== (activeRadioId ?? form.activeRadio)
                      ? 'The uplink mapping is confirmed per radio, for the radio you are operating. Confirm it for this radio on the pass rail during a pass, or make it the active radio first.'
                      : undefined
                  }
                  onChange={(e) => setSatVfoMap(e.target.value as NonNullable<Settings['satVfoMap']>)}
                  aria-label="Satellite VFO mapping"
                >
                  {SAT_VFO_MAPS.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  Which VFO carries your uplink. Match this to how your radio is wired.{' '}
                  <strong>A wrong mapping transmits on your own downlink</strong> — into the
                  satellite&apos;s output passband, on top of everyone else working the bird.
                  Picking one applies immediately and confirms it for the radio you are
                  operating; a second radio gets its own confirmation on the pass rail. Every
                  mapping except Uplink only keeps the downlink corrected.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Minimum shift (Hz)</span>
                <input
                  className="settings-input"
                  type="number"
                  min="0"
                  inputMode="numeric"
                  value={form.satMinShiftHz ?? 20}
                  onChange={(e) => {
                    const n = Number(e.target.value)
                    if (!Number.isNaN(n)) updateNum('satMinShiftHz', n)
                  }}
                  aria-label="Minimum Doppler shift before retuning (Hz)"
                />
                <span className="settings-hint">
                  Corrections smaller than this are not sent. 20 Hz is inaudible on SSB and keeps
                  the CAT link quiet. 0 sends every update.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Update interval (ms)</span>
                <input
                  className="settings-input"
                  type="number"
                  min="0"
                  inputMode="numeric"
                  value={form.satUpdateMs ?? 1000}
                  onChange={(e) => {
                    const n = Number(e.target.value)
                    if (!Number.isNaN(n)) updateNum('satUpdateMs', n)
                  }}
                  aria-label="Doppler update interval (milliseconds)"
                />
                <span className="settings-hint">
                  Shortest gap between corrections. 1000 ms is what a low-orbit pass needs.
                  Shorter fights your own tuning knob and saturates a serial CAT link.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Pass alert sounds</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!form.satPassAlertSoundOff}
                    onChange={(e) => updateBool('satPassAlertSoundOff', !e.target.checked)}
                    aria-label="Audible tones at pass start and end"
                  />
                  <span className="settings-hint">
                    A rising tone the moment an armed pass starts and a falling one when it
                    ends, alongside the popup — hear AOS with your hands on the rotor. On by
                    default; clearing this silences only the tones, never the popups.
                  </span>
                </span>
              </label>
            </div>
          </fieldset>

          {/* ---- Orbital elements (the TLE currency pipeline's operator surface;
                  cloned from the FCC callsign→state fieldset) ---- */}
          <fieldset className="settings-section">
            <legend>Orbital elements</legend>
            <div className="settings-field">
              <div className="lotw-users-row">
                <button
                  type="button"
                  className="settings-test-btn"
                  disabled={tleFetching}
                  onClick={() => {
                    setTleFetching(true)
                    fetchTlesNow()
                      .then((st) => {
                        // A failed ATTEMPT resolves too — the composer turns
                        // the typed outcome into the one operator-voiced
                        // result (raw error stays tooltip material).
                        setTleStatus(st)
                        const m = tleRefreshMessage(st)
                        pushToast(m.text, m.kind, m.kind === 'success' ? 5000 : 8000)
                      })
                      .catch((e) =>
                        // Only "couldn't attempt at all" rejects (a refresh
                        // already in flight) — already operator words.
                        pushToast(`${e instanceof Error ? e.message : e}`, 'error'),
                      )
                      .finally(() => setTleFetching(false))
                  }}
                >
                  {tleFetching ? 'Updating…' : 'Update now'}
                </button>
                <button
                  type="button"
                  className="settings-test-btn"
                  disabled={tleImporting}
                  onClick={() => tleFileRef.current?.click()}
                  title="Import a downloaded element file (Celestrak TLE, AMSAT keps, a new launch's SupGP set) — the offline-shack escape hatch. Imports persist across refreshes; the newest epoch per satellite wins."
                >
                  {tleImporting ? 'Importing…' : 'Import from file'}
                </button>
                <input
                  ref={tleFileRef}
                  type="file"
                  accept=".txt,.tle,text/plain"
                  style={{ display: 'none' }}
                  onChange={(e) => {
                    const f = e.target.files?.[0]
                    e.target.value = ''
                    if (!f) return
                    setTleImporting(true)
                    f.text()
                      .then((text) => importTles(text))
                      .then((st) => {
                        setTleStatus(st)
                        pushToast(
                          `Elements imported — ${st.importedCount} imported, ${st.count} total`,
                          'success',
                          5000,
                        )
                      })
                      .catch((err) =>
                        pushToast(
                          `Element import failed: ${err instanceof Error ? err.message : err}`,
                          'error',
                        ),
                      )
                      .finally(() => setTleImporting(false))
                  }}
                />
                {/* The birds the ceiling holds back, and the ones drifting
                    toward it, ride the line that is ALWAYS here — not the
                    "Last refresh" line below, which renders only while a
                    refresh has failed. A count an operator can read only
                    during an error is not a count. */}
                <span className="settings-hint">
                  {tleStatus && tleStatus.count > 0
                    ? `${[`${tleStatus.count} birds`, ...elementBandParts(tleStatus)].join(' · ')} · ${
                        tleStatus.fetchedAt > 0
                          ? `fetched ${new Date(tleStatus.fetchedAt * 1000).toISOString().slice(0, 10)}`
                          : 'never fetched'
                      } · ${tleStatus.source}${
                        tleStatus.importedCount > 0 ? ` · ${tleStatus.importedCount} imported` : ''
                      }`
                    : 'Not loaded yet — fetched on first launch, then refreshed every 6 h.'}
                </span>
              </div>
              <span className="settings-hint">
                Keplerian elements (TLEs) for the amateur satellites — pass times, pointing and
                Doppler all come from them. Refreshed every 6 h from hamradiotools.io: the bird
                list comes from the SatNOGS database (CC BY-SA 4.0), the elements from CelesTrak
                and SatNOGS. Import a file for an offline shack or a just-launched bird.
              </span>
              {tleStatus?.lastError && (
                // Operator words in the line, the raw error in the tooltip —
                // during the pre-launch window the mirror 404s by design,
                // and "HTTP 404" is not a thing to hand an operator.
                <span className="settings-hint" title={tleRefreshMessage(tleStatus).raw}>
                  Last refresh: {tleRefreshMessage(tleStatus).text}
                </span>
              )}
            </div>
          </fieldset>

          <fieldset className="settings-section">
            <legend>Rotator</legend>
            <p className="settings-note">
              Pointing manners for the rotator picked under <strong>Rig Control</strong>. They
              apply to satellite auto-track.
            </p>
            <div className="settings-grid">
              <div className="settings-field">
                <span className="settings-label">Park position (° az / el)</span>
                <div className="settings-inline-pair">
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="decimal"
                    value={form.rotParkAz ?? 0}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotParkAz', n)
                    }}
                    aria-label="Park azimuth (degrees)"
                  />
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="decimal"
                    value={form.rotParkEl ?? 0}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotParkEl', n)
                    }}
                    aria-label="Park elevation (degrees)"
                  />
                </div>
                <span className="settings-hint">
                  The stow position — wind-safe, or wherever your mast rests. Used only when
                  After a pass is set to Park.
                </span>
              </div>

              <div className="settings-field">
                <span className="settings-label">Ready position (° az / el)</span>
                <div className="settings-inline-pair">
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="decimal"
                    value={form.rotReadyAz ?? 0}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotReadyAz', n)
                    }}
                    aria-label="Ready azimuth (degrees)"
                  />
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="decimal"
                    value={form.rotReadyEl ?? 0}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotReadyEl', n)
                    }}
                    aria-label="Ready elevation (degrees)"
                  />
                </div>
                <span className="settings-hint">
                  Where the antenna waits for the next pass. Used only when After a pass is set
                  to Ready.
                </span>
              </div>

              <label className="settings-field">
                <span className="settings-label">After a pass</span>
                <select
                  className="settings-input"
                  value={form.rotPostPass ?? 'stop'}
                  onChange={(e) => update('rotPostPass', e.target.value)}
                  aria-label="What the rotator does after a pass"
                >
                  {ROT_POST_PASS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  Stop is the default and moves nothing: the antenna stays pointed where the bird
                  set. Park and Ready drive the rotator on their own at LOS, so set those
                  positions above first.
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">Tolerance (° az / el)</span>
                <div className="settings-inline-pair">
                  <input
                    className="settings-input"
                    type="number"
                    min="0"
                    inputMode="decimal"
                    value={form.rotTolAzDeg ?? 2}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotTolAzDeg', n)
                    }}
                    aria-label="Azimuth tolerance (degrees)"
                  />
                  <input
                    className="settings-input"
                    type="number"
                    min="0"
                    inputMode="decimal"
                    value={form.rotTolElDeg ?? 2}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotTolElDeg', n)
                    }}
                    aria-label="Elevation tolerance (degrees)"
                  />
                </div>
                <span className="settings-hint">
                  A new target closer than this is not commanded. Without a deadband the rotator
                  hunts and the relays chatter for the whole pass. 2° is about a G-5500&apos;s own
                  resolution.
                </span>
              </div>

              <div className="settings-field">
                <span className="settings-label">Calibration trim (° az / el)</span>
                <div className="settings-inline-pair">
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="decimal"
                    value={form.rotCalAzDeg ?? 0}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotCalAzDeg', n)
                    }}
                    aria-label="Azimuth calibration trim (degrees)"
                  />
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="decimal"
                    value={form.rotCalElDeg ?? 0}
                    onChange={(e) => {
                      const n = Number(e.target.value)
                      if (!Number.isNaN(n)) updateNum('rotCalElDeg', n)
                    }}
                    aria-label="Elevation calibration trim (degrees)"
                  />
                </div>
                <span className="settings-hint">
                  Added to every command. Use it when the controller reads one heading and the
                  boom points at another.
                </span>
              </div>

              <label className="settings-field">
                <span className="settings-label">Allow flip</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!!form.rotAllowFlip}
                    onChange={(e) => updateBool('rotAllowFlip', e.target.checked)}
                    aria-label="Allow the rotator to flip past 90 degrees elevation"
                  />
                  <span className="settings-hint">
                    Takes a high pass by turning azimuth 180° and running elevation past 90°,
                    instead of swinging the mast around at the top of the pass. Off by default:{' '}
                    <strong>many rotators cannot mechanically go past 90° elevation</strong>.
                    Check your controller before turning this on.
                  </span>
                </span>
              </label>
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Audio ---- */}
          {tab === 'radio' && (
          <>
          {(form.radios?.length ?? 1) > 1 && (
            <div className="radio-config-banner">
              🎚 Audio devices below are for{' '}
              <strong>
                {form.radios?.find((r) => r.id === editingRadioId)?.name ?? 'the selected radio'}
              </strong>
              . Each radio has its OWN input/output — click “Edit” on another radio (in Radios above)
              to set its audio. The live RX audio + waterfall follow whichever radio is active.
            </div>
          )}
          <fieldset className="settings-section">
            <legend>Audio</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Input Device (RX)</span>
                <div className="settings-input-row">
                  <select
                    className="settings-input"
                    value={form.audioIn}
                    onChange={(e) => update('audioIn', e.target.value)}
                  >
                    <option value="">System default</option>
                    {audioInOptions.map((d) => (
                      <option key={d} value={d}>
                        {audioLabel(d, 'input')}
                      </option>
                    ))}
                  </select>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={refreshAudio}
                    disabled={audioLoading}
                    title="Re-scan audio devices"
                  >
                    {audioLoading ? '…' : 'Refresh'}
                  </button>
                </div>
                <span className="settings-hint">Sound card carrying receive audio.</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Output Device (TX)</span>
                <select
                  className="settings-input"
                  value={form.audioOut}
                  onChange={(e) => update('audioOut', e.target.value)}
                >
                  <option value="">System default</option>
                  {audioOutOptions.map((d) => (
                    <option key={d} value={d}>
                      {audioLabel(d, 'output')}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">Sound card feeding the rig (transmit).</span>
              </label>

              <div className="settings-field settings-audio-scope">
                <span className="settings-label">Live input spectrum</span>
                <MiniSpectrum
                  height={84}
                  idleHint="Flat — no audio on the selected input. Check the device above (radio on? right codec?)."
                />
                <span className="settings-hint">
                  What the selected input hears, live — band noise should show as a moving
                  floor. Confirms the RIGHT device before you leave Settings.
                </span>
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  Tx Power <span className="settings-value">{Math.round(form.txLevel * 100)}%</span>
                </span>
                <input
                  className="settings-slider"
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  value={String(form.txLevel)}
                  onChange={(e) => updateNum('txLevel', Number(e.target.value))}
                  onPointerUp={(e) => applyTxLevelLive(Number((e.target as HTMLInputElement).value))}
                  onKeyUp={(e) => applyTxLevelLive(Number((e.target as HTMLInputElement).value))}
                  aria-label="Transmit drive level"
                />
                <span className="settings-hint">
                  The audio <strong>drive</strong> into the rig — the SAME control as the cockpit{' '}
                  <strong>Pwr</strong> slider (they always match now). Trim down until your rig&apos;s
                  ALC is just zero. This is <em>not</em> the rig&apos;s RF watts — set those on the radio.
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">
                  RX Level{' '}
                  {/* Live 100 ms poll (lock-free backend) — setting a gain against a needle
                      that answered 0.5–0.8 s late made level-setting guesswork. */}
                  <span className="settings-value"><LiveRxLevelDb /></span>
                </span>
                <LiveLevelMeter label="RX audio level" variant="full" />
                <span className="settings-hint">
                  A dB scale like WSJT-X — aim for around 30 dB. Anything from ~15–60 dB
                  decodes fine; red means too hot (back off RX Gain or the rig's audio).
                </span>
                {radio?.audioError && (
                  <span className="cat-result fail" role="alert">✗ {radio.audioError}</span>
                )}
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  RX Gain <span className="settings-value">×{(form.rxGain ?? 1).toFixed(1)}</span>
                </span>
                <input
                  className="settings-slider"
                  type="range"
                  min="1"
                  max="8"
                  step="0.1"
                  value={String(form.rxGain ?? 1)}
                  onChange={(e) => updateNum('rxGain', Number(e.target.value))}
                  onPointerUp={(e) => applyRxGainLive(Number((e.target as HTMLInputElement).value))}
                  onKeyUp={(e) => applyRxGainLive(Number((e.target as HTMLInputElement).value))}
                  aria-label="RX capture gain"
                />
                <span className="settings-hint">
                  Boost a quiet interface until RX Level reads around 30 dB — the meter responds
                  as you release the slider. Leave at ×1.0 unless the meter reads low (under ~15 dB)
                  — FT8 decodes on a small signal, so you rarely need much.
                </span>
              </label>
            </div>
          </fieldset>

          <fieldset className="settings-section">
            <legend>Headphone monitor</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Enable monitor</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!!form.monitorEnabled}
                    onChange={(e) => updateBool('monitorEnabled', e.target.checked)}
                    aria-label="Enable headphone monitor"
                  />
                  <span className="settings-hint">
                    Plays the exact audio the decoder hears — for level / RFI diagnosis and
                    listening to the band. Off by default; UNVERIFIED on-air until the attended
                    session. Guards against the rig's TX device by name (System default is
                    resolved to its real device first) — if your devices go by multiple
                    names, pick your headphones explicitly rather than System default.
                  </span>
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">Monitor Output Device</span>
                <select
                  className="settings-input"
                  value={form.monitorDevice ?? ''}
                  onChange={(e) => update('monitorDevice', e.target.value)}
                  disabled={!form.monitorEnabled}
                >
                  <option value="">System default</option>
                  {monitorOutOptions.map((d) => (
                    <option key={d} value={d}>
                      {audioLabel(d, 'output')}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  Your headphones or speakers — must NOT be the rig's TX output device.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  Monitor Level{' '}
                  <span className="settings-value">{Math.round((form.monitorLevel ?? 0.5) * 100)}%</span>
                </span>
                <input
                  className="settings-slider"
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  value={String(form.monitorLevel ?? 0.5)}
                  onChange={(e) => updateNum('monitorLevel', Number(e.target.value))}
                  disabled={!form.monitorEnabled}
                  aria-label="Headphone monitor level"
                />
                <span className="settings-hint">Headphone listening volume (does not affect TX).</span>
              </label>
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Digital (FT8/FT4) — was "Operating"; ~90% FT8 sequencing/decoder knobs ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>Digital (FT8/FT4)</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Transmit &amp; Sequencing</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Transmit period — Tx 1st (even)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.txEven}
                      className={`toggle${form.txEven ? ' on' : ''}`}
                      onClick={() => updateBool('txEven', !form.txEven)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    On = transmit in the even/1st T/R slots; off = odd/2nd. The two stations in a QSO
                    must pick <strong>opposite</strong> periods. Also on the top bar (Tx 1st / Tx 2nd).
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">Tx Watchdog (min)</span>
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="numeric"
                    min="0"
                    value={String(form.txWatchdogMin)}
                    placeholder="6"
                    onChange={(e) => update('txWatchdogMin', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">Auto-halt TX after this many minutes (0 = off).</span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Disable TX after sending 73</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.disableTxAfter73 !== false}
                      className={`toggle${form.disableTxAfter73 !== false ? ' on' : ''}`}
                      onClick={() => updateBool('disableTxAfter73', form.disableTxAfter73 === false)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    After your final 73 goes out, Enable TX drops — working the next station is a
                    deliberate arm (WSJT-X default). A CQ run is unaffected: it returns to CQ.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Double-click arms TX</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.doubleClickSetsTx !== false}
                      className={`toggle${form.doubleClickSetsTx !== false ? ' on' : ''}`}
                      onClick={() => updateBool('doubleClickSetsTx', form.doubleClickSetsTx === false)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Double-clicking a station enables TX so the answer goes straight out (WSJT-X
                    "double-click on call sets Tx enable"). Off = you arm TX yourself each time.
                  </span>
                </div>

                <div className="settings-field">
                  <label>
                    <span className="settings-label">Tune timeout (s)</span>
                    <input
                      className="settings-input"
                      type="number"
                      min={1}
                      max={120}
                      value={form.tuneTimeoutSecs || 12}
                      onChange={(e) => {
                        // '' must mean "back to the 12 s default" — the generic
                        // numeric coercion turned a cleared field into a saved 0.
                        markDirty()
                        const n = e.target.value === '' ? 12 : Math.max(1, Number(e.target.value) || 12)
                        setForm((prev) => (prev ? { ...prev, tuneTimeoutSecs: n } : prev))
                      }}
                    />
                  </label>
                  <span className="settings-hint">
                    Auto-release the tune carrier after this many seconds — never leave a key-down
                    unattended.
                  </span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Auto-CQ &amp; Caller Selection</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label>
                    <span className="settings-label">Stop CQ after N calls</span>
                    <input
                      className="settings-input"
                      type="number"
                      min={1}
                      max={99}
                      value={form.cqMaxCalls ?? ''}
                      placeholder="keep calling"
                      onChange={(e) => updateNullableNum('cqMaxCalls', e.target.value, 1)}
                    />
                  </label>
                  <span className="settings-hint">
                    Blank = WSJT-X behavior: CQ repeats until you stop it (the TX watchdog is the
                    backstop). Set a number to auto-stop an unanswered CQ run after that many calls.
                    The Tempo chat CQ run always stops (default 10 unanswered) — this number
                    overrides that budget too.
                  </span>
                </div>

                <div className="settings-field">
                  <label>
                    <span className="settings-label">Tempo chat: send cycles per message</span>
                    <input
                      className="settings-input"
                      type="number"
                      min={1}
                      max={20}
                      value={form.chatMaxCycles ?? ''}
                      placeholder="3"
                      onChange={(e) => updateNullableNum('chatMaxCycles', e.target.value, 1)}
                    />
                  </label>
                  <span className="settings-hint">
                    A chat message transmits at most this many cycles, then shows "no ack" (tap the
                    bubble to re-send). Blank = 3 (TempoDeep uses 5). Never affects FT8/FT4.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={form.chatImplicitAck ?? true}
                      onChange={(e) => updateBool('chatImplicitAck', e.target.checked)}
                    />
                    <span className="settings-label">Tempo chat: a reply counts as received</span>
                  </label>
                  <span className="settings-hint">
                    When the station you messaged sends a complete message back, stop re-sending and
                    mark yours "confirmed" (works even when the other side isn't Nexus). A real ACK
                    still upgrades it to "Delivered ✓".
                  </span>
                </div>

                <div className="settings-field">
                  <label>
                    <span className="settings-label">Auto-CQ: drop a silent caller after N overs</span>
                    <input
                      className="settings-input"
                      type="number"
                      min={0}
                      max={99}
                      value={form.cqStallOvers ?? ''}
                      placeholder="3"
                      onChange={(e) => updateNullableNum('cqStallOvers', e.target.value, 0)}
                    />
                  </label>
                  <span className="settings-hint">
                    During an Auto-CQ run, if a station answers then goes silent, abandon it and
                    return to calling CQ after this many unanswered overs. Blank = 3; 0 = never
                    abandon (wait for you, like stock WSJT-X).
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">Best caller (auto-CQ pick)</span>
                  <div className="settings-input-row">
                    <select
                      className="settings-input"
                      value={form.bestCaller || 'first'}
                      onChange={(e) => update('bestCaller', e.target.value)}
                    >
                      <option value="first">First to answer (default)</option>
                      <option value="strongest">Strongest signal</option>
                      <option value="farthest">Farthest away</option>
                      <option value="cq_first">Prefer CQ callers</option>
                    </select>
                    <input
                      className="settings-input"
                      type="number"
                      inputMode="numeric"
                      value={form.bestCallerMinSnr ?? ''}
                      placeholder="min SNR dB (optional)"
                      onChange={(e) => updateNullableNum('bestCallerMinSnr', e.target.value, -30)}
                      aria-label="Minimum SNR (dB) to consider when picking the best caller"
                    />
                  </div>
                  <span className="settings-hint">
                    When several stations answer your CQ, which to work first.
                  </span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Logging Behavior</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Auto-log QSOs</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.autoLog}
                      className={`toggle${form.autoLog ? ' on' : ''}`}
                      onClick={() => updateBool('autoLog', !form.autoLog)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">Automatically log completed contacts to the ADIF logbook.</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Prompt before logging</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.promptToLog}
                      className={`toggle${form.promptToLog ? ' on' : ''}`}
                      onClick={() => updateBool('promptToLog', !form.promptToLog)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Show a confirm-and-edit popup when a QSO completes instead of logging silently
                    (WSJT-X “Prompt me to log QSO”). No effect unless Auto-log is on.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Roger with RRR (not RR73)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.preferRrr}
                      className={`toggle${form.preferRrr ? ' on' : ''}`}
                      onClick={() => updateBool('preferRrr', !form.preferRrr)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Acknowledge the final report with a bare RRR (partner still owes a 73) instead of
                    the combined RR73. Off = RR73 (modern FT8 practice).
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Clear DX call after logging</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.clearDxAfterLog}
                      className={`toggle${form.clearDxAfterLog ? ' on' : ''}`}
                      onClick={() => updateBool('clearDxAfterLog', !form.clearDxAfterLog)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Wipe the DX Call / DX Grid fields once a contact is logged (WSJT-X option,
                    off by default).
                  </span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Decoder</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <span className="settings-label">Decode depth</span>
                  <div className="theme-switcher" role="group" aria-label="Decode depth">
                    {([1, 2, 3] as const).map((d) => (
                      <button
                        key={d}
                        type="button"
                        className={`theme-chip${(form.decodeDepth ?? 3) === d ? ' active' : ''}`}
                        aria-pressed={(form.decodeDepth ?? 3) === d}
                        onClick={() => {
                          markDirty()
                          setForm((prev) => (prev ? { ...prev, decodeDepth: d } : prev))
                        }}
                      >
                        {d === 1 ? 'Fast' : d === 2 ? 'Normal' : 'Deep'}
                      </button>
                    ))}
                  </div>
                  <span className="settings-hint">
                    Deep finds the most signals (WSJT-X default); Fast saves CPU on old hardware.
                    All Decoder settings drive the native decoder — on a WSJT-X UDP source
                    (companion mode) decodes arrive already made and none of them apply.
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">Decoder passband (Hz)</span>
                  <div className="settings-input-row">
                    <label className="settings-inline-label">
                      <span>F low</span>
                      <input
                        id="decode-flow"
                        className="settings-input"
                        type="number"
                        inputMode="numeric"
                        min={200}
                        max={2900}
                        step={1}
                        value={form.decodeFLowHz ?? 200}
                        aria-label="Decoder F low (Hz)"
                        onChange={(e) => {
                          if (e.target.value === '') return // mid-edit clear: keep the prior value
                          markDirty()
                          const raw = Number(e.target.value)
                          const clamped = Math.max(200, Math.min(2900, Math.round(raw)))
                          setForm((prev) =>
                            prev
                              ? { ...prev, decodeFLowHz: clamped }
                              : prev,
                          )
                        }}
                        onBlur={() => {
                          setForm((prev) => {
                            if (!prev) return prev
                            const lo = prev.decodeFLowHz ?? 200
                            const hi = prev.decodeFHighHz ?? 2900
                            if (lo >= hi) return { ...prev, decodeFLowHz: Math.min(lo, hi - 1) }
                            return prev
                          })
                        }}
                      />
                    </label>
                    <label className="settings-inline-label">
                      <span>F high</span>
                      <input
                        id="decode-fhigh"
                        className="settings-input"
                        type="number"
                        inputMode="numeric"
                        min={200}
                        max={4000}
                        step={1}
                        value={form.decodeFHighHz ?? 2900}
                        aria-label="Decoder F high (Hz)"
                        onChange={(e) => {
                          if (e.target.value === '') return // mid-edit clear: keep the prior value
                          markDirty()
                          const raw = Number(e.target.value)
                          const clamped = Math.max(200, Math.min(4000, Math.round(raw)))
                          setForm((prev) =>
                            prev
                              ? { ...prev, decodeFHighHz: clamped }
                              : prev,
                          )
                        }}
                        onBlur={() => {
                          setForm((prev) => {
                            if (!prev) return prev
                            const lo = prev.decodeFLowHz ?? 200
                            const hi = prev.decodeFHighHz ?? 2900
                            if (hi <= lo) return { ...prev, decodeFHighHz: Math.max(hi, lo + 1) }
                            return prev
                          })
                        }}
                      />
                    </label>
                  </div>
                  <span className="settings-hint">
                    The decoder&apos;s search range. Default 200–2900 Hz. Raise F high toward 4000 Hz
                    to decode stations calling above ~2.9 kHz (common on crowded FT8 bands); lower the
                    range to focus on a narrow filter or dodge strong close-in QRM.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">A-priori (AP) decoding — FT8</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.apDecode !== false}
                      className={`toggle${form.apDecode !== false ? ' on' : ''}`}
                      onClick={() => updateBool('apDecode', form.apDecode === false)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Retry marginal signals against hypotheses built from your call, the DX call and
                    the QSO state (WSJT-X &quot;Enable AP&quot;, on by default) — including the
                    cross-cycle replay of last cycle&apos;s QSOs. FT8 only: FT4&apos;s AP is part of
                    its Normal/Deep depth and has no separate switch.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">AP: CQ hypothesis only</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.apCqOnly}
                      className={`toggle${form.apCqOnly ? ' on' : ''}`}
                      onClick={() => updateBool('apCqOnly', !form.apCqOnly)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Limit AP to the &quot;CQ&quot; guess — no MyCall/DxCall hypotheses (FT8 and
                    FT4). WSJT-X switches to this by itself after 5 minutes without transmitting,
                    as a guard against stale-context false decodes; here it is your explicit
                    choice. Off = full AP, the stock behavior.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Single decode</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.singleDecode}
                      className={`toggle${form.singleDecode ? ' on' : ''}`}
                      onClick={() => updateBool('singleDecode', !form.singleDecode)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Decode only within ±25 Hz of your green RX marker (the same one-station window
                    WSJT-X uses for a double-click re-decode) instead of the whole passband —
                    isolates one weak station and saves CPU. FT8 and FT4 only: 50 Hz is narrower
                    than a single JT65, Q65 or MSK144 signal, so those modes keep the full
                    passband. Applies while the RX marker sits inside the passband above; off =
                    full passband, the stock behavior.
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">DXpedition mode</span>
                  <div className="theme-switcher" role="group" aria-label="DXpedition mode">
                    {([
                      { value: 'none' as const, label: 'Off' },
                      { value: 'hound' as const, label: 'Hound' },
                    ]).map((op) => (
                      <button
                        key={op.value}
                        type="button"
                        className={`theme-chip${(form.specialOp ?? 'none') === op.value ? ' active' : ''}`}
                        aria-pressed={(form.specialOp ?? 'none') === op.value}
                        onClick={() => {
                          markDirty()
                          setForm((prev) => prev ? { ...prev, specialOp: op.value } : prev)
                        }}
                      >
                        {op.label}
                      </button>
                    ))}
                  </div>
                  <span className="settings-hint">
                    Off = normal FT8/FT4 operation. Hound = DXpedition pile-up discipline (calls
                    above 1000 Hz; your report auto-moves to the Fox&apos;s frequency).
                  </span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Station Housekeeping</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Journey — track a weekly streak</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.journeyStreakEnabled}
                      className={`toggle${form.journeyStreakEnabled ? ' on' : ''}`}
                      onClick={() => updateBool('journeyStreakEnabled', !form.journeyStreakEnabled)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Off by default. A gentle &ldquo;weeks on the air&rdquo; counter on the Journey
                    board — never a daily streak, never a penalty for a break.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Beacon — announce presence (CQ)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.beacon}
                      className={`toggle${form.beacon ? ' on' : ''}`}
                      onClick={() => updateBool('beacon', !form.beacon)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Off = passive (hunt &amp; pounce): Nexus listens and only transmits when you act.
                    On = periodically calls CQ to announce you&apos;re on frequency.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">IR-HARQ — combine retransmissions</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.harqEnabled}
                      className={`toggle${form.harqEnabled ? ' on' : ''}`}
                      onClick={() => updateBool('harqEnabled', !form.harqEnabled)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    On (default) = a weak frame that fails is recovered by joint-combining its
                    retransmissions (RV0+RV1+RV2), and unacknowledged QSO overs escalate redundancy.
                    Off = RV0-only (each frame decoded on its own).
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Clock check (NTP)</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.clockCheck}
                      className={`toggle${form.clockCheck ? ' on' : ''}`}
                      onClick={() => updateBool('clockCheck', !form.clockCheck)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Periodically check your PC clock against an NTP server and show the offset in the
                    top bar. TempoFast/TempoDeep are slot-timed to UTC — keep it within ~0.5 s (NTP / time.is;
                    off-grid: GPS). Turn off for fully-offline operation (no network calls).
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-label" htmlFor="station-power">
                    Station power (W)
                  </label>
                  <input
                    id="station-power"
                    className="settings-input"
                    type="number"
                    min="0"
                    step="1"
                    inputMode="decimal"
                    value={form.stationPowerW ?? ''}
                    placeholder="e.g. 100"
                    onChange={(e) => {
                      markDirty()
                      const raw = e.target.value.trim()
                      const num = raw === '' ? null : Number(raw)
                      setForm((prev) =>
                        prev
                          ? {
                              ...prev,
                              stationPowerW:
                                num !== null && Number.isNaN(num) ? prev.stationPowerW : num,
                            }
                          : prev,
                      )
                    }}
                  />
                  <span className="settings-hint">
                    Your transmit power in watts — unlocks the Journey miles-per-watt &amp; QRP feats.
                    Leave blank if unknown.
                  </span>
                </div>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- JT65: submode only. One fixed 60 s period, unlike the others. ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>JT65 &mdash; classic EME</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Submode (tone spacing)</span>
                <select
                  className="settings-input"
                  value={String(form.jt65Submode ?? 0)}
                  onChange={(e) => updateNum('jt65Submode', Number(e.target.value))}
                >
                  <option value="0">A &mdash; HF standard, narrowest</option>
                  <option value="1">B &mdash; 2x spacing</option>
                  <option value="2">C &mdash; 4x spacing, most Doppler-tolerant</option>
                </select>
                <span className="settings-hint">
                  JT65 always uses a 60&nbsp;s T/R period, so spacing is the only choice. A is
                  what you want on HF; EME operators move up to B or C as Doppler spread on the
                  higher bands smears the tones. Both stations must use the same submode.
                </span>
              </label>
            </div>
            <span className="settings-hint">
              The classic weak-signal and moonbounce mode, decoded and transmitted. Messages are
              the older 22-character format, not the 37-character one FT8 and friends use.
            </span>
          </fieldset>
          )}

          {/* ---- MSK144: meteor scatter. Period is the whole setting. ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>MSK144 &mdash; meteor scatter</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">T/R period</span>
                <select
                  className="settings-input"
                  value={String(form.msk144PeriodS ?? 15)}
                  onChange={(e) => updateNum('msk144PeriodS', Number(e.target.value))}
                >
                  <option value="5">5 s &mdash; fast turnaround, big showers</option>
                  <option value="10">10 s</option>
                  <option value="15">15 s &mdash; the 6 m standard</option>
                  <option value="30">30 s &mdash; sparse pings, more to stack</option>
                </select>
                <span className="settings-hint">
                  MSK144 sends a 72&nbsp;ms message over and over, so a single meteor trail lasting
                  a tenth of a second can carry the whole thing. Shorter periods turn the exchange
                  around faster during a shower; longer ones give the decoder more frames to stack
                  when pings are sparse. Both stations must use the same period.
                </span>
              </label>
            </div>
            <span className="settings-hint">
              MSK144 transmits for nearly the whole period, sending the same 72 ms frame
              hundreds of times &mdash; that is how meteor scatter works, and a contact can
              take many minutes of apparent silence. The audio frequency is fixed at a
              1500 Hz centre; the signal is 1 kHz wide, so there is nowhere to tune it.
              Shorthand (MSK40) messages are off, matching WSJT-X&rsquo;s default.
            </span>
          </fieldset>
          )}

          {/* ---- Beacons (WSPR / FST4W). A SEPARATE surface from the QSO modes:
               there is no exchange, only a schedule. Off by default — beaconing
               keys the radio unattended, so it is always an explicit choice. ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>Beacons — WSPR &amp; FST4W</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Transmit %</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={100}
                  value={String(form.beaconTxPercent ?? 0)}
                  onChange={(e) => updateNum('beaconTxPercent', Number(e.target.value))}
                />
                <span className="settings-hint">
                  Fraction of intervals to transmit on. 0 = listen only. A beacon that
                  transmits every interval hears nothing, so a minority is the convention
                  &mdash; 20&ndash;30% is typical. Below 40% Nexus also avoids
                  back-to-back transmissions while still hitting the rate you asked for.
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Transmit power (dBm)</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={60}
                  value={String(form.beaconPowerDbm ?? 0)}
                  onChange={(e) => updateNum('beaconPowerDbm', Number(e.target.value))}
                />
                <span className="settings-hint">
                  <strong>Required, and it has to be real.</strong> WSPR reports are
                  published to a public propagation database that other operators draw
                  conclusions from, so a wrong figure corrupts their data as well as
                  yours. The beacon stays silent until this is set. 23 = 200 mW,
                  30 = 1 W, 37 = 5 W, 43 = 20 W.
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">FST4W Round Robin slot</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={10}
                  value={String(form.beaconRrSlot ?? 0)}
                  onChange={(e) => updateNum('beaconRrSlot', Number(e.target.value))}
                />
                <span className="settings-hint">
                  0 = use the transmit-% schedule. Otherwise your slot in a coordinated
                  rotation: stations agreeing on the same slot count and each taking a
                  different slot never transmit at the same time, because the assignment
                  is fixed by UTC.
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Round Robin slots</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={10}
                  value={String(form.beaconRrSlots ?? 0)}
                  onChange={(e) => updateNum('beaconRrSlots', Number(e.target.value))}
                />
                <span className="settings-hint">
                  How many stations are in the rotation. Ignored when the slot is 0.
                </span>
              </label>
            </div>
            <span className="settings-hint">
              Beacons transmit your callsign, grid and power &mdash; there is no QSO
              sequence, so Call CQ and S&amp;P are inactive on these tiers. Transmit
              still has to be armed as usual; the schedule never keys a radio whose
              transmit you have not enabled.
            </span>
          </fieldset>
          )}

          {/* ---- FST4 / FST4W: one period setting, shared. Same decoder, same
               slot clock; the tier picks QSO vs beacon. ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>FST4 (QSO) / FST4W (beacon)</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">T/R period</span>
                <select
                  className="settings-input"
                  value={String(form.fst4PeriodS ?? 120)}
                  onChange={(e) => updateNum('fst4PeriodS', Number(e.target.value))}
                >
                  <option value="15">15 s</option>
                  <option value="30">30 s</option>
                  <option value="60">60 s</option>
                  <option value="120">120 s — shortest FST4W beacon interval</option>
                  <option value="300">300 s</option>
                  <option value="900">900 s</option>
                  <option value="1800">1800 s — deepest</option>
                </select>
                <span className="settings-hint">
                  Shared by both tiers. Longer periods hear weaker signals at fewer exchanges per
                  hour. FST4W beacons run at 120/300/900/1800 s; FST4 QSO work is usually
                  15&ndash;60 s. Both stations (or the beacon you are listening for) must be on the
                  same period.
                </span>
              </label>
            </div>
            <span className="settings-hint">
              <strong>FST4</strong> is the QSO mode; <strong>FST4W</strong> is the WSPR-like beacon
              mode &mdash; pick which one on the tier selector. Nexus decodes both and transmits
              neither. Note that FST4W hashed callsigns show as <code>&lt;...&gt;</code>: the
              lookup table upstream fills from a file this build does not carry.
            </span>
          </fieldset>
          )}

          {/* ---- Q65: period + submode. Both change the on-air signal AND the
               decode frame length, so they belong with the mode, not the radio. ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>Q65 &mdash; EME / VHF+ scatter</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">T/R period</span>
                <select
                  className="settings-input"
                  value={String(form.q65PeriodS ?? 60)}
                  onChange={(e) => updateNum('q65PeriodS', Number(e.target.value))}
                >
                  <option value="15">15 s — troposcatter</option>
                  <option value="30">30 s — 6 m meteor / ionoscatter</option>
                  <option value="60">60 s — EME (most common)</option>
                  <option value="120">120 s — deep EME</option>
                  <option value="300">300 s — deepest, microwave EME</option>
                </select>
                <span className="settings-hint">
                  Longer periods integrate longer and hear weaker signals, at one exchange per
                  period. Both stations must use the <strong>same</strong> period. Changing this
                  changes the decode frame length, so it takes effect on the next slot.
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Submode (tone spacing)</span>
                <select
                  className="settings-input"
                  value={String(form.q65Submode ?? 0)}
                  onChange={(e) => updateNum('q65Submode', Number(e.target.value))}
                >
                  <option value="0">A — narrowest, most sensitive</option>
                  <option value="1">B — 2x spacing</option>
                  <option value="2">C — 4x spacing</option>
                  <option value="3">D — 8x spacing</option>
                  <option value="4">E — 16x spacing, most Doppler-tolerant</option>
                </select>
                <span className="settings-hint">
                  Wider spacing survives more Doppler spread and frequency drift but costs
                  sensitivity. Move up the letters as the path degrades — EME on the higher bands
                  usually needs B or C.
                </span>
              </label>
            </div>
            <span className="settings-hint">
              Q65 transmits and receives. The period and submode set both what you hear and
              what you send, and BOTH STATIONS MUST MATCH &mdash; a correspondent on a
              different period or submode will not decode you.
            </span>
          </fieldset>
          )}

          {/* ---- Digital quick-reply macros (moved out of the old Alerts/Macros orphan) ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>Quick-reply macros</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">Chat</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.macros.chat.join(', ')}
                  onChange={(e) => updateMacros('chat', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">Comma-separated chips for Chat.</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">QSO</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.macros.qso.join(', ')}
                  onChange={(e) => updateMacros('qso', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">Chips for sequenced QSOs.</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Band / CQ</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.macros.band.join(', ')}
                  onChange={(e) => updateMacros('band', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">Open broadcasts — the Call CQ launchpad + band feed.</span>
              </label>
            </div>
          </fieldset>
          )}

          {/* ---- Phone (SSB / FM) ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>Phone (SSB / FM)</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Mode</span>
              <label className="settings-field">
                <span className="settings-label">Phone mode</span>
                <select
                  className="settings-input"
                  value={form.phoneMode}
                  onChange={(e) => update('phoneMode', e.target.value)}
                >
                  <option value="ssb">SSB (USB/LSB by band)</option>
                  <option value="fm">FM (VHF/UHF + repeaters)</option>
                </select>
                <span className="settings-hint">FM drives the rig to FM + the shift/tone below.</span>
              </label>

              {form.phoneMode === 'fm' && (
                <>
                  <label className="settings-field">
                    <span className="settings-label">Repeater shift</span>
                    <select
                      className="settings-input"
                      value={form.rptrShift}
                      onChange={(e) => update('rptrShift', e.target.value)}
                    >
                      <option value="simplex">Simplex (no shift)</option>
                      <option value="plus">Plus (+)</option>
                      <option value="minus">Minus (−)</option>
                    </select>
                    <span className="settings-hint">Offset is the band standard (2 m 600 k, 70 cm 5 M…).</span>
                  </label>

                  <label className="settings-field">
                    <span className="settings-label">CTCSS (PL) tone</span>
                    <select
                      className="settings-input"
                      value={String(form.ctcssToneHz)}
                      onChange={(e) =>
                        setForm((p) => (p ? { ...p, ctcssToneHz: Number(e.target.value) } : p))
                      }
                    >
                      <option value="0">Off</option>
                      {CTCSS_TONES.map((t) => (
                        <option key={t} value={String(t)}>
                          {t.toFixed(1)} Hz
                        </option>
                      ))}
                    </select>
                    <span className="settings-hint">Repeater access tone (PL).</span>
                  </label>
                </>
              )}
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Microphone</span>
              <label className="settings-field">
                <span className="settings-label">Voice mic (recording)</span>
                <select
                  className="settings-input"
                  value={form.voiceMicDevice ?? ''}
                  onChange={(e) => update('voiceMicDevice', e.target.value)}
                >
                  <option value="">Same as audio input (default)</option>
                  {voiceMicOptions.map((d) => (
                    <option key={d} value={d}>
                      {audioLabel(d, 'input')}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  Mic used when RECORDING a voice-keyer message. Default records from the audio
                  input device — but on a digital setup that's the rig's RX audio, so you'd record
                  the band, not your voice. Pick your actual mic here. If it can't open, recording
                  falls back to the input device (never silent).
                </span>
              </label>
              <span className="settings-hint">
                Mic gain and voice-keyer message recording are in the Phone cockpit (live CAT +
                one-touch record).
              </span>
            </div>
          </fieldset>
          )}

          {/* ---- CW — the standalone CW home (keyer + F-key macros) ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>CW</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Keyer</span>
              <label className="settings-field">
                <span className="settings-label">Keyer backend</span>
                <select
                  className="settings-input"
                  value={form.cwKeyer ?? 'cat'}
                  onChange={(e) => update('cwKeyer', e.target.value)}
                >
                  <option value="cat">CAT — the rig keys CW (Hamlib send_morse; newer rigs only)</option>
                  <option value="serial">Serial keyline (DTR/RTS) — key the rig&apos;s KEY jack</option>
                  <option value="winkeyer">WinKeyer — K1EL hardware keyer</option>
                  <option value="soundcard">Soundcard — audio tone through SSB (workaround)</option>
                </select>
                <span className="settings-hint">
                  How Nexus sends CW. <strong>CAT</strong> uses the rig&apos;s internal keyer, but older
                  rigs (e.g. IC-756PRO III) don&apos;t support it. <strong>Serial keyline</strong> toggles
                  DTR/RTS into the rig&apos;s KEY jack (rig in CW, rig shapes the signal — the clean
                  N1MM/fldigi method, needs only a keying cable). <strong>WinKeyer</strong> drives a K1EL.
                  <strong>Soundcard</strong> keys an audio tone through SSB — a workaround; set drive so
                  ALC reads zero. Also switchable live from the CW cockpit.
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Sidetone pitch (Hz)</span>
                <input
                  className="settings-input"
                  type="number"
                  min="300"
                  max="1200"
                  step="10"
                  inputMode="numeric"
                  value={form.cwPitchHz}
                  onChange={(e) => updateNum('cwPitchHz', Number(e.target.value))}
                />
                <span className="settings-hint">
                  CW tone pitch (300–1200 Hz) — the soundcard keyer tone and the CW scope
                  zero-beat marker.
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">WinKeyer port</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.winkeyerPort}
                  placeholder="COM6 — K1EL WinKeyer serial port"
                  onChange={(e) => update('winkeyerPort', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">
                  For the WinKeyer CW keyer (select it above). 1200 baud.
                </span>
              </label>
              {form.cwKeyer === 'serial' && (
                <>
                  <label className="settings-field">
                    <span className="settings-label">Keyline serial port</span>
                    <input
                      className="settings-input"
                      type="text"
                      value={form.cwKeyPort ?? ''}
                      placeholder="COM7 — the keying interface (separate from CAT)"
                      onChange={(e) => update('cwKeyPort', e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <span className="settings-hint">
                      The USB-to-serial into your keying interface (Buxcomm, US Navigator, a homebrew
                      DTR cable, …) that plugs into the rig&apos;s KEY jack. Must be a SEPARATE port
                      from CAT. Set the rig to CW and its key-jack to straight-key / bug.
                    </span>
                  </label>
                  <label className="settings-field">
                    <span className="settings-label">Keying line</span>
                    <select
                      className="settings-input"
                      value={form.cwKeyLine ?? 'dtr'}
                      onChange={(e) => update('cwKeyLine', e.target.value)}
                    >
                      <option value="dtr">DTR (the CW convention)</option>
                      <option value="rts">RTS</option>
                    </select>
                    <span className="settings-hint">
                      Which control line keys the rig. DTR is standard (RTS = PTT); flip to RTS if your
                      interface is wired the other way.
                    </span>
                  </label>
                </>
              )}
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">CW ID after 73</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.cwIdAfter73 === true}
                    className={`toggle${form.cwIdAfter73 === true ? ' on' : ''}`}
                    onClick={() => updateBool('cwIdAfter73', form.cwIdAfter73 !== true)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Keys your callsign in CW once the final 73 has fully left the air (stock WSJT-X
                  option, default off). Uses the normal CW keying path — PTT + tone — after the FT8
                  over, never on top of it.
                </span>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Macros (F-key profiles)</span>
              <div className="settings-field cw-macro-editor">
                <span className="settings-label">CW cockpit F-keys</span>
                {/* Named macro profiles — a rotating operator switches sets here (or in one
                    click from the CW cockpit bar). The grid below edits the ACTIVE profile. */}
                <div className="cw-macro-row">
                  <select
                    className="settings-input"
                    value={activeCwIdx}
                    onChange={(e) => selectCwProfile(Number(e.target.value))}
                    aria-label="Active CW macro profile"
                  >
                    {cwProfiles.map((p, i) => (
                      <option key={i} value={i}>
                        {p.name || `Profile ${i + 1}`}
                      </option>
                    ))}
                  </select>
                  <button type="button" className="settings-refresh" onClick={addCwProfile}>
                    New
                  </button>
                  <button type="button" className="settings-refresh" onClick={renameCwProfile}>
                    Rename
                  </button>
                  <button
                    type="button"
                    className="settings-refresh danger"
                    onClick={deleteCwProfile}
                    disabled={cwProfiles.length <= 1}
                    title={cwProfiles.length <= 1 ? 'Keep at least one profile' : 'Delete this profile'}
                  >
                    Delete
                  </button>
                </div>
                {!activeCwMacros.length ? (
                  <div className="cw-macro-row">
                    <span className="settings-hint">
                      Using the built-in F1–F8 set. Customize to make them your own (labels +
                      templates; tokens: {'{MYCALL} {RST} {NAME}'} and ! = the worked call).
                    </span>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={() => setCwMacros(CW_MACRO_DEFAULTS.map((m) => ({ ...m })))}
                    >
                      Customize
                    </button>
                  </div>
                ) : (
                  <>
                    {activeCwMacros.map((m, i) => (
                      <div key={m.key} className="cw-macro-row">
                        <span className="cw-macro-key" title={CW_MACRO_ROLES[m.key] ?? ''}>
                          {m.key}
                        </span>
                        <span className="cw-macro-role">{CW_MACRO_ROLES[m.key] ?? ''}</span>
                        <input
                          className="settings-input cw-macro-label"
                          type="text"
                          value={m.label}
                          onChange={(e) => updateCwMacro(i, 'label', e.target.value)}
                          aria-label={`${m.key} label`}
                          autoComplete="off"
                          spellCheck={false}
                        />
                        <input
                          className="settings-input cw-macro-text"
                          type="text"
                          value={m.text}
                          onChange={(e) => updateCwMacro(i, 'text', e.target.value)}
                          aria-label={`${m.key} text`}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </div>
                    ))}
                    <div className="cw-macro-row">
                      <span className="settings-hint">
                        Tokens: {'{MYCALL} {NAME} {MYGRID} {MYSTATE} {RST}'} · ! = the worked call ·{' '}
                        {'{HISNAME} {HISSTATE}'} = the worked station's QRZ name/state (fill in
                        Settings ▸ Station for {'{MYSTATE}'}; the rest auto-fill from the copilot /
                        roster click + QRZ lookup). Each key KEEPS its role — the Guided copilot's
                        next-step highlight follows the role, so customized text still rolls through
                        F1→F2→F3→F4 exactly as before. Keep the ! token wherever you want the other
                        station's call inserted. Save to apply.
                      </span>
                      <button
                        type="button"
                        className="settings-refresh"
                        onClick={() => setCwMacros([])}
                      >
                        Reset to defaults
                      </button>
                    </div>
                  </>
                )}
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- RTTY — keying backend + signal parameters (TX + RX demod both) ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>RTTY</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Keying</span>
              <label className="settings-field">
                <span className="settings-label">Keying backend</span>
                <select
                  className="settings-input"
                  value={form.rttyBackend ?? 'afsk'}
                  onChange={(e) => update('rttyBackend', e.target.value)}
                >
                  <option value="afsk">AFSK — soundcard tones through the rig in LSB (default)</option>
                  <option value="fsk">True FSK — serial keyline (DTR/RTS), rig in RTTY mode</option>
                </select>
                <span className="settings-hint">
                  How Nexus transmits RTTY. <strong>AFSK</strong> plays the two-tone waveform through
                  the same TX audio path as FT8 (soundcard-clocked = jitter-free; set drive so ALC
                  reads just zero). <strong>True FSK</strong> bit-bangs the rig&apos;s FSK input over a
                  serial control line with the rig in RTTY mode — unlocking its narrow RTTY filters
                  (e.g. the FTDX10&apos;s) — with PTT on CAT or its own line. Software FSK timing is
                  casual/Field-Day grade; AFSK is the timing-cleanest path.
                </span>
              </label>
              {form.rttyBackend === 'fsk' && (
                <>
                  <label className="settings-field">
                    <span className="settings-label">FSK serial port</span>
                    <input
                      className="settings-input"
                      type="text"
                      value={form.rttyFskPort ?? ''}
                      placeholder="COM8 — e.g. the FTDX10's USB Enhanced COM"
                      onChange={(e) => update('rttyFskPort', e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <span className="settings-hint">
                      The port whose control line feeds the rig&apos;s FSK input. Empty = the CAT
                      serial port.
                    </span>
                  </label>
                  <label className="settings-field">
                    <span className="settings-label">FSK data line</span>
                    <select
                      className="settings-input"
                      value={form.rttyFskLine ?? 'dtr'}
                      onChange={(e) => update('rttyFskLine', e.target.value)}
                    >
                      <option value="dtr">DTR (the common wiring — RTS stays free for PTT)</option>
                      <option value="rts">RTS</option>
                    </select>
                    <span className="settings-hint">
                      Which control line carries the data bits. PTT must ride its OWN path — CAT
                      PTT or the separate PTT line, never this one; Nexus refuses a send if they
                      collide.
                    </span>
                  </label>
                </>
              )}
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Signal</span>
              <label className="settings-field">
                <span className="settings-label">Baud rate</span>
                <select
                  className="settings-input"
                  value={String(form.rttyBaud ?? 45.45)}
                  onChange={(e) => updateNum('rttyBaud', Number(e.target.value))}
                >
                  <option value="45.45">45.45 — the HF standard</option>
                  <option value="75">75 — VHF / some nets</option>
                </select>
                <span className="settings-hint">
                  Drives the TX bit clock and the RX demodulator (true 45.45, never rounded to 45).
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">Shift (Hz)</span>
                <select
                  className="settings-input"
                  value={String(form.rttyShiftHz ?? 170)}
                  onChange={(e) => updateNum('rttyShiftHz', Number(e.target.value))}
                >
                  <option value="170">170 — the HF standard</option>
                  <option value="425">425</option>
                  <option value="850">850</option>
                </select>
                <span className="settings-hint">
                  Mark/space spacing — the TX tone pair and the RX demodulator both.
                </span>
              </label>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Reverse (swap mark/space)</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.rttyReverse === true}
                    className={`toggle${form.rttyReverse === true ? ' on' : ''}`}
                    onClick={() => updateBool('rttyReverse', form.rttyReverse !== true)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  The convention is LSB with mark on the lower audio tone. Turn this on when
                  deliberately running the opposite sideband (e.g. AFSK in USB/DATA-U) so the
                  on-air sense stays correct — applies to TX and the RX decoder.
                </span>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- APRS — the internet feed (APRS-IS) + the receive-only iGate. Lives HERE, beside
               the other per-mode settings, because that is where operators look for it: the first
               person to go hunting for these went to APRS, not to Integrations & Feeds. ---- */}
          {tab === 'modes' && (
          <fieldset className="settings-section">
            <legend>APRS</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">APRS-IS (internet feed)</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">APRS-IS feed</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.aprsIsEnabled}
                      className={`toggle${form.aprsIsEnabled ? ' on' : ''}`}
                      onClick={() => updateBool('aprsIsEnabled', !form.aprsIsEnabled)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Plot stations the internet reports alongside the ones your own antenna hears —
                    each one tagged so you can always tell which is which. Runs whether or not the
                    APRS decoder is armed: it uses no radio and never transmits. If internet
                    stations appear while your receiver stays silent, the fault is in the RF chain.
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">Server</span>
                  <input
                    className="settings-input"
                    value={form.aprsIsHost ?? ''}
                    onChange={(e) => update('aprsIsHost', e.target.value)}
                    placeholder="rotate.aprs2.net"
                    spellCheck={false}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">
                    Your regional Tier 2 rotate is best — noam / soam / euro / asia / aunz
                    .aprs2.net. <code>rotate.aprs2.net</code> works anywhere.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">Port</span>
                  <input
                    className="settings-input"
                    type="number"
                    min={1}
                    max={65535}
                    value={form.aprsIsPort ?? 14580}
                    onChange={(e) => updateNum('aprsIsPort', Number(e.target.value))}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">
                    14580 is the filtered port clients and iGates should use. The full-feed ports
                    would send you the entire planet.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">Radius (km)</span>
                  <input
                    className="settings-input"
                    type="number"
                    min={0}
                    max={5000}
                    value={form.aprsIsRadiusKm ?? 150}
                    onChange={(e) => updateNum('aprsIsRadiusKm', Number(e.target.value))}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">
                    How far around your grid square to subscribe. APRS is a local mode; 150 km is a
                    generous 2 m-plus-digipeater horizon. 0 = no distance limit (busy).
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">Watched calls</span>
                  <input
                    className="settings-input"
                    value={(form.aprsIsWatchCalls ?? []).join(', ')}
                    onChange={(e) =>
                      setWatchCalls(
                        e.target.value
                          .split(',')
                          .map((c) => c.trim().toUpperCase())
                          .filter(Boolean),
                      )
                    }
                    placeholder="W9XYZ-9, KD9ABC"
                    spellCheck={false}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">
                    Comma separated. These come through from anywhere on earth, however far outside
                    your radius they are — the club tracker on a road trip, a friend chasing a summit.
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Weather stations</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.aprsIsWeather !== false}
                      className={`toggle${form.aprsIsWeather !== false ? ' on' : ''}`}
                      onClick={() => updateBool('aprsIsWeather', form.aprsIsWeather === false)}
                      disabled={!form.aprsIsEnabled}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">Include weather reports in the feed.</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Objects &amp; items</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.aprsIsObjects !== false}
                      className={`toggle${form.aprsIsObjects !== false ? ' on' : ''}`}
                      onClick={() => updateBool('aprsIsObjects', form.aprsIsObjects === false)}
                      disabled={!form.aprsIsEnabled}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Repeaters, NWS alerts and event markers other stations have placed on the map.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Messages</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.aprsIsMessages !== false}
                      className={`toggle${form.aprsIsMessages !== false ? ' on' : ''}`}
                      onClick={() => updateBool('aprsIsMessages', form.aprsIsMessages === false)}
                      disabled={!form.aprsIsEnabled}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Show APRS text messages from the feed. Display only — replying to an internet
                    message is not wired up.
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">Keep stations for (min)</span>
                  <input
                    className="settings-input"
                    type="number"
                    min={0}
                    max={1440}
                    value={form.aprsStationTtlMin ?? 60}
                    onChange={(e) => updateNum('aprsStationTtlMin', Number(e.target.value))}
                  />
                  <span className="settings-hint">
                    How long a station stays on the map after its last packet. Stations start to fade
                    at a third of this. An hour by default: fixed stations often beacon only every
                    ten to thirty minutes, and a shorter window makes the slow ones blink off between
                    their own beacons. 0 keeps every station forever (no fade, no removal — the
                    2000-station ceiling still applies).
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Receive-only iGate</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.aprsIsUplink}
                      className={`toggle${form.aprsIsUplink ? ' on' : ''}`}
                      onClick={() => updateBool('aprsIsUplink', !form.aprsIsUplink)}
                      disabled={!form.aprsIsEnabled}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Contribute packets <strong>your own antenna hears</strong> to APRS-IS, so
                    stations in your area reach the global map through you. Publishes under{' '}
                    {form.mycall ? <strong>{form.mycall.toUpperCase()}</strong> : 'your callsign'}, so
                    it is a separate choice from watching the feed, and it needs the APRS decoder
                    running to have anything to send. Nexus never sends the other way: gating the
                    internet back onto the air means transmitting unattended.
                  </span>
                </div>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- Frequencies (working-frequency table overrides) ---- */}
          {tab === 'frequencies' && (
          <fieldset className="settings-section">
            <legend>Working Frequencies</legend>
            <p className="settings-note">
              The dial frequency used when a band/mode is selected. These are{' '}
              <strong>overrides</strong> of the stock WSJT-X working-frequency table — leave the
              list empty to use stock everywhere. An override replaces the stock row for its
              band + mode (e.g. to move FT8 to an alternate sub-band).
            </p>

            <div className="settings-field">
              <span className="settings-label">Standard table (read-only)</span>
              <div className="freq-table">
                <div className="freq-row head">
                  <span className="freq-cell">Band</span>
                  <span className="freq-cell">Mode</span>
                  <span className="freq-cell">Dial (MHz)</span>
                </div>
                {STOCK_WORKING_FREQUENCIES.map((r) => {
                  const ov = overrideByKey.get(`${r.band}|${r.mode}`)
                  return (
                    <div className="freq-row" key={`${r.band}-${r.mode}`}>
                      <span className="freq-cell mono">{r.band}</span>
                      <span className="freq-cell">{r.mode}</span>
                      {ov != null ? (
                        <span
                          className="freq-cell mono freq-override"
                          title={`Your override — stock is ${r.mhz.toFixed(6)} MHz`}
                        >
                          {ov.toFixed(6)}
                          <span className="freq-override-tag">override</span>
                        </span>
                      ) : (
                        <span className="freq-cell mono">{r.mhz.toFixed(6)}</span>
                      )}
                    </div>
                  )
                })}
              </div>
              <span className="settings-hint">
                WSJT-X stock dial frequencies. A row with an active override shows your value
                (highlighted) instead of the stock one.
              </span>
            </div>

            <div className="settings-field">
              <span className="settings-label">Your overrides</span>
              {overrides.length === 0 && (
                <span className="settings-hint">None — the stock table is in effect.</span>
              )}
              {overrides.map((o, i) => {
                const dup = dupKeys.has(`${o.band}|${o.mode}`)
                return (
                  <div className={`freq-edit-row${dup ? ' dup' : ''}`} key={i}>
                    <select
                      className="settings-input"
                      value={o.band}
                      aria-label={`Override ${i + 1} band`}
                      onChange={(e) => updateOverride(i, { band: e.target.value })}
                    >
                      {FREQ_BANDS.map((b) => (
                        <option key={b} value={b}>
                          {b}
                        </option>
                      ))}
                    </select>
                    <select
                      className="settings-input"
                      value={o.mode}
                      aria-label={`Override ${i + 1} mode`}
                      onChange={(e) => updateOverride(i, { mode: e.target.value })}
                    >
                      {FREQ_MODES.map((m) => (
                        <option key={m} value={m}>
                          {m}
                        </option>
                      ))}
                    </select>
                    <input
                      className="settings-input"
                      type="number"
                      inputMode="decimal"
                      min="0"
                      step="0.0001"
                      aria-label={`Override ${i + 1} dial frequency in MHz`}
                      value={mhzDraft && mhzDraft.idx === i ? mhzDraft.text : o.mhz.toFixed(6)}
                      onChange={(e) => {
                        setMhzDraft({ idx: i, text: e.target.value })
                        commitMhz(i, e.target.value)
                      }}
                      onBlur={() => setMhzDraft(null)}
                      autoComplete="off"
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={() => removeOverride(i)}
                      aria-label={`Remove the ${o.band} ${o.mode} override`}
                      title="Remove this override"
                    >
                      ✕
                    </button>
                    {dup && (
                      <span className="freq-dup-tag">duplicate band + mode — the last row wins</span>
                    )}
                  </div>
                )
              })}
              <div className="settings-input-row freq-actions">
                <button type="button" className="settings-refresh" onClick={addOverride}>
                  Add override
                </button>
                <button
                  type="button"
                  className="settings-refresh"
                  onClick={resetOverrides}
                  disabled={overrides.length === 0}
                >
                  Reset to standard
                </button>
              </div>
              <span className="settings-hint">
                MHz is the dial (suppressed-carrier) frequency. Save to apply — band switches
                then use your value for that band + mode.
              </span>
            </div>
          </fieldset>
          )}

          {/* ---- Alerts ---- */}
          {tab === 'spots' && (
          <>
          <fieldset className="settings-section">
            <legend>Pounce — new-one alert</legend>
            <p className="settings-note">
              Interrupts you the INSTANT a needed station appears on the cluster or RBN, rather
              than waiting for the spot board to refresh. A loud tone plays whether or not Nexus
              is the window you are looking at, and a banner offers one-click Work.
              {' '}
              Off until you switch it on. How rare "rare" is depends on your own totals: if you
              are chasing your first hundred entities then almost every DX spot is a new one and
              this would never stop talking. Start with <em>New DXCC entity only</em> once your
              log is far enough along that a new one is genuinely an event. Each station alerts
              once per band and mode.
            </p>
            <label className="settings-field">
              <span className="settings-label">Alert me for</span>
              <select
                value={form.pounceThreshold ?? 'off'}
                onChange={(e) => update('pounceThreshold', e.target.value as never)}
              >
                <option value="off">Off (default)</option>
                <option value="atno">New DXCC entity only</option>
                <option value="atnoOrZone">New entity or CQ zone</option>
                <option value="atnoZoneOrState">New entity, zone, or US state</option>
              </select>
            </label>
          </fieldset>

          <fieldset className="settings-section">
            <legend>Accessibility &amp; eyes-free</legend>
            <p className="settings-note">
              Speech and sound cues for operating by ear (screen-reader users, or anyone who wants
              audible feedback). The keyboard and screen-reader labels throughout Nexus are always
              on — these settings only control what comes out of the speakers.
            </p>
            <div className="settings-grid">
              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">Announce decodes (screen reader)</span>
                  <select
                    className="settings-input"
                    value={form.announceVerbosity ?? 'needed'}
                    onChange={(e) => update('announceVerbosity', e.target.value)}
                  >
                    <option value="off">Off</option>
                    <option value="needed">Needed only (calling you / new / watched)</option>
                    <option value="all">All (adds a per-cycle CQ summary)</option>
                  </select>
                </label>
                <span className="settings-hint">
                  What a screen reader speaks as decodes arrive. Silent without a reader running.
                  "Needed" mirrors your alerts; "All" adds a spoken batch summary each cycle.
                </span>
              </div>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">TX / RX earcon</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.soundTxState ?? false}
                    className={`toggle${form.soundTxState ? ' on' : ''}`}
                    onClick={() => updateBool('soundTxState', !form.soundTxState)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">A rising tone when you key up, falling when you unkey — know your TX state by ear.</span>
              </div>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Decode-batch tick</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.soundDecodeTick ?? false}
                    className={`toggle${form.soundDecodeTick ? ' on' : ''}`}
                    onClick={() => updateBool('soundDecodeTick', !form.soundDecodeTick)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">A soft tick each cycle new signals are decoded — the band's rhythm, eyes-free.</span>
              </div>
            </div>
          </fieldset>
          <fieldset className="settings-section">
            <legend>Alerts</legend>
            <div className="settings-grid">
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">My call</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.alertMyCall}
                    className={`toggle${form.alertMyCall ? ' on' : ''}`}
                    onClick={() => updateBool('alertMyCall', !form.alertMyCall)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">Beep + flash when someone directs a call at you.</span>
              </div>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">CQ calls</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.alertCq}
                    className={`toggle${form.alertCq ? ' on' : ''}`}
                    onClick={() => updateBool('alertCq', !form.alertCq)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">Alert on any decoded CQ. Off by default — CQs are constant.</span>
              </div>

              {/* Per-type band scopes: all decode alerts fire on the CURRENT band, so the
                  scope is "should this alert on the band I'm on". VHF+ = 6 m and up. */}
              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">New DXCC</span>
                  <select
                    className="settings-input"
                    value={!form.alertNew ? 'off' : (form.alertDxccBands ?? 'all')}
                    aria-label="New DXCC alert bands"
                    onChange={(e) => changeAlertScope('alertDxccBands', e.target.value)}
                  >
                    <option value="off">Off</option>
                    <option value="hf">HF only</option>
                    <option value="vhf">VHF+ (6 m and up)</option>
                    <option value="all">All bands</option>
                  </select>
                </label>
                <span className="settings-hint">
                  Loud alert on a new DXCC entity — a “new one”. Does NOT alert on every decode.
                </span>
              </div>

              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">New grid</span>
                  <select
                    className="settings-input"
                    value={!form.alertNew ? 'off' : (form.alertGridBands ?? 'vhf')}
                    aria-label="New grid alert bands"
                    onChange={(e) => changeAlertScope('alertGridBands', e.target.value)}
                  >
                    <option value="off">Off</option>
                    <option value="hf">HF only</option>
                    <option value="vhf">VHF+ (6 m and up)</option>
                    <option value="all">All bands</option>
                  </select>
                </label>
                <span className="settings-hint">
                  Quiet toast on a grid you haven&apos;t worked. Default VHF+ only — grid awards
                  (VUCC/FFMA) start at 6 m; on HF nearly every decode is an unworked grid.
                </span>
              </div>

              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">Rare grid 💎</span>
                  <select
                    className="settings-input"
                    value={!form.alertNew ? 'off' : (form.alertRareGridBands ?? 'all')}
                    aria-label="Rare grid alert bands"
                    onChange={(e) => changeAlertScope('alertRareGridBands', e.target.value)}
                  >
                    <option value="off">Off</option>
                    <option value="hf">HF only</option>
                    <option value="vhf">VHF+ (6 m and up)</option>
                    <option value="all">All bands</option>
                  </select>
                </label>
                <span className="settings-hint">
                  The loud 💎 alert for rare/water-only grids (rovers, maritime, DXpeditions) —
                  separate from plain grids so silencing HF chatter keeps the gems.
                </span>
              </div>

            </div>
            <div className="settings-watchlist-block">
              <span className="settings-label">Watch list</span>
              <WatchlistPanel />
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Connections (connector status + log) — moved from Logbook & QSL ---- */}
          {tab === 'logging' && (
          <fieldset className="settings-section">
            <legend>Connections</legend>
            <div className="conn-status-grid">
              {creds.map((c) => (
                <div key={c.connector} className="conn-status-row">
                  <span className={`conn-dot ${c.stored ? 'on' : 'off'}`} aria-hidden="true" />
                  <span className="conn-name">{c.connector}</span>
                  <span className="conn-id">{c.identity || '—'}</span>
                  <span className={`conn-state ${c.stored ? 'on' : 'off'}`}>
                    {c.stored ? 'credential stored' : 'no credential'}
                  </span>
                  {c.connector === 'QRZ Logbook' && (
                    <button
                      type="button"
                      className="settings-test-btn"
                      onClick={runQrzTest}
                      disabled={qrzTest.state === 'testing'}
                      title="Round-trips the QRZ Logbook API (ACTION=STATUS) — proves the key works without logging anything"
                    >
                      {qrzTest.state === 'testing' ? 'Testing…' : 'Test'}
                    </button>
                  )}
                </div>
              ))}
            </div>
            {qrzTest.state !== 'idle' && qrzTest.state !== 'testing' && (
              <p className={`conn-test-result ${qrzTest.state}`}>
                {qrzTest.state === 'ok' ? '✓ QRZ Logbook reachable: ' : '✗ QRZ test failed: '}
                {qrzTest.msg}
                {qrzTest.state === 'fail' && (
                  <>
                    {' '}
                    (Uploads need the per-logbook <strong>API key</strong> from
                    logbook.qrz.com ▸ Settings ▸ API — not your QRZ password.)
                  </>
                )}
              </p>
            )}
            <div className="conn-log">
              <div className="conn-log-head">
                <span>Connection log</span>
                <span className="settings-hint">every save, sync, push, and failure lands here</span>
              </div>
              {connLog.length === 0 ? (
                <p className="conn-log-empty">
                  No events yet this session — save a credential or run a sync and it shows here.
                </p>
              ) : (
                <ul className="conn-log-list">
                  {connLog.slice(0, 40).map((e, i) => (
                    <li key={`${e.tsUnix}-${i}`} className={`conn-ev ${e.level}`}>
                      <span className="conn-ev-time">
                        {new Date(e.tsUnix * 1000).toLocaleTimeString([], { hour12: false })}
                      </span>
                      <span className="conn-ev-name">{e.connector}</span>
                      <span className="conn-ev-msg">{e.message}</span>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </fieldset>
          )}

          {/* ---- Network integrations ---- */}
          {tab === 'logging' && (
          <fieldset className="settings-section">
            <legend>Integrations &amp; Feeds</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Local APIs &amp; Loggers</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">WSJT-X UDP API</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.wsjtxUdp}
                      className={`toggle${form.wsjtxUdp ? ' on' : ''}`}
                      onClick={() => updateBool('wsjtxUdp', !form.wsjtxUdp)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">for JTAlert / GridTracker / loggers</span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">UDP Address</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.wsjtxUdpAddr}
                    placeholder="127.0.0.1:2237"
                    onChange={(e) => update('wsjtxUdpAddr', e.target.value)}
                    disabled={!form.wsjtxUdp}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">host:port for the UDP feed</span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Ham Radio Deluxe logging</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.hrdLogging}
                      className={`toggle${form.hrdLogging ? ' on' : ''}`}
                      onClick={() => updateBool('hrdLogging', !form.hrdLogging)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    push each QSO to HRD Logbook over its QSO-Forwarding UDP port (HRD must be running;
                    don't also run JTAlert/QSO Relay into HRD or you'll double-log)
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">HRD UDP Address</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.hrdUdpAddr}
                    placeholder="127.0.0.1:2333"
                    onChange={(e) => update('hrdUdpAddr', e.target.value)}
                    disabled={!form.hrdLogging}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">HRD QSO-Forwarding host:port (default 127.0.0.1:2333)</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">Companion UDP address</span>
                  <input
                    className="settings-input"
                    value={form.companionAddr ?? ''}
                    onChange={(e) => update('companionAddr', e.target.value)}
                    placeholder="127.0.0.1:2237"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    Where Nexus listens for WSJT-X/JTDX in Companion source mode.
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Write ALL.TXT decode log</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.writeAllTxt}
                      className={`toggle${form.writeAllTxt ? ' on' : ''}`}
                      onClick={() => updateBool('writeAllTxt', !form.writeAllTxt)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    WSJT-X-format decode log for GridTracker / loggers to tail. Written only while
                    this is on, and it first appears after the next decode.
                    {allTxtPath && (
                      <>
                        {' '}Saved at <code>{allTxtPath}</code>.
                      </>
                    )}
                  </span>
                  <button
                    type="button"
                    className="settings-linkbtn"
                    onClick={() => {
                      revealAllTxt().catch(() => {})
                    }}
                  >
                    Reveal in folder
                  </button>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Save a WAV per logged QSO</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.saveQsoWav}
                      className={`toggle${form.saveQsoWav ? ' on' : ''}`}
                      onClick={() => updateBool('saveQsoWav', !form.saveQsoWav)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">Auto-records the last ~60 s of RX audio to the recordings folder on log</span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">Save received audio (.wav per period)</span>
                  <select
                    value={form.saveWav || 'none'}
                    onChange={(e) => update('saveWav', e.target.value)}
                  >
                    <option value="none">None (default)</option>
                    <option value="decodes">Save periods with decodes</option>
                    <option value="all">Save all periods</option>
                  </select>
                  <span className="settings-hint">
                    WAVs land in recordings/periods (12 kHz mono, ~360 KB each). "All" writes
                    ~2 GB/day of continuous monitoring — use for decoder debugging, not always-on.
                  </span>
                </label>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Spot Sources</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">PSK Reporter</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.pskreporter}
                      className={`toggle${form.pskreporter ? ' on' : ''}`}
                      onClick={() => updateBool('pskreporter', !form.pskreporter)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">upload spots to the global map</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">DX Cluster / RBN spots</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.clusterEnabled}
                      className={`toggle${form.clusterEnabled ? ' on' : ''}`}
                      onClick={() => updateBool('clusterEnabled', !form.clusterEnabled)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Surface "new ones" from the Reverse Beacon Network on the Needed board + Connect.
                    Takes effect on restart.
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">Phone/SSB cluster nodes</span>
                  {(form.clusterHosts ?? []).length === 0 ? (
                    <span className="settings-hint cluster-node-empty">
                      No nodes — add one below to get SSB/phone needs (RBN only carries CW + digital).
                    </span>
                  ) : (
                    (form.clusterHosts ?? []).map((host, i) => (
                      <div key={i} className="cluster-node-row">
                        <input
                          className="settings-input"
                          value={host}
                          onChange={(e) =>
                            mutateClusterHosts((hs) => hs.map((h, j) => (j === i ? e.target.value : h)))
                          }
                          placeholder="ve7cc.net:23"
                          spellCheck={false}
                        />
                        <button
                          type="button"
                          className="cluster-node-remove"
                          title="Remove this cluster node"
                          aria-label={`Remove ${host || 'node'}`}
                          onClick={() => mutateClusterHosts((hs) => hs.filter((_, j) => j !== i))}
                        >
                          ✕
                        </button>
                      </div>
                    ))
                  )}
                  <div className="cluster-node-add">
                    <select
                      className="settings-input"
                      value=""
                      onChange={(e) => {
                        const host = e.target.value
                        if (!host) return
                        mutateClusterHosts((hs) =>
                          hs.some((h) => h.trim().toLowerCase() === host.toLowerCase())
                            ? hs
                            : [...hs, host],
                        )
                      }}
                    >
                      <option value="">+ Add a known node…</option>
                      {CLUSTER_PRESETS.map((p) => (
                        <option key={p.host} value={p.host}>
                          {p.label}
                        </option>
                      ))}
                    </select>
                    <button
                      type="button"
                      className="cluster-node-add-blank"
                      title="Add a custom node row"
                      onClick={() => mutateClusterHosts((hs) => [...hs, ''])}
                    >
                      + Custom
                    </button>
                  </div>
                  <span className="settings-hint">
                    We connect to ALL listed nodes and union their human SSB/phone spots — more
                    nodes = wider phone coverage (RBN CW + digital connect automatically; RBN
                    endpoints are ignored here). An added node connects on the next Save; removing
                    one takes effect on restart.
                  </span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Propagation</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Near-region opening watch</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.openingRegional}
                      className={`toggle${form.openingRegional ? ' on' : ''}`}
                      onClick={() => updateBool('openingRegional', !form.openingRegional)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Watch VHF/10 m activity near your QTH (not just your own contacts) so openings flag "open
                    around you" before you've worked anyone. Takes effect on restart.
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">Prediction engine</span>
                  <select
                    value={form.propEngine || 'heuristic'}
                    onChange={(e) => update('propEngine', e.target.value)}
                  >
                    <option value="heuristic">Modelled (fast heuristic)</option>
                    <option value="p533">ITU-R P.533 (full physics)</option>
                  </select>
                  <span className="settings-hint">
                    Drives the per-station path outlook + 24h band×hour grid. P.533 is the real
                    circuit-reliability method (validated against the ITU reference; ~0.1 s per
                    prediction, uses your station power). Live spots always win over any model.
                  </span>
                </label>
              </div>
              <SettingsGroup title="Antenna gain (advanced)" defaultOpen={false}>
                <div className="settings-field">
                  <span className="settings-label">Antenna gain (dBi) — TX / RX</span>
                  <div className="settings-inline-pair">
                    {(['antTxGainDbi', 'antRxGainDbi'] as const).map((k) => (
                      <input
                        key={k}
                        className="settings-input"
                        type="number"
                        step="0.5"
                        min="-10"
                        max="30"
                        inputMode="decimal"
                        aria-label={k === 'antTxGainDbi' ? 'TX antenna gain (dBi)' : 'RX antenna gain (dBi)'}
                        value={form[k] ?? 0}
                        onChange={(e) => {
                          const num = Number(e.target.value)
                          if (!Number.isNaN(num)) updateNum(k, num)
                        }}
                      />
                    ))}
                  </div>
                  <span className="settings-hint">
                    Used by the P.533 link budget only. 0 = a simple wire/vertical (isotropic);
                    a 3-element yagi ≈ 6–8. Honest v1: a plain dB shift — no pattern or
                    takeoff-angle modelling, and the fast heuristic ignores it.
                  </span>
                </div>
              </SettingsGroup>
            </div>
          </fieldset>
          )}

          {/* ---- N3FJP + N1MM loggers (moved from Field Day — they serve everyday club logging) ---- */}
          {tab === 'logging' && (
          <>
          <fieldset className="settings-section">
            <legend>DXKeeper (DXLab Suite)</legend>
            <p className="settings-note">
              Pushes each logged QSO into <strong>DXKeeper</strong> over its TCP Network
              Service. Enable it in DXKeeper under <em>Configuration ▸ Defaults ▸ Network
              Service</em> first.
            </p>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">DXKeeper host</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.dxkeeperHost ?? ''}
                  placeholder="127.0.0.1 (empty = off)"
                  onChange={(e) => update('dxkeeperHost', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">
                  Usually 127.0.0.1 — same PC. Leave blank to disable.
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">DXLab Base Port</span>
                <input
                  className="settings-input"
                  type="number"
                  inputMode="numeric"
                  value={form.dxkeeperBasePort ?? 52000}
                  placeholder="52000"
                  onChange={(e) => {
                    const n = Number(e.target.value)
                    updateNum('dxkeeperBasePort', Number.isFinite(n) ? n : 52000)
                  }}
                />
                {/* Deliberately labelled "Base Port", matching DXKeeper's own config panel.
                    DXKeeper listens on base + 1; nothing listens on the base itself, and
                    operators reliably report "52000" because that is the number their screen
                    shows them. Asking for the base and adding 1 ourselves means the value
                    they read off DXKeeper is the value that works. */}
                <span className="settings-hint">
                  The <em>Base Port</em> from DXKeeper&apos;s Network Service panel (default
                  52000). DXKeeper itself listens on{' '}
                  <strong>{(form.dxkeeperBasePort ?? 52000) + 1}</strong> — Nexus adds the 1 for
                  you.
                </span>
              </label>

              <label className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Let DXKeeper do the uploads</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.dxkeeperUploads === true}
                    className={`toggle${form.dxkeeperUploads === true ? ' on' : ''}`}
                    onClick={() => updateBool('dxkeeperUploads', form.dxkeeperUploads !== true)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Off by default: Nexus already uploads to LoTW / eQSL / ClubLog / QRZ, so
                  turning this on would upload every QSO twice. Note DXKeeper ignores this for
                  Club Log and QRZ if <em>Auto upload</em> is ticked on its own QSL
                  Configuration tab — untick it there.
                </span>
              </label>
            </div>
          </fieldset>

          <fieldset className="settings-section">
            <legend>N3FJP Integration (club master log)</legend>
            <p className="settings-note">
              Each FD contact lands in the club's{' '}
              <strong>N3FJP Field Day Contest Log</strong> the moment you log it — so the whole
              club's score updates in real time. Run N3FJP on the master computer; point Nexus at
              its IP + port (default 1100).
            </p>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">N3FJP host</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.n3fjpHost ?? ''}
                  placeholder="192.168.1.10 (empty = off)"
                  onChange={(e) => update('n3fjpHost', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">IP or hostname of the master log computer. Leave blank to disable.</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">N3FJP port</span>
                <input
                  className="settings-input"
                  type="number"
                  inputMode="numeric"
                  value={form.n3fjpPort ?? 1100}
                  placeholder="1100"
                  onChange={(e) => {
                    markDirty()
                    setForm((prev) => prev ? { ...prev, n3fjpPort: Number(e.target.value) || 1100 } : prev)
                  }}
                  autoComplete="off"
                />
                <span className="settings-hint">N3FJP's API TCP port (default 1100).</span>
              </label>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Use ENTER for Field Day scoring</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.n3fjpUseEnter ?? true}
                    className={`toggle${(form.n3fjpUseEnter ?? true) ? ' on' : ''}`}
                    onClick={() => updateBool('n3fjpUseEnter', !(form.n3fjpUseEnter ?? true))}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Log each FD contact with N3FJP's <strong>ENTER</strong> sequence, which scores the
                  contest — the correct path. Turn off to fall back to a plain <code>ADDDIRECT</code>{' '}
                  insert (may not score). On by default.
                </span>
              </div>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Report my band to N3FJP</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.n3fjpReportBand ?? false}
                    className={`toggle${form.n3fjpReportBand ? ' on' : ''}`}
                    onClick={() => updateBool('n3fjpReportBand', !form.n3fjpReportBand)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Tell N3FJP which band you're on (no CAT needed), so the club's Network Status
                  Display band board shows this position. Off by default.
                </span>
              </div>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Forward every QSO</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.n3fjpUpload ?? false}
                    className={`toggle${form.n3fjpUpload ? ' on' : ''}`}
                    onClick={() => updateBool('n3fjpUpload', !form.n3fjpUpload)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Also push <strong>every</strong> logged QSO (not just Field Day) to N3FJP ACLog on
                  the host above — everyday general logging. N3FJP dedupes, so it's safe to run
                  alongside the Field-Day push.
                </span>
              </div>

              <div className="settings-field">
                <span className="settings-label">Connection test</span>
                <div className="settings-input-row">
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={runN3fjpTest}
                    disabled={n3fjpTest.state === 'testing' || !form.n3fjpHost?.trim()}
                    title="Save settings, then test the N3FJP TCP connection"
                  >
                    {n3fjpTest.state === 'testing' ? 'Testing…' : 'Test N3FJP'}
                  </button>
                </div>
                {n3fjpTest.state !== 'idle' && n3fjpTest.state !== 'testing' && (
                  <span className={`cat-result ${n3fjpTest.state}`} role="status">
                    {n3fjpTest.state === 'ok' ? '✓ ' : '✗ '}{n3fjpTest.msg}
                  </span>
                )}
                <span className="settings-hint">Run this at the club site before the event starts to confirm the API link works.</span>
              </div>
            </div>
          </fieldset>

          <fieldset className="settings-section">
            <legend>N1MM+ Integration</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">N1MM contact broadcast address</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.n1mmAddr ?? ''}
                  placeholder="127.0.0.1:12060 (empty = off)"
                  onChange={(e) => update('n1mmAddr', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">
                  Where the N1MM contact packets go (host:port, UDP). Name the port — consumers
                  stack on one host, and 12060 is often already taken by another logger. Leave
                  blank to disable.{' '}
                  {form.n1mmUpload ? (
                    <strong>Sending for every logged QSO.</strong>
                  ) : (
                    <strong>
                      An address alone sends nothing outside a Field Day event — turn on Broadcast
                      every QSO below for everyday logging.
                    </strong>
                  )}
                </span>
              </label>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">Broadcast every QSO</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.n1mmUpload ?? false}
                    className={`toggle${form.n1mmUpload ? ' on' : ''}`}
                    onClick={() => {
                      const on = !form.n1mmUpload
                      updateBool('n1mmUpload', on)
                      // A toggle with nowhere to send is a dead switch the operator
                      // cannot diagnose. Turning it on with a blank address fills in
                      // the standard local target — visible in the field above and
                      // editable, not a hidden default.
                      if (on && !form.n1mmAddr?.trim()) update('n1mmAddr', '127.0.0.1:12060')
                    }}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">
                  Send the contact packet for <strong>every</strong> logged QSO, not just Field Day
                  — point OpenHamClock or GridTracker at the address above and each contact plots on
                  its map as you log it. One packet per QSO: this never doubles up with the Field Day
                  broadcast, so it is safe to leave on through an event. Off by default; with it off,
                  packets go out <em>only</em> while a Field Day event is running.
                </span>
              </div>
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Confirmations (LoTW / eQSL / QRZ / ClubLog accounts) ---- */}
          {tab === 'logging' && (
          <>
          <fieldset className="settings-section">
            <legend>LoTW users list</legend>
            <div className="settings-field">
              <div className="lotw-users-row">
                <button
                  type="button"
                  className="settings-test-btn"
                  disabled={lotwFetching}
                  onClick={() => {
                    setLotwFetching(true)
                    fetchLotwUsers()
                      .then((st) => {
                        setLotwUsers(st)
                        pushToast(
                          `LoTW list loaded — ${st.count.toLocaleString()} calls`,
                          'success',
                          5000,
                        )
                      })
                      .catch((e) =>
                        pushToast(
                          `LoTW list fetch failed: ${e instanceof Error ? e.message : e}`,
                          'error',
                        ),
                      )
                      .finally(() => setLotwFetching(false))
                  }}
                >
                  {lotwFetching ? 'Fetching…' : 'Fetch now'}
                </button>
                <span className="settings-hint">
                  {lotwUsers && lotwUsers.count > 0
                    ? `${lotwUsers.count.toLocaleString()} calls · fetched ${new Date(lotwUsers.fetchedAt * 1000).toISOString().slice(0, 10)}`
                    : 'Not fetched yet — decode lists gain an L mark on calls that upload to LoTW.'}
                </span>
              </div>
              <label className="settings-label" htmlFor="lotw-max-age" style={{ marginTop: 8 }}>
                Count as a LoTW user if uploaded within (days)
              </label>
              <input
                id="lotw-max-age"
                className="settings-input"
                type="number"
                min="30"
                max="3650"
                step="1"
                style={{ width: '7em' }}
                value={form.lotwMaxAgeDays ?? 365}
                onChange={(e) => {
                  const n = Number(e.target.value)
                  if (!Number.isNaN(n)) updateNum('lotwMaxAgeDays', n)
                }}
              />
              <span className="settings-hint">
                ARRL's activity list updates weekly — refetching more often just returns
                "unchanged". Manual fetch by design (WSJT-X convention).
              </span>
            </div>
          </fieldset>

          <fieldset className="settings-section">
            <legend>Callsign → state database</legend>
            <div className="settings-field">
              <div className="lotw-users-row">
                <button
                  type="button"
                  className="settings-test-btn"
                  disabled={fccFetching}
                  onClick={() => {
                    setFccFetching(true)
                    fetchFccStates()
                      .then((st) => {
                        setFccStates(st)
                        pushToast(
                          `Callsign→state database updated — ${st.count.toLocaleString()} US calls`,
                          'success',
                          5000,
                        )
                      })
                      .catch((e) =>
                        pushToast(
                          `Callsign→state update failed: ${e instanceof Error ? e.message : e}`,
                          'error',
                        ),
                      )
                      .finally(() => setFccFetching(false))
                  }}
                >
                  {fccFetching ? 'Updating…' : 'Update now'}
                </button>
                <span className="settings-hint">
                  {fccStates && fccStates.count > 0
                    ? `${fccStates.count.toLocaleString()} US calls · fetched ${new Date(fccStates.fetchedAt * 1000).toISOString().slice(0, 10)}`
                    : 'Not loaded yet — downloads on first launch, then auto-refreshes weekly.'}
                </span>
              </div>
              <span className="settings-hint">
                A callsign→state index (from the FCC license file) so a New State lights up on
                cluster / CW / SSB spots that carry no grid. Refreshed weekly from
                hamradiotools.io; a live decode grid refines it for rovers.
              </span>
            </div>
          </fieldset>
          <fieldset className="settings-section">
            <legend>Confirmations</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">LoTW</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">LoTW username</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.lotwUsername}
                    placeholder="your LoTW account login"
                    onChange={(e) => update('lotwUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    Often your callsign, but not always — use your LoTW account login. Save settings to apply.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">LoTW password</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={lotwPw}
                      placeholder="LoTW website password"
                      onChange={(e) => setLotwPw(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveLotwPassword}
                      disabled={!lotwPw}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetLotwPassword}
                      title="Remove the stored password from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Your LoTW <strong>website</strong> password (not your TQSL certificate password). Stored in
                    the OS keychain, never on disk; not shown again after you click Set.
                  </span>
                </label>

                <div className="settings-field">
                  <span className="settings-label">LoTW sync</span>
                  <div className="settings-input-row">
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSyncLotw}
                      disabled={lotwSyncing || !form.lotwUsername.trim()}
                    >
                      {lotwSyncing ? 'Syncing…' : 'Sync LoTW now'}
                    </button>
                  </div>
                  <span className="settings-hint">
                    Pulls new confirmations into your log and marks which of your uploads LoTW now holds on file
                    (so they read “waiting on the other op,” not “never uploaded”). The first sync pulls your whole
                    history (can be slow); later syncs are incremental.
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">LoTW Station Location</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.lotwStationLocation}
                    placeholder="exact TQSL Station Location name"
                    onChange={(e) => update('lotwStationLocation', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    For <strong>uploading</strong> to LoTW (the "Upload to LoTW" button in the Logbook). Signing is
                    done by your installed <strong>TQSL</strong> against this named Station Location — set it up in
                    TQSL first; the name must match exactly. No certificate or password is stored by Nexus.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">Sign from ADIF location (travelers)</span>
                  <span className="settings-input-row">
                    <input
                      type="checkbox"
                      checked={!!form.lotwUseAdifLocation}
                      onChange={(e) => updateBool('lotwUseAdifLocation', e.target.checked)}
                      aria-label="Sign LoTW uploads from the ADIF location"
                    />
                    <span className="settings-hint">
                      Turn on if you set TQSL to <em>"use the location in the ADIF file"</em> and don't create
                      named Station Locations (handy if you travel). Nexus then stamps your call + grid
                      (STATION_CALLSIGN / MY_GRIDSQUARE) into the upload and omits the <code>-l</code> argument,
                      so TQSL signs from those and the Station Location above isn't required.{' '}
                      <strong>The whole batch is signed from your current grid above</strong>, so if you operate
                      from more than one location, upload <em>before</em> you move — otherwise earlier contacts
                      are signed with the new grid.
                    </span>
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">TQSL path (optional)</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.tqslPath}
                    placeholder="auto-detect (leave blank)"
                    onChange={(e) => update('tqslPath', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    Only if TQSL is installed somewhere non-standard; otherwise leave blank to auto-detect.
                  </span>
                </label>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">eQSL</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">eQSL username</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.eqslUsername}
                    placeholder="your eQSL.cc account login"
                    onChange={(e) => update('eqslUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    Your eQSL.cc login (often your callsign). Save settings to apply.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">eQSL password</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={eqslPw}
                      placeholder="eQSL.cc account password"
                      onChange={(e) => setEqslPw(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveEqslPassword}
                      disabled={!eqslPw}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetEqslPassword}
                      title="Remove the stored password from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Stored in the OS keychain, never on disk; not shown again after you click Set.
                  </span>
                </label>

                <div className="settings-field">
                  <span className="settings-label">eQSL confirmations</span>
                  <div className="settings-input-row">
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSyncEqsl}
                      disabled={eqslSyncing || !form.eqslUsername.trim()}
                    >
                      {eqslSyncing ? 'Syncing…' : 'Sync eQSL now'}
                    </button>
                  </div>
                  <span className="settings-hint">
                    Download eQSL confirmations into your log. These count as confirmations but{' '}
                    <strong>not</strong> for DXCC/WAS (ARRL doesn't accept eQSL) — a separate tier.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Auto-upload QSOs to eQSL</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.eqslUpload}
                      className={`toggle${form.eqslUpload ? ' on' : ''}`}
                      onClick={() => updateBool('eqslUpload', !form.eqslUpload)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Upload each logged QSO to eQSL.cc as you log it (needs the eQSL username + password above).
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">QRZ</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">QRZ username</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.qrzUsername}
                    placeholder="your QRZ.com account login"
                    onChange={(e) => update('qrzUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    Used to look up a callsign's name + grid when logging. Save settings to apply.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">QRZ password</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={qrzPw}
                      placeholder="QRZ.com account password"
                      onChange={(e) => setQrzPw(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveQrzPassword}
                      disabled={!qrzPw}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetQrzPassword}
                      title="Remove the stored password from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Your QRZ.com login password — <strong>this is what powers callbook lookups</strong>{' '}
                    (name, QTH, grid), and it is separate from the Logbook API key below (that key only
                    uploads QSOs). Stored in the OS keychain, never on disk. Grid &amp; state need a QRZ
                    XML subscription; free accounts return only name/address/country.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">QRZ Logbook API key</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={qrzKey}
                      placeholder="from your QRZ logbook settings page"
                      onChange={(e) => setQrzKey(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveQrzLogbookKey}
                      disabled={!qrzKey}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetQrzLogbookKey}
                      title="Remove the stored Logbook key from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    A <strong>separate</strong> key (not the login password) from your QRZ logbook's settings
                    page — used to upload logged QSOs.
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Auto-upload QSOs to QRZ</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.qrzLogbookUpload}
                      className={`toggle${form.qrzLogbookUpload ? ' on' : ''}`}
                      onClick={() => updateBool('qrzLogbookUpload', !form.qrzLogbookUpload)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Push each logged QSO to your QRZ logbook (needs the Logbook API key above).
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Pull confirmations automatically</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.qrzAutoSync}
                      className={`toggle${form.qrzAutoSync ? ' on' : ''}`}
                      onClick={() => updateBool('qrzAutoSync', !form.qrzAutoSync)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    As people confirm on QRZ, the confirmations flow in on their own — no need to
                    press Sync. After the first run only what CHANGED is fetched. QRZ confirmations
                    show as confirmed but never count toward DXCC or WAS, which need LoTW or a card.
                    {form.qrzLastSyncUnix > 0 && (
                      <>
                        {' '}Last pull: {new Date(form.qrzLastSyncUnix * 1000).toLocaleString()}.
                      </>
                    )}
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">Two-way sync</span>
                  <div className="settings-input-row">
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSyncQrz}
                      disabled={qrzSyncing}
                      title="FETCH your online QRZ logbook and merge it in — pulls QSOs you logged elsewhere plus their confirmations"
                    >
                      {qrzSyncing ? 'Syncing…' : 'Sync from QRZ now'}
                    </button>
                  </div>
                  <span className="settings-hint">
                    Pull your QRZ logbook <strong>down</strong> — adds QSOs you logged elsewhere (e.g. a
                    phone app in the field) and marks QRZ-confirmed contacts. QRZ confirmations count as
                    confirmations but <strong>not</strong> for DXCC/WAS. Safe to run repeatedly (deduped).
                    Needs the Logbook API key above.
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">HamQTH</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">HamQTH username</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.hamqthUsername}
                    placeholder="your HamQTH.com account login"
                    onChange={(e) => update('hamqthUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    A <strong>free</strong> callbook used as a fallback when QRZ isn't configured or has
                    no match — a HamQTH account returns name, grid &amp; US state at no charge. Save
                    settings to apply.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">HamQTH password</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={hamqthPw}
                      placeholder="HamQTH.com account password"
                      onChange={(e) => setHamqthPw(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveHamqthPassword}
                      disabled={!hamqthPw}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetHamqthPassword}
                      title="Remove the stored password from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Stored in the OS keychain, never on disk.
                  </span>
                </label>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">ClubLog</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">ClubLog email</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.clublogEmail}
                    placeholder="your ClubLog account email (not a callsign)"
                    onChange={(e) => update('clublogEmail', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">Your ClubLog login email. Save settings to apply.</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">ClubLog callsign</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.clublogCallsign}
                    placeholder="defaults to your callsign"
                    onChange={(e) => update('clublogCallsign', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">The ClubLog logbook to upload into (empty = your callsign).</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">ClubLog app-password</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={clublogPw}
                      placeholder="a ClubLog Application Password"
                      onChange={(e) => setClublogPw(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveClublogPassword}
                      disabled={!clublogPw}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetClublogPassword}
                      title="Remove the stored ClubLog password from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Use a ClubLog <strong>Application Password</strong> (Settings → App Passwords), not your main
                    password. Stored in the OS keychain.
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">ClubLog API key (application-level)</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.clublogApiKey}
                    placeholder="blank = use the key bundled with this build (if any)"
                    onChange={(e) => update('clublogApiKey', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    This is the <strong>application</strong> credential, not yours — official installer builds
                    bundle one, and you only need email + app-password above. Building from source? Request a
                    free key at clublog.org/requestapikey.php and paste it here (open-source can't ship one —
                    ClubLog auto-revokes published keys).
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Auto-upload QSOs to ClubLog</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.clublogUpload}
                      className={`toggle${form.clublogUpload ? ' on' : ''}`}
                      onClick={() => updateBool('clublogUpload', !form.clublogUpload)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Push each logged QSO to ClubLog in real time (needs the email + app-password above; official builds bundle the API key).
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">HRDLog</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">HRDLog.net upload code</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={hrdlogCode}
                      placeholder="your hrdlog.net upload code"
                      onChange={(e) => setHrdlogCodeField(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveHrdlogCode}
                      disabled={!hrdlogCode}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetHrdlogCode}
                      title="Remove the stored HRDLog.net code from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    The upload code from your HRDLog.net account (Options → your code). Uploads log under your
                    station callsign. Stored in the OS keychain. This is the online HRDLog.net service — separate
                    from the HRD Logbook UDP push under Logging.
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Auto-upload QSOs to HRDLog.net</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.hrdlogUpload}
                      className={`toggle${form.hrdlogUpload ? ' on' : ''}`}
                      onClick={() => updateBool('hrdlogUpload', !form.hrdlogUpload)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Push each logged QSO to HRDLog.net (needs the upload code above). HRDLog.net is a live-logging
                    and awards site — it is <strong>not</strong> an ARRL confirmation source, so an upload here
                    never earns DXCC/WAS credit.
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">RepeaterBook</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">RepeaterBook API token</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={rbToken}
                      placeholder="rbuapp_…"
                      onChange={(e) => setRbTokenField(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveRbToken}
                      disabled={!rbToken}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetRbToken}
                      title="Remove the stored RepeaterBook token from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Optional. Without a token the <strong>Program</strong> section uses the open hearham.com
                    directory. Add a personal token (from your RepeaterBook account's{' '}
                    <strong>API Apps</strong> page) to pull from RepeaterBook.com under your own account
                    instead. Stored in the OS keychain. Shared RepeaterBook access for every Nexus user is
                    pending RepeaterBook's approval; if RepeaterBook is unreachable, Program falls back to
                    hearham.com.
                  </span>
                </label>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Cloudlog / Wavelog</span>
              <p className="settings-note">
                Auto-forward each logged QSO to your self-hosted <strong>Cloudlog</strong> or{' '}
                <strong>Wavelog</strong> logbook (HTTP). The API key is a per-instance token for your
                own server — enter it, your station-profile id, and turn on the toggle.
              </p>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">Base URL</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.cloudlogUrl ?? ''}
                    placeholder="https://log.example.com"
                    onChange={(e) => update('cloudlogUrl', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">Your Cloudlog/Wavelog site root. Leave blank to disable.</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">Station profile id</span>
                  <input
                    className="settings-input"
                    type="text"
                    inputMode="numeric"
                    value={form.cloudlogStationId ?? ''}
                    placeholder="1"
                    onChange={(e) => update('cloudlogStationId', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">The station-location profile to log against (Cloudlog ▸ Station Locations).</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">API key</span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={cloudlogKey}
                      placeholder="your instance API key"
                      onChange={(e) => setCloudlogKeyField(e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSaveCloudlogKey}
                      disabled={!cloudlogKey}
                    >
                      Set
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetCloudlogKey}
                      title="Remove the stored Cloudlog key from the system keychain"
                    >
                      Forget
                    </button>
                  </div>
                  <span className="settings-hint">
                    Cloudlog ▸ Account ▸ API Keys — a key with read/write. Stored in the OS keychain,
                    never on disk.
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Auto-forward QSOs</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.cloudlogUpload ?? false}
                      className={`toggle${form.cloudlogUpload ? ' on' : ''}`}
                      onClick={() => updateBool('cloudlogUpload', !form.cloudlogUpload)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">Push every logged QSO to the instance above as it's logged.</span>
                </div>
              </div>
            </div>
          </fieldset>
          </>
          )}
          {/* ---- Field Day ---- */}
          {tab === 'contesting' && (
            <fieldset className="settings-section">
              <legend>Contest Category</legend>
              {/* ONE switch for every QSO-finding assistance source. It takes effect IMMEDIATELY
                  (its own command, not Save) because an operator flips it as an event starts, and
                  a switch that needed a restart mid-contest would be useless. The form field is
                  synced so a later Save cannot write the stale value back. */}
              <label className="settings-field">
                <span className="settings-label">Unassisted entry</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={!!form.unassistedMode}
                  className={`toggle${form.unassistedMode ? ' on' : ''}`}
                  onClick={() => {
                    const on = !form.unassistedMode
                    updateBool('unassistedMode', on)
                    setUnassistedMode(on)
                      .then(() => getAssistanceJournal().then(setAssistLog))
                      .catch(() => {})
                  }}
                  aria-label={`${form.unassistedMode ? 'End' : 'Declare'} an unassisted contest entry`}
                >
                  <span className="toggle-knob" />
                </button>
                <span className="settings-hint">
                  Turns off the AI CW decoder, DX cluster / RBN spots and the PSK Reporter needs
                  feed together, and records the change with a timestamp. Takes effect at once.
                  Your own settings for each of those are left alone and come back when you switch
                  this off.
                </span>
              </label>
              <AssistanceNote
                unassisted={!!form.unassistedMode}
                sinceUnix={assistLog[0]?.tsUnix ?? null}
              />
              {assistLog.length > 0 && (
                <div className="assist-journal">
                  <span className="settings-label">Assistance record</span>
                  <ul className="assist-journal-list mono">
                    {assistLog.slice(0, 8).map((e) => (
                      <li key={`${e.tsUnix}-${e.note}`}>
                        <span className="assist-journal-ts">
                          {new Date(e.tsUnix * 1000).toISOString().slice(0, 16).replace('T', ' ')}Z
                        </span>
                        <span className={`assist-journal-state${e.unassisted ? ' unassisted' : ''}`}>
                          {e.unassisted ? 'UNASSISTED' : 'assisted'}
                        </span>
                        <span className="assist-journal-note">
                          {e.note}
                          {': '}
                          {e.sources.filter((x) => x.active).map((x) => x.name).join(', ') || 'nothing active'}
                        </span>
                      </li>
                    ))}
                  </ul>
                  <span className="settings-hint">
                    Kept in <code>assistance_journal.json</code> beside your settings, so it
                    survives restarts. Newest first.
                  </span>
                </div>
              )}
            </fieldset>
          )}

          {tab === 'contesting' && (
          <fieldset className="settings-section">
            <legend>Field Day Setup</legend>
            {/* The Field Day MASTER lives here now (Contesting is always visible) — turning it on
                reveals the FD workspace + Class/Section exchange across all modes. */}
            <label className="settings-field">
              <span className="settings-label">Field Day mode</span>
              <button
                type="button"
                role="switch"
                aria-checked={!!form.fdActive}
                className={`toggle${form.fdActive ? ' on' : ''}`}
                onClick={() => updateBool('fdActive', !form.fdActive)}
                aria-label={`${form.fdActive ? 'Disable' : 'Enable'} Field Day mode`}
              >
                <span className="toggle-knob" />
              </button>
              <span className="settings-hint">
                Turn on for Field Day weekend — reveals the Field Day workspace and the
                Class/Section exchange across all modes. Off the rest of the year. Fill in Class +
                Section below to start operating. Save to apply.
              </span>
            </label>
            {form.fdActive && (!form.fdClass.trim() || !form.fdSection.trim()) && (
              <p className="settings-note">
                <strong>Set your Class + Section to start operating.</strong> Field Day mode is on,
                but the station won&apos;t enter Field Day until both are filled in below.
              </p>
            )}
            <div className="settings-grid">
              <div className="settings-field">
                <span className="settings-label">Event</span>
                <div className="theme-switcher" role="group" aria-label="Field Day event">
                  {([
                    { value: 'arrlfd', label: 'ARRL Field Day' },
                    { value: 'wfd',    label: 'Winter Field Day' },
                  ] as { value: string; label: string }[]).map((ev) => (
                    <button
                      key={ev.value}
                      type="button"
                      className={`theme-chip${(form.fdEvent ?? 'arrlfd') === ev.value ? ' active' : ''}`}
                      aria-pressed={(form.fdEvent ?? 'arrlfd') === ev.value}
                      onClick={() => {
                        markDirty()
                        setForm((prev) => prev ? { ...prev, fdEvent: ev.value } : prev)
                      }}
                    >
                      {ev.label}
                    </button>
                  ))}
                </div>
                <span className="settings-hint">Which event you're operating in — affects scoring labels and export headers.</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  {(form.fdEvent ?? 'arrlfd') === 'wfd' ? 'WFD Category' : 'FD Class'}
                </span>
                <input
                  className="settings-input mono"
                  type="text"
                  value={form.fdClass}
                  placeholder={(form.fdEvent ?? 'arrlfd') === 'wfd' ? '2O' : '1D'}
                  onChange={(e) => update('fdClass', e.target.value.toUpperCase())}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">
                  {(form.fdEvent ?? 'arrlfd') === 'wfd'
                    ? 'Transmitters + location: H=Home, I=Indoor, M=Mobile, O=Outdoor (e.g. 2O = 2 transmitters, outdoor).'
                    : 'Number of transmitters + class letter: A=club/group portable, B=1–2 person portable, C=mobile, D=home (mains power), E=home (emergency power), F=EOC. E.g. 3A = 3 transmitters, club portable.'}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">ARRL Section</span>
                <input
                  className={`settings-input mono${fdSectionInvalid ? ' invalid' : ''}`}
                  type="text"
                  value={form.fdSection}
                  placeholder="WI"
                  list="fd-section-list"
                  aria-invalid={fdSectionInvalid}
                  onChange={(e) => update('fdSection', e.target.value.toUpperCase())}
                  autoComplete="off"
                  spellCheck={false}
                />
                <datalist id="fd-section-list">
                  {FD_SECTION_OPTIONS.map((s) => (
                    <option key={s.code} value={s.code}>{`${s.name} · ${s.division}`}</option>
                  ))}
                </datalist>
                {fdSectionInvalid && (
                  <span className="fd-section-warn" role="alert">
                    &ldquo;{form.fdSection}&rdquo; isn&apos;t a known ARRL/RAC section — pick one from the list.
                  </span>
                )}
                <span className="settings-hint">
                  Your ARRL / RAC section (e.g. WI, ENY, ONN). Start typing the code or a state name and pick from
                  the list — validated against all {FD_SECTION_OPTIONS.length} sections. Required for the Cabrillo log.
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">Power multiplier</span>
                <div className="theme-switcher" role="group" aria-label="Field Day power multiplier">
                  {([
                    { value: 5, label: '×5 QRP / battery', hint: 'Runs entirely on battery or other natural power, ≤5W output' },
                    { value: 2, label: '×2 ≤100W',         hint: '100W or less from any power source' },
                    { value: 1, label: '×1 >100W',         hint: 'Over 100W — commercial/generator power' },
                  ] as { value: number; label: string; hint: string }[]).map((p) => (
                    <button
                      key={p.value}
                      type="button"
                      className={`theme-chip${(form.fdPowerMult ?? 1) === p.value ? ' active' : ''}`}
                      aria-pressed={(form.fdPowerMult ?? 1) === p.value}
                      title={p.hint}
                      onClick={() => {
                        markDirty()
                        setForm((prev) => prev ? { ...prev, fdPowerMult: p.value } : prev)
                      }}
                    >
                      {p.label}
                    </button>
                  ))}
                </div>
                <span className="settings-hint">
                  Multiplies your QSO points. QRP/battery = ×5 (ARRL bonus for going off-grid). Choose before the event.
                </span>
              </div>
            </div>
          </fieldset>
          )}
        </div>

        <div className="settings-actions">
          {error && <span className="settings-error" role="alert">{error}</span>}
          {status === 'saved' && !error && (
            <span className="settings-ok" role="status">Saved</span>
          )}
          <button
            type="submit"
            className="settings-save"
            // Not disabled on an empty callsign — clicking routes to the Station tab with a clear
            // message (handleSubmit), rather than a greyed button that gives no reason or fix.
            disabled={status === 'saving'}
          >
            {status === 'saving' ? 'Saving…' : 'Save'}
          </button>
        </div>
      </form>
    </section>
  )
}
