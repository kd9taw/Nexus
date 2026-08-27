import { useEffect, useMemo, useRef, useState } from 'react'
import { SAT_VFO_MAPS } from '../features/satVfo'
import { confirmDialog } from '../confirm'
import { checkRigForm, blocks, MULTI_DATA_MODE_ICOMS, NATIVE_CIV_MODELS, nativeCivBlockedReason, type RigCheck } from '../rigFormChecks'
import {
  confirmSatUplink,
  exportSettingsBundle,
  resetSettings,
  importSettingsBundle,
  saveTextToDownloads,
  setBlockedCalls as apiSetBlockedCalls,
  type SerialPortInfo,
} from '../api'
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
  getPortlessRigModels,
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
// What reads the catalog today: the panel SHELL (its chrome, the tab rail, Save, and the
// toasts/confirms its handlers raise), the whole Appearance tab (Workspace + Features +
// Accessibility), the Logging & Connectors sections above Confirmations (Connections,
// Worked-before (B4) & dupes, Integrations & Feeds, DXKeeper, N3FJP, N1MM+, the LoTW users
// list and the callsign→state database), the Spots & Alerts sections (Pounce + Alerts), the
// two Contesting sections (Contest Category + Field Day Setup), and the Digital tab — the
// FT8/FT4 section's Logging Behavior, Decoder and Station Housekeeping sub-groups plus JT65,
// MSK144, Beacons, FST4, Q65 and the quick-reply macros. Every remaining tab's fieldsets are
// still hardcoded English and are deliberately outside the i18n guard's scope; see its header.
//
// ⚠️ ONE RANGE IS DELIBERATELY STILL ENGLISH INSIDE A MIGRATED SECTION: Digital's "Transmit &
// Sequencing" and "Auto-CQ & Caller Selection" sub-groups. They are the FT-mode TX / timing /
// QSO-management surface, and they move in the transmit-path batch with the stop-line sweeps
// re-run. The comment beside them says so at the call site.
import { setLocale, t, type MessageKey } from '../i18n'
import { LOCALE_NATIVE_NAME, localeChoices, useLocale } from '../i18n/useLocale'
import { T } from '../i18n/T'
import { FD_EVENT_NAMES } from '../fdEvent'
import { loadProfiles, mergeProfile, saveProfile, deleteProfile, type Profile } from '../profiles'
import {
  getAssistanceJournal,
  getConnectionLog,
  getCredentialsStatus,
  CREDENTIALS_CHANGED,
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
import { allTxtLocation, diagLogLocation, recordingsLocation, revealAllTxt, revealDiagLog, revealRecordings } from '../api'
import { findDaxDevices, isDaxPaired } from '../features/dax'
import type { AssistanceEvent, ConnEvent, CredStatus } from '../types'
import { connState, dotClass, stateLabel, whenText } from '../settings/connHealth'
import { SettingsStation } from './SettingsStation'
import { SetupHealth } from './SetupHealth'
import { ThemeSwitcher } from './ThemeSwitcher'
import { LiveLevelMeter, LiveRxLevelDb } from './LiveMeters'
import { WatchlistPanel } from './WatchlistPanel'
import { MiniSpectrum } from './MiniSpectrum'
import { SettingsGroup, SettingsOpenTarget } from './SettingsGroup'
import { SettingsSearch } from './SettingsSearch'
import { resolveTarget } from '../settings/registry'
// The SSTV default-mode picker's rows. A pure module — importing them from SstvView would drag
// the cockpit's canvas/waterfall/api surface into every SettingsPanel test's `../api` mock.
import { SSTV_TX_MODES, TX_MODE_GROUPS } from '../sstvModes'
// The APRS channel list and the grid→channel derivation, shared with the APRS cockpit so the
// picker's options and the derived default can never name different numbers.
import { APRS_FREQS, BEACON_SYMBOLS, NORTH_AMERICA, aprsChannelForGrid } from '../aprsBeacon'
import type { Scale, ScaleMode } from '../useScale'
import { SCALE_STEPS, fitScale } from '../useScale'
import type { Density } from '../useDensity'
import type { FeaturesApi } from '../useFeatures'
import { FEATURES, featureById, featureCategoryLabel, type FeatureCategory, type FeatureDef, type FeatureId } from '../features/registry'
import { PROFILE_LIST } from '../features/profiles'
import { checkForUpdateManual } from '../features/updateCheck'
import { ARRL_SECTIONS_BY_DIVISION } from '../features/arrlSections'

// Serial-port examples and walkthroughs are platform prose: a Mac's ports are /dev/cu.* and
// there is no Device Manager, so a "COM16" placeholder or a CP210x "Enhanced" label is a dead
// end there (mac QA audit, 2026-08-17). The check moved to the shared platform module when
// the modifier-chord labels needed it too.
import { IS_MAC, IS_WINDOWS } from '../platform'

interface Props {
  /** Called after a successful save so the shell can refresh its snapshot. */
  onSaved?: () => void
  /** Where to land. Accepts a section id (`'audio'`), a tab id, a tab label, a legacy tab name
   * (`'Logbook & QSL'`) or a path (`'Settings ▸ Radio ▸ Audio'`) — see `resolveTarget`. The panel
   * opens that tab, scrolls the section into view and expands it if it is a collapsed
   * disclosure. Undefined keeps the default landing (Station), so every existing caller is
   * unchanged.
   *
   * This is the mechanism the app lacked: ~228 "Settings ▸ …" pointers across the UI, the Rust
   * toasts and the docs told the operator a path in prose and then made them walk it, and the
   * panel had no way to be told where to go. */
  target?: string
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
  /** Theme (Light/Dark) — moved here from the top bar (operator, 2026-08-10);
   * optional so hosts/tests without theme wiring render the tab unchanged. */
  theme?: 'light' | 'dark'
  onThemeChange?: (t: 'light' | 'dark') => void
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

// Operator basics (band / dial / sideband are handled by FrequencyControl) live in
// `SettingsStation.tsx` — extracted 2026-08-18 as the i18n pilot, DOM unchanged.

// How transmit is keyed. ⚠️ The `value` is the persisted token the settings file and the radio
// loop read; only the LABEL is prose, and CAT / rigctld / RTS / DTR / VOX inside those labels
// name the keying paths themselves, so they survive translation unchanged.
const PTT_METHODS: { value: string; labelKey: MessageKey }[] = [
  { value: 'cat', labelKey: 'settings.rigControl.ptt.cat' },
  { value: 'rts', labelKey: 'settings.rigControl.ptt.rts' },
  { value: 'dtr', labelKey: 'settings.rigControl.ptt.dtr' },
  { value: 'vox', labelKey: 'settings.rigControl.ptt.vox' },
]

// Standard EIA CTCSS (PL) tones, Hz — for the FM repeater-access tone picker.
const CTCSS_TONES = [
  67.0, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2,
  110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 162.2,
  167.9, 173.8, 179.9, 186.2, 192.8, 203.5, 210.7, 218.1, 225.7, 233.6, 241.8, 250.3,
]

const NUMERIC_KEYS: FieldKey[] = ['dialMhz', 'baud', 'rigctldPort', 'rigModel', 'txWatchdogMin', 'catBrokerPort', 'tuneTimeoutSecs', 'aprsIsPort', 'aprsIsRadiusKm', 'aprsStationTtlMin']

/** The APRS SSID conventions (APRS spec appendix / the de-facto community list). Not enforced
 *  anywhere — an SSID is free-form 0..15 — but naming them is the difference between a number
 *  picker and a choice an operator can make.
 *
 *  The SSID is the stored value and is printed as it stands; what each one CONVENTIONALLY
 *  means is prose, so it is held as a catalog key and resolved when the row is rendered. */
const APRS_SSIDS: { ssid: number; labelKey: MessageKey }[] = [
  { ssid: 0, labelKey: 'settings.aprs.ssid.fixed' },
  { ssid: 1, labelKey: 'settings.aprs.ssid.genericSecondary' },
  { ssid: 2, labelKey: 'settings.aprs.ssid.generic' },
  { ssid: 3, labelKey: 'settings.aprs.ssid.generic' },
  { ssid: 4, labelKey: 'settings.aprs.ssid.generic' },
  { ssid: 5, labelKey: 'settings.aprs.ssid.phone' },
  { ssid: 6, labelKey: 'settings.aprs.ssid.satellite' },
  { ssid: 7, labelKey: 'settings.aprs.ssid.handheld' },
  { ssid: 8, labelKey: 'settings.aprs.ssid.boat' },
  { ssid: 9, labelKey: 'settings.aprs.ssid.mobile' },
  { ssid: 10, labelKey: 'settings.aprs.ssid.igate' },
  { ssid: 11, labelKey: 'settings.aprs.ssid.balloon' },
  { ssid: 12, labelKey: 'settings.aprs.ssid.tracker' },
  { ssid: 13, labelKey: 'settings.aprs.ssid.weather' },
  { ssid: 14, labelKey: 'settings.aprs.ssid.truck' },
  { ssid: 15, labelKey: 'settings.aprs.ssid.generic' },
]

// Standard serial CAT baud rates offered in the Rig baud picker. A rig's manual lists its
// supported rate(s); most modern rigs run 38400 or 115200. Auto-detect may set a value outside
// this list — the picker keeps it as an extra option so it's never silently dropped.
const STANDARD_BAUDS = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200]

/**
 * The baud a settings file carries when nobody has chosen one — `radioPatch`'s own fallback
 * below, mirroring the Rust default (`tempo-app::settings`, `baud: 38400`).
 */
const APP_DEFAULT_BAUD = 38400

/**
 * The single CAT rate a rig can run — for the rigs whose Hamlib backend states it as a fact.
 *
 * ## THE BASIS RULE. An entry may rest on ONE fact, and this is it.
 *
 * A model is listed **only** when `rigctl -m <model> --dump-caps` against the bundled Hamlib
 * reports `Serial speed: N..N` — `serial_rate_min == serial_rate_max`, the backend saying there
 * is no choice of rate at all. The value is that N. Nothing else may put a rig in this table:
 * not a baud menu out of the radio's manual, not a low declared max, not a make.
 *
 * Picking such a rig sets the baud to N, because on a one-rate rig every other value is a
 * setting that cannot work. There is nothing to protect and so no judgement to get wrong.
 *
 * A rig with **no entry keeps whatever baud is set** and is never touched. Finding a working
 * rate for those is `baud_ladder.rs`'s job — it PROBES, and a probe cannot be wrong about a
 * radio the way a table can.
 *
 * ### Why the rule is this narrow — four rounds, each fixing the last one's rig and breaking a new one
 *
 * The predecessor of this table carried a per-radio `{rates, preferred}` transcribed out of
 * hardware manuals across 61 models, and it clobbered a working setting every round:
 * an FT-847 running 57,600 rewritten to 4,800; an FX-4 (fixed 115,200) shipped unable to talk;
 * three Kenwoods dead on a fresh install; and finally an IC-746 row reading `[9600, 19200]`
 * against a manual (Set Mode item 27) that offers 300/1200/4800/9600/19200/AUTO — so an
 * operator running one at 4,800 was silently moved to 19,200, which is the FT-847 bug again,
 * created by the fix for the FT-847 bug.
 *
 * **The root cause was the data, not the logic.** Hand transcription across 61 models has a
 * failure rate, and each wrong row silently overwrites a radio that was working. So the table
 * now holds only what the backend declares, and is checked against it: the caps of every rig
 * the picker offers are dumped by `scripts/gen-hamlib-serial-speeds.mjs` into
 * `__fixtures__/hamlibSerialSpeeds.json`, and `SettingsPanel.rigpicker.test.tsx` fails if a row
 * appears whose model is not `min == max` there, if its rate is not that value, or if a
 * one-rate rig in the catalog is missing a row. **A row cannot be added from a manual.**
 *
 * ⚠️ Deliberately NOT here, and it stays that way: rigs whose declared max is merely low.
 * Hamlib does not enforce `serial_rate_min`/`_max` — measured, model 3085 opens clean at
 * 115,200 against a declared max of 19,200 — and Nexus drives Icoms past it on purpose (the
 * native CI-V scope REQUIRES 115,200 on an IC-705, and `civ/scope.rs` records an IC-9700
 * verified at 57,600 against a declared 38,400). Only `min == max` is a fact; a range is not.
 */
export const RIG_FIXED_BAUD = new Map<number, number>([
  // Yaesu
  [1004, 4800], // FT-1000MP Mark-V
  [1010, 4800], // FT-736R
  [1014, 4800], // FT-920
  [1016, 4800], // FT-990
  [1021, 4800], // FT-100 / FT-100D
  [1024, 4800], // FT-1000MP
  // Kenwood
  [2005, 4800], // TS-690S
  [2007, 4800], // TS-790
  [2009, 4800], // TS-850
  // Elecraft / Lab599 / BG2FX
  [2021, 4800], // K2
  [2050, 9600], // Discovery TX-500
  [2053, 115200], // FX-4/C/CR/L
  // Icom — the IC-726 alone in its family declares one rate (1200..1200); its
  // IC-725/728/729 siblings are 1200..9600 and are therefore left to the operator.
  [3015, 1200], // IC-726
  // Xiegu
  [3088, 19200], // G90
  // Ten-Tec
  [16001, 57600], // TT-550 Pegasus
  [16002, 57600], // TT-538 Jupiter
  [16007, 1200], // TT-516 Argonaut V
  [16008, 57600], // TT-565/566 Orion I/II
  [16009, 1200], // TT-585 Paragon
  [16011, 57600], // TT-588 Omni VII
  [16013, 57600], // TT-599 Eagle
  // Alinco
  [17001, 9600], // DX-77
  [17002, 9600], // DX-SR8
])

/**
 * The baud to apply when `modelNum` is picked, or `null` to leave the setting alone. Pure, and
 * the whole of the rule in [`RIG_FIXED_BAUD`] — a listed rig has exactly one rate, so it gets
 * it; an unlisted rig has no fact behind it, so it is not touched.
 */
export const baudForRig = (modelNum: number, currentBaud: number): number | null => {
  const only = RIG_FIXED_BAUD.get(modelNum)
  if (only === undefined || only === currentBaud) return null
  return only
}

/**
 * ⭐ THE SAME RULE, FOR ROTATORS — and the reason a quarter of the picker could not work.
 *
 * The rotator side shipped ONE app-wide 9600 (`default_rotator_baud()`, and a tooltip that said
 * "GS-232 default 9600" to every owner of every model), and `rotctld_args` forces it onto the
 * daemon as `-s <baud>` whenever a port is set — which OVERRIDES the backend's own declared
 * rate. Five of the thirteen real-hardware entries declare a single rate that is not 9600, so
 * they were shipped unable to talk to their controller: SPID Rot2Prog 600, SPID Rot1Prog 1200,
 * Idiom Press Rotor-EZ / Hy-Gain DCU-1 / Green Heron RT-21 4800. It reads to the operator as
 * "Nexus doesn't work with my rotator" — the 2026-08-18 field report — because the wrong line
 * rate looks exactly like dead hardware.
 *
 * The basis is the rig side's, unchanged: **only `serial_rate_min == serial_rate_max` is a
 * fact; a range is not.** The rows below are DERIVED from the bundled Hamlib by
 * `scripts/gen-hamlib-rotator-speeds.mjs` into `__fixtures__/hamlibRotatorSpeeds.json`, and
 * `SettingsPanel.rotpicker.test.tsx` fails on any row that is not `min == max` there, on any
 * rate that is not that value, and on any one-rate SERIAL rotator in the library with no row.
 * A row cannot be added from a manual.
 *
 * It covers the WHOLE library rather than the curated list, because "Other Hamlib model #…"
 * lets an operator type any number and a fixed-rate backend is fixed however it was chosen.
 */
export const ROT_FIXED_BAUD = new Map<number, number>([
  // Idiom Press / Hy-Gain / Green Heron / DF9GR — the rotorez family, all 4800
  [401, 4800], // Idiom Press Rotor-EZ
  [402, 4800], // Idiom Press RotorCard
  [403, 4800], // Hy-Gain DCU-1/DCU-1X
  [404, 4800], // DF9GR ERC
  [405, 4800], // Green Heron RT-21
  [406, 4800], // Hy-Gain DCU2/DCU3/YRC-1
  // SARtek
  [501, 1200], // SARtek-1
  // SPID — the two that are one-rate; MD-01/02 (903) declares 600..460800 and is left alone
  [901, 600], // Rot2Prog
  [902, 1200], // Rot1Prog
  // M2
  [1001, 9600], // RC2800
  [1002, 9600], // RC2800_EARLY_AZ
  [1003, 9600], // RC2800_EARLY_AZEL
  // Telescope mounts pressed into rotator service
  [1401, 9600], // Celestron NexStar
  [1801, 9600], // Meade LX200/Autostar
  // Prosistel
  [1701, 9600], // D azimuth
  [1702, 9600], // D elevation
  [1703, 9600], // Combi-Track az+el
  [1704, 9600], // D elevation CBOX az
  // The rest of the one-rate serial backends
  [2101, 9600], // SatEL
  [2201, 9600], // Radant AZ-1/AZV-1
  [2501, 9600], // FLIR PTU Serial
  [2601, 57600], // Apex Shared Loop
  [2801, 9600], // Sky-Watcher
])

/**
 * The baud to apply when rotator `modelNum` is picked, or `null` to leave the setting alone —
 * the rotator twin of [`baudForRig`], and the same rule: a listed rotator has exactly one rate,
 * so it gets it; an unlisted one has no fact behind it, so it is not touched.
 */
export const baudForRotator = (modelNum: number, currentBaud: number): number | null => {
  const only = ROT_FIXED_BAUD.get(modelNum)
  if (only === undefined || only === currentBaud) return null
  return only
}

/**
 * The curated rotator list. Model numbers and the `(az)` / `(az/el)` suffixes are checked
 * against the generated caps fixture by `SettingsPanel.rotpicker.test.tsx`, so neither can be
 * typed from a manual either.
 *
 * ⚠️ REMOVED, and it must not come back: **EA4TX ARS (1101/1102) is Hamlib's PARALLEL-PORT
 * backend** (`port: "parallel"` in the fixture — it declares no serial rate at all), and it was
 * offered here with a serial-port box and a baud. It could not work as presented, and the brand
 * label steered ARS-USB owners — whose box speaks GS-232 over USB — away from the entry that
 * does work. They belong on **GS-232 (generic)**, which now says so.
 */
export const ROTATOR_MODELS: { model: number; label: string }[] = [
  { model: 601, label: 'Yaesu GS-232A (az/el)' },
  { model: 603, label: 'Yaesu GS-232B (az/el)' },
  { model: 602, label: 'GS-232 (generic, az/el) — also EA4TX ARS-USB, LVB, ST2' },
  { model: 605, label: 'Yaesu/Kenpro GS-23 (az/el)' },
  { model: 606, label: 'Yaesu/Kenpro GS-232 (az/el)' },
  { model: 607, label: 'AMSAT LVB Tracker (az/el)' },
  { model: 901, label: 'SPID Rot2Prog (az/el)' },
  { model: 902, label: 'SPID Rot1Prog (az)' },
  { model: 903, label: 'SPID MD-01/02, ROT2 mode (az/el)' },
  { model: 202, label: 'EasyComm II' },
  { model: 204, label: 'EasyComm III' },
  { model: 401, label: 'Idiom Press Rotor-EZ (az)' },
  { model: 403, label: 'Hy-Gain DCU-1/DCU-1X (az)' },
  { model: 406, label: 'Hy-Gain DCU2/DCU3/YRC-1 (az)' },
  { model: 404, label: 'DF9GR ERC (az)' },
  { model: 405, label: 'Green Heron RT-21' },
  { model: 1001, label: 'M2 RC2800 (az/el)' },
  { model: 1701, label: 'Prosistel D (az)' },
  { model: 1703, label: 'Prosistel Combi-Track (az/el)' },
  { model: 1, label: 'Dummy (testing — no hardware)' },
]

/** WSJT-X Split Operation choices (Settings ▸ Radio parity). The stored `value` is the token;
 * the label is WSJT-X's own wording for the choice and is prose, named by the hint beside it. */
const SPLIT_MODES: { value: NonNullable<Settings['splitMode']>; labelKey: MessageKey }[] = [
  { value: 'none', labelKey: 'settings.rigControl.split.none' },
  { value: 'rig', labelKey: 'settings.rigControl.split.rig' },
  { value: 'fakeit', labelKey: 'settings.rigControl.split.fakeit' },
]

// SAT_VFO_MAPS moved to features/satVfo.ts — the Satellites readiness rail
// mirrors this setting live, and the two surfaces must share ONE label list.

/** What the rotator does when a pass ends. The stored `value` is the token the backend reads;
 * only the LABEL is prose, so it resolves from the catalog at the call site. */
const ROT_POST_PASS: { value: string; labelKey: MessageKey }[] = [
  { value: 'stop', labelKey: 'settings.rotator.postPass.stop' },
  { value: 'park', labelKey: 'settings.rotator.postPass.park' },
  { value: 'ready', labelKey: 'settings.rotator.postPass.ready' },
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
 * The microwave rows (Batch 3): 33 cm's old exclusion rationale is gone — `band_for_dial`
 * names 902 MHz and the privilege table carries 902–928 — and 13 cm/6 cm/3 cm/1.25 cm join
 * for QO-100-class and terrestrial microwave stations (an IC-905 could not be declared).
 * 9 cm is offered for coverage/routing but carries NO US privilege segments (allocation
 * removed) — the picker shows it TX-locked for US classes, which is the honest answer.
 * Modes stay FT8/FT4 — the override table's own scope. */
const FREQ_BANDS = ['160m', '80m', '60m', '40m', '30m', '20m', '17m', '15m', '12m', '10m', '6m', '4m', '2m', '1.25m', '70cm', '33cm', '23cm', '13cm', '9cm', '6cm', '3cm', '1.25cm']
const FREQ_MODES = ['FT8', 'FT4']

/** The mode classes radio ROUTING decides on, with the operator-facing labels (must match the Rust
 * `RouteMode::label`). Coarser than the submode list on purpose: five rules cover a whole station.
 *
 * ⚠️ NOT IN THE CATALOG, DELIBERATELY (i18n phase 2). These five are MODE NAMES — the same
 * invariant vocabulary that keeps the Phone / CW / Digital tabs above untranslated — and this
 * array's own contract is that they match the Rust `RouteMode::label`, which no locale reaches.
 * Translating this copy alone would break that parity. The words around them (`Any mode`,
 * `Satellite`, the rule summary) ARE prose and live in `en.ts`. */
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
// Nine-tab IA: identity → radio (one rig's facts, in setup order) → the three operating modes the
// nav rail itself presents (Phone · CW · Digital) → what-am-I-told (spots+alerts) → where-QSOs-go
// (logging+connectors) → contesting → app prefs.
//
// This replaces the 0.17.0 eight-tab set. `Modes` was one page carrying eleven fieldsets, so the
// mode an operator came for sat behind every other mode's; it splits into the three the rail
// already shows, with Digital parenting its six rail sub-items (FT · Tempo · RTTY · SSTV · APRS,
// plus the weak-signal tiers). `Frequencies` folded into Digital as "Working frequencies
// (FT8/FT4)" — the table only ever held FT8/FT4 rows while its name promised every band plan.
//
// ⚠️ THIS ARRAY MUST STAY A LITERAL `{ id, label }` LIST. `docs-match-code.test.ts:420-484` parses
// it out of this file with a regex and asserts the settings-reference manual's `##` headings equal
// these labels in this order. Importing it from the registry would leave that parser with nothing
// to read; `settings/registry.test.ts` cross-checks the two instead.
type SettingsTab =
  | 'station'
  | 'radio'
  | 'phone'
  | 'cw'
  | 'digital'
  | 'spots'
  | 'logging'
  | 'contesting'
  | 'appearance'

//
// `label` is the ENGLISH source string and stays literal for the two parsers above. What the
// rail RENDERS is `labelKey` when there is one — and three tabs deliberately have none:
// **Phone, CW and Digital are mode names**, invariant technical tokens by the same rule that
// keeps 14.074 MHz out of the catalog (this array's own header calls them "the three
// operating modes the nav rail itself presents"), so they render from `label` unchanged in
// every language.
const SETTINGS_TABS: { id: SettingsTab; label: string; labelKey?: MessageKey }[] = [
  { id: 'station', label: 'Station', labelKey: 'settings.tabs.station' },
  { id: 'radio', label: 'Radio', labelKey: 'settings.tabs.radio' },
  { id: 'phone', label: 'Phone' },
  { id: 'cw', label: 'CW' },
  { id: 'digital', label: 'Digital' },
  { id: 'spots', label: 'Spots & Alerts', labelKey: 'settings.tabs.spots' },
  { id: 'logging', label: 'Logging & Connectors', labelKey: 'settings.tabs.logging' },
  { id: 'contesting', label: 'Contesting', labelKey: 'settings.tabs.contesting' },
  { id: 'appearance', label: 'Appearance', labelKey: 'settings.tabs.appearance' },
]

/** How the OmniRig connection option is offered, as a function of the platform.
 *
 * OmniRig is a Windows COM server, so off Windows the choice cannot work at all. It is still
 * OFFERED — disabled, with the reason in its own label — rather than hidden: OmniRig is named
 * in the rig docs and used by half the Windows logging ecosystem, and an operator who goes
 * looking for it on a Mac must find the answer in the control rather than conclude the build
 * is broken.
 *
 * Pure and takes the platform as an argument so BOTH directions are testable — jsdom is never
 * Windows, so a component-only test could only ever see the disabled half. */
export function omnirigChoiceFor(isWindows: boolean): { disabled: boolean; label: string } {
  return isWindows
    ? { disabled: false, label: t('settings.rigControl.conn.omnirig') }
    : { disabled: true, label: t('settings.rigControl.conn.omnirig.unavailable') }
}

/** [`omnirigChoiceFor`] for the machine this is actually running on. */
export function omnirigChoice(): { disabled: boolean; label: string } {
  return omnirigChoiceFor(IS_WINDOWS)
}

/** Pick just the per-radio CAT/audio/PTT/rotator/native fields — the flat rig form and a radio
 * profile share these exact field names, so this serves BOTH directions: build the save patch
 * from the form, and load a radio's profile into the form (`{...form, ...radioPatch(profile)}`). */
export function radioPatch(s: Partial<RadioProfilePatch>): RadioProfilePatch {
  // `??` only fills genuinely-absent (null/undefined) fields — 0 / '' legit values are preserved.
  return {
    pttMethod: s.pttMethod ?? 'vox',
    rigModel: s.rigModel ?? 0,
    rigModelName: s.rigModelName ?? '',
    serialPort: s.serialPort ?? '',
    pttSerialPort: s.pttSerialPort ?? '',
    baud: s.baud ?? APP_DEFAULT_BAUD,
    rigConn: s.rigConn ?? 'serial',
    rigAddr: s.rigAddr ?? '',
    // ⚠️ PER-RADIO, so it MUST be here — see the Flex note below. Which radio inside OmniRig
    // this profile drives is a property of the profile, and dropping it on Save would move an
    // operator's RIG 2 radio silently onto RIG 1.
    omnirigSlot: s.omnirigSlot ?? 1,
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
    // ⚠️ THE FLEX THREE BELONG HERE, and their absence was silent data loss (2026-08-17 Flex
    // audit). Every save of the rig form while EDITING a non-active radio routes through
    // `persistRadioForm` → `updateRadioProfile(radioPatch(form))`, so a field this function does
    // not return is dropped on Save while the panel reports success — the operator configures
    // radio 2 and radio 1's Flex address is gone. Same class as `pttSerialPort`, and the backend
    // now has a serde-computed guard (`every_per_radio_field_is_reachable_through_the_patch`)
    // that fails when a per-radio field is added without a home in this patch.
    flexRadioIp: s.flexRadioIp ?? '',
    flexNativePan: s.flexNativePan ?? false,
    flexNativeAudio: s.flexNativeAudio ?? false,
  }
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

/**
 * The example network addresses the Logging & Connectors fields show INSIDE a placeholder.
 *
 * Invariant tokens, gathered rather than inlined for the reason `STATION_EXAMPLES` states: a
 * bare address in a `placeholder=` is the moment someone has to decide which category the
 * value is in, and an address a translator "localises" connects to nothing. The prose beside
 * them — "(empty = off)" — is a catalog entry that interpolates one of these.
 */
const LOGGER_EXAMPLES = {
  dxkeeperHost: '127.0.0.1',
  n3fjpHost: '192.168.1.10',
  n1mmAddr: '127.0.0.1:12060',
  clusterNode: 've7cc.net:23',
} as const

/**
 * The Confirmations placeholders that are TOKENS rather than prose — same rule as
 * `LOGGER_EXAMPLES`, one category up: a "localised" `rbuapp_` prefix matches no token
 * RepeaterBook issues, and a translated example hostname resolves nowhere. The station-profile
 * placeholder is the bare number `1` and stays inline, as every number alone does.
 */
const CONFIRMATION_EXAMPLES = {
  rbToken: 'rbuapp_…',
  cloudlogUrl: 'https://log.example.com',
} as const

/**
 * The serial-device names the Rig & CAT port fields show INSIDE a placeholder, one per
 * platform — same rule as `LOGGER_EXAMPLES`. A device name is whatever the OS enumerated, so a
 * "localised" `COM16` or `/dev/cu.usbserial-1420` names no port on any machine; the prose
 * around them ("Select or type, e.g. …") is a catalog entry that interpolates one of these.
 */
const RIG_EXAMPLES = {
  macSerialPort: '/dev/cu.usbserial-1420',
  serialPort: 'COM16',
} as const

/**
 * The rotator's own device and address examples — the same rule as `RIG_EXAMPLES` one section
 * down the tab. A controller's serial device is whatever the OS enumerated and `127.0.0.1:4533`
 * is rotctld's default address; a "localised" one reaches nothing. The prose around them
 * ("host:port — e.g. …") is a catalog entry that interpolates one of these.
 */
const ROTATOR_EXAMPLES = {
  macPort: '/dev/cu.usbserial-1420',
  port: 'COM7 / /dev/ttyUSB1',
  host: '127.0.0.1:4533',
} as const

/**
 * OmniRig's two rig slots, named the way OmniRig's own window names them.
 *
 * Same category as `RIG_EXAMPLES`: this is not prose, it is the string an operator reads in
 * ANOTHER program's interface and has to match this picker against. A translated "RIG 1" names
 * nothing in OmniRig, so the label the panel shows is also the label OmniRig shows. Indexed by
 * the stored slot number, which is the `value` beside it.
 */
const OMNIRIG_SLOTS: Record<1 | 2, string> = { 1: 'RIG 1', 2: 'RIG 2' }

/**
 * The quick-reply macro set named for a Q-code.
 *
 * Same category as `OMNIRIG_SLOTS`: QSO is the hobby's own three letters, identical in every
 * language, so it is a token rather than a catalog entry — exactly as the CQ inside the
 * "Band / CQ" label beside it is a token inside a label whose other word is prose. It lives
 * here rather than inline because a bare word in JSX text is what the i18n guard reads as
 * un-migrated English (i18n/hardcoded-strings.test.ts).
 */
const MACRO_SET_QSO = 'QSO'

/** The CW F1 macro's role, which is the Q-code that key sends — same category as
 *  `MACRO_SET_QSO`, and the reason it sits beside it rather than in the catalog. */
const MACRO_SET_CQ = 'CQ'

/**
 * The keying-interface port examples the CW and RTTY placeholders show — same rule as
 * `RIG_EXAMPLES` above. A COM number is whatever the OS enumerated, so a "localised" one names
 * no port; the prose around it ("… — the keying interface") is a catalog entry that
 * interpolates one of these.
 */
const KEYING_EXAMPLES = {
  winkeyerPort: 'COM6',
  cwKeyPort: 'COM7',
  rttyFskPort: 'COM8',
} as const

/**
 * APRS's own wire formats, shown as examples or written into a frame.
 *
 * `ssid` and `watchCalls` are callsigns, `path` is a digipeater path the packet carries
 * verbatim, and `isHost` is a hostname — all four are matched literally by something outside
 * this program, so all four stay here and are interpolated into the prose beside them.
 */
const APRS_EXAMPLES = {
  ssid: 'KD9TAW-9',
  watchCalls: 'W9XYZ-9, KD9ABC',
  path: 'WIDE1-1, WIDE2-1',
  isHost: 'rotate.aprs2.net',
} as const

/**
 * The FCC rule the SSTV callsign burn-in satisfies. A citation is an address into a legal
 * text — the same in every language, and a decimal comma in it would name a different rule.
 */
const SSTV_ID_RULE = '§97.119(b)(4)'

/** The ISS SSTV downlink, in MHz. A dial reading: formatted invariantly, never written into
 *  a sentence (`i18n/index.ts`, the invariant-token rule). */
const ISS_SSTV_MHZ = 145.8

/**
 * Tokens the panel prints beside a number or stores as a `<select>` value, where the row has
 * no prose in it at all. Named rather than written inline for the `MACRO_SET_QSO` reason: a
 * bare word in JSX text is what the i18n guard reads as un-migrated English. `RTS` is a serial
 * control line and `Hz` a unit symbol — the same category as `LINK_TOKENS` in the Now-Bar.
 */
const SERIAL_LINE_RTS = 'RTS'
const UNIT_HZ = 'Hz'

export function SettingsPanel({
  onSaved,
  target,
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
  theme,
  onThemeChange,
}: Props) {
  // Restore-from-backup (#28 item 4). A hidden file input, the same shape the Logbook's ADIF
  // import uses — Tauri has no native picker wired here and this needs none.
  const backupFileRef = useRef<HTMLInputElement | null>(null)
  const onRestoreBackup = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0]
    e.target.value = '' // so re-picking the SAME file fires onChange again
    if (!f) return
    // Destructive and not undoable, so it asks — and names what goes, because "restore" sounds
    // additive and is not.
    if (
      !(await confirmDialog({
        title: t('settings.backup.restore.confirm.title', { file: f.name }),
        body: t('settings.backup.restore.confirm.body'),
        confirmLabel: t('settings.backup.restore.confirm.action'),
        danger: true,
      }))
    ) {
      return
    }
    await withErrorToast(async () => {
      const snap = await importSettingsBundle(await f.text())
      if (!snap) return
      // Re-read and re-seed the form. Without this the panel goes on rendering the PRE-restore
      // values, so a restore looks like it did nothing — and the stale form is still live, so the
      // next Save writes the old values straight back over the restored ones.
      const fresh = await getSettings()
      setForm(fresh)
      setEditingRadioId(fresh.activeRadio)
      dirtyRef.current = false
      onSaved?.()
      pushToast(t('settings.backup.restore.done'), 'success')
    }, t('settings.backup.restore.failed'))
  }

  // Reset everything to factory defaults. Destructive, immediate, and the one control in this
  // block an operator can reach by accident, so it confirms through the same danger dialog the
  // restore uses — and the wording points AT the backup beside it, because the machinery that
  // makes this reversible is one button away and there is no excuse not to name it.
  //
  // Reloads the form from the backend rather than trusting the returned snapshot: `reset_settings`
  // runs `ensure_radio_profiles` and friends, so what LANDED is not the bare `Settings::default()`
  // this asked for, and a form seeded from anything else would show a roster the engine is not
  // driving.
  const handleResetConfig = async () => {
    if (
      !(await confirmDialog({
        title: t('settings.backup.reset.confirm.title'),
        body: t('settings.backup.reset.confirm.body'),
        confirmLabel: t('settings.backup.reset.confirm.action'),
        danger: true,
      }))
    ) {
      return
    }
    const snap = await withErrorToast(
      () => resetSettings(),
      t('settings.backup.reset.failed'),
    )
    if (snap) {
      const fresh = await getSettings()
      setForm(fresh)
      setEditingRadioId(fresh.activeRadio)
      dirtyRef.current = false
      onSaved?.()
      pushToast(t('settings.backup.reset.done'), 'success')
    }
  }
  const [form, setForm] = useState<Settings | null>(null)
  // The blocklist editor's text — its OWN write path (apiSetBlockedCalls, the narrow
  // verb), never the form save: the engine deliberately ignores blockedCalls in a
  // whole-struct save so a stale form can't revert a mid-QSO Alt-click. Seeded from the
  // loaded settings; "unsaved" only relative to its own Apply.
  const [blockedText, setBlockedText] = useState<string | null>(null)
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
  // Re-render the panel when the language changes, so the picker's own labels switch with it
  // (the panel is the one screen guaranteed to be open at that moment).
  const locale = useLocale()
  // Read LIVE, not at module load: the catalogs are installed during startup, after this
  // module is imported (see localeChoices).
  const languages = localeChoices()
  const [allTxtPath, setAllTxtPath] = useState('')
  const [diagLogPath, setDiagLogPath] = useState('')
  const [recordingsPath, setRecordingsPath] = useState('')
  const [status, setStatus] = useState<'idle' | 'loading' | 'saving' | 'saved'>('loading')
  const [error, setError] = useState<string | null>(null)
  const [rigModels, setRigModels] = useState<[number, string][]>([])
  // Full Hamlib catalog (thousands of entries) — fetched lazily only when the
  // operator checks "Show all models", so the common case (curated ~50) stays fast.
  const [allRigModels, setAllRigModels] = useState<[number, string][]>([])
  const [allRigModelsLoading, setAllRigModelsLoading] = useState(false)
  const [showAllRigModels, setShowAllRigModels] = useState(false)
  // Rig-picker search text. The picker itself STAYS a native <select>: that is the control a
  // screen reader can actually work: on the Chromium/WebView2 IA2 bridge JAWS reads, a
  // <select> is a first-class combobox — name, current value, "n of m", and JAWS's own
  // first-letter type-ahead. An <input list> + <datalist> would let a name be typed, but its
  // suggestion popup is browser chrome rather than DOM and is not put on that bridge, so a
  // blind operator gets an edit box that silently offers nothing (and the datalist submits a
  // LABEL, not the model number this field stores). A11y here is always-on, never a mode
  // (0.9.6 design rule), so searchability arrives as a filter field that narrows the options.
  const [rigFilter, setRigFilter] = useState('')
  const [serialPorts, setSerialPorts] = useState<string[]>([])
  /** Findings from the last save attempt — warnings stay visible after a save that proceeded. */
  const [rigChecks, setRigChecks] = useState<RigCheck[]>([])
  /** Models needing no serial port, from the backend. Empty = rule unread; see checkRigForm. */
  const [portlessRigModels, setPortlessRigModels] = useState<number[]>([])
  // Port -> USB product label ("USB-Enhanced-SERIAL-B CH342"), so the picker can tell a
  // dual-serial rig's two interfaces apart (Xiegu CAT is on SERIAL-B).
  const [portLabels, setPortLabels] = useState<Record<string, string>>({})
  /**
   * The port rows exactly as the backend sent them, kept ALONGSIDE the names and labels above
   * rather than replacing them. The pickers keep working on plain strings; only the two pre-save
   * topology checks read these, and they read optional fields that are absent on every platform
   * that cannot prove USB topology. See `checkRigForm`.
   */
  const [portInfos, setPortInfos] = useState<SerialPortInfo[]>([])
  const applyPorts = (infos: SerialPortInfo[]) => {
    setSerialPorts(infos.map((i) => i.name))
    setPortLabels(Object.fromEntries(infos.map((i) => [i.name, i.label])))
    setPortInfos(infos)
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
    diagLogLocation()
      .then((p) => {
        if (alive) setDiagLogPath(p)
      })
      .catch(() => {})
    recordingsLocation()
      .then((p) => {
        if (alive) setRecordingsPath(p)
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
  /** The device rows as sent, for the same-radio check only — see `portInfos` above. */
  const [audioInfos, setAudioInfos] = useState<AudioDevices>({ input: [], output: [] })
  const applyAudio = (d: AudioDevices) => {
    setAudioInfos(d)
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
    setQrzTest({ state: 'testing', msg: t('settings.connections.test.testing') })
    qrzTestConnection()
      .then((msg) => setQrzTest({ state: 'ok', msg }))
      .catch((e) => setQrzTest({ state: 'fail', msg: String(e) }))
  }
  const [n3fjpTest, setN3fjpTest] = useState<{ state: 'idle' | 'testing' | 'ok' | 'fail'; msg: string }>({ state: 'idle', msg: '' })
  const runN3fjpTest = () => {
    setN3fjpTest({ state: 'testing', msg: t('settings.connections.test.testing') })
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
  // The two LOGS are live and belong on a timer: they grow while the operator watches.
  useEffect(() => {
    let live = true
    const load = () => {
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
  // ⚠️ CREDENTIAL STATUS IS NOT ON THAT TIMER, and must not go back on it (#154). Answering
  // "is a password stored?" opens an OS-keychain session PER CONNECTOR; doing it every 5 s
  // crashed gnome-keyring-daemon on Fedora 44 in a restart loop that lasted as long as the app
  // was open — the operator's journal caught it aborting in `service_method_open_session`.
  // The answer changes only when a secret is saved or cleared, and `api.ts` raises
  // CREDENTIALS_CHANGED when that happens, so this reads once and then only on real news.
  useEffect(() => {
    let live = true
    const pull = () => {
      getCredentialsStatus().then((c) => live && setCreds(c)).catch(() => {})
    }
    pull()
    window.addEventListener(CREDENTIALS_CHANGED, pull)
    return () => {
      live = false
      window.removeEventListener(CREDENTIALS_CHANGED, pull)
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
  // Where a deep link asked us to land. Resolved once per `target` change so a caller can pass
  // prose ("Settings ▸ Radio ▸ Audio") or a bare section id and get the same result; an
  // unresolvable target leaves the default landing rather than doing nothing.
  const resolvedTarget = useMemo(() => (target ? resolveTarget(target) : null), [target])
  const [tab, setTab] = useState<SettingsTab>(resolvedTarget?.tab ?? 'station')
  // The section a deep link is pointing at, published to collapsed `SettingsGroup`s so one
  // containing the target opens itself — a target the operator still cannot see is not found.
  const [openTarget, setOpenTarget] = useState<string | null>(resolvedTarget?.section ?? null)
  // A later target (the operator clicks a second pointer while Settings is already open) must
  // move the panel, not be swallowed because the component is already mounted.
  useEffect(() => {
    if (!resolvedTarget) return
    setTab(resolvedTarget.tab)
    setOpenTarget(resolvedTarget.section ?? null)
  }, [resolvedTarget])
  // Scroll AFTER the tab's JSX has mounted — only the active tab renders, so the anchor does not
  // exist until the tab switch has painted. Two frames: one for the tab body, one for a
  // disclosure that `SettingsGroup` opens in its own effect.
  useEffect(() => {
    if (!openTarget) return
    let inner = 0
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => {
        const el = document.getElementById(`settings-${openTarget}`)
        // `block: 'start'` (not the usual 'nearest'): a deep link means "show me this section",
        // so it belongs at the top of the scroller, not merely on screen.
        el?.scrollIntoView({ block: 'start', behavior: 'smooth' })
      })
    })
    return () => {
      cancelAnimationFrame(outer)
      cancelAnimationFrame(inner)
    }
  }, [openTarget, tab])
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
    // The backend's own "needs no serial port" rule, fetched once. On failure it stays empty and
    // the pre-save port check declines to block — see checkRigForm.
    getPortlessRigModels()
      .then((m) => mounted && Array.isArray(m) && setPortlessRigModels(m))
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

  // SSTV drive, stored as a PERCENT (unlike the caps above, which are 0–1 fractions) because
  // the control it seeds is a percent slider. Blank = null = never touch the rig's power, which
  // is the shipped behaviour — `updateNum` cannot express that state, it would store 0.
  const updateSstvTxPower = (pct: string) => {
    markDirty()
    const t = pct.trim()
    const n = t === '' ? null : Math.min(100, Math.max(0, Math.round(Number(t))))
    setForm((prev) =>
      prev
        ? { ...prev, sstvTxPowerPct: t !== '' && !Number.isFinite(n as number) ? prev.sstvTxPowerPct : n }
        : prev,
    )
  }

  // Apply RX capture gain to the LIVE audio stream (not just the form). Called on
  // release — `set_rx_gain` persists + returns a snapshot, so we commit once the
  // operator lets go of the slider rather than on every drag tick (which would
  // disk-thrash). The RX Level meter then responds within a poll, so the control
  // visibly "works" instead of appearing dead until a full Save.
  const applyRxGainLive = (value: number) => {
    void setRxGain(value).catch(() => pushToast(t('settings.audio.rxGain.failed'), 'error'))
  }

  // Apply TX drive (Pwr) to the LIVE radio, the SAME value as the cockpit "Pwr" slider —
  // set_tx_level persists + updates the snapshot, so the cockpit reflects it immediately.
  // Throttled to ~16 Hz while dragging (not gated entirely on release): set_tx_level writes
  // the whole settings.json to disk on every call, so applying on every onChange tick would
  // trade release-only jank for disk-thrash on every drag pixel. `force` (pointerUp/keyUp)
  // bypasses the throttle window so the final settled value is never silently dropped by it.
  const txLevelThrottleRef = useRef(0)
  const applyTxLevelLive = (value: number, force = false) => {
    const now = Date.now()
    if (!force && now - txLevelThrottleRef.current < 60) return
    txLevelThrottleRef.current = now
    void setTxLevel(value).catch(() => pushToast(t('settings.audio.txPower.failed'), 'error'))
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

  // The RF digipeater path, edited as one comma-separated field for the same reason as the
  // budlist above. An EMPTY list is a real value here — "direct, no digipeaters" — so this
  // must never coerce empty to a default.
  const setAprsPath = (hops: string[]) => {
    markDirty()
    setForm((prev) => (prev ? { ...prev, aprsPath: hops } : prev))
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
  // The ROLE table moved to the catalog with the CW tab's editor (batch 17), as the comment
  // that stood here said it would. A role says what the key is FOR, which is prose — except
  // F1's, which is the Q-code the key sends and stays a token beside `MACRO_SET_QSO`.
  //
  // ⚠️ `CW_MACRO_DEFAULTS` DID NOT MOVE AND MUST NOT. It is SEED DATA: the moment "Customize"
  // is pressed these labels are written into `settings.json` as the operator's own macro set,
  // so a translated label is a translated PERSISTED value, stale the day the locale changes.
  // Its `text` is macro tokens ({MYCALL}, {RST}) and was never translatable in any language.
  const CW_MACRO_ROLES: Record<string, string> = {
    F1: MACRO_SET_CQ,
    F2: t('settings.cw.macros.role.answer'),
    F3: t('settings.cw.macros.role.report'),
    F4: t('settings.cw.macros.role.signOff'),
    F5: t('settings.cw.macros.role.myCall'),
    F6: t('settings.cw.macros.role.hisCall'),
    F7: t('settings.cw.macros.role.askRepeat'),
    F8: t('settings.cw.macros.role.query'),
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
    // The suggested name is a DISPLAY label, not seed data in the `CW_MACRO_DEFAULTS` sense:
    // profiles are keyed by index everywhere, nothing reads the name back, and the same entry
    // is what the picker falls back to for a profile with no name at all.
    const name = window
      .prompt(
        t('settings.cw.macros.profiles.addPrompt'),
        t('settings.cw.macros.profiles.unnamed', { n: cwProfiles.length + 1 }),
      )
      ?.trim()
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
    const name = window.prompt(t('settings.cw.macros.profiles.renamePrompt'), cur.name)?.trim()
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
      pushToast(t('settings.satellites.vfoMap.failed'), 'error'),
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

  const resetOverrides = async () => {
    if (
      (form?.workingFrequencies?.length ?? 0) > 0 &&
      !(await confirmDialog({
        title: t('settings.workingFreq.reset.confirm.title'),
        body: t('settings.workingFreq.reset.confirm.body'),
        confirmLabel: t('settings.workingFreq.reset.confirm.action'),
        danger: true,
      }))
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

  // Which baud a rig pick may impose — and, more to the point, when it may NOT. The rates,
  // the rule and why it is a rule live at `RIG_FIXED_BAUD` / `baudForRig` (module scope).
  const selectRig = (modelNum: number) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const baud = baudForRig(modelNum, prev.baud)
      return { ...prev, rigModel: modelNum, rigModelName: findRigModelName(modelNum), ...(baud ? { baud } : {}) }
    })
  }

  // The rotator twin, and it exists because the rotator picker did NOT have one: picking a
  // model wrote `rotatorModel` and nothing else, so a SPID or a Green Heron kept the app-wide
  // 9600 that its backend does not run at. Same rule, same shape — see `ROT_FIXED_BAUD`.
  const selectRotator = (modelNum: number) => {
    markDirty()
    setForm((prev) => {
      if (!prev) return prev
      const baud = baudForRotator(modelNum, prev.rotatorBaud ?? 9600)
      return { ...prev, rotatorModel: modelNum, ...(baud ? { rotatorBaud: baud } : {}) }
    })
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
    // Adding does not switch the station onto the new radio -- see Engine::add_radio. Select it
    // for EDITING instead, so the operator lands on the profile they just made and can configure
    // it, while the rig they are actually operating keeps running.
    void withErrorToast(() => addRadio(), t('settings.radios.add.failed')).then(async (s) => {
      if (!s) return
      reloadRadios()
      const fresh = await getSettings()
      const added = fresh.radios?.[fresh.radios.length - 1]
      if (added) setEditingRadioId(added.id)
    })
  }
  const handleRemoveRadio = async (id: number) => {
    // Destructive + immediate + unrecoverable (drops the radio's CAT/audio config, its unique
    // rigctld port, and band coverage) — confirm before removing.
    const r = form?.radios?.find((p) => p.id === id)
    if (
      !(await confirmDialog({
        title: t('settings.radios.remove.confirm.title', {
          name: r?.name ?? t('settings.radios.thisRadio'),
        }),
        body: t('settings.radios.remove.confirm.body'),
        confirmLabel: t('settings.radios.remove.confirm.action'),
        danger: true,
      }))
    ) {
      return
    }
    void withErrorToast(() => removeRadio(id), t('settings.radios.remove.failed')).then((s) => {
      if (!s) return
      // If the deleted radio was the one the rig form is editing, retarget the form: a
      // dangling editingRadioId routes every later Save through update_radio_profile(<gone
      // id>), which no-ops on a missing id — so station-wide edits (callsign, credentials)
      // silently stop persisting with no error.
      setEditingRadioId((cur) => (cur === id ? activeRadioId : cur))
      reloadRadios()
    })
  }
  const handleRenameRadio = (id: number, name: string) => {
    void withErrorToast(() => renameRadio(id, name), t('settings.radios.rename.failed')).then((s) => s && reloadRadios())
  }
  const handleToggleRadioBand = (id: number, band: string) => {
    const radio = form?.radios?.find((r) => r.id === id)
    if (!radio) return
    const bands = radio.bands.includes(band)
      ? radio.bands.filter((b) => b !== band)
      : [...radio.bands, band]
    void withErrorToast(() => setRadioBands(id, bands), t('settings.radios.bands.failed')).then(
      (s) => s && reloadRadios(),
    )
  }
  // --- Band+mode routing rules. Live verbs like the roster above: each edit persists immediately
  // and re-pulls, so the rules survive a later stale-form Save (and an edit made while the rig form
  // is pointed at a non-active radio, where Save goes through update_radio_profile and drops the
  // form entirely).
  const mutateRules = (next: RoutingRule[]) => {
    void withErrorToast(() => setRoutingRules(next), t('settings.routing.rules.failed')).then(
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
    void withErrorToast(() => setDefaultRadio(id), t('settings.radios.default.failed')).then(
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
  const handleConfigureRadio = async (id: number) => {
    if (id === editingRadioId) return
    if (
      dirtyRef.current &&
      !(await confirmDialog({
        title: t('settings.radios.edit.confirm.title'),
        body: t('settings.radios.edit.confirm.body'),
        confirmLabel: t('settings.radios.edit.confirm.action'),
        danger: true,
      }))
    ) {
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
  const handleMakeActive = async (id: number) => {
    if (
      dirtyRef.current &&
      !(await confirmDialog({
        title: t('settings.radios.makeActive.confirm.title'),
        body: t('settings.radios.makeActive.confirm.body'),
        confirmLabel: t('settings.radios.makeActive.confirm.action'),
        danger: true,
      }))
    ) {
      return
    }
    void withErrorToast(() => setActiveRadio(id), t('settings.radios.switch.failed')).then((s) => {
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
      withErrorToast(() => detectRigs(), t('settings.detect.usb.failed')),
      discoverFlex().catch((e) => {
        pushToast(
          t('settings.detect.flex.scanFailed', {
            error: String(e instanceof Error ? e.message : e),
          }),
          'info',
          6000,
        )
        return []
      }),
    ])
    setDetectedFlex(flexes)
    setDetecting(false)
    if (rigs) {
      setDetected(rigs)
      if (rigs.length === 0 && flexes.length === 0)
        pushToast(t('settings.detect.none'), 'info')
    }
  }

  // One-click apply a detected rig: fill model (if identified) + port + paired audio.
  const applyDetectedRig = (r: DetectedRig) => {
    if (!form) return
    markDirty()
    const baud = r.suggestedModel != null ? baudForRig(r.suggestedModel, form.baud) : null
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
        t('settings.detect.applied.interface', {
          device: r.interfaceName,
          port: r.portName,
        }),
        'success',
      )
    } else if (r.suggestedModel == null) {
      // Unidentified rig (bridge chip only, no model) — instead of making the operator pick a model
      // and Test CAT by hand, chain straight into the port Auto-test, which sweeps COMMON_CAT_MODELS
      // + bauds to find the one that actually answers. Pass the freshly-applied form (state is async).
      pushToast(
        t('settings.detect.applied.identifying', {
          device: r.product || t('settings.detect.unknownRadio'),
          port: r.portName,
        }),
        'info',
      )
      void handleAutoTestPorts(applied)
    } else {
      pushToast(
        t('settings.detect.applied.review', {
          device: r.suggestedModelName ?? (r.product || t('settings.detect.unknownRadio')),
          port: r.portName,
        }),
        'success',
      )
    }
  }

  // One-click apply a discovered Flex: network conn via SmartSDR CAT's default
  // slice-A TCP port + the SmartSDR CAT dialect model (the WSJT-X-proven path). One model
  // serves BOTH product lines — SmartSDR CAT presents the same command set for a 6000 and an
  // 8000 (2026-08-17 Flex audit, critic gap #9), so a discovered FLEX-8400 lands here too.
  //
  // ⚠️ 127.0.0.1:5002 IS A WINDOWS FACT, NOT A FLEX FACT (2026-08-17 Flex audit, wave-1 #35/#58).
  // It is the SmartSDR CAT *app's* default slice-A port, and FlexRadio ships SmartSDR CAT and the
  // DAX drivers in the Windows suite ONLY. macOS has been a shipped platform since 1.5.0 and
  // Linux longer, and on both this address is a program that cannot be installed: the operator
  // got a config that fails Test CAT with an instant ECONNREFUSED and a toast naming software
  // they cannot obtain. So the localhost proxy is applied only where it can exist; elsewhere the
  // rig-model/conn/IP that ARE true are still applied, `rigAddr` is left for the operator, and
  // the toast says what to put there.
  const applyDetectedFlex = (f: { model: string; nickname: string; ip: string }) => {
    markDirty()
    setForm((prev) =>
      prev
        ? {
            ...prev,
            rigConn: 'network',
            ...(IS_WINDOWS ? { rigAddr: '127.0.0.1:5002' } : {}),
            rigModel: 2036,
            rigModelName: 'FlexRadio FLEX-6xxx / 8xxx (SmartSDR CAT)',
            // Keep the discovered radio IP — the native panadapter / DAX path connects to the rig
            // directly over VITA-49 at this address (CAT still rides the localhost SmartSDR proxy
            // above). Discovery already knows it; dropping it left the native features unreachable.
            flexRadioIp: f.ip || (prev.flexRadioIp ?? ''),
          }
        : prev,
    )
    // The radio is named by its MODEL plus, when it has one, the nickname its owner gave it —
    // both tokens, assembled here so the two sentences below each carry ONE `{{radio}}` slot.
    const named = `${f.model}${f.nickname ? ` "${f.nickname}"` : ''}`
    pushToast(
      IS_WINDOWS
        ? t('settings.detect.flex.applied', { radio: named, ip: f.ip })
        : t('settings.detect.flex.found', { radio: named, ip: f.ip }),
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
      setError(t('settings.cat.callsignRequired'))
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
        const name =
          form.radios?.find((r) => r.id === editingRadioId)?.name ??
          t('settings.radios.unnamed', { id: editingRadioId })
        setCatResult({ ok: true, detail: t('settings.cat.savedNotTested', { name }) })
      } else {
        const result = await testCat()
        setCatResult(result)
      }
    } catch {
      setCatResult({ ok: false, detail: t('settings.cat.test.failed') })
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
      setCatResult({ ok: false, detail: t('settings.cat.autoTest.failed') })
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
    pushToast(t('settings.profiles.saved', { name: newProfileName.trim() }), 'success')
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
    pushToast(t('settings.profiles.loaded', { name: p.name }), 'success')
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
    }, t('settings.connections.lotw.password.saveFailed'))
    if (ok) {
      setLotwPw('')
      pushToast(t('settings.connections.lotw.password.saved'), 'success')
    }
  }

  const onForgetLotwPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearLotwPassword()
      return true
    }, t('settings.connections.lotw.password.clearFailed'))
    if (ok) {
      setLotwPw('')
      pushToast(t('settings.connections.lotw.password.cleared'), 'success')
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
    }, t('settings.connections.lotw.sync.failed'))
    setLotwSyncing(false)
    if (r) {
      // ONE sentence with its two optional clauses interpolated whole — each clause is its own
      // catalog entry, so neither is a fragment a translator has to reassemble.
      const orphans = r.orphans.length
        ? t('settings.connections.sync.unmatched', { count: r.orphans.length })
        : ''
      const promoted = r.promoted
        ? t('settings.connections.lotw.sync.promoted', { count: r.promoted })
        : ''
      pushToast(
        t('settings.connections.lotw.sync.done', {
          confirmed: r.newlyConfirmed,
          credited: r.newlyCredited,
          promoted,
          unmatched: orphans,
        }),
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
    }, t('settings.connections.eqsl.password.saveFailed'))
    if (ok) {
      setEqslPw('')
      updateBool('eqslUpload', true)
      pushToast(t('settings.connections.eqsl.password.saved'), 'success')
    }
  }

  const onForgetEqslPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearEqslPassword()
      return true
    }, t('settings.connections.eqsl.password.clearFailed'))
    if (ok) {
      setEqslPw('')
      updateBool('eqslUpload', false)
      pushToast(t('settings.connections.eqsl.password.cleared'), 'success')
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
    }, t('settings.connections.eqsl.sync.failed'))
    setEqslSyncing(false)
    if (r) {
      const orphans = r.orphans.length
        ? t('settings.connections.sync.unmatched', { count: r.orphans.length })
        : ''
      // eQSL is non-award-grade, so report newlyConfirmedAny (newlyConfirmed is
      // award-only and always 0 for eQSL).
      pushToast(
        t('settings.connections.eqsl.sync.done', {
          confirmed: r.newlyConfirmedAny,
          unmatched: orphans,
        }),
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
    }, t('settings.connections.qrz.password.saveFailed'))
    if (ok) {
      setQrzPw('')
      pushToast(t('settings.connections.qrz.password.saved'), 'success')
    }
  }

  const onForgetQrzPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearQrzPassword()
      return true
    }, t('settings.connections.qrz.password.clearFailed'))
    if (ok) {
      setQrzPw('')
      pushToast(t('settings.connections.qrz.password.cleared'), 'success')
    }
  }

  const onSaveHamqthPassword = async () => {
    if (!hamqthPw) return
    const ok = await withErrorToast(async () => {
      await setHamqthPassword(hamqthPw)
      return true
    }, t('settings.connections.hamqth.password.saveFailed'))
    if (ok) {
      setHamqthPw('')
      pushToast(t('settings.connections.hamqth.password.saved'), 'success')
    }
  }

  const onForgetHamqthPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearHamqthPassword()
      return true
    }, t('settings.connections.hamqth.password.clearFailed'))
    if (ok) {
      setHamqthPw('')
      pushToast(t('settings.connections.hamqth.password.cleared'), 'success')
    }
  }

  const onSaveQrzLogbookKey = async () => {
    if (!qrzKey) return
    const ok = await withErrorToast(async () => {
      await setQrzLogbookKey(qrzKey)
      return true
    }, t('settings.connections.qrz.key.saveFailed'))
    if (ok) {
      setQrzKey('')
      updateBool('qrzLogbookUpload', true)
      pushToast(t('settings.connections.qrz.key.saved'), 'success')
    }
  }

  const onForgetQrzLogbookKey = async () => {
    const ok = await withErrorToast(async () => {
      await clearQrzLogbookKey()
      return true
    }, t('settings.connections.qrz.key.clearFailed'))
    if (ok) {
      setQrzKey('')
      updateBool('qrzLogbookUpload', false)
      pushToast(t('settings.connections.qrz.key.cleared'), 'success')
    }
  }

  const onSyncQrz = async () => {
    setQrzSyncing(true)
    // Two-way pull: FETCH the online QRZ logbook and merge it in (new QSOs +
    // confirmations). Uses the saved Logbook API key, so no form save is needed.
    const r = await withErrorToast(syncQrz, t('settings.connections.qrz.sync.failed'))
    setQrzSyncing(false)
    if (r) {
      const added = r.added ?? 0
      const orphans = r.orphans.length
        ? t('settings.connections.sync.unmatched', { count: r.orphans.length })
        : ''
      // QRZ-native confirmations are non-award-grade, so report newlyConfirmedAny.
      // `count` is the added QSOs — it picks the plural form as well as filling the slot.
      pushToast(
        t('settings.connections.qrz.sync.done', {
          count: added,
          confirmed: r.newlyConfirmedAny,
          unmatched: orphans,
        }),
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
    }, t('settings.connections.clublog.password.saveFailed'))
    if (ok) {
      setClublogPw('')
      updateBool('clublogUpload', true)
      pushToast(t('settings.connections.clublog.password.saved'), 'success')
    }
  }

  const onForgetClublogPassword = async () => {
    const ok = await withErrorToast(async () => {
      await clearClublogPassword()
      return true
    }, t('settings.connections.clublog.password.clearFailed'))
    if (ok) {
      setClublogPw('')
      updateBool('clublogUpload', false)
      pushToast(t('settings.connections.clublog.password.cleared'), 'success')
    }
  }

  const onSaveHrdlogCode = async () => {
    if (!hrdlogCode) return
    const ok = await withErrorToast(async () => {
      await setHrdlogCode(hrdlogCode)
      return true
    }, t('settings.connections.hrdlog.code.saveFailed'))
    if (ok) {
      setHrdlogCodeField('')
      updateBool('hrdlogUpload', true)
      pushToast(t('settings.connections.hrdlog.code.saved'), 'success')
    }
  }

  const onForgetHrdlogCode = async () => {
    const ok = await withErrorToast(async () => {
      await clearHrdlogCode()
      return true
    }, t('settings.connections.hrdlog.code.clearFailed'))
    if (ok) {
      setHrdlogCodeField('')
      updateBool('hrdlogUpload', false)
      pushToast(t('settings.connections.hrdlog.code.cleared'), 'success')
    }
  }

  const onSaveRbToken = async () => {
    if (!rbToken) return
    const ok = await withErrorToast(async () => {
      await setRepeaterbookToken(rbToken)
      return true
    }, t('settings.connections.repeaterbook.token.saveFailed'))
    if (ok) {
      setRbTokenField('')
      pushToast(t('settings.connections.repeaterbook.token.saved'), 'success')
    }
  }

  const onForgetRbToken = async () => {
    const ok = await withErrorToast(async () => {
      await setRepeaterbookToken('')
      return true
    }, t('settings.connections.repeaterbook.token.clearFailed'))
    if (ok) {
      setRbTokenField('')
      pushToast(t('settings.connections.repeaterbook.token.cleared'), 'success')
    }
  }

  const onSaveCloudlogKey = async () => {
    if (!cloudlogKey) return
    const ok = await withErrorToast(async () => {
      await setCloudlogKey(cloudlogKey)
      return true
    }, t('settings.connections.cloudlog.key.saveFailed'))
    if (ok) {
      setCloudlogKeyField('')
      pushToast(t('settings.connections.cloudlog.key.saved'), 'success')
    }
  }

  const onForgetCloudlogKey = async () => {
    const ok = await withErrorToast(async () => {
      await clearCloudlogKey()
      return true
    }, t('settings.connections.cloudlog.key.clearFailed'))
    if (ok) {
      setCloudlogKeyField('')
      pushToast(t('settings.connections.cloudlog.key.cleared'), 'success')
    }
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!form) return
    if (!form.mycall.trim()) {
      // Don't dead-end on another tab: route the operator to where the fix is instead of a
      // silently-greyed Save button with a context-free "required" error.
      setTab('station')
      setError(t('settings.save.callsignFirst'))
      return
    }
    // Check the RADIO before saving it. Until now the callsign was the only validated field, so
    // every way of getting the rig wrong saved silently and then behaved like broken hardware —
    // the symptom always shows up far from the cause. Errors block and name the fix; warnings are
    // stated and the operator proceeds, because an unusual-but-correct station must never be
    // locked out of its own configuration by a heuristic.
    const rigProblems = checkRigForm(
      form,
      serialPorts,
      portlessRigModels,
      portInfos,
      audioInfos,
    )
    setRigChecks(rigProblems)
    if (blocks(rigProblems)) {
      setTab('radio')
      setError(
        rigProblems.find((c) => c.level === 'error')?.message ?? t('settings.save.checkRadio'),
      )
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
      setError(msg || t('settings.save.failed'))
    }
  }

  if (!form) {
    return (
      <section className="panel settings-panel">
        <div className="panel-header">
          <h2>{t('settings.panel.title')}</h2>
        </div>
        <p className="empty">{t('settings.panel.loading')}</p>
      </section>
    )
  }

  // One feature row. Core features are always on and can't be switched, so they
  // show a static "always on" badge instead of a toggle — a locked switch next to
  // the real, toggleable settings just reads as broken.
  //
  // ⚠️ `f.label` and `f.oneLine` are the REGISTRY's words (`features/registry.ts`), rendered
  // and interpolated as values. They move with that registry, not with this panel.
  const featureRow = (f: FeatureDef) => {
    if (f.core) {
      return (
        <div className="settings-field" key={f.id}>
          <div className="settings-toggle">
            <span className="settings-label">{f.label}</span>
            <span className="feature-always-on">{t('settings.features.alwaysOn')}</span>
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
            aria-label={
              on
                ? t('settings.features.toggle.aria.disable', { feature: f.label })
                : t('settings.features.toggle.aria.enable', { feature: f.label })
            }
          >
            <span className="toggle-knob" />
          </button>
        </label>
        <span className="settings-hint">
          {f.oneLine}
          {depOff &&
            ` ${t('settings.features.dependsOn', {
              feature: featureById(depOff as FeatureId)?.label ?? depOff,
            })}`}
        </span>
      </div>
    )
  }

  // serial-port options include the current value even if not in the enumerated
  // list (e.g. a port that has since disappeared), so it stays selectable.
  const portOptions = form.serialPort && !serialPorts.includes(form.serialPort)
    ? [form.serialPort, ...serialPorts]
    : serialPorts

  // Rig-model search. Tokens must ALL appear, and both sides are stripped to letters+digits,
  // so "ft847", "ft 847", "yaesu 847" and "1001" all land on the FT-847 — an operator types
  // what is printed on the radio, not our label's punctuation. The number is part of the
  // haystack so a model number typed here finds its rig too.
  const rigSearchKey = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '')
  const rigModelList = showAllRigModels ? allRigModels : rigModels
  const rigFilterTokens = rigFilter.trim().split(/\s+/).map(rigSearchKey).filter(Boolean)
  const rigModelMatches =
    rigFilterTokens.length === 0
      ? rigModelList
      : rigModelList.filter(([num, name]) => {
          const hay = rigSearchKey(`${name}${num}`)
          return rigFilterTokens.every((t) => hay.includes(t))
        })
  // The chosen rig is never filtered out of its own picker: a <select> whose value matches no
  // option displays the FIRST one instead, so the form would read as a different radio than
  // it holds. Counted separately from the matches, so the announcement stays honest.
  const rigModelOptions =
    rigFilterTokens.length === 0 || rigModelMatches.some(([n]) => n === form.rigModel)
      ? rigModelMatches
      : [...rigModelList.filter(([n]) => n === form.rigModel), ...rigModelMatches]

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
      : t('settings.audio.device.notInList', { device: name })

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
  // Why native CI-V cannot be offered for THIS radio, or null when it can. Derived rather than
  // inlined so the control and its explanation can never disagree about the condition.
  const civBlocked = nativeCivBlockedReason(form.rigModel, form.rigConn)
  const fdSectionInvalid =
    form.fdSection.trim() !== '' && !FD_SECTION_CODES.has(form.fdSection.trim().toUpperCase())

  return (
    <SettingsOpenTarget.Provider value={openTarget}>
    <section className="panel settings-panel">
      <div className="panel-header">
        <h2>{t('settings.panel.title')}</h2>
        <span className="settings-sub">{t('settings.panel.subtitle')}</span>
        {/* Search sits in the HEADER, above the rail, because it is the way in that does not
            require guessing which tab something is under — which is the whole failure it exists
            to answer. Picking a result reuses the same deep-link machinery a pointer elsewhere
            in the app uses: it opens the tab, scrolls the section in and expands it if it is a
            collapsed disclosure. */}
        <SettingsSearch onPick={(sectionId) => {
          // `target`, not `t` — `t` is the catalog lookup, and shadowing it here would make
          // the next translated string added inside this callback silently uncallable.
          const target = resolveTarget(sectionId)
          if (!target) return
          setTab(target.tab)
          setOpenTarget(target.section ?? null)
        }} />
        <span className="settings-build" title={t('settings.panel.build.title')}>
          {t('settings.panel.build', { id: __BUILD_ID__ })}
        </span>
        <button
          type="button"
          className="settings-update-btn"
          onClick={() => void checkForUpdateManual()}
          title={t('settings.panel.update.title')}
        >
          {t('settings.panel.update.label')}
        </button>
      </div>

      <form className="settings-form" onSubmit={handleSubmit}>
        <div className="settings-tabs" role="tablist" aria-label={t('settings.panel.tabs.aria')}>
          {/* Contesting is always visible now (0.17.0 decision) — capability, not config, gates
              tabs; the Field Day master switch lives inside the Contesting tab.
              The loop variable is `tb`, not `t`: `t` is the catalog lookup this file calls. */}
          {SETTINGS_TABS.map((tb) => (
            <button
              key={tb.id}
              type="button"
              role="tab"
              aria-selected={tab === tb.id}
              className={`settings-tab${tab === tb.id ? ' active' : ''}`}
              onClick={() => setTab(tb.id)}
            >
              {tb.labelKey ? t(tb.labelKey) : tb.label}
            </button>
          ))}
        </div>
        <div className="settings-scroll">
          {/* ---- Workspace (UI-only prefs, applied live like the theme) ---- */}
          {tab === 'appearance' && (
          <fieldset className="settings-section" id="settings-workspace">
            <legend>{t('settings.workspace.legend')}</legend>
            <div className="settings-grid">
              {/* LANGUAGE. Rendered only when a second catalog is actually installed — a picker
                  offering one language is a control that cannot do anything, and this app's
                  premise is that a setting exists because it changes something. It appears the
                  day a translation ships and not before. The native name is what a picker shows
                  (an operator looking for German is looking for "Deutsch"), with the tag beside
                  it for the ones that share a word. */}
              {languages.length > 1 && (
                <div className="settings-field">
                  <span className="settings-label">{t('settings.workspace.language.label')}</span>
                  <select
                    className="settings-input"
                    value={locale}
                    onChange={(e) => setLocale(e.target.value)}
                    aria-label={t('settings.workspace.language.label')}
                  >
                    {languages.map((l) => (
                      <option key={l} value={l}>
                        {LOCALE_NATIVE_NAME[l] ?? l}
                      </option>
                    ))}
                  </select>
                  <span className="settings-hint">{t('settings.workspace.language.hint')}</span>
                </div>
              )}
              {/* Theme lives HERE now, not the top bar (operator, 2026-08-10): Light/Dark
                  is a set-once preference, and the bar keeps only the Field quick toggle. */}
              {theme && onThemeChange && (
                <div className="settings-field">
                  <span className="settings-label">{t('settings.workspace.theme.label')}</span>
                  <ThemeSwitcher theme={theme} onChange={onThemeChange} />
                  <span className="settings-hint">{t('settings.workspace.theme.hint')}</span>
                </div>
              )}
              <div className="settings-field">
                <span className="settings-label">{t('settings.workspace.scale.label')}</span>
                <div
                  className="theme-switcher"
                  role="group"
                  aria-label={t('settings.workspace.scale.mode.aria')}
                >
                  <button
                    type="button"
                    className={`theme-chip${scaleMode === 'auto' ? ' active' : ''}`}
                    aria-pressed={scaleMode === 'auto'}
                    onClick={() => onScaleModeChange('auto')}
                  >
                    {t('settings.workspace.scale.auto')}
                  </button>
                  <button
                    type="button"
                    className={`theme-chip${scaleMode !== 'auto' ? ' active' : ''}`}
                    aria-pressed={scaleMode !== 'auto'}
                    onClick={() => onScaleModeChange(scale)}
                  >
                    {t('settings.workspace.scale.manual')}
                  </button>
                </div>
                {scaleMode === 'auto' ? (
                  <>
                    <span className="settings-hint">
                      {t('settings.workspace.scale.cap.label')}
                    </span>
                    <div
                      className="theme-switcher"
                      role="group"
                      aria-label={t('settings.workspace.scale.cap.aria')}
                    >
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
                                ? t('settings.workspace.scale.cap.unreachable', {
                                    fits: autoCeil,
                                    wanted: s,
                                  })
                                : undefined
                            }
                            onClick={() => onScaleCapChange(s)}
                          >
                            {s}%
                          </button>
                        )
                      })}
                    </div>
                    {/* Three WHOLE messages, not one opening plus a tail: the second sentence
                        is a different answer in each case, and a sentence assembled from two
                        keys cannot be reordered by a translator who needs to. */}
                    <span className="settings-hint">
                      {autoCeil < 100
                        ? t('settings.workspace.scale.auto.hint.tooSmall', {
                            scale,
                            fits: autoCeil,
                          })
                        : autoCeil < MAX_STEP
                          ? t('settings.workspace.scale.auto.hint.limited', {
                              scale,
                              fits: autoCeil,
                            })
                          : t('settings.workspace.scale.auto.hint.full', { scale })}
                    </span>
                  </>
                ) : (
                  <>
                    <div
                      className="theme-switcher"
                      role="group"
                      aria-label={t('settings.workspace.scale.aria')}
                    >
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
                      {t('settings.workspace.scale.manual.hint')}
                    </span>
                  </>
                )}
              </div>

              <div className="settings-field">
                <span className="settings-label">{t('settings.workspace.density.label')}</span>
                <div
                  className="theme-switcher"
                  role="group"
                  aria-label={t('settings.workspace.density.aria')}
                >
                  <button
                    type="button"
                    className={`theme-chip${density !== 'dense' ? ' active' : ''}`}
                    aria-pressed={density !== 'dense'}
                    onClick={() => onDensityChange('standard')}
                  >
                    {t('settings.workspace.density.standard')}
                  </button>
                  <button
                    type="button"
                    className={`theme-chip${density === 'dense' ? ' active' : ''}`}
                    aria-pressed={density === 'dense'}
                    onClick={() => onDensityChange('dense')}
                  >
                    {t('settings.workspace.density.dense')}
                  </button>
                </div>
                <span className="settings-hint">{t('settings.workspace.density.hint')}</span>
              </div>

              <div className="settings-field">
                <span className="settings-label">{t('settings.workspace.panes.label')}</span>
                <button type="button" className="settings-refresh" onClick={onResetLayout}>
                  {t('settings.workspace.panes.reset')}
                </button>
                <span className="settings-hint">{t('settings.workspace.panes.hint')}</span>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- Features (modular toggles + goal profiles) ---- */}
          {tab === 'appearance' && (
          <fieldset className="settings-section" id="settings-features">
            <legend>{t('settings.features.legend')}</legend>
            <div className="settings-field">
              <span className="settings-label">{t('settings.features.profile.label')}</span>
              <div
                className="theme-switcher settings-profiles"
                role="group"
                aria-label={t('settings.features.profile.aria')}
              >
                {PROFILE_LIST.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    className={`theme-chip${features.profile === p.id ? ' active' : ''}`}
                    aria-pressed={features.profile === p.id}
                    title={p.blurb}
                    onClick={() => {
                      // Switching from a hand-tuned set discards it — confirm first.
                      void (async () => {
                        if (
                          features.profile !== 'custom' ||
                          (await confirmDialog({
                            // `p.label` is the profiles registry's word, interpolated as a value.
                            title: t('settings.features.profile.confirm.title', {
                              profile: p.label,
                            }),
                            body: t('settings.features.profile.confirm.body'),
                            confirmLabel: t('settings.features.profile.confirm.action'),
                            danger: true,
                          }))
                        ) {
                          features.applyProfile(p.id)
                        }
                      })()
                    }}
                  >
                    {p.label}
                  </button>
                ))}
                {features.profile === 'custom' && (
                  <span
                    className="theme-chip active"
                    aria-disabled="true"
                    title={t('settings.features.profile.custom.title')}
                  >
                    {t('settings.features.profile.custom.label')}
                  </span>
                )}
              </div>
              <span className="settings-hint">
                {features.profile === 'custom'
                  ? t('settings.features.profile.hint.custom')
                  : t('settings.features.profile.hint.preset')}
                {onRerunWizard && (
                  <>
                    {' '}
                    <button type="button" className="settings-linkbtn" onClick={onRerunWizard}>
                      {t('settings.features.rerunWizard')}
                    </button>
                  </>
                )}
              </span>
            </div>

            {/* Core spine first, as a locked group (spec §4.4). */}
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.features.core.title')}</span>
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
                  {/* ⚠️ The category is the REGISTRY's own vocabulary (`FeatureCategory` in
                      `features/registry.ts`). The union value is the group KEY; the word on
                      screen resolves through the registry, beside every feature label. */}
                  <span className="settings-featgroup-title">{featureCategoryLabel(cat)}</span>
                  <div className="settings-grid">
                    {isContesting && (
                      <div className="settings-field">
                        <label className="settings-toggle">
                          {/* One master switch offered in two places — the label and its two
                              accessible names are shared with Settings ▸ Contesting, so both
                              read the same entries. Only the hint differs. */}
                          <span className="settings-label">{t('settings.fieldDay.mode.label')}</span>
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
                            aria-label={
                              form.fdActive
                                ? t('settings.fieldDay.mode.aria.disable')
                                : t('settings.fieldDay.mode.aria.enable')
                            }
                          >
                            <span className="toggle-knob" />
                          </button>
                        </label>
                        <span className="settings-hint">
                          {t('settings.features.fieldDay.hint')}
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
          <SettingsStation
            form={form}
            error={error}
            bandPlan={bandPlan}
            onUpdate={update}
            onSetFreq={setFreq}
          />
          )}

          {/* ---- Rig control ---- */}
          {tab === 'radio' && (
          <>
          <SetupHealth
            radio={radio}
            catResult={catResult}
            onProveTx={onProveTx}
            // In-panel navigation: the strip is already on the Radio tab, so this only has to
            // scroll and expand — the same mechanism a deep link from elsewhere in the app uses.
            onGoTo={(section) => setOpenTarget(section)}
          />
          {/* The wizard re-entry, ON THE RADIO TAB — its only home used to be a text link
              buried in a hint on the Appearance tab (operator, 2026-08-09: "there is no
              features tab"), which is not where anyone looks to redo setup. The Appearance
              link stays for the profile-chip context; this button is the discoverable one. */}
          {onRerunWizard && (
            <div className="settings-field">
              <button type="button" className="settings-linkbtn" onClick={onRerunWizard}>
                Re-run setup wizard…
              </button>
              <span className="settings-hint">
                The guided setup — detect the radio, test CAT, pair audio. It edits in place;
                nothing is lost by re-running it.
              </span>
            </div>
          )}
          {/* Dual-radio roster (P2). Always shown — the "+ Add radio" button is the discovery
              affordance a single-radio operator sees; the per-radio list + band coverage only
              matter once there's a 2nd radio. */}
          <fieldset className="settings-section" id="settings-radios">
            <legend>{t('settings.radios.legend')}</legend>
            {editingRadioId != null && editingRadioId !== form.activeRadio && (
              <p className="settings-note radio-editing-note">
                <T
                  k="settings.radios.editingNote"
                  tags={{ b: <strong /> }}
                  vals={{
                    name:
                      form.radios?.find((r) => r.id === editingRadioId)?.name ??
                      t('settings.radios.anotherRadio'),
                  }}
                />
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
                        placeholder={t('settings.radios.name.placeholder')}
                        onBlur={(e) => {
                          const v = e.target.value.trim()
                          if (v && v !== r.name) handleRenameRadio(r.id, v)
                        }}
                        autoComplete="off"
                        spellCheck={false}
                      />
                      {isActive && (
                        <span
                          className="radio-active-badge"
                          title={t('settings.radios.active.title')}
                        >
                          {t('settings.radios.active.badge')}
                        </span>
                      )}
                      {isEditing && !isActive && (
                        <span
                          className="radio-editing-badge"
                          title={t('settings.radios.editing.title')}
                        >
                          {t('settings.radios.editing.badge')}
                        </span>
                      )}
                      {!isEditing && (
                        <button
                          type="button"
                          className="settings-refresh"
                          onClick={() => handleConfigureRadio(r.id)}
                          title={t('settings.radios.edit.title')}
                        >
                          {t('settings.radios.edit.action')}
                        </button>
                      )}
                      {!isActive && (
                        <button
                          type="button"
                          className="settings-refresh"
                          onClick={() => handleMakeActive(r.id)}
                          title={t('settings.radios.makeActive.title')}
                        >
                          {t('settings.radios.makeActive.action')}
                        </button>
                      )}
                      {multi && (
                        // Rendered DISABLED on the active card, never absent: a missing button
                        // reads as a bug ("the 9700 won't delete" — Mac field report,
                        // 2026-08-17), a disabled one with a title teaches the rule.
                        <button
                          type="button"
                          className="settings-refresh danger"
                          onClick={() => handleRemoveRadio(r.id)}
                          disabled={isActive}
                          title={
                            isActive
                              ? t('settings.radios.remove.title.blocked')
                              : t('settings.radios.remove.title')
                          }
                        >
                          {t('settings.radios.remove.action')}
                        </button>
                      )}
                    </div>
                    {/* ⚠️ THREE ADDRESSES, ALL UNLABELLED (2026-08-17 Flex audit, wave-1 #57). A
                        network Flex has a CAT address (the PC running SmartSDR CAT), a radio
                        address (the rig's own :4992, for the native panadapter/DAX) and the local
                        CAT-helper port — and this row printed two of them as bare numbers with
                        nothing saying which was which. Naming them costs one word each; the row
                        keeps its shape, so no pane sizing is involved. */}
                    {/* ONE sentence, four holes. Every value in it is a TOKEN — a Hamlib
                        model name, OmniRig's own slot name, a COM port or host:port, the OS's
                        sound-device name, a TCP port — and only the labels between them are
                        prose, which is why they cannot be four separate fragments. */}
                    <div className="radio-card-meta">
                      {t('settings.radios.card.meta', {
                        rig:
                          r.rigConn === 'omnirig'
                            ? t('settings.radios.card.omnirig')
                            : r.rigModelName && r.rigModelName !== 'None / VOX'
                              ? r.rigModelName
                              : t('settings.radios.card.noModel'),
                        cat:
                          r.rigConn === 'omnirig'
                            ? `OmniRig ${OMNIRIG_SLOTS[r.omnirigSlot === 2 ? 2 : 1]}`
                            : r.rigConn === 'network'
                              ? r.rigAddr || t('settings.radios.card.noAddress')
                              : r.serialPort || t('settings.radios.card.noPort'),
                        flex:
                          (r.flexRadioIp ?? '').trim() !== ''
                            ? t('settings.radios.card.meta.flex', { ip: r.flexRadioIp ?? '' })
                            : '',
                        audio: r.audioIn
                          ? (audioLabels.input[r.audioIn] ?? r.audioIn)
                          : t('settings.radios.card.audioDefault'),
                        port: r.rigctldPort,
                      })}
                    </div>
                    {multi && (
                      <div className="radio-band-coverage">
                        <span className="settings-hint">
                          {t('settings.radios.bands.hint')}
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
                {t('settings.radios.add.action')}
              </button>
              <span className="settings-hint">
                {(form.radios?.length ?? 1) > 1
                  ? t('settings.radios.hint.multi', {
                      name:
                        form.radios?.find((r) => r.id === editingRadioId)?.name ??
                        t('settings.radios.selectedRadio'),
                    })
                  : t('settings.radios.hint.single')}
              </span>
            </div>
            {(form.radios?.length ?? 1) > 1 && (
              <div className="routing-rules">
                <span className="settings-hint">
                  <T k="settings.routing.intro" tags={{ b: <strong /> }} />
                </span>
                {(form.routingRules ?? []).length === 0 && (
                  <p className="settings-note">{t('settings.routing.empty')}</p>
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
                        aria-label={t('settings.routing.rule.mode.aria', { n: i + 1 })}
                      >
                        <option value="">{t('settings.routing.rule.mode.any')}</option>
                        {ROUTE_MODES.map(([v, label]) => (
                          <option key={v} value={v}>
                            {label}
                          </option>
                        ))}
                        <option value="satellite" title={t('settings.routing.rule.satellite.title')}>
                          {t('settings.routing.rule.satellite')}
                        </option>
                      </select>
                      <span className="routing-rule-arrow">→</span>
                      <select
                        className="settings-input"
                        value={rule.radio}
                        onChange={(e) => handlePatchRule(i, { radio: Number(e.target.value) })}
                        aria-label={t('settings.routing.rule.radio.aria', { n: i + 1 })}
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
                        title={t('settings.routing.rule.up.title')}
                        aria-label={t('settings.routing.rule.up.aria', { n: i + 1 })}
                      >
                        ↑
                      </button>
                      <button
                        type="button"
                        className="settings-refresh"
                        onClick={() => handleMoveRule(i, 1)}
                        disabled={i === (form.routingRules?.length ?? 0) - 1}
                        title={t('settings.routing.rule.down.title')}
                        aria-label={t('settings.routing.rule.down.aria', { n: i + 1 })}
                      >
                        ↓
                      </button>
                      <button
                        type="button"
                        className="settings-refresh danger"
                        onClick={() => handleRemoveRule(i)}
                        aria-label={t('settings.routing.rule.remove.aria', { n: i + 1 })}
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
                      {t('settings.routing.rule.summary', {
                        bands:
                          rule.bands.length === 0
                            ? t('settings.routing.rule.summary.anyBand')
                            : rule.bands.join(', '),
                        mode:
                          rule.context === 'satellite'
                            ? t('settings.routing.rule.satellite')
                            : rule.mode
                              ? ROUTE_MODE_LABEL[rule.mode]
                              : t('settings.routing.rule.summary.anyMode'),
                        radio:
                          form.radios?.find((r) => r.id === rule.radio)?.name ??
                          t('settings.routing.rule.summary.radio', { id: rule.radio }),
                      })}
                    </span>
                  </div>
                ))}
                <div className="radios-actions">
                  <button type="button" className="settings-refresh" onClick={handleAddRule}>
                    {t('settings.routing.add.action')}
                  </button>
                  <label className="settings-input-row routing-default">
                    <span className="settings-label">{t('settings.routing.default.label')}</span>
                    <select
                      className="settings-input"
                      value={form.defaultRadio ?? ''}
                      onChange={(e) =>
                        handleSetDefaultRadio(e.target.value === '' ? null : Number(e.target.value))
                      }
                      aria-label={t('settings.routing.default.aria')}
                    >
                      <option value="">{t('settings.routing.default.stay')}</option>
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
                  <span className="settings-label">{t('settings.routing.test.label')}</span>
                  <div className="settings-input-row">
                    <select
                      className="settings-input"
                      value={routeTest.band}
                      onChange={(e) => runRouteTest(e.target.value, routeTest.mode)}
                      aria-label={t('settings.routing.test.band.aria')}
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
                      aria-label={t('settings.routing.test.mode.aria')}
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
                      {t('settings.routing.test.action')}
                    </button>
                  </div>
                  {routeTestResult && (
                    <p className="settings-note routing-test-result">
                      <T
                        k="settings.routing.test.result"
                        tags={{ b: <strong /> }}
                        vals={{
                          band: routeTest.band,
                          mode: ROUTE_MODE_LABEL[routeTest.mode],
                          radio: routeTestResult,
                        }}
                      />
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
                    aria-label={t('settings.radios.simultaneous.aria')}
                  />
                  <span className="settings-hint">
                    <T k="settings.radios.simultaneous.hint" tags={{ b: <strong /> }} />
                  </span>
                </span>
              </label>
            )}
          </fieldset>
          <fieldset className="settings-section" id="settings-profiles">
            <legend>{t('settings.profiles.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.profiles.list.label')}</span>
                <div className="settings-input-row">
                  <select
                    className="settings-input"
                    value={selectedProfile}
                    onChange={(e) => setSelectedProfile(e.target.value)}
                  >
                    <option value="">{t('settings.profiles.list.none')}</option>
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
                    title={t('settings.profiles.load.title')}
                  >
                    {t('settings.profiles.load.action')}
                  </button>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={handleDeleteProfile}
                    disabled={!selectedProfile}
                  >
                    {t('settings.profiles.delete.action')}
                  </button>
                </div>
                <span className="settings-hint">{t('settings.profiles.list.hint')}</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.profiles.save.label')}</span>
                <div className="settings-input-row">
                  <input
                    className="settings-input"
                    type="text"
                    value={newProfileName}
                    placeholder={t('settings.profiles.save.placeholder')}
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
                    {t('settings.profiles.save.action')}
                  </button>
                </div>
                <span className="settings-hint">{t('settings.profiles.save.hint')}</span>
              </label>
            </div>
          </fieldset>

          <fieldset className="settings-section" id="settings-rig-control">
            <legend>{t('settings.rigControl.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.ptt.label')}</span>
                <select
                  className="settings-input"
                  value={form.pttMethod}
                  onChange={(e) => update('pttMethod', e.target.value)}
                >
                  {PTT_METHODS.map((m) => (
                    <option key={m.value} value={m.value}>
                      {t(m.labelKey)}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">{t('settings.rigControl.ptt.hint')}</span>
              </label>

              {(form.pttMethod === 'rts' || form.pttMethod === 'dtr') && (
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.rigControl.pttPort.label')}
                  </span>
                  <input
                    className="settings-input"
                    list="serial-port-list"
                    value={form.pttSerialPort}
                    placeholder={t('settings.rigControl.pttPort.placeholder', {
                      example: IS_MAC ? RIG_EXAMPLES.macSerialPort : RIG_EXAMPLES.serialPort,
                    })}
                    onChange={(e) => update('pttSerialPort', e.target.value)}
                  />
                  <span className="settings-hint">
                    <T k="settings.rigControl.pttPort.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>
              )}

              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.catRts.label')}</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={form.catRtsKeysPtt ?? false}
                    onChange={(e) => updateBool('catRtsKeysPtt', e.target.checked)}
                  />
                </span>
                <span className="settings-hint">
                  <T k="settings.rigControl.catRts.hint" tags={{ b: <strong /> }} />
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">{t('settings.rigControl.detect.label')}</span>
                <div className="settings-input-row">
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={onDetectRigs}
                    disabled={detecting}
                  >
                    {detecting
                      ? t('settings.rigControl.detect.scanning')
                      : t('settings.rigControl.detect.action')}
                  </button>
                </div>
                {(detected.length > 0 || detectedFlex.length > 0) && (
                  <ul className="rig-detect-list">
                    {detectedFlex.map((f, i) => (
                      <li className="rig-detect" key={`flex-${f.ip}-${i}`}>
                        <div className="rig-detect-main">
                          <span className="rig-detect-name">
                            {/* Model + the nickname its owner gave it: both tokens, assembled
                                here so the sentence carries ONE hole. */}
                            {t('settings.rigControl.detect.flex.name', {
                              radio: `${f.model}${f.nickname ? ` “${f.nickname}”` : ''}`,
                            })}
                          </span>
                          <span className="rig-detect-meta">
                            {t('settings.rigControl.detect.flex.meta', { ip: f.ip })}
                          </span>
                        </div>
                        <button type="button" className="settings-save" onClick={() => applyDetectedFlex(f)}>
                          {t('settings.rigControl.detect.use')}
                        </button>
                      </li>
                    ))}
                    {detected.map((r, i) => (
                      <li className="rig-detect" key={`${r.portName}-${i}`}>
                        <div className="rig-detect-main">
                          <span className="rig-detect-name">
                            {r.interfaceName ??
                              r.suggestedModelName ??
                              (r.product || t('settings.rigControl.detect.unknownRadio'))}
                          </span>
                          <span className="rig-detect-meta">
                            {r.portName} · {r.chip}
                            {/* Dual-UART Icoms (IC-7610/9700) show up as TWO rows that both
                                say the rig's name — the A/B tag is the only tie-breaker. */}
                            {r.civSide === true
                              ? t('settings.rigControl.detect.civ.isCiv')
                              : r.civSide === false
                                ? t('settings.rigControl.detect.civ.notCiv')
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
                              {/* `interfaceNote` is the backend's own sentence — interpolated
                                  as a VALUE, translated in phase 3, never here. */}
                              <T
                                k="settings.rigControl.detect.interface"
                                tags={{ em: <em /> }}
                                vals={{ note: r.interfaceNote ?? '' }}
                              />
                            </span>
                          )}
                          {!r.suggestedModel && !r.interfaceName && (
                            <span className="rig-detect-nomodel">
                              <T k="settings.rigControl.detect.noModel" tags={{ em: <em /> }} />
                            </span>
                          )}
                          {r.driverNote && !r.driverBundled && (
                            <span className="rig-detect-driver">
                              {r.driverNote}
                              {r.driverUrl && (
                                <>
                                  {' '}
                                  <a href={r.driverUrl} target="_blank" rel="noreferrer">
                                    {t('settings.rigControl.detect.driverLink')}
                                  </a>
                                </>
                              )}
                            </span>
                          )}
                        </div>
                        <button type="button" className="settings-save" onClick={() => applyDetectedRig(r)}>
                          {t('settings.rigControl.detect.use')}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                <span className="settings-hint">{t('settings.rigControl.detect.hint')}</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.rigModel.label')}</span>
                {/* Type-to-find. It sits BEFORE the select, so it is what the wrapping label
                    now targets — hence the explicit aria-label on the select below, which
                    also fixes the name it used to get (the whole label's text, hints and
                    all). See the `rigFilter` note above for why this is not a datalist. */}
                <input
                  className="settings-input"
                  type="text"
                  value={rigFilter}
                  placeholder={t('settings.rigControl.rigModel.filter.placeholder')}
                  onChange={(e) => setRigFilter(e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                  aria-label={t('settings.rigControl.rigModel.filter.aria')}
                />
                {/* Mounted unconditionally and EMPTY until there is something to say: a live
                    region added to the DOM at the same moment as its first content is a
                    coin-flip with a screen reader — the AT registers regions it can already
                    see. Sighted operators watch the list shrink; this is the same fact, said. */}
                <span className="settings-hint" role="status">
                  {rigFilterTokens.length === 0
                    ? ''
                    : rigModelMatches.length === 0
                      ? t('settings.rigControl.rigModel.filter.none')
                      : t('settings.rigControl.rigModel.filter.count', {
                          count: rigModelMatches.length,
                        })}
                </span>
                <div className="settings-input-row">
                  <select
                    className="settings-input"
                    value={String(form.rigModel)}
                    onChange={(e) => selectRig(Number(e.target.value))}
                    aria-label={t('settings.rigControl.rigModel.label')}
                  >
                    <option value="0">{t('settings.rigControl.rigModel.none')}</option>
                    {rigModelOptions.map(([num, name]) => (
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
                    placeholder={t('settings.rigControl.rigModel.number.placeholder')}
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
                    aria-label={t('settings.rigControl.rigModel.number.aria')}
                  />
                </div>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={showAllRigModels}
                    onChange={(e) => onToggleShowAllRigModels(e.target.checked)}
                    aria-label={t('settings.rigControl.rigModel.showAll.aria')}
                  />
                  <span className="settings-hint">
                    {t('settings.rigControl.rigModel.showAll.hint', {
                      loading: allRigModelsLoading
                        ? t('settings.rigControl.rigModel.showAll.loading')
                        : '',
                    })}
                  </span>
                </span>
                <span className="settings-hint">{t('settings.rigControl.rigModel.hint')}</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.conn.label')}</span>
                <select
                  className="settings-input"
                  value={form.rigConn || 'serial'}
                  onChange={(e) => update('rigConn', e.target.value)}
                >
                  <option value="serial">{t('settings.rigControl.conn.serial')}</option>
                  <option value="network">{t('settings.rigControl.conn.network')}</option>
                  {/* Offered on every platform and DISABLED off Windows, rather than hidden:
                      OmniRig is named in the docs and in half the Windows logging ecosystem, so
                      a mac/Linux operator who goes looking for it must find the answer here
                      instead of concluding the build is broken. */}
                  <option value="omnirig" disabled={omnirigChoice().disabled}>
                    {omnirigChoice().label}
                  </option>
                </select>
                <span className="settings-hint">
                  <T k="settings.rigControl.conn.hint" tags={{ b: <strong /> }} />
                </span>
                <span className="settings-hint">
                  <T
                    k="settings.rigControl.conn.omnirig.hint"
                    tags={{ b: <strong />, em: <em /> }}
                    vals={{
                      availability: omnirigChoice().disabled
                        ? t('settings.rigControl.conn.omnirig.unavailable.why')
                        : t('settings.rigControl.conn.omnirig.install'),
                    }}
                  />
                </span>
              </label>

              {form.rigConn === 'omnirig' && (
                <label className="settings-field">
                  <span className="settings-label">{t('settings.rigControl.omnirig.label')}</span>
                  <select
                    className="settings-input"
                    value={String(form.omnirigSlot || 1)}
                    onChange={(e) => updateNum('omnirigSlot', Number(e.target.value))}
                    aria-label={t('settings.rigControl.omnirig.aria')}
                  >
                    {/* ⚠️ NOT PROSE. These are the labels OmniRig's OWN window puts on its two
                        slots; they are how the operator matches this picker to that window, so
                        they read the same in every language (RIG_EXAMPLES, beside the device
                        names, for the same reason). */}
                    <option value="1">{OMNIRIG_SLOTS[1]}</option>
                    <option value="2">{OMNIRIG_SLOTS[2]}</option>
                  </select>
                  <span className="settings-hint">{t('settings.rigControl.omnirig.hint')}</span>
                </label>
              )}

              {form.rigConn === 'network' && (
                <label className="settings-field">
                  <span className="settings-label">{t('settings.rigControl.netAddr.label')}</span>
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
                          pushToast(
                            t('settings.rigControl.dax.paired', {
                              input: dax.input,
                              output: dax.output,
                            }),
                            'success',
                            6000,
                          )
                        }}
                        title={t('settings.rigControl.dax.title')}
                      >
                        {t('settings.rigControl.dax.action', { device: dax.input })}
                      </button>
                    ) : null
                  })()}
                  <span className="settings-hint">{t('settings.rigControl.netAddr.hint')}</span>
                  {/* Where an SDR operator finds the real number. Nothing here is a default —
                      every one of these programs lets the operator move the port, and the
                      Thetis field report was a rig served on a port that is Thetis's TCI
                      default, not its CAT default. Read the port out of the program.

                      ⚠️ THIS SENTENCE HAS BEEN WRONG TWICE. It first said TCI was "a WebSocket
                      protocol Nexus can't drive" (false — Hamlib has a TCI backend). It was then
                      rewritten to send the operator to model 7, on the evidence that
                      `strings libhamlib-4.dll` contains `tci1x.c`. That does not follow, and it
                      was checked the wrong way: source strings can be present while the backend
                      is not registered in the build. Measured on the real delivery path —
                        rigctld.exe -m 7 -r 127.0.0.1:50001  ->  "Unknown rig num 7, or
                        initialization error."
                      while `-m 1` starts fine, and `rigctl -l` lists no TCI/Expert/SunSDR entry
                      at all. Model 7 was removed from the catalog (2026-08-06); pointing anyone
                      at it was pointing them at a daemon that refuses to start.

                      The check that settles this class is one command: start rigctld on the
                      model and see whether it does. Do not infer a backend from a grep. */}
                  <span className="settings-hint">
                    <T k="settings.rigControl.netAddr.sdrPorts" tags={{ b: <strong />, em: <em /> }} />
                  </span>
                </label>
              )}

              {/* Serial Port + Baud belong to whoever OPENS the port. With Network that is
                  rigctld over TCP; with OmniRig it is OmniRig itself, which owns the rig type,
                  the port and the baud — so asking for them here would be asking the operator
                  to configure the same radio twice and get it wrong once. */}
              {form.rigConn !== 'network' && form.rigConn !== 'omnirig' && (
                <>
              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.serialPort.label')}</span>
                <div className="settings-input-row">
                  {/* Combobox, not a bare <select>: some driver setups (virtual/SO2R COM
                      ports) make enumeration come back empty, so the operator must be able
                      to TYPE a port (e.g. COM16) — the datalist just offers the found ports. */}
                  <input
                    className="settings-input"
                    list="serial-port-list"
                    value={form.serialPort}
                    placeholder={t('settings.rigControl.serialPort.placeholder', {
                      example: IS_MAC ? RIG_EXAMPLES.macSerialPort : RIG_EXAMPLES.serialPort,
                    })}
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
                    title={t('settings.rigControl.serialPort.refresh.title')}
                  >
                    {portsLoading ? '…' : t('settings.rigControl.serialPort.refresh.action')}
                  </button>
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={() => handleAutoTestPorts()}
                    disabled={catTesting}
                    title={t('settings.rigControl.serialPort.autoTest.title')}
                  >
                    {catTesting ? '…' : t('settings.rigControl.serialPort.autoTest.action')}
                  </button>
                </div>
                <span className="settings-hint">
                  {/* Never say "tty device": on a Mac the tty.* node is exactly the twin the
                      picker hides because it hangs CAT on carrier detect — cu.* is the one. */}
                  {IS_MAC
                    ? t('settings.rigControl.serialPort.hint.mac')
                    : t('settings.rigControl.serialPort.hint.other')}
                  {[3088, 3087, 3091, 3089, 3076].includes(form.rigModel) && (
                    <>
                      {' '}
                      <T
                        k="settings.rigControl.serialPort.xiegu"
                        tags={{ b: <strong /> }}
                        vals={{
                          note: IS_MAC
                            ? ''
                            : t('settings.rigControl.serialPort.xiegu.comNumber'),
                        }}
                      />
                    </>
                  )}
                  {[3078, 3081].includes(form.rigModel) && (
                    <>
                      {' '}
                      {/* One whole sentence per platform, each named by its own key: the guard
                          reads keys as literals, and a key computed in a ternary is a key no
                          extractor can see. */}
                      {IS_MAC ? (
                        <T k="settings.rigControl.serialPort.icom.mac" tags={{ b: <strong /> }} />
                      ) : (
                        <T k="settings.rigControl.serialPort.icom.other" tags={{ b: <strong /> }} />
                      )}
                    </>
                  )}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.baud.label')}</span>
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
                  <T k="settings.rigControl.baud.hint" tags={{ em: <em /> }} />
                </span>
              </label>
                </>
              )}

              <div className="settings-field">
                <span className="settings-label">{t('settings.rigControl.split.label')}</span>
                <div
                  className="theme-switcher"
                  role="group"
                  aria-label={t('settings.rigControl.split.label')}
                >
                  {SPLIT_MODES.map((m) => (
                    <button
                      key={m.value}
                      type="button"
                      className={`theme-chip${(form.splitMode ?? 'none') === m.value ? ' active' : ''}`}
                      aria-pressed={(form.splitMode ?? 'none') === m.value}
                      onClick={() => setSplitMode(m.value)}
                    >
                      {t(m.labelKey)}
                    </button>
                  ))}
                </div>
                <span className="settings-hint">{t('settings.rigControl.split.hint')}</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.rigControl.wheel.label')}{' '}
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
                  aria-label={t('settings.rigControl.wheel.aria')}
                />
                <span className="settings-hint">{t('settings.rigControl.wheel.hint')}</span>
              </label>
            </div>
            <SettingsGroup
              id="rig-advanced"
              title={t('settings.rigControl.advanced.title')}
              defaultOpen={false}
            >
              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.rigControl.rigctldPort.label')}
                </span>
                <input
                  className="settings-input"
                  type="number"
                  inputMode="numeric"
                  value={String(form.rigctldPort)}
                  placeholder="4532"
                  onChange={(e) => update('rigctldPort', e.target.value)}
                  autoComplete="off"
                />
                <span className="settings-hint">{t('settings.rigControl.rigctldPort.hint')}</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.rigControl.plainSsb.label')}</span>
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
                  <T k="settings.rigControl.plainSsb.hint" tags={{ b: <strong /> }} />
                </span>
              </label>

              {/* Offered on the ENGINE's question — the model number — not on what the model
                  NAME happens to look like. And when the answer is "this radio qualifies but
                  the connection cannot carry it", the control is DISABLED WITH THE REASON
                  rather than hidden: an absent control is a mystery, a greyed one with a
                  sentence is an answer. (IC-7610 report, 2026-08-19.) */}
              {NATIVE_CIV_MODELS.includes(form.rigModel) && (
                  <label className="settings-field">
                    <span className="settings-label">
                      {t('settings.rigControl.icomNative.label')}
                    </span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.icomNativeCat ?? false}
                      className={`toggle${form.icomNativeCat ? ' on' : ''}`}
                      disabled={civBlocked !== null}
                      onClick={() => updateBool('icomNativeCat', !form.icomNativeCat)}
                    >
                      <span className="toggle-knob" />
                    </button>
                    <span className="settings-hint">
                      {civBlocked === 'network' ? (
                        'Not available on a network connection: the CI-V engine speaks to the radio over its serial port, and a LAN-connected radio has none for Nexus to open. Connect this radio by USB to use it.'
                      ) : civBlocked === 'omnirig' ? (
                        'Not available through OmniRig: OmniRig holds the COM port, so Nexus cannot open it to speak CI-V.'
                      ) : (
                        <T k="settings.rigControl.icomNative.hint" tags={{ b: <strong /> }} />
                      )}
                    </span>
                  </label>
                )}

              {/* WHICH Icom data mode. Only for the radios that have more than one, and only
                  through the native CI-V engine — Hamlib's PKT modes always select D1, so
                  offering the choice on that path would be a control that does nothing. */}
              {MULTI_DATA_MODE_ICOMS.includes(form.rigModel) && (
                <label className="settings-field">
                  <span className="settings-label">Data mode</span>
                  <select
                    className="settings-input"
                    value={String(form.icomDataMode ?? 1)}
                    disabled={!form.icomNativeCat}
                    onChange={(e) => updateNum('icomDataMode', Number(e.target.value))}
                  >
                    <option value="1">D1</option>
                    <option value="2">D2</option>
                    <option value="3">D3</option>
                  </select>
                  <span className="settings-hint">
                    {form.icomNativeCat
                      ? 'Which DATA mode this radio is put into for digital operating. Pick the one your USB audio is wired to — D1 unless you changed it on the radio. Needs a bench check on a real radio before it is trusted.'
                      : 'Needs the native CI-V connection above: through Hamlib the radio always lands on D1.'}
                  </span>
                </label>
              )}

              {/* SmartSDR-only streams, gated on the MODEL NUMBER — never on the model's name.
                  A name-sniff for "flex" also matched the PowerSDR entry, offering an ANAN/HL2
                  operator a SmartSDR VITA-49 stream that radio cannot serve. `native_spectrum_kind`
                  in Rust has always gated on 2036|23005; this is the UI catching up. */}
              {form.rigConn === 'network' &&
                ([2036, 23005].includes(form.rigModel) || (form.flexRadioIp ?? '').trim() !== '') && (
                  <label className="settings-field">
                    <span className="settings-label">
                      {t('settings.rigControl.flexPan.label')}
                    </span>
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
                      <T k="settings.rigControl.flexPan.hint" tags={{ b: <strong /> }} />
                    </span>
                  </label>
                )}

              {/* Same model-number gate as the panadapter above — see the note there. */}
              {form.rigConn === 'network' &&
                ([2036, 23005].includes(form.rigModel) || (form.flexRadioIp ?? '').trim() !== '') && (
                  <label className="settings-field">
                    <span className="settings-label">
                      {t('settings.rigControl.flexAudio.label')}
                    </span>
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
                      <T k="settings.rigControl.flexAudio.hint" tags={{ b: <strong /> }} />
                    </span>
                  </label>
                )}

              {form.rigConn !== 'network' &&
                /IC-?\s?(7300|7610|9700|705|905)\b/i.test(form.rigModelName ?? '') &&
                form.icomNativeCat && (
                  <label className="settings-field">
                    <span className="settings-label">{t('settings.rigControl.civLog.label')}</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={civLogPath !== null}
                      className={`toggle${civLogPath !== null ? ' on' : ''}`}
                      onClick={() =>
                        void withErrorToast(
                          () => civDiagnosticLog(civLogPath === null),
                          t('settings.rigControl.civLog.failed'),
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
                        <T
                          k="settings.rigControl.civLog.recording"
                          tags={{ b: <strong />, code: <code /> }}
                          vals={{ path: civLogPath }}
                        />
                      ) : (
                        t('settings.rigControl.civLog.idle')
                      )}
                    </span>
                  </label>
                )}

              {(form.rigModel === 2036 || form.rigModel === 23005) && (
                <label className="settings-field">
                  <span className="settings-label">{t('settings.rigControl.flexIp.label')}</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.flexRadioIp}
                    placeholder="e.g. 192.168.1.50"
                    onChange={(e) => update('flexRadioIp', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">
                    <T k="settings.rigControl.flexIp.hint" tags={{ em: <em /> }} />
                  </span>
                </label>
              )}

              {/* The CAT-broker toggle/port/PTT that lived here moved into the "Share this
                  radio with other programs" block below (#53) — ONE share affordance. The
                  port stays editable here for the rare collision, nothing else. */}
              {form.catBroker && (
                <label className="settings-field">
                  <span className="settings-label">{t('settings.rigControl.sharingPort.label')}</span>
                  <input
                    className="settings-input"
                    type="number"
                    inputMode="numeric"
                    value={String(form.catBrokerPort)}
                    placeholder="4532"
                    onChange={(e) => update('catBrokerPort', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">
                    {t('settings.rigControl.sharingPort.hint')}
                  </span>
                </label>
              )}
            </SettingsGroup>
            <div className="settings-cat-test">
              <button
                type="button"
                className="settings-testcat"
                onClick={handleTestCat}
                disabled={catTesting}
                title={t('settings.rigControl.testCat.title')}
              >
                {catTesting
                  ? t('settings.rigControl.testCat.testing')
                  : t('settings.rigControl.testCat.action')}
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
          </fieldset>

          {/* AUDIO SITS HERE, DIRECTLY UNDER THE CAT ROWS, AND THAT PLACEMENT IS THE POINT.
              Picking the COM port and picking the sound card are not two topics — on a
              single-cable interface (Digirig, IC-705, FT-891) they are the same cable, set in
              the same minute. They used to be 1,129 lines and six fieldsets apart, with the
              satellite and rotator stack wedged between them, so an operator setting up a rig
              had to scroll past hardware they may not own to finish the job they started.
              Every established program in the hobby puts Radio and Audio adjacent (WSJT-X's
              Radio/Audio tabs, fldigi's Rig Control/Soundcard siblings) — an operator arriving
              from one of them expects it. Keep them together. */}
          {(form.radios?.length ?? 1) > 1 && (
            <div className="radio-config-banner">
              <T
                k="settings.audio.multiRadio.note"
                tags={{ b: <strong /> }}
                vals={{
                  radio:
                    form.radios?.find((r) => r.id === editingRadioId)?.name ??
                    t('settings.audio.multiRadio.selectedRadio'),
                }}
              />
            </div>
          )}
          <fieldset className="settings-section" id="settings-audio">
            <legend>{t('settings.audio.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.audio.input.label')}</span>
                <div className="settings-input-row">
                  <select
                    className="settings-input"
                    value={form.audioIn}
                    onChange={(e) => update('audioIn', e.target.value)}
                  >
                    <option value="">{t('settings.audio.device.systemDefault')}</option>
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
                    title={t('settings.audio.refresh.title')}
                  >
                    {audioLoading ? '…' : t('settings.audio.refresh.action')}
                  </button>
                </div>
                <span className="settings-hint">{t('settings.audio.input.hint')}</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.audio.output.label')}</span>
                <select
                  className="settings-input"
                  value={form.audioOut}
                  onChange={(e) => update('audioOut', e.target.value)}
                >
                  <option value="">{t('settings.audio.device.systemDefault')}</option>
                  {audioOutOptions.map((d) => (
                    <option key={d} value={d}>
                      {audioLabel(d, 'output')}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">{t('settings.audio.output.hint')}</span>
              </label>

              <div className="settings-field settings-audio-scope">
                <span className="settings-label">{t('settings.audio.spectrum.label')}</span>
                <MiniSpectrum height={84} idleHint={t('settings.audio.spectrum.idle')} />
                <span className="settings-hint">{t('settings.audio.spectrum.hint')}</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.audio.txPower.label')}{' '}
                  <span className="settings-value">{Math.round(form.txLevel * 100)}%</span>
                </span>
                <input
                  className="settings-slider"
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  // Slider POSITION is not tx_level directly: position^2 -> level, sqrt(level)
                  // -> position. The real hardware-usable drive range (0 up to just past where
                  // ALC engages) sits in only the bottom ~15-20% of a linear slider, so a
                  // square-law curve spreads that critical region across most of the travel.
                  // The stored/persisted tx_level meaning is completely unchanged - only how
                  // far the slider has to travel to reach a given level. Rounded to the 0.01
                  // step grid: an unrounded sqrt() rarely lands on a step boundary, and a
                  // range input with a step-mismatched value fails HTML5 constraint validation
                  // - which silently blocks the WHOLE settings form's submit, not just this
                  // field, the moment this component mounts with a level that doesn't happen
                  // to have a perfect-square root.
                  value={String(Math.round(Math.sqrt(form.txLevel) * 100) / 100)}
                  onChange={(e) => {
                    const level = Number(e.target.value) ** 2
                    updateNum('txLevel', level)
                    applyTxLevelLive(level)
                  }}
                  onPointerUp={(e) =>
                    applyTxLevelLive(Number((e.target as HTMLInputElement).value) ** 2, true)
                  }
                  onKeyUp={(e) =>
                    applyTxLevelLive(Number((e.target as HTMLInputElement).value) ** 2, true)
                  }
                  aria-label={t('settings.audio.txPower.aria')}
                />
                <span className="settings-hint">
                  <T k="settings.audio.txPower.hint" tags={{ b: <strong />, em: <em /> }} />
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">
                  {t('settings.audio.rxLevel.label')}{' '}
                  {/* Live 100 ms poll (lock-free backend) — setting a gain against a needle
                      that answered 0.5–0.8 s late made level-setting guesswork. */}
                  <span className="settings-value"><LiveRxLevelDb /></span>
                </span>
                <LiveLevelMeter label={t('settings.audio.rxLevel.meter')} variant="full" />
                <span className="settings-hint">{t('settings.audio.rxLevel.hint')}</span>
                {radio?.audioError && (
                  <span className="cat-result fail" role="alert">✗ {radio.audioError}</span>
                )}
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.audio.rxGain.label')}{' '}
                  <span className="settings-value">×{(form.rxGain ?? 1).toFixed(1)}</span>
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
                  aria-label={t('settings.audio.rxGain.aria')}
                />
                <span className="settings-hint">{t('settings.audio.rxGain.hint')}</span>
              </label>
            </div>
          </fieldset>

          <fieldset className="settings-section" id="settings-headphone-monitor">
            <legend>{t('settings.headphoneMonitor.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.headphoneMonitor.enable.label')}
                </span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!!form.monitorEnabled}
                    onChange={(e) => updateBool('monitorEnabled', e.target.checked)}
                    aria-label={t('settings.headphoneMonitor.enable.aria')}
                  />
                  <span className="settings-hint">
                    {t('settings.headphoneMonitor.enable.hint')}
                  </span>
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.headphoneMonitor.device.label')}
                </span>
                <select
                  className="settings-input"
                  value={form.monitorDevice ?? ''}
                  onChange={(e) => update('monitorDevice', e.target.value)}
                  disabled={!form.monitorEnabled}
                >
                  <option value="">{t('settings.audio.device.systemDefault')}</option>
                  {monitorOutOptions.map((d) => (
                    <option key={d} value={d}>
                      {audioLabel(d, 'output')}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  {t('settings.headphoneMonitor.device.hint')}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.headphoneMonitor.level.label')}{' '}
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
                  aria-label={t('settings.headphoneMonitor.level.aria')}
                />
                <span className="settings-hint">
                  {t('settings.headphoneMonitor.level.hint')}
                </span>
              </label>
            </div>
          </fieldset>

          {/* ---- Satellite Doppler + rotator manners (Phase 1 sat station) ---- */}
          <fieldset className="settings-section" id="settings-satellite-doppler">
            <legend>{t('settings.satelliteDoppler.legend')}</legend>
            <p className="settings-note">{t('settings.satelliteDoppler.note')}</p>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.satelliteDoppler.enable.label')}
                </span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!form.satDopplerOff}
                    onChange={(e) => updateBool('satDopplerOff', !e.target.checked)}
                    aria-label={t('settings.satelliteDoppler.enable.aria')}
                  />
                  <span className="settings-hint">
                    {t('settings.satelliteDoppler.enable.hint')}
                  </span>
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.satelliteDoppler.vfoMap.label')}
                </span>
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
                      ? t('settings.satelliteDoppler.vfoMap.otherRadio')
                      : undefined
                  }
                  onChange={(e) => setSatVfoMap(e.target.value as NonNullable<Settings['satVfoMap']>)}
                  aria-label={t('settings.satelliteDoppler.vfoMap.aria')}
                >
                  {SAT_VFO_MAPS.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">
                  <T k="settings.satelliteDoppler.vfoMap.hint" tags={{ b: <strong /> }} />
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.satelliteDoppler.minShift.label')}
                </span>
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
                  aria-label={t('settings.satelliteDoppler.minShift.aria')}
                />
                <span className="settings-hint">
                  {t('settings.satelliteDoppler.minShift.hint')}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.satelliteDoppler.interval.label')}
                </span>
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
                  aria-label={t('settings.satelliteDoppler.interval.aria')}
                />
                <span className="settings-hint">
                  {t('settings.satelliteDoppler.interval.hint')}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">
                  {t('settings.satelliteDoppler.passSounds.label')}
                </span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!form.satPassAlertSoundOff}
                    onChange={(e) => updateBool('satPassAlertSoundOff', !e.target.checked)}
                    aria-label={t('settings.satelliteDoppler.passSounds.aria')}
                  />
                  <span className="settings-hint">
                    {t('settings.satelliteDoppler.passSounds.hint')}
                  </span>
                </span>
              </label>
            </div>
          </fieldset>

          {/* ---- Orbital elements (the TLE currency pipeline's operator surface;
                  cloned from the FCC callsign→state fieldset) ---- */}
          <fieldset className="settings-section" id="settings-orbital-elements">
            <legend>{t('settings.orbitalElements.legend')}</legend>
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
                  {tleFetching
                    ? t('settings.orbitalElements.update.busy')
                    : t('settings.orbitalElements.update.action')}
                </button>
                <button
                  type="button"
                  className="settings-test-btn"
                  disabled={tleImporting}
                  onClick={() => tleFileRef.current?.click()}
                  title={t('settings.orbitalElements.import.title')}
                >
                  {tleImporting
                    ? t('settings.orbitalElements.import.busy')
                    : t('settings.orbitalElements.import.action')}
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
                          t('settings.orbitalElements.import.ok', {
                            imported: st.importedCount,
                            total: st.count,
                          }),
                          'success',
                          5000,
                        )
                      })
                      .catch((err) =>
                        pushToast(
                          t('settings.orbitalElements.import.failed', {
                            error: `${err instanceof Error ? err.message : err}`,
                          }),
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
                    ? // A LIST of independent chips, not a sentence — the shape
                      // `elementBandSummary` already uses, so its parts drop straight in.
                      // `source` is the backend's one-word provenance and stays a token.
                      [
                        t('settings.orbitalElements.status.birds', { count: tleStatus.count }),
                        ...elementBandParts(tleStatus),
                        tleStatus.fetchedAt > 0
                          ? t('settings.orbitalElements.status.fetched', {
                              date: new Date(tleStatus.fetchedAt * 1000)
                                .toISOString()
                                .slice(0, 10),
                            })
                          : t('settings.orbitalElements.status.neverFetched'),
                        tleStatus.source,
                        ...(tleStatus.importedCount > 0
                          ? [
                              t('settings.orbitalElements.status.imported', {
                                count: tleStatus.importedCount,
                              }),
                            ]
                          : []),
                      ].join(' · ')
                    : t('settings.orbitalElements.status.empty')}
                </span>
              </div>
              <span className="settings-hint">{t('settings.orbitalElements.hint')}</span>
              {tleStatus?.lastError && (
                // Operator words in the line, the raw error in the tooltip —
                // during the pre-launch window the mirror 404s by design,
                // and "HTTP 404" is not a thing to hand an operator.
                <span className="settings-hint" title={tleRefreshMessage(tleStatus).raw}>
                  {t('settings.orbitalElements.lastRefresh', {
                    detail: tleRefreshMessage(tleStatus).text,
                  })}
                </span>
              )}
            </div>
          </fieldset>

          <fieldset className="settings-section" id="settings-rotator">
            <legend>{t('settings.rotator.legend')}</legend>
            <p className="settings-note">{t('settings.rotator.note')}</p>
            <div className="settings-grid">
              {/* THE MODEL AND ITS PORT LIVE HERE NOW. They were inside Rig &amp; CAT, which
                  meant the one affordance the app has for a silent rotator — the cockpit's
                  "Rotator not answering" chip, whose whole job is the model/port — deep-linked
                  to a section that contained neither, and a Settings search for "rotator"
                  landed on the pointing manners alone (rotor review 2026-08-18, findings
                  26/32/42/50). The registry says where a setting lives; this is where the
                  registry always said the rotator lived. */}
              <div className="settings-field">
                <span className="settings-label">{t('settings.rotator.model.label')}</span>
                {(() => {
                  const model = form.rotatorModel ?? 0
                  const curated = new Set(['0', ...ROTATOR_MODELS.map((r) => String(r.model))])
                  const modelStr = String(model)
                  const isOther = rotOther || !curated.has(modelStr)
                  return (
                    <>
                      <select
                        value={isOther ? 'other' : modelStr}
                        onChange={(e) => {
                          const v = e.target.value
                          if (v === 'other') {
                            setRotOther(true)
                            setRotCustom(model > 0 ? String(model) : '')
                          } else {
                            setRotOther(false)
                            selectRotator(Number(v))
                          }
                        }}
                        aria-label={t('settings.rotator.model.label')}
                      >
                        <option value="0">{t('settings.rotator.model.none')}</option>
                        {ROTATOR_MODELS.map((r) => (
                          <option key={r.model} value={r.model}>
                            {r.label}
                          </option>
                        ))}
                        <option value="other">{t('settings.rotator.model.other')}</option>
                      </select>
                      {isOther && (
                        <input
                          className="settings-input"
                          type="number"
                          min="1"
                          placeholder={t('settings.rotator.model.number.placeholder')}
                          // Falls back to the CONFIGURED model rather than staying blank: the
                          // box is local state seeded only when the operator picks "Other" by
                          // hand, so re-opening Settings — or switching radios — used to render
                          // an empty field over a perfectly good saved model number.
                          value={rotCustom || (model > 0 ? String(model) : '')}
                          onChange={(e) => {
                            setRotCustom(e.target.value)
                            const n = Number(e.target.value)
                            // Only ever commit a REAL model; an incomplete
                            // entry leaves the last valid value in the form.
                            if (Number.isInteger(n) && n > 0) selectRotator(n)
                          }}
                          aria-label={t('settings.rotator.model.number.aria')}
                        />
                      )}
                    </>
                  )
                })()}
                <span className="settings-hint">{t('settings.rotator.model.hint')}</span>
              </div>

              {(form.rotatorModel ?? 0) > 1 && (
                <div className="settings-field">
                  <span className="settings-label">{t('settings.rotator.port.label')}</span>
                  <div className="settings-inline-pair">
                    <input
                      className="settings-input"
                      type="text"
                      value={form.rotatorPort ?? ''}
                      placeholder={IS_MAC ? ROTATOR_EXAMPLES.macPort : ROTATOR_EXAMPLES.port}
                      onChange={(e) => update('rotatorPort', e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                      aria-label={t('settings.rotator.port.aria')}
                    />
                    <input
                      className="settings-input"
                      type="number"
                      value={form.rotatorBaud ?? 9600}
                      onChange={(e) => {
                        const n = Number(e.target.value)
                        if (!Number.isNaN(n)) updateNum('rotatorBaud', n)
                      }}
                      aria-label={t('settings.rotator.baud.aria')}
                      title={t('settings.rotator.baud.title')}
                    />
                  </div>
                  {/* The old hint here said "GS-232 default 9600" to EVERY model, which is how
                      a SPID owner (600 baud) was told the number that kills his rotator was
                      right. It now names THIS model's own declared rate, and says so loudly
                      when the saved value cannot work. */}
                  {(() => {
                    const only = ROT_FIXED_BAUD.get(form.rotatorModel ?? 0)
                    const set = form.rotatorBaud ?? 9600
                    if (only === undefined) {
                      return (
                        <span className="settings-hint">{t('settings.rotator.baud.hint.any')}</span>
                      )
                    }
                    // ⚠️ THE RATES GO IN AS ALREADY-FORMATTED STRINGS. `toLocaleString()` on a
                    // baud rate is the defect the Rig & CAT baud picker has (a German install
                    // reads "9.600"), and it is NOT fixed here: passing the numbers would let
                    // `invariantNumber` render "9600" and change visible English, which this
                    // batch may not do. Both fix together, in their own change.
                    if (only === set) {
                      return (
                        <span className="settings-hint">
                          {t('settings.rotator.baud.hint.fixed', { rate: only.toLocaleString() })}
                        </span>
                      )
                    }
                    return (
                      <span className="settings-hint settings-warn">
                        <T
                          k="settings.rotator.baud.hint.wrong"
                          tags={{ b: <strong /> }}
                          vals={{ rate: only.toLocaleString(), set: set.toLocaleString() }}
                        />
                      </span>
                    )
                  })()}
                </div>
              )}

              <div className="settings-field">
                <span className="settings-label">{t('settings.rotator.external.label')}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.rotatorHost}
                  placeholder={t('settings.rotator.external.placeholder', {
                    example: ROTATOR_EXAMPLES.host,
                  })}
                  onChange={(e) => update('rotatorHost', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                  aria-label={t('settings.rotator.external.aria')}
                />
                <span className="settings-hint">{t('settings.rotator.external.hint')}</span>
              </div>

              <div className="settings-field">
                <span className="settings-label">{t('settings.rotator.park.label')}</span>
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
                    aria-label={t('settings.rotator.park.az.aria')}
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
                    aria-label={t('settings.rotator.park.el.aria')}
                  />
                </div>
                <span className="settings-hint">{t('settings.rotator.park.hint')}</span>
              </div>

              <div className="settings-field">
                <span className="settings-label">{t('settings.rotator.ready.label')}</span>
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
                    aria-label={t('settings.rotator.ready.az.aria')}
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
                    aria-label={t('settings.rotator.ready.el.aria')}
                  />
                </div>
                <span className="settings-hint">{t('settings.rotator.ready.hint')}</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">{t('settings.rotator.postPass.label')}</span>
                <select
                  className="settings-input"
                  value={form.rotPostPass ?? 'stop'}
                  onChange={(e) => update('rotPostPass', e.target.value)}
                  aria-label={t('settings.rotator.postPass.aria')}
                >
                  {ROT_POST_PASS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {t(o.labelKey)}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">{t('settings.rotator.postPass.hint')}</span>
              </label>

              <div className="settings-field">
                <span className="settings-label">{t('settings.rotator.tolerance.label')}</span>
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
                    aria-label={t('settings.rotator.tolerance.az.aria')}
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
                    aria-label={t('settings.rotator.tolerance.el.aria')}
                  />
                </div>
                <span className="settings-hint">{t('settings.rotator.tolerance.hint')}</span>
              </div>

              <div className="settings-field">
                <span className="settings-label">{t('settings.rotator.calibration.label')}</span>
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
                    aria-label={t('settings.rotator.calibration.az.aria')}
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
                    aria-label={t('settings.rotator.calibration.el.aria')}
                  />
                </div>
                <span className="settings-hint">{t('settings.rotator.calibration.hint')}</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">{t('settings.rotator.flip.label')}</span>
                <span className="settings-input-row">
                  <input
                    type="checkbox"
                    checked={!!form.rotAllowFlip}
                    onChange={(e) => updateBool('rotAllowFlip', e.target.checked)}
                    aria-label={t('settings.rotator.flip.aria')}
                  />
                  <span className="settings-hint">
                    <T k="settings.rotator.flip.hint" tags={{ b: <strong /> }} />
                  </span>
                </span>
              </label>
            </div>
          </fieldset>

          {/* Everything Rig Control accepted because its NAME had no exclusion criterion. A
              subsystem heading in a radio app admits anything rig-adjacent, so "Rig Control"
              had grown to hold the rotator, band-edge tones, per-mode power caps, the setup
              backup, rig sharing and foreign-PTT permission — 1,011 lines, most of which an
              operator hunting a COM port had to scroll past. These are real settings and they
              keep working exactly as before; they are transmit POLICY and station plumbing, not
              the CAT link, so they get a heading that says so and stop burying it. */}
          <fieldset className="settings-section" id="settings-transmit-limits">
            <legend>{t('settings.transmit.legend')}</legend>
            {/* Band-edge tones live with the RIG, not with Digital: useBandEdgeTones is
                called at App top level off snap.radio.txAllowed (App.tsx:739), so the cue
                fires on phone and CW exactly as it does on FT8. It was only ever filed
                under Digital by accident. */}
            <div className="settings-field">
              <label className="settings-toggle">
                <span className="settings-label">{t('settings.transmit.bandEdgeTones.label')}</span>
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
              <span className="settings-hint">{t('settings.transmit.bandEdgeTones.hint')}</span>
            </div>

            <div className="settings-field">
              <span className="settings-label">{t('settings.transmit.powerCaps.label')}</span>
              <div className="settings-power-caps">
                {/* The three names are MODE names — tokens, exactly as they are everywhere else
                    in this panel — and the cap itself is a percentage. Only the prose moved. */}
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
              <span className="settings-hint">{t('settings.transmit.powerCaps.hint')}</span>
            </div>

            <p className="settings-note">
              <T
                k="settings.transmit.note"
                tags={{ b: <strong />, code: <code />, em: <em /> }}
              />
            </p>

            {/* Sharing the rig with another program (#48, rogerloxton). A serial port is
                exclusive-open, so while Nexus holds it nothing else can — which is why VarAC
                and FreeDV cannot reach the radio and why the answer looks like "quit Nexus".
                But Nexus does not hoard the rig: it drives it through Hamlib's rigctld, which
                is a SERVER, and VarAC, FreeDV, WSJT-X, JS8Call, N1MM and fldigi all speak that
                protocol as "Hamlib NET rigctl". Pointing them here shares the radio live, both
                programs at once, no swapping cables and nothing to release.

                The address existed all along — `rigctld_port` is per-radio and validated
                unique — and was simply never shown to anyone. That is the whole defect. */}
            {/* Back up / restore the whole station (#28 item 4). Until now there was no way
                to keep a copy of any of this: settings.json sits in a config folder most
                operators never open, and the honest answer to "how do I move to a new laptop"
                was to go and find it. */}
            <div className="settings-field">
              <span className="settings-label">{t('settings.transmit.backup.label')}</span>
              <div className="rig-share-row">
                <button
                  type="button"
                  className="settings-linkbtn"
                  onClick={() =>
                    withErrorToast(async () => {
                      const text = await exportSettingsBundle()
                      const stamp = new Date().toISOString().slice(0, 10)
                      // The FILE NAME is invariant (batch 8): a translated word in it would
                      // reduce a non-Latin locale's backup to `nexus-settings-.json`.
                      const path = await saveTextToDownloads(`nexus-settings-${stamp}.json`, text)
                      pushToast(t('settings.transmit.backup.done', { path }), 'success')
                    }, t('settings.transmit.backup.failed'))
                  }
                  title={t('settings.transmit.backup.title')}
                >
                  {t('settings.transmit.backup.action')}
                </button>
                <input
                  ref={backupFileRef}
                  type="file"
                  accept=".json,application/json"
                  style={{ display: 'none' }}
                  onChange={onRestoreBackup}
                />
                <button
                  type="button"
                  className="settings-linkbtn"
                  onClick={() => backupFileRef.current?.click()}
                  title={t('settings.transmit.restore.title')}
                >
                  {t('settings.transmit.restore.action')}
                </button>
              </div>
              <span className="settings-hint">
                <T k="settings.transmit.backup.hint" tags={{ b: <strong /> }} />
              </span>
            </div>

            {/* Start over. Deliberately the LAST thing in this block: an operator who arrives
                here wanting a clean slate reads the backup affordance on the way past. */}
            <div className="settings-field">
              <span className="settings-label">{t('settings.transmit.reset.label')}</span>
              <div className="rig-share-row">
                <button
                  type="button"
                  className="settings-linkbtn danger"
                  onClick={handleResetConfig}
                  title={t('settings.transmit.reset.title')}
                >
                  {t('settings.transmit.reset.action')}
                </button>
              </div>
              <span className="settings-hint">
                <T k="settings.transmit.reset.hint" tags={{ b: <strong /> }} />
              </span>
            </div>

            {/* THE share affordance (#53) — one block, and it advertises the CAT BROKER, not the
                Hamlib daemon's port. The daemon is torn down by Test CAT and every CAT-config
                save (eleven trigger fields), and rogerloxton's VarAC log shows what a shared
                client sees when its server restarts or stalls behind our serial polling: empty
                reads and doubled replies. The broker is Nexus itself answering from live state —
                microseconds, and it survives every reconfiguration. The Advanced broker toggle
                that used to live separately is merged here: two share affordances meant neither
                told the whole story. */}
            <div className="settings-field">
              <span className="settings-label">{t('settings.transmit.share.label')}</span>
              <div className="rig-share-row">
                <button
                  type="button"
                  role="switch"
                  aria-checked={form.catBroker}
                  className={`toggle${form.catBroker ? ' on' : ''}`}
                  onClick={() => updateBool('catBroker', !form.catBroker)}
                >
                  <span className="toggle-knob" />
                </button>
                {form.catBroker && (
                  <>
                    <code className="rig-share-addr mono">
                      127.0.0.1:{form.catBrokerPort || 4532}
                    </code>
                    <button
                      type="button"
                      className="settings-linkbtn"
                      onClick={() => {
                        void navigator.clipboard
                          ?.writeText(`127.0.0.1:${form.catBrokerPort || 4532}`)
                          .catch(() => {})
                      }}
                      title={t('settings.transmit.share.copy.title')}
                    >
                      {t('settings.transmit.share.copy.action')}
                    </button>
                  </>
                )}
              </div>
              {/* The PROGRAM NAMES (VarAC, FreeDV, WSJT-X, JS8Call, fldigi) and the rig-model
                  name `Hamlib NET rigctl` are the other software's own, so they are written in
                  the sentence and emphasised by the markers this call site supplies. */}
              {form.catBroker ? (
                <span className="settings-hint">
                  <T k="settings.transmit.share.hint.on" tags={{ b: <strong />, em: <em /> }} />
                </span>
              ) : (
                <span className="settings-hint">
                  <T k="settings.transmit.share.hint.off" tags={{ em: <em /> }} />
                </span>
              )}
              {form.catBroker && (form.radios?.length ?? 0) > 1 && (
                <span className="settings-hint">
                  <T
                    k="settings.transmit.share.hint.direct"
                    tags={{
                      em: <em />,
                      code: <code className="rig-share-direct mono" />,
                    }}
                    vals={{ address: `127.0.0.1:${form.rigctldPort || 4532}` }}
                  />
                </span>
              )}
              {form.catBroker && (
                <div className="settings-field">
                  <span className="settings-label">{t('settings.transmit.foreignPtt.label')}</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.catBrokerPtt ?? false}
                    className={`toggle${form.catBrokerPtt ? ' on' : ''}`}
                    onClick={() => updateBool('catBrokerPtt', !form.catBrokerPtt)}
                  >
                    <span className="toggle-knob" />
                  </button>
                  <span className="settings-hint">{t('settings.transmit.foreignPtt.hint')}</span>
                </div>
              )}
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Audio ---- */}

          {/* ---- Digital (FT8/FT4) — was "Operating"; ~90% FT8 sequencing/decoder knobs ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-digital-ft8-ft4">
            <legend>{t('settings.digital.legend')}</legend>
            {/* ⚠️ THE NEXT TWO SUB-GROUPS ARE DELIBERATELY NOT MIGRATED, and stay in English
                here until the transmit-path batch moves them. "Transmit & Sequencing" and
                "Auto-CQ & Caller Selection" are the FT-mode TX / timing / QSO-management
                surface — the T/R period, the TX watchdog, disable-after-73, double-click-arms-
                TX, the tune timeout, the CQ budget, the blocked-caller list and the best-caller
                pick. Every label, hint and accessible name in them moves in its own batch with
                the stop-line sweeps re-run, deliberately, because WSJT-X parity here is a
                compatibility contract that cannot be checked in CI. Everything BELOW them —
                Logging Behavior, Decoder, Station Housekeeping — is in the catalog. */}
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
                    How many unanswered CQs before Nexus pauses. Default 8, then it waits and calls
                    again — a run that nobody answers stops holding the frequency. Blank = WSJT-X
                    behaviour: CQ repeats until you stop it, with the TX watchdog as the only
                    backstop. The Tempo chat CQ run always stops (default 10 unanswered) — this
                    number overrides that budget too.
                  </span>
                </div>

                <div className="settings-field">
                  <label>
                    <span className="settings-label">Wait before calling CQ again</span>
                    <input
                      className="settings-input"
                      type="number"
                      min={0}
                      max={3600}
                      value={form.cqPauseSecs ?? ''}
                      placeholder="180"
                      onChange={(e) => updateNullableNum('cqPauseSecs', e.target.value, 0)}
                    />
                  </label>
                  <span className="settings-hint">
                    Seconds off the air after an unanswered run, before the next one starts.
                    Default 180 (three minutes). 0 = do not resume: the run just stops, which is
                    what happened before this setting existed. You are still LISTENING through the
                    pause — a station that calls you is worked as normal, and answering anyone
                    resets the count, so a busy run never pauses at all.
                  </span>
                </div>

                <div className="settings-field">
                  <label>
                    <span className="settings-label">Blocked callsigns</span>
                    <input
                      className="settings-input"
                      type="text"
                      value={blockedText ?? (form.blockedCalls ?? []).join(' ')}
                      placeholder="none — e.g. PD2BS K1ABC"
                      onChange={(e) => setBlockedText(e.target.value)}
                      onBlur={() => {
                        if (blockedText == null) return
                        const calls = blockedText
                          .split(/[\s,;]+/)
                          .map((c) => c.trim().toUpperCase())
                          .filter((c) => c.length > 0)
                        void apiSetBlockedCalls(calls)
                          .then(() => {
                            setForm((prev) => (prev ? { ...prev, blockedCalls: calls } : prev))
                            setBlockedText(null)
                          })
                          .catch(() => {})
                      }}
                    />
                  </label>
                  <span className="settings-hint">
                    Stations your auto-responder must never answer when they reply to your CQ —
                    they are passed over for the next caller, and shown dimmed (or hidden) in the
                    roster and Band Activity. Base call matched, so PD2BS also blocks PD2BS/P.
                    Alt-double-click any decode or roster row does the same thing. Saved as you
                    leave the field.
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
              <span className="settings-featgroup-title">{t('settings.digital.logging.title')}</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.autoLog.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.autoLog.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.promptToLog.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.promptToLog.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.preferRrr.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.preferRrr.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.clearDxAfterLog.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.clearDxAfterLog.hint')}</span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.digital.decoder.title')}</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <span className="settings-label">{t('settings.digital.decodeDepth.label')}</span>
                  {/* The chip row is a radio group whose accessible name IS the field label —
                      one entry, read twice, rather than two identical ones that could drift. */}
                  <div
                    className="theme-switcher"
                    role="group"
                    aria-label={t('settings.digital.decodeDepth.label')}
                  >
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
                        {d === 1
                          ? t('settings.digital.decodeDepth.fast')
                          : d === 2
                            ? t('settings.digital.decodeDepth.normal')
                            : t('settings.digital.decodeDepth.deep')}
                      </button>
                    ))}
                  </div>
                  <span className="settings-hint">{t('settings.digital.decodeDepth.hint')}</span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">{t('settings.digital.passband.label')}</span>
                  <div className="settings-input-row">
                    <label className="settings-inline-label">
                      <span>{t('settings.digital.passband.low')}</span>
                      <input
                        id="decode-flow"
                        className="settings-input"
                        type="number"
                        inputMode="numeric"
                        min={200}
                        max={2900}
                        step={1}
                        value={form.decodeFLowHz ?? 200}
                        aria-label={t('settings.digital.passband.low.aria')}
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
                      <span>{t('settings.digital.passband.high')}</span>
                      <input
                        id="decode-fhigh"
                        className="settings-input"
                        type="number"
                        inputMode="numeric"
                        min={200}
                        max={4000}
                        step={1}
                        value={form.decodeFHighHz ?? 2900}
                        aria-label={t('settings.digital.passband.high.aria')}
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
                  <span className="settings-hint">{t('settings.digital.passband.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.apDecode.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.apDecode.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.apCqOnly.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.apCqOnly.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.singleDecode.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.singleDecode.hint')}</span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">{t('settings.digital.dxpedition.label')}</span>
                  <div
                    className="theme-switcher"
                    role="group"
                    aria-label={t('settings.digital.dxpedition.label')}
                  >
                    {([
                      { value: 'none' as const, label: t('settings.digital.dxpedition.off') },
                      // ⚠️ "Hound" stays in the code: it is WSJT-X's own name for the calling
                      // side of a DXpedition QSO, a role token exactly as Fox is, and it names
                      // nothing once translated (i18n/index.ts, the invariant-token rule).
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
                  <span className="settings-hint">{t('settings.digital.dxpedition.hint')}</span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">
                {t('settings.digital.housekeeping.title')}
              </span>
              <div className="settings-grid">
                {/* ⚠️ The beacon toggle is a CONFIGURATION control, not a transmit control: it
                    chooses whether the station announces itself, and it can neither key, unkey
                    nor stop anything. Same reading as Tx Power's drive slider — nothing in this
                    sub-group is on any cockpit's stop-line census. */}
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.digital.journeyStreak.label')}
                    </span>
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
                  <span className="settings-hint">{t('settings.digital.journeyStreak.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.beacon.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.beacon.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.harq.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.harq.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.digital.clockCheck.label')}</span>
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
                  <span className="settings-hint">{t('settings.digital.clockCheck.hint')}</span>
                </div>

                {/* The placeholder below is a POWER in watts — a token, so it stays written
                    here exactly as the example addresses and device names in Rig & CAT do. */}
                <div className="settings-field">
                  <label className="settings-label" htmlFor="station-power">
                    {t('settings.digital.stationPower.label')}
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
                  <span className="settings-hint">{t('settings.digital.stationPower.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-label" htmlFor="units">
                    {t('settings.digital.units.label')}
                  </label>
                  <select
                    id="units"
                    className="settings-input"
                    value={form.units ?? 'auto'}
                    onChange={(e) => update('units', e.target.value)}
                  >
                    <option value="auto">{t('settings.digital.units.auto')}</option>
                    <option value="metric">{t('settings.digital.units.metric')}</option>
                    <option value="imperial">{t('settings.digital.units.imperial')}</option>
                  </select>
                  <span className="settings-hint">{t('settings.digital.units.hint')}</span>
                </div>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- JT65: submode only. One fixed 60 s period, unlike the others. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-jt65">
            <legend>{t('settings.jt65.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.jt65.submode.label')}</span>
                {/* The <option> VALUE is the WSJT-X submode index the settings file stores;
                    only the label is read, and the A/B/C in it is the submode's own name. */}
                <select
                  className="settings-input"
                  value={String(form.jt65Submode ?? 0)}
                  onChange={(e) => updateNum('jt65Submode', Number(e.target.value))}
                >
                  <option value="0">{t('settings.jt65.submode.a')}</option>
                  <option value="1">{t('settings.jt65.submode.b')}</option>
                  <option value="2">{t('settings.jt65.submode.c')}</option>
                </select>
                <span className="settings-hint">{t('settings.jt65.submode.hint')}</span>
              </label>
            </div>
            <span className="settings-hint">{t('settings.jt65.hint')}</span>
          </fieldset>
          )}

          {/* ---- MSK144: meteor scatter. Period is the whole setting. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-msk144">
            <legend>{t('settings.msk144.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.msk144.period.label')}</span>
                {/* "10 s" carries no prose at all — it is a period and its unit, so there is
                    nothing in it to translate and it stays written here. */}
                <select
                  className="settings-input"
                  value={String(form.msk144PeriodS ?? 15)}
                  onChange={(e) => updateNum('msk144PeriodS', Number(e.target.value))}
                >
                  <option value="5">{t('settings.msk144.period.fast')}</option>
                  <option value="10">10 s</option>
                  <option value="15">{t('settings.msk144.period.standard')}</option>
                  <option value="30">{t('settings.msk144.period.sparse')}</option>
                </select>
                <span className="settings-hint">{t('settings.msk144.period.hint')}</span>
              </label>
            </div>
            <span className="settings-hint">{t('settings.msk144.hint')}</span>
          </fieldset>
          )}

          {/* ---- Beacons (WSPR / FST4W). A SEPARATE surface from the QSO modes:
               there is no exchange, only a schedule. Off by default — beaconing
               keys the radio unattended, so it is always an explicit choice. ---- */}
          {tab === 'digital' && (() => {
          // Round Robin is active only with a real rotation: a slot picked AND ≥2
          // stations in it. slots=1 is degenerate (the rotation would claim every
          // interval), so the engine falls back to the transmit-% schedule (#101b).
          const rrActive = (form.beaconRrSlot ?? 0) > 0 && (form.beaconRrSlots ?? 0) >= 2
          const rrDegenerate = (form.beaconRrSlot ?? 0) > 0 && (form.beaconRrSlots ?? 0) < 2
          return (
          <fieldset className="settings-section" id="settings-beacons-wspr-fst4w">
            <legend>{t('settings.beacons.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.beacons.txPercent.label')}</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={100}
                  disabled={rrActive}
                  title={rrActive ? t('settings.beacons.txPercent.title') : undefined}
                  value={String(form.beaconTxPercent ?? 0)}
                  onChange={(e) => updateNum('beaconTxPercent', Number(e.target.value))}
                />
                <span className="settings-hint">
                  {rrActive ? (
                    <T k="settings.beacons.txPercent.hint.roundRobin" tags={{ b: <strong /> }} />
                  ) : (
                    t('settings.beacons.txPercent.hint')
                  )}
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.beacons.power.label')}</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={60}
                  value={String(form.beaconPowerDbm ?? 0)}
                  onChange={(e) => updateNum('beaconPowerDbm', Number(e.target.value))}
                />
                <span className="settings-hint">
                  <T k="settings.beacons.power.hint" tags={{ b: <strong /> }} />
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.beacons.rrSlot.label')}</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={10}
                  value={String(form.beaconRrSlot ?? 0)}
                  onChange={(e) => updateNum('beaconRrSlot', Number(e.target.value))}
                />
                <span className="settings-hint">{t('settings.beacons.rrSlot.hint')}</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.beacons.rrSlots.label')}</span>
                <input
                  className="settings-input"
                  type="number"
                  min={0}
                  max={10}
                  value={String(form.beaconRrSlots ?? 0)}
                  onChange={(e) => updateNum('beaconRrSlots', Number(e.target.value))}
                />
                <span className="settings-hint">
                  {rrDegenerate ? (
                    <T k="settings.beacons.rrSlots.hint.degenerate" tags={{ b: <strong /> }} />
                  ) : (
                    t('settings.beacons.rrSlots.hint')
                  )}
                </span>
              </label>
            </div>
            <span className="settings-hint">{t('settings.beacons.hint')}</span>
          </fieldset>
          )
          })()}

          {/* ---- FST4 / FST4W: one period setting, shared. Same decoder, same
               slot clock; the tier picks QSO vs beacon. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-fst4">
            <legend>{t('settings.fst4.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.fst4.period.label')}</span>
                {/* The five rows with no prose beside them are a period and its unit — nothing
                    in them to translate, so they stay written here. */}
                <select
                  className="settings-input"
                  value={String(form.fst4PeriodS ?? 120)}
                  onChange={(e) => updateNum('fst4PeriodS', Number(e.target.value))}
                >
                  <option value="15">15 s</option>
                  <option value="30">30 s</option>
                  <option value="60">60 s</option>
                  <option value="120">{t('settings.fst4.period.shortestBeacon')}</option>
                  <option value="300">300 s</option>
                  <option value="900">900 s</option>
                  <option value="1800">{t('settings.fst4.period.deepest')}</option>
                </select>
                <span className="settings-hint">{t('settings.fst4.period.hint')}</span>
              </label>
            </div>
            <span className="settings-hint">
              <T k="settings.fst4.hint" tags={{ b: <strong />, code: <code /> }} />
            </span>
          </fieldset>
          )}

          {/* ---- Q65: period + submode. Both change the on-air signal AND the
               decode frame length, so they belong with the mode, not the radio. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-q65">
            <legend>{t('settings.q65.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.q65.period.label')}</span>
                <select
                  className="settings-input"
                  value={String(form.q65PeriodS ?? 60)}
                  onChange={(e) => updateNum('q65PeriodS', Number(e.target.value))}
                >
                  <option value="15">{t('settings.q65.period.tropo')}</option>
                  <option value="30">{t('settings.q65.period.meteor')}</option>
                  <option value="60">{t('settings.q65.period.eme')}</option>
                  <option value="120">{t('settings.q65.period.deepEme')}</option>
                  <option value="300">{t('settings.q65.period.microwaveEme')}</option>
                </select>
                <span className="settings-hint">
                  <T k="settings.q65.period.hint" tags={{ b: <strong /> }} />
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.q65.submode.label')}</span>
                {/* The <option> VALUE is the submode index the settings file stores; only the
                    label is read, and the A…E in it is the submode's own name. */}
                <select
                  className="settings-input"
                  value={String(form.q65Submode ?? 0)}
                  onChange={(e) => updateNum('q65Submode', Number(e.target.value))}
                >
                  <option value="0">{t('settings.q65.submode.a')}</option>
                  <option value="1">{t('settings.q65.submode.b')}</option>
                  <option value="2">{t('settings.q65.submode.c')}</option>
                  <option value="3">{t('settings.q65.submode.d')}</option>
                  <option value="4">{t('settings.q65.submode.e')}</option>
                </select>
                <span className="settings-hint">{t('settings.q65.submode.hint')}</span>
              </label>
            </div>
            <span className="settings-hint">{t('settings.q65.hint')}</span>
          </fieldset>
          )}

          {/* ---- Digital quick-reply macros (moved out of the old Alerts/Macros orphan) ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-quick-reply-macros">
            <legend>{t('settings.quickReply.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.quickReply.chat.label')}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.macros.chat.join(', ')}
                  onChange={(e) => updateMacros('chat', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">{t('settings.quickReply.chat.hint')}</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{MACRO_SET_QSO}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.macros.qso.join(', ')}
                  onChange={(e) => updateMacros('qso', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">{t('settings.quickReply.qso.hint')}</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.quickReply.band.label')}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.macros.band.join(', ')}
                  onChange={(e) => updateMacros('band', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">{t('settings.quickReply.band.hint')}</span>
              </label>
            </div>
          </fieldset>
          )}

          {/* ---- Phone (SSB / FM) ----
               MIGRATED to the string catalog (i18n/hardcoded-strings.test.ts) with the CW tab
               and the rest of the per-mode sections. The <select> VALUES are persisted tokens
               and stay here, as do the CTCSS tones and their unit. */}
          {tab === 'phone' && (
          <fieldset className="settings-section" id="settings-phone">
            <legend>{t('settings.phone.legend')}</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.phone.mode.title')}</span>
              <label className="settings-field">
                <span className="settings-label">{t('settings.phone.mode.label')}</span>
                <select
                  className="settings-input"
                  value={form.phoneMode}
                  onChange={(e) => update('phoneMode', e.target.value)}
                >
                  <option value="ssb">{t('settings.phone.mode.ssb')}</option>
                  <option value="fm">{t('settings.phone.mode.fm')}</option>
                </select>
                <span className="settings-hint">{t('settings.phone.mode.hint')}</span>
              </label>

              {form.phoneMode === 'fm' && (
                <>
                  <label className="settings-field">
                    <span className="settings-label">{t('settings.phone.shift.label')}</span>
                    <select
                      className="settings-input"
                      value={form.rptrShift}
                      onChange={(e) => update('rptrShift', e.target.value)}
                    >
                      <option value="simplex">{t('settings.phone.shift.simplex')}</option>
                      <option value="plus">{t('settings.phone.shift.plus')}</option>
                      <option value="minus">{t('settings.phone.shift.minus')}</option>
                    </select>
                    <span className="settings-hint">{t('settings.phone.shift.hint')}</span>
                  </label>

                  <label className="settings-field">
                    <span className="settings-label">{t('settings.phone.ctcss.label')}</span>
                    <select
                      className="settings-input"
                      value={String(form.ctcssToneHz)}
                      onChange={(e) =>
                        setForm((p) => (p ? { ...p, ctcssToneHz: Number(e.target.value) } : p))
                      }
                    >
                      <option value="0">{t('settings.phone.ctcss.off')}</option>
                      {CTCSS_TONES.map((tone) => (
                        <option key={tone} value={String(tone)}>
                          {tone.toFixed(1)} {UNIT_HZ}
                        </option>
                      ))}
                    </select>
                    <span className="settings-hint">{t('settings.phone.ctcss.hint')}</span>
                  </label>
                </>
              )}
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.phone.mic.title')}</span>
              <label className="settings-field">
                <span className="settings-label">{t('settings.phone.voiceMic.label')}</span>
                <select
                  className="settings-input"
                  value={form.voiceMicDevice ?? ''}
                  onChange={(e) => update('voiceMicDevice', e.target.value)}
                >
                  <option value="">{t('settings.phone.voiceMic.default')}</option>
                  {voiceMicOptions.map((d) => (
                    <option key={d} value={d}>
                      {audioLabel(d, 'input')}
                    </option>
                  ))}
                </select>
                <span className="settings-hint">{t('settings.phone.voiceMic.hint')}</span>
              </label>
              <span className="settings-hint">{t('settings.phone.mic.hint')}</span>
            </div>
          </fieldset>
          )}

          {/* ---- CW — the standalone CW home (keyer + F-key macros) ---- */}
          {tab === 'cw' && (
          <fieldset className="settings-section" id="settings-cw">
            <legend>{t('settings.cw.legend')}</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.cw.keyer.title')}</span>
              <label className="settings-field">
                <span className="settings-label">{t('settings.cw.keyer.label')}</span>
                <select
                  className="settings-input"
                  value={form.cwKeyer ?? 'cat'}
                  onChange={(e) => update('cwKeyer', e.target.value)}
                >
                  <option value="cat">{t('settings.cw.keyer.cat')}</option>
                  <option value="serial">{t('settings.cw.keyer.serial')}</option>
                  <option value="winkeyer">{t('settings.cw.keyer.winkeyer')}</option>
                  <option value="soundcard">{t('settings.cw.keyer.soundcard')}</option>
                </select>
                <span className="settings-hint">
                  <T k="settings.cw.keyer.hint" tags={{ b: <strong /> }} />
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.cw.pitch.label')}</span>
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
                <span className="settings-hint">{t('settings.cw.pitch.hint')}</span>
              </label>
              {/* Gated on its own backend, exactly like the keyline port/line below. Shipped
                  unconditionally through 0.27, which made it the ONLY visible port box under
                  Keyer: an operator on the default `cat` backend filled it in, saved, and
                  nothing keyed — the box he could see belonged to a keyer he had not chosen. */}
              {form.cwKeyer === 'winkeyer' && (
                <label className="settings-field">
                  <span className="settings-label">{t('settings.cw.winkeyerPort.label')}</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.winkeyerPort}
                    placeholder={t('settings.cw.winkeyerPort.placeholder', {
                      example: KEYING_EXAMPLES.winkeyerPort,
                    })}
                    onChange={(e) => update('winkeyerPort', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">{t('settings.cw.winkeyerPort.hint')}</span>
                </label>
              )}
              {form.cwKeyer === 'serial' && (
                <>
                  <label className="settings-field">
                    <span className="settings-label">{t('settings.cw.keyPort.label')}</span>
                    <input
                      className="settings-input"
                      type="text"
                      value={form.cwKeyPort ?? ''}
                      placeholder={t('settings.cw.keyPort.placeholder', {
                        example: KEYING_EXAMPLES.cwKeyPort,
                      })}
                      onChange={(e) => update('cwKeyPort', e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    {/* "US Navigator" was listed here as an example through 0.27. The Timewave
                        Navigator keys through a K1EL WinKey micro, which ignores DTR/RTS
                        entirely — so this hint pointed a Navigator owner at the one backend
                        that cannot drive his hardware, and CW just never keyed. */}
                    <span className="settings-hint">
                      <T k="settings.cw.keyPort.hint" tags={{ b: <strong /> }} />
                    </span>
                  </label>
                  <label className="settings-field">
                    <span className="settings-label">{t('settings.cw.keyLine.label')}</span>
                    <select
                      className="settings-input"
                      value={form.cwKeyLine ?? 'dtr'}
                      onChange={(e) => update('cwKeyLine', e.target.value)}
                    >
                      <option value="dtr">{t('settings.cw.keyLine.dtr')}</option>
                      <option value="rts">{SERIAL_LINE_RTS}</option>
                    </select>
                    <span className="settings-hint">{t('settings.cw.keyLine.hint')}</span>
                  </label>
                </>
              )}
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.cw.idAfter73.label')}</span>
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
                <span className="settings-hint">{t('settings.cw.idAfter73.hint')}</span>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.cw.macros.title')}</span>
              <div className="settings-field cw-macro-editor">
                <span className="settings-label">{t('settings.cw.macros.label')}</span>
                {/* Named macro profiles — a rotating operator switches sets here (or in one
                    click from the CW cockpit bar). The grid below edits the ACTIVE profile. */}
                <div className="cw-macro-row">
                  <select
                    className="settings-input"
                    value={activeCwIdx}
                    onChange={(e) => selectCwProfile(Number(e.target.value))}
                    aria-label={t('settings.cw.macros.profiles.aria')}
                  >
                    {cwProfiles.map((p, i) => (
                      <option key={i} value={i}>
                        {p.name || t('settings.cw.macros.profiles.unnamed', { n: i + 1 })}
                      </option>
                    ))}
                  </select>
                  <button type="button" className="settings-refresh" onClick={addCwProfile}>
                    {t('settings.cw.macros.profiles.add')}
                  </button>
                  <button type="button" className="settings-refresh" onClick={renameCwProfile}>
                    {t('settings.cw.macros.profiles.rename')}
                  </button>
                  <button
                    type="button"
                    className="settings-refresh danger"
                    onClick={deleteCwProfile}
                    disabled={cwProfiles.length <= 1}
                    title={
                      cwProfiles.length <= 1
                        ? t('settings.cw.macros.profiles.keepOne')
                        : t('settings.cw.macros.profiles.deleteTitle')
                    }
                  >
                    {t('settings.cw.macros.profiles.delete')}
                  </button>
                </div>
                {!activeCwMacros.length ? (
                  <div className="cw-macro-row">
                    {/* The macro tokens live INSIDE the entry — `{MYCALL}` and friends are
                        matched literally by the expander, and the catalog interpolates on
                        `{{double}}` braces precisely so they pass through untouched. */}
                    <span className="settings-hint">{t('settings.cw.macros.builtin.hint')}</span>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={() => setCwMacros(CW_MACRO_DEFAULTS.map((m) => ({ ...m })))}
                    >
                      {t('settings.cw.macros.customize')}
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
                          aria-label={t('settings.cw.macros.row.label.aria', { key: m.key })}
                          autoComplete="off"
                          spellCheck={false}
                        />
                        <input
                          className="settings-input cw-macro-text"
                          type="text"
                          value={m.text}
                          onChange={(e) => updateCwMacro(i, 'text', e.target.value)}
                          aria-label={t('settings.cw.macros.row.text.aria', { key: m.key })}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </div>
                    ))}
                    <div className="cw-macro-row">
                      <span className="settings-hint">{t('settings.cw.macros.tokens.hint')}</span>
                      <button
                        type="button"
                        className="settings-refresh"
                        onClick={() => setCwMacros([])}
                      >
                        {t('settings.cw.macros.reset')}
                      </button>
                    </div>
                  </>
                )}
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- RTTY — keying backend + signal parameters (TX + RX demod both) ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-rtty">
            <legend>{t('settings.rtty.legend')}</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.rtty.keying.title')}</span>
              <label className="settings-field">
                <span className="settings-label">{t('settings.rtty.backend.label')}</span>
                <select
                  className="settings-input"
                  value={form.rttyBackend ?? 'afsk'}
                  onChange={(e) => update('rttyBackend', e.target.value)}
                >
                  <option value="afsk">{t('settings.rtty.backend.afsk')}</option>
                  <option value="fsk">{t('settings.rtty.backend.fsk')}</option>
                </select>
                <span className="settings-hint">
                  <T k="settings.rtty.backend.hint" tags={{ b: <strong /> }} />
                </span>
              </label>
              {form.rttyBackend === 'fsk' && (
                <>
                  <label className="settings-field">
                    <span className="settings-label">{t('settings.rtty.fskPort.label')}</span>
                    <input
                      className="settings-input"
                      type="text"
                      value={form.rttyFskPort ?? ''}
                      placeholder={t('settings.rtty.fskPort.placeholder', {
                        example: KEYING_EXAMPLES.rttyFskPort,
                      })}
                      onChange={(e) => update('rttyFskPort', e.target.value)}
                      autoComplete="off"
                      spellCheck={false}
                    />
                    <span className="settings-hint">{t('settings.rtty.fskPort.hint')}</span>
                  </label>
                  <label className="settings-field">
                    <span className="settings-label">{t('settings.rtty.fskLine.label')}</span>
                    <select
                      className="settings-input"
                      value={form.rttyFskLine ?? 'dtr'}
                      onChange={(e) => update('rttyFskLine', e.target.value)}
                    >
                      <option value="dtr">{t('settings.rtty.fskLine.dtr')}</option>
                      <option value="rts">{SERIAL_LINE_RTS}</option>
                    </select>
                    <span className="settings-hint">{t('settings.rtty.fskLine.hint')}</span>
                  </label>
                </>
              )}
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.rtty.signal.title')}</span>
              <label className="settings-field">
                <span className="settings-label">{t('settings.rtty.baud.label')}</span>
                <select
                  className="settings-input"
                  value={String(form.rttyBaud ?? 45.45)}
                  onChange={(e) => updateNum('rttyBaud', Number(e.target.value))}
                >
                  <option value="45.45">{t('settings.rtty.baud.hf')}</option>
                  <option value="75">{t('settings.rtty.baud.vhf')}</option>
                </select>
                <span className="settings-hint">{t('settings.rtty.baud.hint')}</span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.rtty.shift.label')}</span>
                {/* 425 and 850 are rows with nothing in them but the shift itself — no prose
                    to translate, so they stay written here. */}
                <select
                  className="settings-input"
                  value={String(form.rttyShiftHz ?? 170)}
                  onChange={(e) => updateNum('rttyShiftHz', Number(e.target.value))}
                >
                  <option value="170">{t('settings.rtty.shift.hf')}</option>
                  <option value="425">425</option>
                  <option value="850">850</option>
                </select>
                <span className="settings-hint">{t('settings.rtty.shift.hint')}</span>
              </label>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.rtty.reverse.label')}</span>
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
                <span className="settings-hint">{t('settings.rtty.reverse.hint')}</span>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- PSK — receive-only this phase (Keyboard Modes Phase 1). ONE deliberate
               setting: the auto-arm opt-out. PSK31 must work with ZERO configuration —
               open the screen, click a trace, read the text — so everything else
               (netted frequency, AFC, squelch) is cockpit/decoder state, not schema. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-psk">
            <legend>{t('settings.psk.legend')}</legend>
            <div className="settings-field">
              <label className="settings-toggle">
                <span className="settings-label">{t('settings.psk.rxAutoArm.label')}</span>
                <button
                  type="button"
                  role="switch"
                  // ⚠️ `!== false`, not `!!` — the default is ON, so an absent key reads as on.
                  aria-checked={form.pskRxAutoArm !== false}
                  className={`toggle${form.pskRxAutoArm !== false ? ' on' : ''}`}
                  onClick={() => updateBool('pskRxAutoArm', form.pskRxAutoArm === false)}
                >
                  <span className="toggle-knob" />
                </button>
              </label>
              <span className="settings-hint">{t('settings.psk.rxAutoArm.hint')}</span>
            </div>
          </fieldset>
          )}

          {/* ---- SSTV — what the SSTV screen starts on, and the ISS pass auto-arm. The ISS
               switch used to live in Rig & CAT, where nothing about it is a rig model, port,
               baud, framing or keying; it is here now, beside the rest of SSTV. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-sstv">
            <legend>{t('settings.sstv.legend')}</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.sstv.receiving.title')}</span>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.sstv.rxAutoArm.label')}</span>
                  <button
                    type="button"
                    role="switch"
                    // ⚠️ `!== false`, not `!!` — the default is ON, so an absent key reads as on.
                    aria-checked={form.sstvRxAutoArm !== false}
                    className={`toggle${form.sstvRxAutoArm !== false ? ' on' : ''}`}
                    onClick={() => updateBool('sstvRxAutoArm', form.sstvRxAutoArm === false)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                <span className="settings-hint">{t('settings.sstv.rxAutoArm.hint')}</span>
              </div>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.sstv.issAutoArm.label')}</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={!!form.issSstvAutoArm}
                    className={`toggle${form.issSstvAutoArm ? ' on' : ''}`}
                    onClick={() => updateBool('issSstvAutoArm', !form.issSstvAutoArm)}
                  >
                    <span className="toggle-knob" />
                  </button>
                </label>
                {/* The ISS downlink is a dial reading, so it is interpolated as a value and
                    never written inside the sentence (the invariant-token rule). */}
                <span className="settings-hint">
                  {t('settings.sstv.issAutoArm.hint', { freq: ISS_SSTV_MHZ.toFixed(3) })}
                </span>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">
                {t('settings.sstv.transmitting.title')}
              </span>
              <label className="settings-field">
                <span className="settings-label">{t('settings.sstv.txMode.label')}</span>
                <select
                  className="settings-input"
                  value={form.sstvDefaultTxMode ?? 'auto'}
                  onChange={(e) => update('sstvDefaultTxMode', e.target.value)}
                >
                  <option value="auto">{t('settings.sstv.txMode.auto')}</option>
                  {TX_MODE_GROUPS.map((g) => (
                    <optgroup key={g} label={g}>
                      {SSTV_TX_MODES.filter((m) => m.group === g).map((m) => (
                        <option key={m.slug} value={m.slug}>
                          {m.name} · ≈{m.seconds}s · {m.width}×{m.height}
                        </option>
                      ))}
                    </optgroup>
                  ))}
                </select>
                <span className="settings-hint">
                  <T k="settings.sstv.txMode.hint" tags={{ b: <strong /> }} />
                </span>
              </label>
              <label className="settings-field">
                <span className="settings-label">{t('settings.sstv.txPower.label')}</span>
                <span className="settings-input-row">
                  <input
                    type="number"
                    min={0}
                    max={100}
                    inputMode="numeric"
                    placeholder="—"
                    value={form.sstvTxPowerPct == null ? '' : String(form.sstvTxPowerPct)}
                    onChange={(e) => updateSstvTxPower(e.target.value)}
                    aria-label={t('settings.sstv.txPower.aria')}
                  />
                  <span>%</span>
                </span>
                <span className="settings-hint">{t('settings.sstv.txPower.hint')}</span>
              </label>
            </div>
            {/* Not a control — the answer to the question this section otherwise invites. The
                plate is drawn in Rust before encoding so no webview path can bypass it, and its
                geometry is derived from the demodulator's own window lengths, not chosen. */}
            <p className="settings-note">
              {t('settings.sstv.callsignNote', { rule: SSTV_ID_RULE })}
            </p>
          </fieldset>
          )}

          {/* ---- APRS — the internet feed (APRS-IS) + the receive-only iGate. Lives HERE, beside
               the other per-mode settings, because that is where operators look for it: the first
               person to go hunting for these went to APRS, not to Integrations & Feeds. ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-aprs">
            <legend>{t('settings.aprs.legend')}</legend>
            {/* The RF side. ⚠️ NOTHING HERE MAY CARRY `disabled={!form.aprsIsEnabled}` — that is
                the internet feed's gate, and copying it down would put RF APRS behind an
                internet connection it does not need. */}
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.aprs.rf.title')}</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.channel.label')}</span>
                  <select
                    className="settings-input"
                    value={form.aprsChannelMhz == null ? '' : String(form.aprsChannelMhz)}
                    onChange={(e) => updateNullableNum('aprsChannelMhz', e.target.value, 0)}
                  >
                    {/* Naming the derived number is the whole mitigation for a table of
                        approximate bounding boxes: a wrong guess is visible, not silent. Both
                        readings are interpolated — a channel is a dial frequency and never
                        belongs inside a translatable sentence. */}
                    <option value="">
                      {form.mygrid
                        ? t('settings.aprs.channel.derived', {
                            freq: aprsChannelForGrid(form.mygrid).toFixed(3),
                          })
                        : t('settings.aprs.channel.default', {
                            freq: NORTH_AMERICA.toFixed(3),
                          })}
                    </option>
                    {APRS_FREQS.map(([f, region]) => (
                      <option key={f} value={String(f)}>
                        {f.toFixed(3)} · {region}
                      </option>
                    ))}
                    {/* A stored channel outside the list is kept as its own option rather than
                        silently dropped — the STANDARD_BAUDS precedent. */}
                    {form.aprsChannelMhz != null &&
                      !APRS_FREQS.some(([f]) => f === form.aprsChannelMhz) && (
                        <option value={String(form.aprsChannelMhz)}>
                          {t('settings.aprs.channel.custom', {
                            freq: form.aprsChannelMhz.toFixed(3),
                          })}
                        </option>
                      )}
                  </select>
                  <span className="settings-hint">
                    <T k="settings.aprs.channel.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.symbol.label')}</span>
                  <select
                    className="settings-input"
                    value={`${form.aprsSymbolTable ?? '/'}${form.aprsSymbolCode ?? '>'}`}
                    onChange={(e) => {
                      const v = e.target.value
                      update('aprsSymbolTable', v[0])
                      update('aprsSymbolCode', v[1])
                    }}
                  >
                    {BEACON_SYMBOLS.map(([table, code, label]) => (
                      <option key={`${table}${code}`} value={`${table}${code}`}>
                        {label}
                      </option>
                    ))}
                  </select>
                  <span className="settings-hint">{t('settings.aprs.symbol.hint')}</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.comment.label')}</span>
                  <input
                    className="settings-input"
                    type="text"
                    maxLength={43}
                    value={form.aprsComment ?? ''}
                    onChange={(e) => update('aprsComment', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">{t('settings.aprs.comment.hint')}</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.path.label')}</span>
                  <input
                    className="settings-input"
                    type="text"
                    value={(form.aprsPath ?? []).join(', ')}
                    onChange={(e) =>
                      setAprsPath(
                        e.target.value
                          .split(',')
                          .map((s) => s.trim().toUpperCase())
                          .filter(Boolean),
                      )
                    }
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    <T
                      k="settings.aprs.path.hint"
                      tags={{ code: <code /> }}
                      vals={{ path: APRS_EXAMPLES.path }}
                    />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.ssid.label')}</span>
                  <select
                    className="settings-input"
                    value={form.aprsSsid == null ? '' : String(form.aprsSsid)}
                    onChange={(e) => updateNullableNum('aprsSsid', e.target.value, 0)}
                  >
                    {/* '' = null = follow the callsign, exactly like the channel picker above.
                        NOT `updateNum` — a plain number cannot express "I have not chosen", and
                        writing 0 unconditionally would demote a station whose call is KD9TAW-9. */}
                    <option value="">{t('settings.aprs.ssid.fromCallsign')}</option>
                    {APRS_SSIDS.map(({ ssid, labelKey }) => (
                      <option key={ssid} value={String(ssid)}>
                        {ssid} — {t(labelKey)}
                      </option>
                    ))}
                  </select>
                  <span className="settings-hint">
                    <T
                      k="settings.aprs.ssid.hint"
                      tags={{ b: <strong />, code: <code /> }}
                      vals={{ example: APRS_EXAMPLES.ssid }}
                    />
                  </span>
                </label>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">{t('settings.aprs.is.title')}</span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.aprs.is.enabled.label')}</span>
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
                  <span className="settings-hint">{t('settings.aprs.is.enabled.hint')}</span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.is.host.label')}</span>
                  <input
                    className="settings-input"
                    value={form.aprsIsHost ?? ''}
                    onChange={(e) => update('aprsIsHost', e.target.value)}
                    placeholder={APRS_EXAMPLES.isHost}
                    spellCheck={false}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">
                    <T
                      k="settings.aprs.is.host.hint"
                      tags={{ code: <code /> }}
                      vals={{ host: APRS_EXAMPLES.isHost }}
                    />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.is.port.label')}</span>
                  <input
                    className="settings-input"
                    type="number"
                    min={1}
                    max={65535}
                    value={form.aprsIsPort ?? 14580}
                    onChange={(e) => updateNum('aprsIsPort', Number(e.target.value))}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">{t('settings.aprs.is.port.hint')}</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.is.radius.label')}</span>
                  <input
                    className="settings-input"
                    type="number"
                    min={0}
                    max={5000}
                    value={form.aprsIsRadiusKm ?? 150}
                    onChange={(e) => updateNum('aprsIsRadiusKm', Number(e.target.value))}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">{t('settings.aprs.is.radius.hint')}</span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.is.watchCalls.label')}</span>
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
                    placeholder={APRS_EXAMPLES.watchCalls}
                    spellCheck={false}
                    disabled={!form.aprsIsEnabled}
                  />
                  <span className="settings-hint">{t('settings.aprs.is.watchCalls.hint')}</span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.aprs.is.weather.label')}</span>
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
                  <span className="settings-hint">{t('settings.aprs.is.weather.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.aprs.is.objects.label')}</span>
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
                  <span className="settings-hint">{t('settings.aprs.is.objects.hint')}</span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.aprs.is.messages.label')}</span>
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
                  <span className="settings-hint">{t('settings.aprs.is.messages.hint')}</span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.aprs.stationTtl.label')}</span>
                  <input
                    className="settings-input"
                    type="number"
                    min={0}
                    max={1440}
                    value={form.aprsStationTtlMin ?? 60}
                    onChange={(e) => updateNum('aprsStationTtlMin', Number(e.target.value))}
                  />
                  <span className="settings-hint">{t('settings.aprs.stationTtl.hint')}</span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">{t('settings.aprs.is.uplink.label')}</span>
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
                  {/* Two WHOLE sentences, not a stem and two tails: "Publishes under KD9TAW"
                      and "Publishes under your callsign" are different statements, and a
                      language that orders them differently cannot be served by a glued tail. */}
                  <span className="settings-hint">
                    {form.mycall ? (
                      <T
                        k="settings.aprs.is.uplink.hint.call"
                        tags={{ b: <strong /> }}
                        vals={{ call: form.mycall.toUpperCase() }}
                      />
                    ) : (
                      <T k="settings.aprs.is.uplink.hint.noCall" tags={{ b: <strong /> }} />
                    )}
                  </span>
                </div>
              </div>
            </div>
          </fieldset>
          )}

          {/* ---- Frequencies (working-frequency table overrides) ---- */}
          {tab === 'digital' && (
          <fieldset className="settings-section" id="settings-working-frequencies">
            <legend>{t('settings.workingFrequencies.legend')}</legend>
            <p className="settings-note">
              <T k="settings.workingFrequencies.note" tags={{ b: <strong /> }} />
            </p>

            <div className="settings-field">
              <span className="settings-label">
                {t('settings.workingFrequencies.stock.label')}
              </span>
              {/* Every dial reading below is DATA, formatted invariantly by `toFixed(6)`; the
                  band and mode cells print the table's own tokens. Only the three column
                  headings are words. */}
              <div className="freq-table">
                <div className="freq-row head">
                  <span className="freq-cell">{t('settings.workingFrequencies.stock.band')}</span>
                  <span className="freq-cell">{t('settings.workingFrequencies.stock.mode')}</span>
                  <span className="freq-cell">{t('settings.workingFrequencies.stock.dial')}</span>
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
                          title={t('settings.workingFrequencies.stock.overrideTitle', {
                            mhz: r.mhz.toFixed(6),
                          })}
                        >
                          {ov.toFixed(6)}
                          <span className="freq-override-tag">
                            {t('settings.workingFrequencies.stock.overrideTag')}
                          </span>
                        </span>
                      ) : (
                        <span className="freq-cell mono">{r.mhz.toFixed(6)}</span>
                      )}
                    </div>
                  )
                })}
              </div>
              <span className="settings-hint">
                {t('settings.workingFrequencies.stock.hint')}
              </span>
            </div>

            <div className="settings-field">
              <span className="settings-label">
                {t('settings.workingFrequencies.overrides.label')}
              </span>
              {overrides.length === 0 && (
                <span className="settings-hint">
                  {t('settings.workingFrequencies.overrides.none')}
                </span>
              )}
              {overrides.map((o, i) => {
                const dup = dupKeys.has(`${o.band}|${o.mode}`)
                return (
                  <div className={`freq-edit-row${dup ? ' dup' : ''}`} key={i}>
                    <select
                      className="settings-input"
                      value={o.band}
                      aria-label={t('settings.workingFrequencies.overrides.band.aria', {
                        n: i + 1,
                      })}
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
                      aria-label={t('settings.workingFrequencies.overrides.mode.aria', {
                        n: i + 1,
                      })}
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
                      aria-label={t('settings.workingFrequencies.overrides.mhz.aria', {
                        n: i + 1,
                      })}
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
                      aria-label={t('settings.workingFrequencies.overrides.remove.aria', {
                        band: o.band,
                        mode: o.mode,
                      })}
                      title={t('settings.workingFrequencies.overrides.remove.title')}
                    >
                      ✕
                    </button>
                    {dup && (
                      <span className="freq-dup-tag">
                        {t('settings.workingFrequencies.overrides.duplicate')}
                      </span>
                    )}
                  </div>
                )
              })}
              <div className="settings-input-row freq-actions">
                <button type="button" className="settings-refresh" onClick={addOverride}>
                  {t('settings.workingFrequencies.overrides.add')}
                </button>
                <button
                  type="button"
                  className="settings-refresh"
                  onClick={resetOverrides}
                  disabled={overrides.length === 0}
                >
                  {t('settings.workingFrequencies.overrides.reset')}
                </button>
              </div>
              <span className="settings-hint">
                {t('settings.workingFrequencies.overrides.hint')}
              </span>
            </div>
          </fieldset>
          )}

          {/* ---- Alerts ---- */}
          {tab === 'spots' && (
          <>
          {/* MIGRATED to the string catalog (i18n/hardcoded-strings.test.ts) with the Spots
              and alert panels this section configures, so the wording matches on both sides.
              The <select> VALUES are persisted tokens and stay here. The rest of this file is
              NOT migrated — see the guard's scope note. */}
          <fieldset className="settings-section" id="settings-pounce">
            <legend>{t('settings.pounce.legend')}</legend>
            <p className="settings-note">
              <T k="settings.pounce.note" tags={{ em: <em /> }} />
            </p>
            <label className="settings-field">
              <span className="settings-label">{t('settings.pounce.threshold.label')}</span>
              <select
                value={form.pounceThreshold ?? 'off'}
                onChange={(e) => update('pounceThreshold', e.target.value as never)}
              >
                <option value="off">{t('settings.pounce.threshold.off')}</option>
                <option value="atno">{t('settings.pounce.threshold.atno')}</option>
                <option value="atnoOrZone">{t('settings.pounce.threshold.atnoOrZone')}</option>
                <option value="atnoZoneOrState">{t('settings.pounce.threshold.atnoZoneOrState')}</option>
              </select>
            </label>
          </fieldset>

          </>
          )}

          {/* Accessibility rides with the other display + presentation prefs, not with the spot
              feeds. It sat on Spots & Alerts only because the earcons happen to fire on decode
              alerts; the settings themselves are about how the app SPEAKS and is read, which is
              what an operator opens Appearance for. Reunited with UI scale and density. */}
          {tab === 'appearance' && (
          <>
          {/* MIGRATED to the string catalog (i18n/hardcoded-strings.test.ts) with the rest of
              the Appearance tab. The <select> VALUES are persisted tokens and stay here. */}
          <fieldset className="settings-section" id="settings-accessibility">
            <legend>{t('settings.accessibility.legend')}</legend>
            <p className="settings-note">{t('settings.accessibility.note')}</p>
            <div className="settings-grid">
              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">
                    {t('settings.accessibility.announce.label')}
                  </span>
                  <select
                    className="settings-input"
                    value={form.announceVerbosity ?? 'needed'}
                    onChange={(e) => update('announceVerbosity', e.target.value)}
                  >
                    <option value="off">{t('settings.accessibility.announce.off')}</option>
                    <option value="needed">{t('settings.accessibility.announce.needed')}</option>
                    <option value="all">{t('settings.accessibility.announce.all')}</option>
                  </select>
                </label>
                <span className="settings-hint">
                  {t('settings.accessibility.announce.hint')}
                </span>
              </div>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">
                    {t('settings.accessibility.txRxEarcon.label')}
                  </span>
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
                <span className="settings-hint">
                  {t('settings.accessibility.txRxEarcon.hint')}
                </span>
              </div>
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">
                    {t('settings.accessibility.decodeTick.label')}
                  </span>
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
                <span className="settings-hint">
                  {t('settings.accessibility.decodeTick.hint')}
                </span>
              </div>
            </div>
          </fieldset>
          </>
          )}

          {tab === 'spots' && (
          <>
          {/* MIGRATED to the string catalog — see the note on the Pounce section above. The
              band-scope <option> VALUES are persisted tokens; the four labels are one shared
              vocabulary because the same four choices mean the same thing on all three. */}
          <fieldset className="settings-section" id="settings-alerts">
            <legend>{t('settings.alerts.legend')}</legend>
            <div className="settings-grid">
              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.alerts.myCall.label')}</span>
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
                <span className="settings-hint">{t('settings.alerts.myCall.hint')}</span>
              </div>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.alerts.cq.label')}</span>
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
                <span className="settings-hint">{t('settings.alerts.cq.hint')}</span>
              </div>

              {/* Per-type band scopes: all decode alerts fire on the CURRENT band, so the
                  scope is "should this alert on the band I'm on". VHF+ = 6 m and up. */}
              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">{t('settings.alerts.dxcc.label')}</span>
                  <select
                    className="settings-input"
                    value={!form.alertNew ? 'off' : (form.alertDxccBands ?? 'all')}
                    aria-label={t('settings.alerts.dxcc.aria')}
                    onChange={(e) => changeAlertScope('alertDxccBands', e.target.value)}
                  >
                    <option value="off">{t('settings.alerts.scope.off')}</option>
                    <option value="hf">{t('settings.alerts.scope.hf')}</option>
                    <option value="vhf">{t('settings.alerts.scope.vhf')}</option>
                    <option value="all">{t('settings.alerts.scope.all')}</option>
                  </select>
                </label>
                <span className="settings-hint">{t('settings.alerts.dxcc.hint')}</span>
              </div>

              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">{t('settings.alerts.grid.label')}</span>
                  <select
                    className="settings-input"
                    value={!form.alertNew ? 'off' : (form.alertGridBands ?? 'vhf')}
                    aria-label={t('settings.alerts.grid.aria')}
                    onChange={(e) => changeAlertScope('alertGridBands', e.target.value)}
                  >
                    <option value="off">{t('settings.alerts.scope.off')}</option>
                    <option value="hf">{t('settings.alerts.scope.hf')}</option>
                    <option value="vhf">{t('settings.alerts.scope.vhf')}</option>
                    <option value="all">{t('settings.alerts.scope.all')}</option>
                  </select>
                </label>
                <span className="settings-hint">{t('settings.alerts.grid.hint')}</span>
              </div>

              <div className="settings-field">
                <label className="settings-inline-label">
                  <span className="settings-label">{t('settings.alerts.rareGrid.label')}</span>
                  <select
                    className="settings-input"
                    value={!form.alertNew ? 'off' : (form.alertRareGridBands ?? 'vhf')}
                    aria-label={t('settings.alerts.rareGrid.aria')}
                    onChange={(e) => changeAlertScope('alertRareGridBands', e.target.value)}
                  >
                    <option value="off">{t('settings.alerts.scope.off')}</option>
                    <option value="hf">{t('settings.alerts.scope.hf')}</option>
                    <option value="vhf">{t('settings.alerts.scope.vhf')}</option>
                    <option value="all">{t('settings.alerts.scope.all')}</option>
                  </select>
                </label>
                <span className="settings-hint">{t('settings.alerts.rareGrid.hint')}</span>
              </div>

            </div>
            <div className="settings-watchlist-block">
              <span className="settings-label">{t('settings.alerts.watchlist.label')}</span>
              <WatchlistPanel />
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Connections (connector status + log) — moved from Logbook & QSL ---- */}
          {tab === 'logging' && (
          <fieldset className="settings-section" id="settings-connections">
            <legend>{t('settings.connections.legend')}</legend>
            <div className="conn-status-grid">
              {creds.map((c) => {
                // The dot comes from the last ROUND TRIP, not from `stored` — see
                // settings/connHealth.ts. A stored secret proves only that a save took.
                // ⚠️ The state word and the "failed 10m ago …" line are that module's prose
                // and move with it; the connector name and identity are data.
                const state = connState(c)
                const dot = dotClass(state)
                const when = whenText(c, state)
                return (
                <div key={c.id} className="conn-status-row">
                  <span className={`conn-dot ${dot}`} aria-hidden="true" />
                  <span className="conn-name">{c.connector}</span>
                  <span className="conn-id">{c.identity || '—'}</span>
                  <span className={`conn-state ${dot}`}>{stateLabel(state)}</span>
                  {/* Before the Test button, which takes margin-left:auto — a span after it
                      would be shoved to the far edge of the row. */}
                  {when && (
                    <span className="conn-when" title={c.lastFailureDetail ?? ''}>
                      {when}
                    </span>
                  )}
                  {c.id === 'qrz-logbook' && (
                    <button
                      type="button"
                      className="settings-test-btn"
                      onClick={runQrzTest}
                      disabled={qrzTest.state === 'testing'}
                      title={t('settings.connections.qrz.test.title')}
                    >
                      {qrzTest.state === 'testing'
                        ? t('settings.connections.test.busy')
                        : t('settings.connections.qrz.test.action')}
                    </button>
                  )}
                </div>
                )
              })}
            </div>
            {qrzTest.state !== 'idle' && qrzTest.state !== 'testing' && (
              <p className={`conn-test-result ${qrzTest.state}`}>
                {qrzTest.state === 'ok' ? (
                  t('settings.connections.qrz.test.ok', { detail: qrzTest.msg })
                ) : (
                  <T
                    k="settings.connections.qrz.test.fail"
                    tags={{ b: <strong /> }}
                    vals={{ detail: qrzTest.msg }}
                  />
                )}
              </p>
            )}
            <div className="conn-log">
              <div className="conn-log-head">
                <span>{t('settings.connections.log.title')}</span>
                <span className="settings-hint">{t('settings.connections.log.hint')}</span>
              </div>
              {connLog.length === 0 ? (
                <p className="conn-log-empty">{t('settings.connections.log.empty')}</p>
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

          {tab === 'logging' && (
            <fieldset className="settings-section" id="settings-connections-b4">
              <legend>{t('settings.b4.legend')}</legend>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">{t('settings.b4.matchMode.label')}</span>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={form.b4MatchMode ?? false}
                    className={`toggle${form.b4MatchMode ? ' on' : ''}`}
                    onClick={() => updateBool('b4MatchMode', !form.b4MatchMode)}
                  >
                    <span className="toggle-knob" />
                  </button>
                  <span className="settings-hint">{t('settings.b4.matchMode.hint')}</span>
                </label>
              </div>
            </fieldset>
          )}

          {/* ---- Network integrations ---- */}
          {tab === 'logging' && (
          <fieldset className="settings-section" id="settings-integrations-feeds">
            <legend>{t('settings.integrations.legend')}</legend>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">
                {t('settings.integrations.local.title')}
              </span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    {/* ⚠️ A label that is nothing but names — the program, the protocol and
                        the interface — so it is invariant and stays here, exactly as the
                        Phone/CW/Digital tab labels do. */}
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
                  <span className="settings-hint">{t('settings.integrations.wsjtxUdp.hint')}</span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.integrations.udpAddr.label')}</span>
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
                  <span className="settings-hint">{t('settings.integrations.udpAddr.hint')}</span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.integrations.hrdLogging.label')}
                    </span>
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
                  <span className="settings-hint">{t('settings.integrations.hrdLogging.hint')}</span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">{t('settings.integrations.hrdAddr.label')}</span>
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
                  <span className="settings-hint">{t('settings.integrations.hrdAddr.hint')}</span>
                  {form.hrdLogging && radio?.hrdLinkUp != null && (
                    <span
                      className={`settings-hint ${radio.hrdLinkUp ? 'ok' : 'warn'}`}
                      style={{ color: radio.hrdLinkUp ? 'var(--ok)' : 'var(--state-weak)' }}
                    >
                      {radio.hrdLinkUp
                        ? t('settings.integrations.hrd.linkUp')
                        : t('settings.integrations.hrd.linkDown', {
                            count: radio.hrdQueued ?? 0,
                          })}
                    </span>
                  )}
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.integrations.companionAddr.label')}
                  </span>
                  <input
                    className="settings-input"
                    value={form.companionAddr ?? ''}
                    onChange={(e) => update('companionAddr', e.target.value)}
                    placeholder="127.0.0.1:2237"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.integrations.companionAddr.hint')}
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.integrations.allTxt.label')}
                    </span>
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
                    {t('settings.integrations.allTxt.hint')}
                    {allTxtPath && (
                      <>
                        {' '}
                        <T
                          k="settings.integrations.allTxt.path"
                          tags={{ code: <code /> }}
                          vals={{ path: allTxtPath }}
                        />
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
                    {t('settings.integrations.allTxt.reveal')}
                  </button>
                </div>

                {/* The diagnostic log has no toggle — it is written from the first moments of
                    startup, deliberately, because the failures worth diagnosing are the early
                    ones. That is exactly why it needs a line HERE: it was the one
                    operator-facing artefact nothing in the interface named, on a file we ask
                    people to send us when something goes wrong. */}
                <div className="settings-field">
                  <span className="settings-label">Diagnostic log</span>
                  <span className="settings-hint">
                    Always on. A plain-text record of what Nexus did — startup steps, the CAT and
                    audio device open, updater checks, and any failure — so a “it won’t start” or
                    “it stopped decoding” report has something to look at. Passwords, API keys and
                    tokens are masked before anything is written, so it is safe to attach to a bug
                    report. Bounded to two files, about 8 MB in total.
                    {diagLogPath && (
                      <>
                        {' '}Saved at <code>{diagLogPath}</code>.
                      </>
                    )}
                  </span>
                  <button
                    type="button"
                    className="settings-linkbtn"
                    onClick={() => {
                      revealDiagLog().catch(() => {})
                    }}
                  >
                    Reveal in folder
                  </button>
                </div>

                {/* The DEBUG tier. Deliberately a SEPARATE field under the log rather than a
                    line in its hint: it is a session switch an operator is talked into by us
                    ("turn this on and send me the log"), not a preference, and it should read
                    as the exception it is. Applies live — no restart — because whatever is
                    being chased is usually happening right now. */}
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">Extra detail in the diagnostic log</span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={form.diagDebugLog}
                      className={`toggle${form.diagDebugLog ? ' on' : ''}`}
                      onClick={() => updateBool('diagDebugLog', !form.diagDebugLog)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    Records each transmission, per-period decode counts and CAT traffic. Leave
                    this OFF for normal operating — it is for a session where something is being
                    chased, and the extra volume shortens how far back the log reaches. The log
                    says at the top when it was on.
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.integrations.qsoWav.label')}
                    </span>
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
                  <span className="settings-hint">
                    {t('settings.integrations.qsoWav.hint')}
                    {recordingsPath && (
                      <>
                        {' '}
                        <T
                          k="settings.integrations.qsoWav.path"
                          tags={{ code: <code /> }}
                          vals={{ path: recordingsPath }}
                        />
                      </>
                    )}
                  </span>
                  <button
                    type="button"
                    className="settings-linkbtn"
                    onClick={() => {
                      revealRecordings().catch(() => {})
                    }}
                  >
                    {t('settings.integrations.qsoWav.reveal')}
                  </button>
                </div>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.integrations.saveWav.label')}
                  </span>
                  <select
                    value={form.saveWav || 'none'}
                    onChange={(e) => update('saveWav', e.target.value)}
                  >
                    <option value="none">{t('settings.integrations.saveWav.none')}</option>
                    <option value="decodes">{t('settings.integrations.saveWav.decodes')}</option>
                    <option value="all">{t('settings.integrations.saveWav.all')}</option>
                  </select>
                  <span className="settings-hint">{t('settings.integrations.saveWav.hint')}</span>
                </label>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">
                {t('settings.integrations.spotSources.title')}
              </span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    {/* ⚠️ The reporting service's own name — invariant, like a callsign. */}
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
                  <span className="settings-hint">
                    {t('settings.integrations.pskreporter.hint')}
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.integrations.clusterSpots.label')}
                    </span>
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
                    {t('settings.integrations.clusterSpots.hint')}
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">
                    {t('settings.integrations.clusterNodes.label')}
                  </span>
                  {(form.clusterHosts ?? []).length === 0 ? (
                    <span className="settings-hint cluster-node-empty">
                      {t('settings.integrations.clusterNodes.empty')}
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
                          placeholder={LOGGER_EXAMPLES.clusterNode}
                          spellCheck={false}
                        />
                        <button
                          type="button"
                          className="cluster-node-remove"
                          title={t('settings.integrations.clusterNodes.remove.title')}
                          aria-label={
                            host
                              ? t('settings.integrations.clusterNodes.remove.aria', { host })
                              : t('settings.integrations.clusterNodes.remove.ariaBlank')
                          }
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
                      <option value="">
                        {t('settings.integrations.clusterNodes.add.option')}
                      </option>
                      {/* Each preset LABEL names a real node (its callsign, its port and why
                          you would pick it) — data about the cluster mesh, invariant, and it
                          lives in CLUSTER_PRESETS above. */}
                      {CLUSTER_PRESETS.map((p) => (
                        <option key={p.host} value={p.host}>
                          {p.label}
                        </option>
                      ))}
                    </select>
                    <button
                      type="button"
                      className="cluster-node-add-blank"
                      title={t('settings.integrations.clusterNodes.addCustom.title')}
                      onClick={() => mutateClusterHosts((hs) => [...hs, ''])}
                    >
                      {t('settings.integrations.clusterNodes.addCustom.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    {t('settings.integrations.clusterNodes.hint')}
                  </span>
                </div>
              </div>
            </div>

            <div className="settings-featgroup">
              <span className="settings-featgroup-title">
                {t('settings.integrations.propagation.title')}
              </span>
              <div className="settings-grid">
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.integrations.openingWatch.label')}
                    </span>
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
                    {t('settings.integrations.openingWatch.hint')}
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.integrations.propEngine.label')}
                  </span>
                  <select
                    value={form.propEngine || 'heuristic'}
                    onChange={(e) => update('propEngine', e.target.value)}
                  >
                    <option value="heuristic">
                      {t('settings.integrations.propEngine.heuristic')}
                    </option>
                    <option value="p533">{t('settings.integrations.propEngine.p533')}</option>
                  </select>
                  <span className="settings-hint">
                    {t('settings.integrations.propEngine.hint')}
                  </span>
                </label>
              </div>
              <SettingsGroup
                id="antenna-gain"
                title={t('settings.antennaGain.title')}
                defaultOpen={false}
              >
                <div className="settings-field">
                  <span className="settings-label">{t('settings.antennaGain.label')}</span>
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
                        aria-label={
                          k === 'antTxGainDbi'
                            ? t('settings.antennaGain.tx.aria')
                            : t('settings.antennaGain.rx.aria')
                        }
                        value={form[k] ?? 0}
                        onChange={(e) => {
                          const num = Number(e.target.value)
                          if (!Number.isNaN(num)) updateNum(k, num)
                        }}
                      />
                    ))}
                  </div>
                  <span className="settings-hint">{t('settings.antennaGain.hint')}</span>
                </div>
              </SettingsGroup>
            </div>
          </fieldset>
          )}

          {/* ---- N3FJP + N1MM loggers (moved from Field Day — they serve everyday club logging) ---- */}
          {tab === 'logging' && (
          <>
          <fieldset className="settings-section" id="settings-dxkeeper">
            {/* ⚠️ The legend is two product names and nothing else — invariant, and it stays
                here for the same reason the mode-name tab labels do. */}
            <legend>DXKeeper (DXLab Suite)</legend>
            <p className="settings-note">
              <T k="settings.dxkeeper.note" tags={{ b: <strong />, em: <em /> }} />
            </p>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.dxkeeper.host.label')}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.dxkeeperHost ?? ''}
                  placeholder={t('settings.dxkeeper.host.placeholder', {
                    example: LOGGER_EXAMPLES.dxkeeperHost,
                  })}
                  onChange={(e) => update('dxkeeperHost', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">{t('settings.dxkeeper.host.hint')}</span>
              </label>

              <label className="settings-field">
                {/* ⚠️ NOT a catalog entry: this label names a control in DXKeeper's OWN
                    English interface — see the note below the input — and a translated one
                    sends the operator looking for a field that is not there. */}
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
                  <T
                    k="settings.dxkeeper.basePort.hint"
                    tags={{ em: <em />, b: <strong /> }}
                    vals={{ port: (form.dxkeeperBasePort ?? 52000) + 1 }}
                  />
                </span>
              </label>

              <label className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.dxkeeper.uploads.label')}</span>
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
                  <T k="settings.dxkeeper.uploads.hint" tags={{ em: <em /> }} />
                </span>
              </label>
            </div>
          </fieldset>

          <fieldset className="settings-section" id="settings-n3fjp">
            <legend>{t('settings.n3fjp.legend')}</legend>
            <p className="settings-note">
              <T k="settings.n3fjp.note" tags={{ b: <strong /> }} />
            </p>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.n3fjp.host.label')}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.n3fjpHost ?? ''}
                  placeholder={t('settings.n3fjp.host.placeholder', {
                    example: LOGGER_EXAMPLES.n3fjpHost,
                  })}
                  onChange={(e) => update('n3fjpHost', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">{t('settings.n3fjp.host.hint')}</span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.n3fjp.port.label')}</span>
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
                <span className="settings-hint">{t('settings.n3fjp.port.hint')}</span>
              </label>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.n3fjp.useEnter.label')}</span>
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
                  <T k="settings.n3fjp.useEnter.hint" tags={{ b: <strong />, code: <code /> }} />
                </span>
              </div>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.n3fjp.reportBand.label')}</span>
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
                <span className="settings-hint">{t('settings.n3fjp.reportBand.hint')}</span>
              </div>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.n3fjp.forwardAll.label')}</span>
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
                  <T k="settings.n3fjp.forwardAll.hint" tags={{ b: <strong /> }} />
                </span>
              </div>

              <div className="settings-field">
                <span className="settings-label">{t('settings.n3fjp.test.label')}</span>
                <div className="settings-input-row">
                  <button
                    type="button"
                    className="settings-refresh"
                    onClick={runN3fjpTest}
                    disabled={n3fjpTest.state === 'testing' || !form.n3fjpHost?.trim()}
                    title={t('settings.n3fjp.test.title')}
                  >
                    {n3fjpTest.state === 'testing'
                      ? t('settings.connections.test.busy')
                      : t('settings.n3fjp.test.action')}
                  </button>
                </div>
                {n3fjpTest.state !== 'idle' && n3fjpTest.state !== 'testing' && (
                  // A pass/fail glyph and the answer the connection itself gave — neither is
                  // this panel's prose, so neither is in the catalog.
                  <span className={`cat-result ${n3fjpTest.state}`} role="status">
                    {n3fjpTest.state === 'ok' ? '✓ ' : '✗ '}{n3fjpTest.msg}
                  </span>
                )}
                <span className="settings-hint">{t('settings.n3fjp.test.hint')}</span>
              </div>
            </div>
          </fieldset>

          <fieldset className="settings-section" id="settings-n1mm">
            <legend>{t('settings.n1mm.legend')}</legend>
            <div className="settings-grid">
              <label className="settings-field">
                <span className="settings-label">{t('settings.n1mm.addr.label')}</span>
                <input
                  className="settings-input"
                  type="text"
                  value={form.n1mmAddr ?? ''}
                  placeholder={t('settings.n1mm.addr.placeholder', {
                    example: LOGGER_EXAMPLES.n1mmAddr,
                  })}
                  onChange={(e) => update('n1mmAddr', e.target.value)}
                  autoComplete="off"
                  spellCheck={false}
                />
                <span className="settings-hint">
                  {t('settings.n1mm.addr.hint')}{' '}
                  {form.n1mmUpload ? (
                    <strong>{t('settings.n1mm.addr.sending')}</strong>
                  ) : (
                    <strong>{t('settings.n1mm.addr.idle')}</strong>
                  )}
                </span>
              </label>

              <div className="settings-field">
                <label className="settings-toggle">
                  <span className="settings-label">{t('settings.n1mm.broadcastAll.label')}</span>
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
                  <T
                    k="settings.n1mm.broadcastAll.hint"
                    tags={{ b: <strong />, em: <em /> }}
                  />
                </span>
              </div>
            </div>
          </fieldset>
          </>
          )}

          {/* ---- Confirmations (LoTW / eQSL / QRZ / ClubLog accounts) ---- */}
          {tab === 'logging' && (
          <>
          <fieldset className="settings-section" id="settings-lotw-users">
            <legend>{t('settings.lotwUsers.legend')}</legend>
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
                        // The grouping is this call site's, not the catalog's — `t()` has no
                        // locale-aware number path (see the invariant-token rule).
                        pushToast(
                          t('settings.lotwUsers.fetch.done', {
                            count: st.count.toLocaleString(),
                          }),
                          'success',
                          5000,
                        )
                      })
                      .catch((e) =>
                        pushToast(
                          t('settings.lotwUsers.fetch.failed', {
                            detail: e instanceof Error ? e.message : String(e),
                          }),
                          'error',
                        ),
                      )
                      .finally(() => setLotwFetching(false))
                  }}
                >
                  {lotwFetching
                    ? t('settings.lotwUsers.fetch.busy')
                    : t('settings.lotwUsers.fetch.action')}
                </button>
                <span className="settings-hint">
                  {lotwUsers && lotwUsers.count > 0
                    ? t('settings.lotwUsers.status', {
                        count: lotwUsers.count.toLocaleString(),
                        date: new Date(lotwUsers.fetchedAt * 1000).toISOString().slice(0, 10),
                      })
                    : t('settings.lotwUsers.empty')}
                </span>
              </div>
              <label className="settings-label" htmlFor="lotw-max-age" style={{ marginTop: 8 }}>
                {t('settings.lotwUsers.maxAge.label')}
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
              <span className="settings-hint">{t('settings.lotwUsers.maxAge.hint')}</span>
            </div>
          </fieldset>

          <fieldset className="settings-section" id="settings-callsign-state">
            <legend>{t('settings.callsignState.legend')}</legend>
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
                          t('settings.callsignState.update.done', {
                            count: st.count.toLocaleString(),
                          }),
                          'success',
                          5000,
                        )
                      })
                      .catch((e) =>
                        pushToast(
                          t('settings.callsignState.update.failed', {
                            detail: e instanceof Error ? e.message : String(e),
                          }),
                          'error',
                        ),
                      )
                      .finally(() => setFccFetching(false))
                  }}
                >
                  {fccFetching
                    ? t('settings.callsignState.update.busy')
                    : t('settings.callsignState.update.action')}
                </button>
                <span className="settings-hint">
                  {fccStates && fccStates.count > 0
                    ? t('settings.callsignState.status', {
                        count: fccStates.count.toLocaleString(),
                        date: new Date(fccStates.fetchedAt * 1000).toISOString().slice(0, 10),
                      })
                    : t('settings.callsignState.empty')}
                </span>
              </div>
              <span className="settings-hint">{t('settings.callsignState.hint')}</span>
            </div>
          </fieldset>
          <fieldset className="settings-section" id="settings-confirmations">
            <legend>{t('settings.confirmations.legend')}</legend>
            <div className="settings-featgroup">
              {/* ⚠️ Every featgroup title in this fieldset is a service's own name and nothing
                  else — invariant, like a callsign, and it stays here. */}
              <span className="settings-featgroup-title">LoTW</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.lotw.username.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.lotwUsername}
                    placeholder={t('settings.confirmations.lotw.username.placeholder')}
                    onChange={(e) => update('lotwUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.lotw.username.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.lotw.password.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={lotwPw}
                      placeholder={t('settings.confirmations.lotw.password.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetLotwPassword}
                      title={t('settings.confirmations.credential.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T k="settings.confirmations.lotw.password.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <div className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.lotw.sync.label')}
                  </span>
                  <div className="settings-input-row">
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSyncLotw}
                      disabled={lotwSyncing || !form.lotwUsername.trim()}
                    >
                      {lotwSyncing
                        ? t('settings.confirmations.lotw.sync.busy')
                        : t('settings.confirmations.lotw.sync.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T
                      k="settings.confirmations.lotw.sync.hint"
                      tags={{ b: <strong />, em: <em /> }}
                    />
                  </span>
                </div>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.lotw.stationLocation.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.lotwStationLocation}
                    placeholder={t('settings.confirmations.lotw.stationLocation.placeholder')}
                    onChange={(e) => update('lotwStationLocation', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    <T
                      k="settings.confirmations.lotw.stationLocation.hint"
                      tags={{ b: <strong /> }}
                    />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.lotw.adifLocation.label')}
                  </span>
                  <span className="settings-input-row">
                    <input
                      type="checkbox"
                      checked={!!form.lotwUseAdifLocation}
                      onChange={(e) => {
                        updateBool('lotwUseAdifLocation', e.target.checked)
                        // The two can never be true at once, so turning traveler mode ON
                        // turns the timer OFF rather than leaving a switch that is checked
                        // and silently refused. The backend gate is the one that holds.
                        if (e.target.checked) updateBool('lotwAutoUpload', false)
                      }}
                      aria-label={t('settings.confirmations.lotw.adifLocation.aria')}
                    />
                    <span className="settings-hint">
                      <T
                        k="settings.confirmations.lotw.adifLocation.hint"
                        tags={{ b: <strong />, em: <em />, code: <code /> }}
                      />
                    </span>
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.lotw.tqslPath.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.tqslPath}
                    placeholder={t('settings.confirmations.lotw.tqslPath.placeholder')}
                    onChange={(e) => update('tqslPath', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.lotw.tqslPath.hint')}
                  </span>
                </label>

                {/* Deliberately NOT worded like the sibling "Auto-upload QSOs to …" switches:
                    those push one QSO over HTTP as you log it, this hands a whole batch to
                    TQSL on a timer. Promising push-as-you-log would be a lie about a path
                    that cannot do it. Disabled in traveler mode, with the reason on screen —
                    a dead control that does not say why reads as a bug. */}
                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.lotw.autoUpload.label')}
                    </span>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={!!form.lotwAutoUpload}
                      disabled={!!form.lotwUseAdifLocation}
                      className={`toggle${form.lotwAutoUpload ? ' on' : ''}`}
                      onClick={() => updateBool('lotwAutoUpload', !form.lotwAutoUpload)}
                    >
                      <span className="toggle-knob" />
                    </button>
                  </label>
                  <span className="settings-hint">
                    <T k="settings.confirmations.lotw.autoUpload.hint" tags={{ b: <strong /> }} />
                    {form.lotwUseAdifLocation && (
                      <>
                        {' '}
                        <strong>{t('settings.confirmations.lotw.autoUpload.blocked')}</strong>
                      </>
                    )}
                    {!!form.lotwLastAutoUploadUnix && form.lotwLastAutoUploadUnix > 0 && (
                      <>
                        {' '}
                        {/* The stamp is a LOCAL wall-clock time the panel formats itself, exactly
                            as the LoTW-users count does — `t()` has no locale formatter. */}
                        {t('settings.confirmations.lotw.autoUpload.lastRun', {
                          when: new Date(form.lotwLastAutoUploadUnix * 1000).toLocaleString(),
                        })}
                      </>
                    )}
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">eQSL</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.eqsl.username.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.eqslUsername}
                    placeholder={t('settings.confirmations.eqsl.username.placeholder')}
                    onChange={(e) => update('eqslUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.eqsl.username.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.eqsl.password.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={eqslPw}
                      placeholder={t('settings.confirmations.eqsl.password.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetEqslPassword}
                      title={t('settings.confirmations.credential.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    {t('settings.confirmations.eqsl.password.hint')}
                  </span>
                </label>

                <div className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.eqsl.sync.label')}
                  </span>
                  <div className="settings-input-row">
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSyncEqsl}
                      disabled={eqslSyncing || !form.eqslUsername.trim()}
                    >
                      {eqslSyncing
                        ? t('settings.confirmations.eqsl.sync.busy')
                        : t('settings.confirmations.eqsl.sync.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T k="settings.confirmations.eqsl.sync.hint" tags={{ b: <strong /> }} />
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.eqsl.upload.label')}
                    </span>
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
                    {t('settings.confirmations.eqsl.upload.hint')}
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">QRZ</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.qrz.username.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.qrzUsername}
                    placeholder={t('settings.confirmations.qrz.username.placeholder')}
                    onChange={(e) => update('qrzUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.qrz.username.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.qrz.password.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={qrzPw}
                      placeholder={t('settings.confirmations.qrz.password.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetQrzPassword}
                      title={t('settings.confirmations.credential.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T k="settings.confirmations.qrz.password.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.qrz.apiKey.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={qrzKey}
                      placeholder={t('settings.confirmations.qrz.apiKey.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetQrzLogbookKey}
                      title={t('settings.confirmations.qrz.apiKey.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T k="settings.confirmations.qrz.apiKey.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.qrz.upload.label')}
                    </span>
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
                    {t('settings.confirmations.qrz.upload.hint')}
                  </span>
                </div>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.qrz.autoSync.label')}
                    </span>
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
                    {t('settings.confirmations.qrz.autoSync.hint')}
                    {form.qrzLastSyncUnix > 0 && (
                      <>
                        {' '}
                        {/* Local wall-clock time, formatted by the panel — see the LoTW stamp. */}
                        {t('settings.confirmations.qrz.autoSync.lastPull', {
                          when: new Date(form.qrzLastSyncUnix * 1000).toLocaleString(),
                        })}
                      </>
                    )}
                  </span>
                </div>

                <div className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.qrz.sync.label')}
                  </span>
                  <div className="settings-input-row">
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onSyncQrz}
                      disabled={qrzSyncing}
                      title={t('settings.confirmations.qrz.sync.title')}
                    >
                      {qrzSyncing
                        ? t('settings.confirmations.qrz.sync.busy')
                        : t('settings.confirmations.qrz.sync.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T k="settings.confirmations.qrz.sync.hint" tags={{ b: <strong /> }} />
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">HamQTH</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.hamqth.username.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.hamqthUsername}
                    placeholder={t('settings.confirmations.hamqth.username.placeholder')}
                    onChange={(e) => update('hamqthUsername', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    <T k="settings.confirmations.hamqth.username.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.hamqth.password.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={hamqthPw}
                      placeholder={t('settings.confirmations.hamqth.password.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetHamqthPassword}
                      title={t('settings.confirmations.credential.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    {t('settings.confirmations.hamqth.password.hint')}
                  </span>
                </label>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">ClubLog</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.clublog.email.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.clublogEmail}
                    placeholder={t('settings.confirmations.clublog.email.placeholder')}
                    onChange={(e) => update('clublogEmail', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.clublog.email.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.clublog.callsign.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.clublogCallsign}
                    placeholder={t('settings.confirmations.clublog.callsign.placeholder')}
                    onChange={(e) => update('clublogCallsign', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.clublog.callsign.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.clublog.password.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={clublogPw}
                      placeholder={t('settings.confirmations.clublog.password.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetClublogPassword}
                      title={t('settings.confirmations.clublog.password.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T k="settings.confirmations.clublog.password.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.clublog.apiKey.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.clublogApiKey}
                    placeholder={t('settings.confirmations.clublog.apiKey.placeholder')}
                    onChange={(e) => update('clublogApiKey', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    <T k="settings.confirmations.clublog.apiKey.hint" tags={{ b: <strong /> }} />
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.clublog.upload.label')}
                    </span>
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
                    {t('settings.confirmations.clublog.upload.hint')}
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">HRDLog</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.hrdlog.code.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={hrdlogCode}
                      placeholder={t('settings.confirmations.hrdlog.code.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetHrdlogCode}
                      title={t('settings.confirmations.hrdlog.code.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    {t('settings.confirmations.hrdlog.code.hint')}
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.hrdlog.upload.label')}
                    </span>
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
                    <T k="settings.confirmations.hrdlog.upload.hint" tags={{ b: <strong /> }} />
                  </span>
                </div>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">RepeaterBook</span>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.repeaterbook.token.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={rbToken}
                      placeholder={CONFIRMATION_EXAMPLES.rbToken}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetRbToken}
                      title={t('settings.confirmations.repeaterbook.token.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    <T
                      k="settings.confirmations.repeaterbook.token.hint"
                      tags={{ b: <strong /> }}
                    />
                  </span>
                </label>
              </div>
            </div>
            <div className="settings-featgroup">
              <span className="settings-featgroup-title">Cloudlog / Wavelog</span>
              <p className="settings-note">
                <T k="settings.confirmations.cloudlog.note" tags={{ b: <strong /> }} />
              </p>
              <div className="settings-grid">
                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.cloudlog.url.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    value={form.cloudlogUrl ?? ''}
                    placeholder={CONFIRMATION_EXAMPLES.cloudlogUrl}
                    onChange={(e) => update('cloudlogUrl', e.target.value)}
                    autoComplete="off"
                    spellCheck={false}
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.cloudlog.url.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.cloudlog.stationId.label')}
                  </span>
                  <input
                    className="settings-input"
                    type="text"
                    inputMode="numeric"
                    value={form.cloudlogStationId ?? ''}
                    placeholder="1"
                    onChange={(e) => update('cloudlogStationId', e.target.value)}
                    autoComplete="off"
                  />
                  <span className="settings-hint">
                    {t('settings.confirmations.cloudlog.stationId.hint')}
                  </span>
                </label>

                <label className="settings-field">
                  <span className="settings-label">
                    {t('settings.confirmations.cloudlog.apiKey.label')}
                  </span>
                  <div className="settings-input-row">
                    <input
                      className="settings-input"
                      type="password"
                      value={cloudlogKey}
                      placeholder={t('settings.confirmations.cloudlog.apiKey.placeholder')}
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
                      {t('settings.confirmations.credential.set.action')}
                    </button>
                    <button
                      type="button"
                      className="settings-refresh"
                      onClick={onForgetCloudlogKey}
                      title={t('settings.confirmations.cloudlog.apiKey.forget.title')}
                    >
                      {t('settings.confirmations.credential.forget.action')}
                    </button>
                  </div>
                  <span className="settings-hint">
                    {t('settings.confirmations.cloudlog.apiKey.hint')}
                  </span>
                </label>

                <div className="settings-field">
                  <label className="settings-toggle">
                    <span className="settings-label">
                      {t('settings.confirmations.cloudlog.upload.label')}
                    </span>
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
                  <span className="settings-hint">
                    {t('settings.confirmations.cloudlog.upload.hint')}
                  </span>
                </div>
              </div>
            </div>
          </fieldset>
          </>
          )}
          {/* ---- Field Day ---- */}
          {tab === 'contesting' && (
            /* Migrated to the catalog (i18n phase 2) with the Field Day section below, and
               paired with the POTA/SOTA + Field Day surfaces so the wording matches on both
               sides. Invariant and staying here: the class/category and section placeholders
               (1D, 2O, WI), the section list itself, and the event names, which are the
               events' own (FD_EVENT_NAMES). The rest of this file is NOT migrated — see the
               guard's scope note. */
            <fieldset className="settings-section" id="settings-contest-category">
              <legend>{t('settings.contestCategory.legend')}</legend>
              {/* ONE switch for every QSO-finding assistance source. It takes effect IMMEDIATELY
                  (its own command, not Save) because an operator flips it as an event starts, and
                  a switch that needed a restart mid-contest would be useless. The form field is
                  synced so a later Save cannot write the stale value back. */}
              <label className="settings-field">
                <span className="settings-label">{t('settings.contestCategory.unassisted.label')}</span>
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
                  aria-label={
                    form.unassistedMode
                      ? t('settings.contestCategory.unassisted.aria.end')
                      : t('settings.contestCategory.unassisted.aria.declare')
                  }
                >
                  <span className="toggle-knob" />
                </button>
                <span className="settings-hint">
                  {t('settings.contestCategory.unassisted.hint')}
                </span>
              </label>
              <AssistanceNote
                unassisted={!!form.unassistedMode}
                sinceUnix={assistLog[0]?.tsUnix ?? null}
              />
              {assistLog.length > 0 && (
                <div className="assist-journal">
                  <span className="settings-label">{t('settings.contestCategory.journal.label')}</span>
                  <ul className="assist-journal-list mono">
                    {assistLog.slice(0, 8).map((e) => (
                      <li key={`${e.tsUnix}-${e.note}`}>
                        <span className="assist-journal-ts">
                          {new Date(e.tsUnix * 1000).toISOString().slice(0, 16).replace('T', ' ')}Z
                        </span>
                        <span className={`assist-journal-state${e.unassisted ? ' unassisted' : ''}`}>
                          {e.unassisted
                            ? t('settings.contestCategory.journal.unassisted')
                            : t('settings.contestCategory.journal.assisted')}
                        </span>
                        {/* The note and the source names are the engine's own words. */}
                        <span className="assist-journal-note">
                          {e.note}
                          {': '}
                          {e.sources.filter((x) => x.active).map((x) => x.name).join(', ') ||
                            t('settings.contestCategory.journal.noSources')}
                        </span>
                      </li>
                    ))}
                  </ul>
                  <span className="settings-hint">
                    <T k="settings.contestCategory.journal.hint" tags={{ code: <code /> }} />
                  </span>
                </div>
              )}
            </fieldset>
          )}

          {tab === 'contesting' && (
          <fieldset className="settings-section" id="settings-field-day">
            <legend>{t('settings.fieldDay.legend')}</legend>
            {/* The Field Day MASTER lives here now (Contesting is always visible) — turning it on
                reveals the FD workspace + Class/Section exchange across all modes. */}
            <label className="settings-field">
              <span className="settings-label">{t('settings.fieldDay.mode.label')}</span>
              <button
                type="button"
                role="switch"
                aria-checked={!!form.fdActive}
                className={`toggle${form.fdActive ? ' on' : ''}`}
                onClick={() => updateBool('fdActive', !form.fdActive)}
                aria-label={
                  form.fdActive
                    ? t('settings.fieldDay.mode.aria.disable')
                    : t('settings.fieldDay.mode.aria.enable')
                }
              >
                <span className="toggle-knob" />
              </button>
              <span className="settings-hint">{t('settings.fieldDay.mode.hint')}</span>
            </label>
            {form.fdActive && (!form.fdClass.trim() || !form.fdSection.trim()) && (
              <p className="settings-note">
                <T k="settings.fieldDay.needExchange" tags={{ b: <strong /> }} />
              </p>
            )}
            <div className="settings-grid">
              <div className="settings-field">
                <span className="settings-label">{t('settings.fieldDay.event.label')}</span>
                <div className="theme-switcher" role="group" aria-label={t('settings.fieldDay.event.aria')}>
                  {/* The event NAMES are the events' own — invariant, shared with the header. */}
                  {([
                    { value: 'arrlfd', label: FD_EVENT_NAMES.arrlfd },
                    { value: 'wfd',    label: FD_EVENT_NAMES.wfd },
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
                <span className="settings-hint">{t('settings.fieldDay.event.hint')}</span>
              </div>

              <label className="settings-field">
                <span className="settings-label">
                  {(form.fdEvent ?? 'arrlfd') === 'wfd'
                    ? t('settings.fieldDay.category.label')
                    : t('settings.fieldDay.class.label')}
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
                    ? t('settings.fieldDay.category.hint')
                    : t('settings.fieldDay.class.hint')}
                </span>
              </label>

              <label className="settings-field">
                <span className="settings-label">{t('settings.fieldDay.section.label')}</span>
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
                    {t('settings.fieldDay.section.invalid', { section: form.fdSection })}
                  </span>
                )}
                <span className="settings-hint">
                  {t('settings.fieldDay.section.hint', { count: FD_SECTION_OPTIONS.length })}
                </span>
              </label>

              <div className="settings-field">
                <span className="settings-label">{t('settings.fieldDay.power.label')}</span>
                <div className="theme-switcher" role="group" aria-label={t('settings.fieldDay.power.aria')}>
                  {([
                    { value: 5, labelKey: 'settings.fieldDay.power.qrp.label',     hintKey: 'settings.fieldDay.power.qrp.hint' },
                    { value: 2, labelKey: 'settings.fieldDay.power.hundred.label', hintKey: 'settings.fieldDay.power.hundred.hint' },
                    { value: 1, labelKey: 'settings.fieldDay.power.high.label',    hintKey: 'settings.fieldDay.power.high.hint' },
                  ] as const).map((p) => (
                    <button
                      key={p.value}
                      type="button"
                      className={`theme-chip${(form.fdPowerMult ?? 1) === p.value ? ' active' : ''}`}
                      aria-pressed={(form.fdPowerMult ?? 1) === p.value}
                      title={t(p.hintKey)}
                      onClick={() => {
                        markDirty()
                        setForm((prev) => prev ? { ...prev, fdPowerMult: p.value } : prev)
                      }}
                    >
                      {t(p.labelKey)}
                    </button>
                  ))}
                </div>
                <span className="settings-hint">{t('settings.fieldDay.power.hint')}</span>
              </div>
            </div>
          </fieldset>
          )}
        </div>

        <div className="settings-actions">
          {/* Warnings from the last save. They did NOT block it -- the save went through -- so
              they are stated once and left visible rather than interrupting. An operator whose
              setup is unusual but correct must be able to read them and carry on. */}
          {rigChecks
            .filter((c) => c.level === 'warning')
            .map((c) => (
              <span className="settings-warn" role="status" key={c.message}>
                {c.message}
              </span>
            ))}
          {error && <span className="settings-error" role="alert">{error}</span>}
          {status === 'saved' && !error && (
            <span className="settings-ok" role="status">{t('settings.panel.saved')}</span>
          )}
          <button
            type="submit"
            className="settings-save"
            // Not disabled on an empty callsign — clicking routes to the Station tab with a clear
            // message (handleSubmit), rather than a greyed button that gives no reason or fix.
            disabled={status === 'saving'}
          >
            {status === 'saving' ? t('settings.panel.saving') : t('settings.panel.save')}
          </button>
        </div>
      </form>
    </section>
    </SettingsOpenTarget.Provider>
  )
}
