// Settings registry — the single source of truth for WHERE a setting lives.
//
// WHY THIS EXISTS. Settings has been reorganised four times (0.8.2 sub-sections, 0.9.2
// mode-first, 0.17.0 fourteen→eight tabs, plus a stream of individual relocations), and each
// time the arrangement decayed again because nothing declared where a new field belonged. The
// panel refills at roughly three fields a day; the day before this registry was written, a
// global metric/imperial preference landed inside `<legend>Digital (FT8/FT4)</legend>`. A
// placement that lives only in the shape of an 8,000-line JSX file cannot be checked, searched,
// or documented — so it drifts, and the next reorg is the only tool anyone has.
//
// This module makes placement DECLARED data instead. Three things read it:
//   1. deep links  — `resolveTarget()` turns "Settings ▸ Audio" into a tab + an anchor, so the
//                    app's ~228 prose pointers can actually take an operator there.
//   2. the docs    — `docs/guide/settings-reference.md` is generated from these tables, so a
//                    move updates the manual instead of stranding it (the 0.17 consolidation
//                    left nineteen files pointing at tabs that no longer existed).
//   3. the guards  — registry.test.ts checks the panel renders exactly these sections, that the
//                    Advanced rule holds, and that no section grows past the density ceiling.
//
// It is pure data + pure helpers (no React, no storage), so it is fully unit-testable in node —
// the same shape as `features/registry.ts`, which has served the same purpose for 22 features.
//
// ⚠️ A SECTION'S `id` IS A PUBLIC NAME. It is the anchor in the manual, the target of every
// deep link, and the slug in `settings-reference.md`. Ids are deliberately INDEPENDENT of the
// tab a section currently sits on, so moving a section between tabs is a one-word data edit
// that breaks no link. Rename an id only with a matching entry in `SECTION_ALIASES`.

/** A tab on the Settings rail. The rail is horizontal and must fit one row at the
 * supported 1024×768 floor, so labels stay short. */
export type SettingsTabId =
  | 'station'
  | 'radio'
  | 'phone'
  | 'cw'
  | 'digital'
  | 'spots'
  | 'logging'
  | 'contesting'
  | 'appearance'
  | 'configurations'

export interface SettingsTabDef {
  id: SettingsTabId
  label: string
}

/** The tab rail, in render order. `SettingsPanel.tsx` re-exports its own `SETTINGS_TABS`
 * from this list — `docs-match-code.test.ts` parses that declaration and asserts the manual's
 * `##` headings equal these labels IN ORDER, so this array and the manual move together. */
export const SETTINGS_TABS: SettingsTabDef[] = [
  { id: 'station', label: 'Station' },
  { id: 'radio', label: 'Radio' },
  // The three mode tabs mirror the nav rail's own top-level shape (Phone · CW · Digital), which
  // is the taxonomy the operator already navigates. It is also the only axis that can house SSTV
  // and APRS: neither has an `OperatingMode` variant in the backend (SSTV rides Phone, APRS rides
  // FM, settings.rs:1848-1851), so a split built on the rig-mode policy structurally cannot hold
  // them. Digital parents the six rail sub-items.
  { id: 'phone', label: 'Phone' },
  { id: 'cw', label: 'CW' },
  { id: 'digital', label: 'Digital' },
  { id: 'spots', label: 'Spots & Alerts' },
  { id: 'logging', label: 'Logging & Connectors' },
  { id: 'contesting', label: 'Contesting' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'configurations', label: 'Config' },
]

export interface SettingsSectionDef {
  /** Stable public anchor. Never encodes the tab — see the header warning. */
  id: string
  /** Plain-text form of the `<legend>` the panel renders (no HTML entities). */
  label: string
  tab: SettingsTabId
  /** Words an operator would actually type or scan for, that the label does NOT contain.
   * Every persona walkthrough found the searchable vocabulary lives in hint text, not
   * labels — "COM port" and "sound card" appear in neither `Rig Control` nor `Audio`. */
  keywords: string[]
  /** Rendered inside a collapsed `SettingsGroup` disclosure. A deep link to anything in
   * here must expand it on the way — a target the operator cannot see has not been found. */
  advanced?: boolean
  /** The section id this is nested inside, for `SettingsGroup` disclosures. */
  parent?: string
  /** True when a first-hour operator needs this to get on the air. Paired with `advanced`
   * by the rule test below: nothing needed in hour one may hide in a disclosure. */
  neededInHourOne?: boolean
}

/**
 * Every named section of the Settings panel, in render order within its tab.
 *
 * ⚠️ THIS DESCRIBES THE PANEL AS IT IS, NOT AS IT SHOULD BE. It was written before the IA
 * restructure deliberately, so the guard tests could prove it matches today's panel first.
 * The restructure then edits the `tab` fields here and the JSX wrappers together.
 */
export const SETTINGS_SECTIONS: SettingsSectionDef[] = [
  // ---- Station -----------------------------------------------------------------
  {
    id: 'operator-radio',
    label: 'Operator & Radio',
    tab: 'station',
    neededInHourOne: true,
    keywords: ['callsign', 'call sign', 'grid', 'maidenhead', 'locator', 'license', 'licence',
      'class', 'extra', 'general', 'technician', 'operator name', 'state', 'my call'],
  },

  // ---- Radio -------------------------------------------------------------------
  {
    id: 'radios',
    label: 'Radios',
    tab: 'radio',
    neededInHourOne: true,
    keywords: ['roster', 'second radio', 'two radios', 'dual radio', 'so2r', 'routing',
      'band routing', 'add radio', 'which radio'],
  },
  {
    id: 'profiles',
    label: 'Profiles',
    tab: 'radio',
    keywords: ['home', 'field', 'portable', 'saved setup', 'preset'],
  },
  {
    // ⚠️ THE NAME CARRIES AN EXCLUSION CRITERION, DELIBERATELY: if it is not a model, port,
    // baud, framing or keying line, it does not belong here. "Rig Control" had none — a
    // subsystem heading in a radio app admits anything rig-adjacent — so it accreted the
    // rotator, band-edge tones, per-mode power caps, the setup backup, rig sharing and
    // foreign-PTT permission until it was 1,011 lines and the audio settings had been pushed
    // over a thousand lines away from the COM port they belong beside. Keep the boundary.
    id: 'rig-control',
    label: 'Rig & CAT',
    tab: 'radio',
    neededInHourOne: true,
    keywords: ['cat', 'com port', 'serial port', 'usb', 'baud', 'ptt', 'rig model', 'hamlib',
      'rigctld', 'flex', 'smartsdr', 'icom', 'ci-v', 'civ', 'yaesu', 'kenwood', 'detect',
      'test cat', 'split', 'stop bits', 'parity', 'handshake', 'data bits', 'rig control'],
  },
  {
    id: 'rig-advanced',
    label: 'Advanced',
    tab: 'radio',
    parent: 'rig-control',
    advanced: true,
    keywords: ['rigctld port', 'cat broker', 'sharing port', 'native ci-v', 'flex ip',
      'panadapter', 'dax', 'diagnostic log', 'plain ssb', 'data modes', 'no rf', 'red light',
      'rigblaster', 'mic jack', 'pktusb', 'data-u', 'usb-d',
      // The #145 declarations. The words here are the SYMPTOM, not the setting name — an
      // operator whose rig keys the moment Nexus opens does not search for "handshake".
      'serial handshake', 'flow control', 'xonxoff', 'rts state', 'dtr state', 'keying line',
      'keys at launch', 'transmits at startup', 'stuck ptt', 'stuck transmit'],
  },
  {
    id: 'audio',
    label: 'Audio',
    tab: 'radio',
    neededInHourOne: true,
    keywords: ['sound card', 'soundcard', 'audio device', 'input', 'output', 'codec', 'mic',
      'microphone', 'speaker', 'usb audio', 'cm108', 'dax', 'tx level', 'rx gain', 'drive',
      'alc', 'power', 'levels'],
  },
  {
    id: 'headphone-monitor',
    // The ID stays `headphone-monitor` — it is a deep-link target and renaming it breaks links
    // that already exist. The LABEL is what the operator reads, and "monitor" told them the
    // opposite of what this does; see the note beside these strings in en.ts.
    label: 'Receive audio on this computer',
    tab: 'radio',
    // 'monitor' stays a KEYWORD even though it left the label: an operator who learned the old
    // name, or who reasons from the rig's MONI control, must still land here — and landing here is
    // how they discover it is the receive side. Searching for the wrong word is not a wrong search.
    keywords: ['headphones', 'monitor', 'listen', 'receive audio', 'rx audio', 'speakers',
      'sidetone out', 'passthrough'],
  },
  {
    id: 'satellite-doppler',
    label: 'Satellite Doppler',
    tab: 'radio',
    keywords: ['satellite', 'sat', 'bird', 'doppler', 'vfo map', 'uplink', 'downlink',
      'full duplex', 'linear', 'transponder'],
  },
  {
    id: 'orbital-elements',
    label: 'Orbital elements',
    tab: 'radio',
    keywords: ['tle', 'keps', 'keplerian', 'celestrak', 'satnogs', 'orbit', 'satellite'],
  },
  {
    id: 'rotator',
    label: 'Rotator',
    tab: 'radio',
    keywords: ['rotor', 'rotator', 'azimuth', 'elevation', 'park', 'beam', 'antenna turn',
      'rotctld', 'yaesu g-5500'],
  },
  {
    id: 'amplifier',
    label: 'Amplifier',
    tab: 'radio',
    // Top-level on the Radio tab, NOT nested under rig-control: an amplifier is a per-radio
    // EXTERNAL DEVICE on its own serial port, exactly the shape of the rotator above, and
    // rig-control's own header states the exclusion — "if it is not a model, port, baud,
    // framing or keying line, it does not belong here".
    //
    // Keywords are ENGLISH-ONLY DATA and are never translated (the search matches these
    // strings, not the rendered legend), and at least one must be a word the label does not
    // already contain — 'amp' alone is a substring of 'amplifier' and the registry guard
    // rejects a section whose every keyword is.
    keywords: ['linear', 'spe', 'expert', '1.3k-fa', '1.5k-fa', '2k-fa', 'elecraft',
      'kpa500', 'kpa1500', 'swr', 'pa temp', 'watts out'],
  },
  {
    id: 'transmit-limits',
    label: 'Transmit limits & sharing',
    tab: 'radio',
    // Backup, restore and reset all MOVED to the Config tab, so their keywords went with them —
    // a search term that lands the operator on a section no longer holding the control is worse
    // than no keyword at all. What stays here is what this section still does.
    keywords: ['band edge', 'edge tone', 'max power', 'power limit', 'watts', 'safety',
      'share rig', 'rigctld address', 'other programs',
      'foreign ptt', 'wsjt-x share', 'n1mm share'],
  },

  // ---- Modes -------------------------------------------------------------------
  {
    id: 'digital-ft8-ft4',
    label: 'Digital (FT8/FT4)',
    tab: 'digital',
    neededInHourOne: true,
    // `tune`/`tune timeout` are here because the Tune timeout — the auto-release on a key-down
    // carrier — sits in this section's "Transmit & Sequencing" group and had NO searchable word
    // anywhere in the registry, so search returned nothing and no deep link could name it.
    keywords: ['ft8', 'ft4', 'auto sequence', 'sequencing', 'tx enable', 'watchdog', 'decode',
      'depth', 'deep', 'auto cq', 'cq', 'hound', 'fox', 'dxpedition', 'auto log', 'blocked',
      'ap decode', 'f low', 'f high', 'tune', 'tune timeout', 'tune carrier', 'key down',
      'tx period', 't/r period', 'disable tx after 73', 'tune power', 'low power tune',
      'atu power', 'loop antenna', 'db reports', 'comments', 'reports to comments'],
  },
  {
    id: 'jt65',
    label: 'JT65 — classic EME',
    tab: 'digital',
    keywords: ['jt65', 'eme', 'moonbounce', 'submode'],
  },
  {
    id: 'msk144',
    label: 'MSK144 — meteor scatter',
    tab: 'digital',
    keywords: ['msk144', 'meteor', 'scatter', 'ping', 'six metres', '6m'],
  },
  {
    id: 'beacons-wspr-fst4w',
    label: 'Beacons — WSPR & FST4W',
    tab: 'digital',
    keywords: ['wspr', 'whisper', 'fst4w', 'beacon', 'propagation report', 'tx percent',
      'dbm', 'round robin'],
  },
  {
    id: 'fst4',
    label: 'FST4 (QSO) / FST4W (beacon)',
    tab: 'digital',
    keywords: ['fst4', 'fst4w', 'lf', 'mf', '2200m', '630m', 'period'],
  },
  {
    id: 'q65',
    label: 'Q65 — EME / VHF+ scatter',
    tab: 'digital',
    keywords: ['q65', 'eme', 'moonbounce', 'aircraft scatter', 'submode', 'vhf', 'uhf'],
  },
  {
    id: 'quick-reply-macros',
    label: 'Quick-reply macros',
    tab: 'digital',
    keywords: ['macro', 'quick reply', 'canned', 'message', 'chat', 'shortcut'],
  },
  {
    id: 'phone',
    label: 'Phone (SSB / FM)',
    tab: 'phone',
    neededInHourOne: true,
    keywords: ['ssb', 'usb', 'lsb', 'voice', 'fm', 'repeater', 'shift', 'offset', 'ctcss',
      'pl tone', 'tone', 'mic', 'microphone', 'voice keyer'],
  },
  {
    id: 'cw',
    label: 'CW',
    tab: 'cw',
    neededInHourOne: true,
    keywords: ['morse', 'keyer', 'winkeyer', 'paddle', 'straight key', 'wpm', 'speed',
      'sidetone', 'pitch', 'keyline', 'f-key', 'macro', 'iambic'],
  },
  {
    id: 'rtty',
    label: 'RTTY',
    tab: 'digital',
    keywords: ['rtty', 'baudot', 'fsk', 'afsk', 'shift', 'baud', '45.45', '170', 'reverse',
      'mark', 'space', 'auto arm', 'start receiving', 'not decoding'],
  },
  {
    id: 'psk',
    label: 'PSK',
    tab: 'digital',
    keywords: ['psk31', 'bpsk', 'varicode', 'keyboard', 'ragchew', 'warble', '14.070',
      'afc', 'auto arm', 'start receiving', 'narrow band', 'digipan'],
  },
  {
    id: 'sstv',
    label: 'SSTV',
    tab: 'digital',
    keywords: ['slow scan', 'slow-scan', 'television', 'picture', 'image', 'photo', 'scottie',
      'martin', 'robot', 'pd120', 'iss', 'ariss', '145.800', '14.230', 'gallery'],
  },
  {
    id: 'aprs',
    label: 'APRS',
    tab: 'digital',
    keywords: ['aprs', 'aprs-is', 'igate', 'beacon', 'symbol', 'digipeater', 'path', 'wide1-1',
      'ssid', 'position', 'weather', 'packet', 'tnc', '144.390', '144.800', 'channel',
      'frequency', 'comment', 'symbol table', 'over the air'],
  },

  // ---- Frequencies -------------------------------------------------------------
  {
    id: 'working-frequencies',
    label: 'Working Frequencies',
    tab: 'digital',
    keywords: ['frequency', 'dial', 'band plan', 'override', 'calling frequency', 'mhz',
      'ft8 frequency', 'ft4 frequency'],
  },

  // ---- Spots & Alerts ----------------------------------------------------------
  {
    id: 'pounce',
    label: 'Pounce — new-one alert',
    tab: 'spots',
    keywords: ['pounce', 'atno', 'new one', 'alert', 'needed', 'threshold'],
  },
  {
    id: 'alerts',
    label: 'Alerts',
    tab: 'spots',
    keywords: ['alert', 'notify', 'my call', 'cq', 'new dxcc', 'new grid', 'rare', 'watch list',
      'wanted', 'sound', 'lotw', 'confirm', 'confirmation', 'confirm tier'],
  },

  // ---- Logging & Connectors ----------------------------------------------------
  {
    id: 'connections',
    label: 'Connections',
    tab: 'logging',
    keywords: ['status', 'connector', 'health', 'log', 'connection log', 'test', 'upload',
      'not uploading', 'last upload', 'failing', 'revoked', 'wrong password', 'green'],
  },
  {
    id: 'connections-b4',
    label: 'Worked-before (B4) & dupes',
    tab: 'logging',
    keywords: ['b4', 'worked before', 'dupe', 'duplicate', 'match mode', 'band', 'highlight',
      'wsjt-x', 'already worked'],
  },
  {
    id: 'integrations-feeds',
    label: 'Integrations & Feeds',
    tab: 'logging',
    keywords: ['wsjt-x', 'udp', 'gridtracker', 'jtalert', 'hrd', 'ham radio deluxe', 'psk',
      'pskreporter', 'cluster', 'rbn', 'reverse beacon', 'spot source', 'all.txt', 'wav',
      'recording', 'propagation', 'diagnostic log', 'nexus-diag.log', 'log file', 'debug log',
      'troubleshooting', 'bug report', 'crash'],
  },
  {
    id: 'antenna-gain',
    label: 'Antenna gain (advanced)',
    tab: 'logging',
    parent: 'integrations-feeds',
    advanced: true,
    keywords: ['antenna', 'gain', 'dbi', 'p.533', 'prediction'],
  },
  {
    id: 'dxkeeper',
    label: 'DXKeeper (DXLab Suite)',
    tab: 'logging',
    keywords: ['dxkeeper', 'dxlab', 'commander', 'logger'],
  },
  {
    id: 'n3fjp',
    label: 'N3FJP Integration (club master log)',
    tab: 'logging',
    keywords: ['n3fjp', 'club log', 'field day', 'master log', 'amateur contact log'],
  },
  {
    id: 'n1mm',
    label: 'N1MM+ Integration',
    tab: 'logging',
    keywords: ['n1mm', 'contest logger', 'broadcast', 'contactinfo'],
  },
  {
    id: 'lotw-users',
    label: 'LoTW users list',
    tab: 'logging',
    keywords: ['lotw', 'logbook of the world', 'users', 'marks', 'known users'],
  },
  {
    id: 'callsign-state',
    label: 'Callsign → state database',
    tab: 'logging',
    keywords: ['fcc', 'state', 'was', 'callsign database', 'us states'],
  },
  {
    id: 'country-file',
    label: 'Country file (DXCC)',
    tab: 'logging',
    keywords: ['cty', 'cty.dat', 'dxcc', 'country file', 'entities', 'ad1c', 'prefix',
      'country'],
  },
  {
    id: 'confirmations',
    label: 'Confirmations',
    tab: 'logging',
    neededInHourOne: true,
    keywords: ['lotw', 'eqsl', 'qrz', 'clublog', 'club log', 'hrdlog', 'cloudlog', 'wavelog',
      'hamqth', 'qsl', 'upload', 'auto-upload', 'password', 'api key', 'credential', 'login',
      'callbook', 'repeaterbook', 'tqsl', 'station location'],
  },

  // ---- Contesting --------------------------------------------------------------
  {
    id: 'contest-category',
    label: 'Contest Category',
    tab: 'contesting',
    keywords: ['contest', 'assisted', 'unassisted', 'category', 'entry'],
  },
  {
    id: 'field-day',
    label: 'Field Day Setup',
    tab: 'contesting',
    keywords: ['field day', 'arrl', 'class', 'section', 'exchange', 'power multiplier',
      'winter field day', 'rules', 'rules year', 'rules update', 'wfd'],
  },
  // A club site answers "who are you?" three different ways — the call that goes on the air,
  // the tent you are sitting in, and the person at the key — and all three already existed as
  // settings (`mycall`, `fd_position_name`, `fd_operator`). What did not exist was one place
  // that showed them as a set: two lived on Station, the third sat under a networking heading
  // on Contesting, and the club report that produced this section is an operator asking what
  // the position name was even for. So this is a VIEW of three existing fields, deliberately
  // not new state, placed where a Field Day operator meets them.
  {
    id: 'field-day-identity',
    label: "Who's who at this event",
    tab: 'contesting',
    keywords: ['callsign', 'club call', 'position name', 'position', 'tent', 'trailer',
      'operator', 'operator at the key', 'multi-op', 'multiop', 'swap seats', 'who is operating',
      'station name', 'my call'],
  },
  {
    id: 'field-day-club',
    label: 'Field Day Club Sync',
    tab: 'contesting',
    // 'position' and 'tent' moved to `field-day-identity` with the control they name. Search
    // scores an exact keyword above everything but a label, and ties break alphabetically, so
    // leaving either word here would have kept sending "position"/"tent" to the networking
    // section — which is exactly the section the operator could not make sense of.
    keywords: ['club', 'sync', 'host', 'join', 'multi-op', 'multiop', 'lan',
      'network', 'band board', 'scoreboard', 'dupe sharing', 'discover', 'find club events',
      'club log', 'spectator', 'tv', 'projector', 'big screen'],
  },

  // ---- Appearance --------------------------------------------------------------
  {
    id: 'workspace',
    label: 'Workspace',
    tab: 'appearance',
    keywords: ['theme', 'dark', 'light', 'ui scale', 'text size', 'font size', 'zoom',
      'density', 'compact', 'pane', 'layout'],
  },
  {
    // The read-only LAN page. Filed under Appearance because it is a way of LOOKING at
    // Connect, not a station or contest setting — and the keywords carry the words an
    // operator would actually search for ("tv", "chromecast", "cast", "browser").
    id: 'connect-web',
    label: 'Connect on a TV',
    tab: 'appearance',
    keywords: ['tv', 'television', 'big screen', 'wall display', 'browser', 'lan', 'hamclock',
      'network', 'web page', 'cast', 'chromecast', 'firestick', 'fire stick', 'tablet',
      'phone', 'remote view', 'read only', 'shack tv'],
  },
  {
    id: 'features',
    label: 'Features',
    tab: 'appearance',
    keywords: ['sections', 'enable', 'disable', 'turn off', 'hide', 'profile', 'goal',
      'pota', 'setup wizard', 'field day mode'],
  },
  {
    id: 'accessibility',
    label: 'Accessibility & eyes-free',
    tab: 'appearance',
    keywords: ['screen reader', 'announce', 'blind', 'earcon', 'sound', 'a11y', 'speech',
      'eyes free', 'tick'],
  },

  // ---- Config ------------------------------------------------------------------
  // Backup and Restore previously sat under Radio -> "Transmit limits & sharing", which is why
  // no one found them: backing up a whole station has nothing to do with transmit limits. The
  // keywords are deliberately wide because this is what an operator searches for in a panic.
  {
    id: 'configurations',
    label: 'Backup & reset',
    tab: 'configurations',
    keywords: ['backup', 'back up', 'restore', 'reset', 'factory', 'defaults', 'start over',
      'export', 'import', 'export settings', 'new computer', 'migrate', 'move to a new laptop',
      'wipe', 'clean slate', 'start again', 'settings file'],
  },
]

/** Tab names that USED to exist, mapped to where their content lives now. A prose pointer or a
 * doc link written against an older layout must still land somewhere sensible — the 0.17
 * consolidation left nineteen files citing dead tabs, and four of those names are STILL live in
 * the tree today (`Features` ×11, `Logbook & QSL` ×6, `Integrations` ×4, `Connections` ×3).
 * Lower-cased on lookup. */
export const TAB_ALIASES: Record<string, SettingsTabId> = {
  // The tab this reorganisation split. `Modes` was one page holding eleven fieldsets; it is now
  // Phone, CW and Digital. A bare "Settings ▸ Modes" can no longer name one page, so it lands on
  // Digital — the tab that kept the majority of what Modes held (FT8/FT4, the weak-signal tiers,
  // RTTY, SSTV, APRS, Tempo). A pointer that also names its section (the overwhelming majority,
  // e.g. "Settings ▸ Modes ▸ CW") never reaches this line: `resolveTarget` matches the section
  // first and lands exactly.
  modes: 'digital',
  // Folded INTO Digital, renamed "Working frequencies (FT8/FT4)" — the table only ever held
  // FT8/FT4 rows (`FREQ_MODES`), while its top-level name promised park, net and repeater
  // frequencies it does not have.
  frequencies: 'digital',
  // 0.17 consolidation (fourteen → eight)
  'logbook & qsl': 'logging',
  'logbook and qsl': 'logging',
  integrations: 'logging',
  'integrations & feeds': 'logging',
  connections: 'logging',
  qsl: 'logging',
  features: 'appearance',
  rig: 'radio',
  'rig / cat': 'radio',
  'rig control': 'radio',
  cat: 'radio',
  audio: 'radio',
  alerts: 'spots',
  'watch list': 'spots',
  'spot sources': 'spots',
  satellites: 'radio',
  'field day': 'contesting',
  workspace: 'appearance',
  appearance: 'appearance',
}

/** Section ids that have been renamed. Same contract as `TAB_ALIASES`, one level down. */
export const SECTION_ALIASES: Record<string, string> = {
  'rig-cat': 'rig-control',
  'rig--cat': 'rig-control',
  'audio-devices': 'audio',
  'sound-card': 'audio',
  'logbook-qsl': 'confirmations',
  'qsl-services': 'confirmations',
  'setup-health': 'radios',
}

/** Where a deep link should land: a tab, and optionally a section to scroll to. */
export interface SettingsTarget {
  tab: SettingsTabId
  /** A `SETTINGS_SECTIONS` id. The panel scrolls it into view and expands it if collapsed. */
  section?: string
}

const norm = (s: string) => s.trim().toLowerCase()

/** Slug used for anchors and for the generated manual's heading ids. Mirrors the `slug()` in
 * `docs-match-code.test.ts` so a section's DOM anchor and its doc anchor agree. */
export function sectionSlug(label: string): string {
  return label
    .toLowerCase()
    .replace(/[^a-z0-9\s-]/g, '')
    .trim()
    .replace(/\s+/g, '-')
}

export function tabById(id: string): SettingsTabDef | undefined {
  return SETTINGS_TABS.find((t) => t.id === id)
}

export function sectionById(id: string): SettingsSectionDef | undefined {
  // Normalise on BOTH sides. Ids are lowercase by convention, but callers pass the operator's
  // own spelling — prose says "Settings ▸ Q65", "MSK144", "JT65", never "q65". Comparing a raw
  // caller string against a lowercase id silently returned undefined for every mode written the
  // way a ham writes it, which is precisely the dead end this module exists to prevent.
  const resolved = norm(SECTION_ALIASES[norm(id)] ?? id)
  return SETTINGS_SECTIONS.find((s) => s.id === resolved)
}

export function sectionsForTab(tab: SettingsTabId): SettingsSectionDef[] {
  return SETTINGS_SECTIONS.filter((s) => s.tab === tab)
}

/**
 * Resolve a human or legacy reference into a landing place.
 *
 * Accepts, in order of precision: a section id (`'audio'`), a tab id (`'radio'`), a tab LABEL
 * (`'Logging & Connectors'`), a legacy tab name (`'Logbook & QSL'`), or a path
 * (`'Radio ▸ Audio'`, `'Settings > Modes > CW'`). Returns `null` only when nothing matches,
 * so a caller can fall back to opening Settings plainly rather than doing nothing.
 *
 * A stale pointer MUST degrade to a landing, never to a dead end — that is the whole reason
 * the alias tables exist.
 */
export function resolveTarget(raw: string): SettingsTarget | null {
  if (!raw) return null
  // Strip a leading "Settings" and split on the separators the app and docs actually use.
  const parts = raw
    .split(/[▸>›»→]|->/)
    .map((p) => p.trim())
    .filter((p) => p && norm(p) !== 'settings')
  if (!parts.length) return null

  // Walk right-to-left: the most specific trailing part wins, so "Radio ▸ Audio" lands on the
  // Audio section rather than merely the Radio tab.
  for (let i = parts.length - 1; i >= 0; i--) {
    const p = parts[i]
    const bySectionId = sectionById(p)
    if (bySectionId) return { tab: bySectionId.tab, section: bySectionId.id }
    const bySectionLabel = SETTINGS_SECTIONS.find((s) => norm(s.label) === norm(p))
    if (bySectionLabel) return { tab: bySectionLabel.tab, section: bySectionLabel.id }
    const bySlug = SETTINGS_SECTIONS.find((s) => sectionSlug(s.label) === norm(p))
    if (bySlug) return { tab: bySlug.tab, section: bySlug.id }
  }
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i]
    const tab = SETTINGS_TABS.find((t) => t.id === norm(p) || norm(t.label) === norm(p))
    if (tab) {
      // A tab matched; see whether a later part names one of ITS sections.
      const rest = parts.slice(i + 1)
      for (const r of rest) {
        const sec = sectionsForTab(tab.id).find(
          (s) => norm(s.label) === norm(r) || s.id === norm(r),
        )
        if (sec) return { tab: tab.id, section: sec.id }
      }
      return { tab: tab.id }
    }
    const alias = TAB_ALIASES[norm(p)]
    if (alias) {
      // A legacy name may ALSO be a live section id (e.g. "Audio", "CW") — prefer the section
      // so the operator lands on the control, not merely the page that now contains it.
      const sec = SETTINGS_SECTIONS.find((s) => norm(s.label) === norm(p) || s.id === norm(p))
      return sec ? { tab: sec.tab, section: sec.id } : { tab: alias }
    }
  }
  return null
}

// ---------------------------------------------------------------------------
// Search — 0.17's item B6, three batches late.
//
// The audit that produced the 0.17 plan called 230 controls without search
// "disqualifying"; it is 280-odd now. Two prior reorganisations shipped instead, and each
// decayed, because rearranging is a way of hoping the operator guesses the same shape you did,
// while search lets them say what they want in their own words.
//
// ⚠️ IT SEARCHES `keywords`, NOT JUST LABELS, AND THAT IS THE WHOLE POINT. Every persona
// walkthrough found the vocabulary operators actually type lives in HINT text, not in headings:
// nothing in "Rig & CAT" or "Audio" contains "COM port" or "sound card", and issue #62 was an
// operator who could not find a toggle that had already shipped. The keyword lists exist to carry
// those words, and `registry.test.ts` refuses a keyword that merely repeats its own label.
// ---------------------------------------------------------------------------

export interface SettingsHit {
  section: SettingsSectionDef
  /** What actually matched — the UI shows it so a result never looks arbitrary ("Audio —
   * matched 'sound card'"). */
  matched: string
  /** Higher is better. Only the ordering matters; the numbers are not an API. */
  score: number
}

/** Lowercase, with punctuation reduced to single spaces — the form used for word splitting. */
const fold = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim()

/** Lowercase with punctuation REMOVED, so a hyphen the operator did or did not type stops
 * mattering: `wsjt-x` ⇄ `wsjtx`, `ci-v` ⇄ `civ`, `pl-tone` ⇄ `pl tone`.
 *
 * ⚠️ BOTH FORMS ARE NEEDED, and the first version of this shipped only the space form while its
 * comment claimed this behaviour. `fold('wsjt-x')` is `'wsjt x'`, which does NOT contain
 * `'wsjtx'` — so the hyphen-insensitivity was a comment, not a feature. The test that was
 * supposed to cover it passed anyway, because both spellings happened to be listed as keywords;
 * it proved nothing until a deliberately-broken `fold` failed to break it. */
const squash = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '')

/**
 * Rank the settings sections against what the operator typed.
 *
 * Scoring, highest first: an exact label; a label that starts with the query; an exact keyword;
 * a keyword that starts with the query; a label containing it; a keyword containing it. A
 * multi-word query must match every word somewhere in the section (label, id or keywords), so
 * "audio device" finds Audio while "audio contest" finds nothing rather than both.
 *
 * Returns `[]` for a blank query — the caller shows nothing rather than the whole panel.
 */
export function searchSettings(query: string, limit = 8): SettingsHit[] {
  const q = fold(query)
  if (!q) return []
  const words = q.split(' ').filter(Boolean)

  const hits: SettingsHit[] = []
  for (const section of SETTINGS_SECTIONS) {
    const label = fold(section.label)
    const id = fold(section.id)
    const keywords = section.keywords.map((k) => ({ raw: k, folded: fold(k), squashed: squash(k) }))
    const labelSquashed = squash(section.label)
    const qs = squash(query)
    // Every word must land somewhere, or a two-word query would match on its weakest half.
    const everyWordLands = words.every(
      (w) =>
        label.includes(w) ||
        id.includes(w) ||
        labelSquashed.includes(squash(w)) ||
        keywords.some((k) => k.folded.includes(w) || k.squashed.includes(squash(w))),
    )
    if (!everyWordLands) continue

    let score = 0
    let matched = section.label
    if (label === q) score = 100
    else if (label.startsWith(q)) score = 90
    else {
      // Each tier checks the spaced form first, then the punctuation-free one, so a query
      // typed with or without the hyphen lands identically.
      const exactKw = keywords.find((k) => k.folded === q || k.squashed === qs)
      const prefixKw = keywords.find((k) => k.folded.startsWith(q) || k.squashed.startsWith(qs))
      const containsKw = keywords.find((k) => k.folded.includes(q) || k.squashed.includes(qs))
      if (exactKw) {
        score = 80
        matched = exactKw.raw
      } else if (label.includes(q) || labelSquashed.includes(qs)) score = 70
      else if (prefixKw) {
        score = 60
        matched = prefixKw.raw
      } else if (containsKw) {
        score = 50
        matched = containsKw.raw
      } else {
        // Only the all-words-land path got us here — name the keyword carrying the first word
        // so the row still explains itself.
        score = 40
        matched = keywords.find((k) => k.folded.includes(words[0]))?.raw ?? section.label
      }
    }
    // A first-hour setting outranks a tie, and something buried in a disclosure is nudged UP:
    // being hard to see by eye is precisely why someone is searching for it.
    if (section.neededInHourOne) score += 3
    if (section.advanced) score += 2
    hits.push({ section, matched, score })
  }

  return hits
    .sort((a, b) => b.score - a.score || a.section.label.localeCompare(b.section.label))
    .slice(0, limit)
}
