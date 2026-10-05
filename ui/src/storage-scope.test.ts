// Which browser-storage keys are PRIVATE to a window and which are shared by the whole
// station — the classification itself, and a scan proving the call sites agree with it.
//
// Two failure modes, and they are not symmetric:
//   - a shared key scoped by mistake → silent cross-talk stops; the operator sets a
//     preference and it mysteriously does not stick. Worst case: an "already fired"
//     dedupe set re-alerts the SAME event once per open window.
//   - a per-surface key left shared → two windows overwrite each other's layout.
// The scan below catches both, and catches the nastier variant of the second: a key
// written from more than one component where only one site got migrated.
//
// Reads the tree the same way wire-consistency.test.ts reads dto.rs. Deliberately NOT a
// jsdom test — nothing here needs a DOM; the behavioural half lives in
// features/windowScope.test.ts.
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join, relative } from 'node:path'
import { scopedKey, surfaceKey } from './features/windowScope'
import { panelStorageKey } from './features/panelState'
import { PALETTE_ROLES } from './features/paletteRoles'
import { SCALE_KEYS } from './spectrum/scaleSettings'

/** Every base key routed through the per-surface scope. Adding one here without routing
 *  it (or routing one without adding it here) fails `routes exactly the per-surface keys`
 *  below — the list and the code cannot drift apart. */
export const PER_SURFACE = [
  'neededFilters',
  'nexus-ui-scale-mode',
  // APRS's body (layout L7): the station list column's width and the side the map stands on —
  // statements about THIS window's shape, like Connect's rail widths and Operate's rail side.
  'nexus.aprs.mapSide',
  'nexus.aprs.railWidth',
  'nexus.awardsTab',
  // The Awards and Satellites views' column split (layout L7): a proportion of THIS window's grid.
  'nexus.awards.columns',
  // Whether THIS window's Connect draws the dashboard bar over its header (a layout writes it: Frame +
  // bar on, every other layout and Reset layout off). A pop-out inherits the main window's until it
  // picks a layout of its own, like the placement it is read back with.
  'nexus.connect.bar',
  'nexus.connect.config',
  'nexus.connect.globe3d.layers',
  'nexus.connect.insights.collapsed',
  'nexus.connect.intent',
  // Each Connect intent's remembered map (projection, layers, colour mode, 2-D/3-D) —
  // features/intentMapSettings. Per-surface for the reasons projection and layers were; the
  // old shared projection/layers/map3d keys stay listed because it still READS them to migrate.
  'nexus.connect.intents',
  'nexus.connect.layers',
  // The Layers panel folded to its pill (MapLayersPanel) — per window, like the Conditions rail,
  // and one record for the 2-D and 3-D panels, which sit in the same place.
  'nexus.connect.layersPanel.collapsed',
  // Connect's POTA/SOTA box keeps its OWN copy of the board's filters (PotaSotaView OTA_KEYS.box),
  // per window like the view's `nexus.ota.*` below: a chip in the box never moves the view's.
  'nexus.connect.ota.bandFilter',
  'nexus.connect.ota.hideWorked',
  'nexus.connect.ota.modeFilter',
  'nexus.connect.ota.program',
  'nexus.connect.ota.sortAsc',
  'nexus.connect.ota.sortKey',
  'nexus.connect.map3d',
  // The map's full-screen chrome-hide. Per-surface for the same reason map3d is — it is a
  // statement about ONE window — and the one per-surface key here that is deliberately read
  // WITHOUT `surfaceGet`'s inheritance (MapView's `loadFull`): a window shape carried into a
  // brand-new pop-out opens it with chrome the operator never hid.
  'nexus.connect.mapfull',
  'nexus.connect.projection',
  // The two rail widths the operator dragged (2026-09-13). A width is a statement about one
  // window's shape: a pop-out inherits it on first open and clamps it against its own box.
  'nexus.connect.railWidths',
  // The dashboard rail beside the cockpits (features/dashRail): which box is in each slot, whether it
  // shows beside each section, and its width — statements about THIS window, like Connect's.
  'nexus.dashrail.config',
  'nexus.dashrail.sections',
  'nexus.dashrail.width',
  'nexus.decodes.filter',
  // The Phone cockpit's Needed pane's own filter record (#345). The board's `neededFilters`
  // under another name, for the same reason: what THIS board shows. Never that key — a chip in
  // the pane and one on the Needed view must not move each other.
  'nexus.phone.neededFilters',
  // …and the CW cockpit's (plan H8: CW gains Phone's two feeds), for the same reason and apart
  // from both of the above.
  'nexus.cw.neededFilters',
  'nexus.decodes.hideB4',
  'nexus.decodes.hideBlocked',
  'nexus.decodes.hideConfirmed',
  // Band Activity's newest-on-top order (#276) — the same class as the chips above it: a
  // statement about what THIS pane shows, so a torn-off Operate window may read the other way.
  'nexus.decodes.newestTop',
  'nexus.logbook.globespin',
  'nexus.operate.layout',
  // Which side Operate's rail stands on (layout L5): a layout choice of THIS window, like the
  // Classic / Roster pick on either side of it.
  'nexus.operate.railSide',
  'nexus.operateLayout',
  'nexus.ota.bandFilter',
  // Hide worked today: a statement about what THIS board shows, like the filters beside it — a
  // POTA board and a SOTA board in two windows may answer it differently.
  'nexus.ota.hideWorked',
  'nexus.ota.modeFilter',
  'nexus.ota.program',
  'nexus.ota.sortAsc',
  'nexus.ota.sortKey',
  'nexus.phonescope.dss',
  'nexus.phonescope.flow',
  // The rig scope's slow-scope look (smooth scroll or a row per sweep): like 3D and flow, a look of
  // THIS window's scope, so a torn-off cockpit can show the other one beside it.
  'nexus.phonescope.rows',
  'nexus.phonescope.win',
  'nexus.roster.filters',
  'nexus.sats.columns',
  'nexus.sats.favOnly',
  'nexus.spotlegend',
  // Connect's bottom strip height (layout L7): a % of THIS window's grid, like the strips below.
  'nexus.split.connect.strip',
  'nexus.split.cw.scope',
  // JS8's waterfall height (layout L2): a % of THIS window's shell, like the three beside it.
  'nexus.split.js8.waterfall',
  // The Logbook's globe band height (layout L7): a % of THIS window's list, the same kind.
  'nexus.split.logbook.globe',
  'nexus.split.operate.waterfall',
  // Operate Classic's Tx1–Tx6 machine height (layout L5), the same kind.
  'nexus.split.operate.tx',
  'nexus.split.phone.scope',
  // RTTY's and PSK's waterfall heights (layout L6), the same kind; and SSTV's picture stage.
  'nexus.split.psk.waterfall',
  'nexus.split.rtty.waterfall',
  'nexus.split.sstv.stage',
  'nexus.view',
  'nexus.waterfall.dss',
  'nexus.waterfall.flow',
  'nexus.waterfall.zoom',
  'tempo-left-rail-w',
  // Which Tempo rail the operator set last — the one that keeps its width when the pair must give
  // (usePaneWidths.fitRails). A statement about THIS window's two widths, like them.
  'tempo-rail-last',
  'tempo-right-rail-w',
]

/** Keys that describe the STATION or the PERSON and must never be scoped. Listed rather
 *  than inferred so a failure names the key that leaked. */
const SHARED = [
  // Display units: one preference the whole station shares, like the country exclude.
  'nexus.units',
  // Logbook globe on/off (D#278): how this operator wants the Logbook to look, not a window
  // property — the same classification as units (features/logbookGlobe).
  'nexus.logbook.globe',
  // Prose language: the same preference in every window, for the same reason units are. A
  // pop-out band map reading a different language than the window that spawned it would be
  // the shape of bug this list exists to prevent.
  'nexus.locale',
  // Wildcard call-hide: a standing display preference across windows (features/hideCalls).
  'nexus.decodes.hideCalls',
  // Arbitrary-entity country excludes, stored beside the curated keys (F4MQS).
  'nexus.decodes.countryExclude.entities',
  // Whole continents hidden from band activity (#229), stored beside the entity picks and
  // SHARED on the same ruling as the country exclude below.
  'nexus.decodes.countryExclude.continents',
  'nexus.navOrder', // left-rail section order — a person/station preference, same in every window
  'nexus-density',
  // Text size (#215): a fact about this screen and the eyes in front of it, like density. A
  // pop-out showing a different size from the window that spawned it would read as broken.
  'nexus-text-size',
  // The Logbook's "More columns" wide table (#239): a standing display preference, the same in
  // every window, like density.
  'nexus.logbook.moreColumns',
  // Field mode: being outdoors is a fact about the STATION, not a window — a pop-out beside
  // the main window in the same sunlight must follow it (DetachedPanel mirrors the hook).
  'nexus-field-mode',
  // High contrast (#215), the other input to `data-contrast`: a fact about the operator's
  // eyes and this screen, so every window of the station agrees, exactly like field mode.
  'nexus-high-contrast',
  // Night (Off / On / Auto at dusk): a fact about the room the station is in after dark, so a
  // pop-out beside the main window dims with it (DetachedPanel runs its own useNight).
  'nexus-night',
  // The optional local-time clock beside UTC (#253). Per MACHINE, like density and field mode:
  // local time is a fact about this computer's time zone, and every window of it agrees.
  'nexus-local-clock',
  // The colour roles (Settings ▸ Appearance ▸ Colours), one key per role: a preference about this
  // screen and the eyes in front of it, the same class as the theme, so every window agrees. The
  // role table is their one source (`classifies every colour-role key`, below).
  'nexus-palette-accent',
  'nexus-palette-amber',
  'nexus-palette-cyan',
  'nexus-palette-ok',
  'nexus-palette-readout',
  // The built-in theme (Settings ▸ Appearance ▸ Theme; useSkin.ts): part of the theme, the same
  // class, so a pop-out paints the theme the window that opened it paints.
  'nexus-skin',
  'nexus-motion',
  'nexus-ui-scale-cap',
  'nexus.connect.chaseDefault.v1',
  'nexus.connect.mode',
  'nexus.cw.sensitivity',
  // The FT country-exclusion list. SHARED, and the one key here where that is a RULING
  // rather than a classification: a standing statement about how this operator chases is
  // not a property of a window. Per-surface, a torn-off band map would inherit it once and
  // then diverge on its first toggle, so the pop-out would show the decodes the main
  // window hides — two surfaces disagreeing about what is on the band.
  'nexus.decodes.countryExclude',
  'nexus.decodes.countryExclude.paused',
  'nexus.cw.tuneStep',
  'nexus.cwAssist',
  'nexus.dev.xray',
  // The Field Day contacts-per-hour goal. SHARED: a target rate is a statement about how this
  // operator is running the event, not about one window — a torn-off board showing a different
  // goal than the cockpit would be two surfaces disagreeing about the same target.
  'nexus.fd.rateGoal',
  'nexus.dxped.alarms',
  'nexus.dxped.chasing',
  'nexus.features.v1',
  'nexus.features.wizardSeen',
  // ★-pinned JS8 calls. SHARED, and NOT durable: which calls this operator is holding at the
  // top of the roster is a fact about the operator rather than a window, but it is about who is
  // on the band right now — one click to remake, meaningless tomorrow — so it does not join the
  // watch list and the chase sets in DURABLE_KEYS (features/js8Pins).
  'nexus.js8.pins',
  'nexus.memory.bank.v1',
  'nexus.memory.bank.v2',
  'nexus.needed.autopop',
  // The Remote listener's volume (remote-web/audio-listen, 2026-10-03). SHARED: how loud this
  // browser plays the station is the listener's choice, not one window's, like the alert opt-ins.
  'nexus.remote.listenVolume',
  // The Remote operator's Mic level (remote-web/mic-level, 2026-10-03). SHARED for the same reason: how
  // loud this browser's microphone goes is a fact about the operator's microphone, not one window.
  'nexus.remote.micLevel',
  // Remote need alerts, opted in per browser. SHARED like autopop: whether this operator wants
  // to be told about new needs is not a fact about one window (remote-web/useNeedAlerts).
  'nexus.remote.needAlerts',
  // Remote rare-DX and new-POTA-activation alerts: the same per-browser opt-in, SHARED for the
  // same reason (remote-web/useRareDxAlerts, remote-web/usePotaAlerts).
  'nexus.remote.potaAlerts',
  'nexus.remote.rareDxAlerts',
  // The Remote page's relay test switch: 'force' makes every stream use the relay alone
  // (remote-web/stream-link, 2026-10-03). Set by hand in the console, for the whole browser: SHARED,
  // like the workspace flag below.
  'nexus.remote.relay',
  // The Remote page's flag that shows the old watch/control workspace again, hidden while streaming
  // is proven (remote-web/RemoteApp, 2026-10-02). Set by hand or by the compiled sweep, for the whole
  // browser: SHARED, like the alert opt-ins.
  'nexus.remote.workspace',
  'nexus.operate.tuneStep',
  'nexus.panels.wfDetached.v1',
  'nexus.phone.tuneStep',
  'nexus.profiles',
  'nexus.program.chirpHowtoSeen.v1',
  'nexus.program.recents.v1',
  'nexus.sats.alarms',
  'nexus.sats.chasing',
  'nexus.sats.chasingNorad',
  // The one-time seed marker. SHARED for the same reason the ★ set it seeded
  // is: per-surface, a second window would find "never seeded" and star ten
  // birds on top of whatever the operator had settled on.
  'nexus.sats.seeded',
  // Which received picture the viewer pop-out is showing. SHARED on purpose and it is the
  // whole mechanism: the main window writes it and the viewer's `storage` listener follows,
  // so clicking a second thumbnail re-points the window that is already open instead of
  // opening another. Per-surface, the two documents would never see each other's writes.
  'nexus.sstv.viewer.path',
  'nexus.waterfall.detached',
  'nexus.waterfall.gain',
  'nexus.waterfall.palette',
  'nexus.waterfall.zero',
  // Each cockpit's scope scale record (spectrum/scaleSettings.ts: averaging, detector, G/Z, window).
  // Per cockpit, not per window, for the reason the waterfall's G/Z above are shared: a contrast
  // calibration against the station's noise floor that re-calibrating per window would surprise.
  'nexus.scope.phone',
  'nexus.scope.cw',
  'nexus.scope.operate',
  'nexus.scope.js8',
  'nexus.scope.rtty',
  'nexus.scope.psk',
  'nexus.scope.sstv',
  'nexus.scope.tempo',
  // The spectrum renderer's hidden backend setting (spectrum/choose.ts): WebGL2 or canvas-2D on THIS
  // machine, whatever the automatic choice says. A fact about the machine's graphics, not a window.
  'nexus.spectrum.backend',
  // The RF scope pane's, one record for the pane in all five digital cockpits (the same picture).
  'nexus.scope.rfpan',
  'nexus.watchlist',
  'nexus.workspace',
  'tempo-onboarded',
  'tempo-theme',
]

/** The subset of SHARED whose whole job is "this already happened". Per-surface here does
 *  not merely annoy — it re-fires the same alert once per open window, mid-pass. */
const DEDUPE = [
  'nexus-journey-seen',
  'nexus.dxped.alarms.fired',
  'nexus.sats.alarms.fired',
  // "The seed notice was read." Per-surface, the notice would reappear in
  // every pop-out and again after every window open — an announcement about a
  // one-time event, made repeatedly.
  'nexus.sats.seedAck',
  'nexus.update.dismissedVersion',
  'tempo-achievements-seen',
]

/**
 * Keys held in sessionStorage through SpotsPanel's `useSessionState`. sessionStorage is
 * already per-webview by the platform, so these need no scope suffix — the same guarantee
 * PER_SURFACE buys, by a different mechanism (windowScope.test.ts leans on it for seenSet).
 * Their lifetime is deliberately shorter: the Spots filters are meant to survive a view
 * switch and die on app exit, unlike the roster/board filters, which persist across restarts.
 *
 * Listed AND scanned for (see `classifies every useSessionState key`), because the raw-storage
 * scan below structurally cannot see them: the helper takes the key as a PARAMETER, so no
 * literal ever appears at a `sessionStorage.*` call site. This list used to name a stale trio
 * — `nexus.spots.modes` (renamed to hiddenModes), `.bands`, `.sort` — while the four keys
 * added after it went unlisted entirely, so it recorded a verdict on one key that no longer
 * existed and none on four that did.
 */
const SESSION_SCOPED = [
  'nexus.spots.bands',
  'nexus.spots.filtersOpen',
  // The band a PANE of the board follows, and the modes it opens on as an allow-list (#345: the
  // Phone cockpit's Spots pane). Each is kept only by that pane; the view keeps `hiddenModes`
  // instead and no follow switch at all. A pane keeps EVERY key here as its own copy,
  // `<key>.<scope>` (`nexus.spots.bands.phone`): same store, same lifetime, same verdict. Connect's
  // Spots box is a pane that names no modes and follows no band, so it keeps `hiddenModes` in
  // its copy (`nexus.spots.hiddenModes.connect`), the view's rule, and no follow switch.
  'nexus.spots.followBand',
  'nexus.spots.shownModes',
  'nexus.spots.hiddenModes',
  // Hide worked and the window it reaches back over. Session-lived like the filters around them:
  // on again next launch, which is the default the operator chose.
  'nexus.spots.hideWorked',
  'nexus.spots.licensedOnly',
  'nexus.spots.localOnly',
  'nexus.spots.query',
  'nexus.spots.sort',
  // Spotted-from filter (#174): the reporting voices' continents and countries.
  'nexus.spots.spotterConts',
  'nexus.spots.spotterEntities',
  'nexus.spots.states',
  'nexus.spots.workedWindow',
]

describe('zero migration: the main window keeps the exact key strings already on disk', () => {
  // Asserted as LITERALS, key by key. A property test over the helper passes just as
  // happily against `${base}.main`, which is precisely what would orphan every saved
  // layout, zoom, projection and board filter the moment an operator upgrades.
  it.each(PER_SURFACE)('%s is byte-identical on the main surface', (base) => {
    expect(surfaceKey(base, 'main')).toBe(base)
  })

  it('spells out the keys most expensive to lose', () => {
    expect(surfaceKey('tempo-right-rail-w', 'main')).toBe('tempo-right-rail-w')
    expect(surfaceKey('tempo-left-rail-w', 'main')).toBe('tempo-left-rail-w')
    expect(surfaceKey('nexus-ui-scale-mode', 'main')).toBe('nexus-ui-scale-mode')
    expect(surfaceKey('nexus.connect.config', 'main')).toBe('nexus.connect.config')
    expect(surfaceKey('nexus.connect.projection', 'main')).toBe('nexus.connect.projection')
    expect(surfaceKey('neededFilters', 'main')).toBe('neededFilters')
    expect(surfaceKey('nexus.split.operate.waterfall', 'main')).toBe('nexus.split.operate.waterfall')
  })

  it('leaves the panel record on its own (already-shipped, already-suffixed) spelling', () => {
    // nexus.panels.* shipped in 0.15.0 ALREADY suffixed, so for that one key the
    // byte-identical string is the SUFFIXED one — the opposite of every other key. It
    // therefore builds its key itself, and this is what keeps the two rules apart.
    expect(panelStorageKey('operate', 'main')).toBe('nexus.panels.operate.main')
    expect(panelStorageKey('operate', 'w1')).toBe('nexus.panels.operate.w1')
    expect(surfaceKey('nexus.panels.operate', 'main')).toBe('nexus.panels.operate')
  })

  it('suffixes only above main, and never for the global scope', () => {
    expect(surfaceKey('nexus.view', 'w1')).toBe('nexus.view.w1')
    expect(surfaceKey('nexus.view', 'w2')).toBe('nexus.view.w2')
    expect(surfaceKey('nexus.view', 'r3')).toBe('nexus.view.r3')
    for (const inst of ['main', 'w2', 'r3']) {
      expect(scopedKey('tempo-theme', 'global', inst)).toBe('tempo-theme')
    }
  })

  it('keeps the radio scope bare until an r<id> surface exists', () => {
    // The tune-step and waterfall-calibration keys are shared TODAY and belong on 'radio'
    // once r<id> windows are openable (an IC-9700 on 2 m does not want the HF rig's step
    // size or noise-floor contrast). This is what makes that promotion a no-op on disk
    // instead of a rename that resets them.
    for (const base of ['nexus.phone.tuneStep', 'nexus.waterfall.gain']) {
      expect(scopedKey(base, 'radio', 'main')).toBe(base)
      expect(scopedKey(base, 'radio', 'w1')).toBe(base)
      expect(scopedKey(base, 'radio', 'r2')).toBe(`${base}.r2`)
    }
  })
})

// ── Call-site scan ──────────────────────────────────────────────────────────────────
const SRC = fileURLToPath(new URL('.', import.meta.url))

function sources(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name === 'assets' || name === 'data') continue
    const full = join(dir, name)
    if (statSync(full).isDirectory()) sources(full, out)
    else if (/\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) && !/\.d\.ts$/.test(name)) {
      out.push(full)
    }
  }
  return out
}

/** Call sites that pass a key THROUGH (a function parameter or a JSX prop), so the literal
 *  lives at their callers. Declared explicitly, and each declaration is verified below. */
const INDIRECT: Record<string, string[]> = {
  'usePaneWidths.ts:key': ['tempo-right-rail-w', 'tempo-left-rail-w'],
  'components/ConnectView.tsx:key': ['nexus.connect.intent'],
  // Reserved generic pane-grid persistence. Carries NO key today: no `PaneLayoutSpec`
  // literal exists in the tree (Connect composes the pure helpers and persists through its
  // own scoped `nexus.connect.config`). Routed through the scope helper anyway so the next
  // view to adopt `usePaneLayout` inherits per-surface behaviour instead of landing an
  // unscoped layout key — which no test could catch, since its literal would not be in the
  // classification either.
  'features/paneLayout.ts:spec': [],
  'components/PaneSeam.tsx:storageKey': [
    'nexus.split.operate.waterfall',
    'nexus.split.cw.scope',
    'nexus.split.phone.scope',
    'nexus.split.js8.waterfall',
    'nexus.split.rtty.waterfall',
    'nexus.split.psk.waterfall',
    'nexus.split.sstv.stage',
    'nexus.split.operate.tx',
    'nexus.split.connect.strip',
    'nexus.split.logbook.globe',
  ],
  // The Needed board's filter record: its own key as the view and the pop-out, or the key the
  // host of a PANE of it passes (NeededPane — the Phone and CW cockpits', #345 and plan H8).
  'components/NeededPanel.tsx:key': ['neededFilters', 'nexus.phone.neededFilters', 'nexus.cw.neededFilters'],
  // The POTA/SOTA board's filters, read and written through its key table (OTA_KEYS): the view's
  // own six, or the Connect box's six.
  'components/PotaSotaView.tsx:keys': [
    'nexus.ota.program',
    'nexus.ota.bandFilter',
    'nexus.ota.modeFilter',
    'nexus.ota.sortKey',
    'nexus.ota.sortAsc',
    'nexus.ota.hideWorked',
    'nexus.connect.ota.program',
    'nexus.connect.ota.bandFilter',
    'nexus.connect.ota.modeFilter',
    'nexus.connect.ota.sortKey',
    'nexus.connect.ota.sortAsc',
    'nexus.connect.ota.hideWorked',
  ],
}

const routed = new Set<string>()
const indirect = new Set<string>()
for (const file of sources(SRC)) {
  const rel = relative(SRC, file).replace(/\\/g, '/')
  if (rel === 'features/windowScope.ts') continue // the definition, not a call site
  const text = readFileSync(file, 'utf8')
  for (const m of text.matchAll(/\bsurface(?:Get|Set|Key)\(\s*('[^']*'|"[^"]*"|[A-Za-z_$][\w$]*)/g)) {
    const arg = m[1]
    if (arg.startsWith("'") || arg.startsWith('"')) {
      routed.add(arg.slice(1, -1))
      continue
    }
    const decl = text.match(new RegExp(`\\bconst ${arg}\\s*=\\s*'([^']*)'`))
    if (decl) routed.add(decl[1])
    else indirect.add(`${rel}:${arg}`)
  }
}

/** Every RAW `localStorage.(get|set|remove)Item` in the tree, resolved to the key it names —
 *  literal or `const`-declared. This is the half the `routed` scan structurally cannot see. */
const rawUses: { file: string; key: string; op: string }[] = []
for (const file of sources(SRC)) {
  const rel = relative(SRC, file).replace(/\\/g, '/')
  if (rel === 'features/windowScope.ts') continue // the definition, not a call site
  const text = readFileSync(file, 'utf8')
  for (const m of text.matchAll(
    /localStorage\.(getItem|setItem|removeItem)\(\s*('[^']*'|"[^"]*"|[A-Za-z_$][\w$]*)/g,
  )) {
    const [, op, arg] = m
    if (arg.startsWith("'") || arg.startsWith('"')) {
      rawUses.push({ file: rel, key: arg.slice(1, -1), op })
      continue
    }
    const decl = text.match(new RegExp(`\\bconst ${arg}\\s*=\\s*'([^']*)'`))
    if (decl) rawUses.push({ file: rel, key: decl[1], op })
  }
}

describe('call sites agree with the classification', () => {
  /**
   * THE HALF-MIGRATED KEY — the defect the `routed` scan below is structurally blind to,
   * and the reason this test exists.
   *
   * `routed` is a UNION of every key seen at a `surface*` call. So a key whose READ was
   * migrated and whose WRITE was not still appears in it, and set-equality passes. Proven,
   * not assumed: reverting one `surfaceSet(TAB_KEY, …)` in AwardsJourney.tsx back to a raw
   * `localStorage.setItem` left all 745 tests green.
   *
   * That is the worst-shaped defect available here. The pop-out READS its own key (empty,
   * so it inherits) but WRITES the main window's — so it silently overwrites the main
   * window while appearing to have private state, and nobody sees it until the main window
   * is reopened. Checking raw uses directly is total, and cheap.
   */
  /**
   * COMPLETENESS. Every other test here checks that the keys we CLASSIFIED are handled
   * right; none checks that we classified every key. An unclassified key is an unreviewed
   * key — a new one added next week gets a verdict from nobody, which is the drift this
   * file exists to prevent.
   *
   * `nexus.__probe` is exempt: it is a write-then-delete probe for read-only storage, never
   * persisted, so it has no scope to get wrong.
   *
   * The sessionStorage keys are classified in SESSION_SCOPED and checked by their own scan —
   * this one cannot see them at all (the key reaches `sessionStorage` as a parameter), which
   * is exactly why hand-listing them here had gone stale unnoticed.
   */
  it('classifies every storage key in the tree', () => {
    const EXEMPT = new Set([
      'nexus.__probe', // transient write/delete probe
    ])
    const classified = new Set([...PER_SURFACE, ...SHARED, ...DEDUPE, ...SESSION_SCOPED])
    const seen = new Set<string>()
    for (const file of sources(SRC)) {
      const text = readFileSync(file, 'utf8')
      for (const m of text.matchAll(
        /(?:local|session)Storage\.(?:getItem|setItem|removeItem)\(\s*('[^']*'|"[^"]*"|[A-Za-z_$][\w$]*)/g,
      )) {
        const arg = m[1]
        if (arg.startsWith("'") || arg.startsWith('"')) {
          seen.add(arg.slice(1, -1))
          continue
        }
        const decl = text.match(new RegExp(`\\bconst ${arg}\\s*=\\s*'([^']*)'`))
        if (decl) seen.add(decl[1])
      }
    }
    const unclassified = [...seen].filter((k) => !classified.has(k) && !EXEMPT.has(k)).sort()
    expect(unclassified).toEqual([])
  })

  /**
   * The sessionStorage seam, scanned rather than remembered.
   *
   * `useSessionState(key, init)` receives its key as a PARAMETER, so the raw-storage scan
   * above — which resolves only literals and same-file `const`s — sees none of these keys and
   * can never report them unclassified. That blind spot is how the hand-written exemption came
   * to name a renamed key and miss four real ones for four releases.
   *
   * Asserted as SET EQUALITY, so it fails in both directions: a new `useSessionState` key is
   * unclassified until listed, a renamed one breaks its old entry, and a stale entry for a key
   * no longer in the tree fails too. Same contract the `INDIRECT` seams get for `surface*`.
   */
  it('classifies every useSessionState key', () => {
    const sessionKeys = new Set<string>()
    for (const file of sources(SRC)) {
      const text = readFileSync(file, 'utf8')
      // `[^(]*` skips any generic argument (`useSessionState<string[]>('…')`) without
      // tripping over `>` inside it. The helper's own definition takes `key: string`, not a
      // literal, so it never matches itself.
      for (const m of text.matchAll(/useSessionState[^(]*\(\s*'([^']*)'/g)) sessionKeys.add(m[1])
    }
    expect([...sessionKeys].sort()).toEqual([...SESSION_SCOPED].sort())
  })

  it('keeps the sessionStorage keys OUT of the per-surface scope helper', () => {
    // They are already per-webview; routing one through surfaceKey would suffix a key that is
    // private by construction, and the pop-out would silently start from empty.
    expect(SESSION_SCOPED.filter((k) => routed.has(k))).toEqual([])
  })

  it('leaves NO per-surface key on a raw localStorage call', () => {
    const leaks = rawUses
      .filter((u) => PER_SURFACE.includes(u.key))
      .map((u) => `${u.file}: localStorage.${u.op}(${u.key})`)
    expect(leaks).toEqual([])
  })


  it('routes exactly the per-surface keys through the scope helper', () => {
    const all = [...routed, ...Object.values(INDIRECT).flat()]
    expect([...new Set(all)].sort()).toEqual([...PER_SURFACE].sort())
  })

  it('never routes a shared key — that would be silent cross-talk between windows', () => {
    expect([...SHARED, ...DEDUPE].filter((k) => routed.has(k))).toEqual([])
  })

  it('accounts for every pass-through seam', () => {
    expect([...indirect].sort()).toEqual(Object.keys(INDIRECT).sort())
  })

  it('classifies every strip divider’s key — a JSX `storageKey` literal no call-site scan can see', () => {
    // PaneSeam's strip kind stores through `surfaceGet(storageKey)`, so the literal lives in a JSX
    // attribute at each caller and the scan above never meets it: a new divider's key (JS8's
    // waterfall, layout L2) went unclassified and every test stayed green. Every one, found in
    // the source, must be declared as that seam's key and classified per surface.
    const found = new Set<string>()
    for (const file of sources(SRC)) {
      for (const m of readFileSync(file, 'utf8').matchAll(/storageKey="([^"]+)"/g)) found.add(m[1])
    }
    expect(found.size, 'the scan found no divider key at all — it is reading nothing').toBeGreaterThanOrEqual(3)
    expect([...found].sort()).toEqual([...INDIRECT['components/PaneSeam.tsx:storageKey']].sort())
    for (const k of found) expect(PER_SURFACE, `${k} is not classified`).toContain(k)
  })

  it('checks the pass-through seams really carry the keys they claim', () => {
    const panes = readFileSync(join(SRC, 'usePaneWidths.ts'), 'utf8')
    expect(panes).toContain("const KEY_RIGHT = 'tempo-right-rail-w'")
    expect(panes).toContain("const KEY_LEFT = 'tempo-left-rail-w'")
    expect(readFileSync(join(SRC, 'components/ConnectView.tsx'), 'utf8')).toContain(
      "persisted('nexus.connect.intent'",
    )
    for (const [file, key] of [
      ['components/OperateCockpit.tsx', 'nexus.split.operate.waterfall'],
      ['components/CwCockpit.tsx', 'nexus.split.cw.scope'],
      ['components/PhoneCockpit.tsx', 'nexus.split.phone.scope'],
      ['components/Js8Cockpit.tsx', 'nexus.split.js8.waterfall'],
      ['components/RttyCockpit.tsx', 'nexus.split.rtty.waterfall'],
      ['components/PskCockpit.tsx', 'nexus.split.psk.waterfall'],
      ['components/SstvView.tsx', 'nexus.split.sstv.stage'],
      ['components/OperateCockpit.tsx', 'nexus.split.operate.tx'],
      ['components/ConnectView.tsx', 'nexus.split.connect.strip'],
      ['components/Logbook.tsx', 'nexus.split.logbook.globe'],
    ]) {
      expect(readFileSync(join(SRC, file), 'utf8')).toContain(`storageKey="${key}"`)
    }
    // The Needed board's `key`: its own when nobody passes one, the Phone pane's when it does.
    const needed = readFileSync(join(SRC, 'components/NeededPanel.tsx'), 'utf8')
    expect(needed).toContain("const FILTER_KEY = 'neededFilters'")
    expect(needed).toContain('const filterKey = pane?.filterKey ?? FILTER_KEY')
    const phone = readFileSync(join(SRC, 'components/PhoneCockpit.tsx'), 'utf8')
    expect(phone).toContain("const PHONE_NEEDED_FILTERS = 'nexus.phone.neededFilters'")
    expect(phone).toContain('pane={{ filterKey: PHONE_NEEDED_FILTERS,')
    const cw = readFileSync(join(SRC, 'components/CwCockpit.tsx'), 'utf8')
    expect(cw).toContain("const CW_NEEDED_FILTERS = 'nexus.cw.neededFilters'")
    expect(cw).toContain('pane={{ filterKey: CW_NEEDED_FILTERS,')
    // The POTA/SOTA board's key table: every key the seam claims is a literal in it, the board
    // reads through it and nothing else, and it picks the box's half only as a box.
    const ota = readFileSync(join(SRC, 'components/PotaSotaView.tsx'), 'utf8')
    for (const k of INDIRECT['components/PotaSotaView.tsx:keys']) expect(ota).toContain(`'${k}'`)
    expect(ota).toContain('const keys = pane ? OTA_KEYS.box : OTA_KEYS.view')
    expect(ota).not.toMatch(/surface(?:Get|Set)\(\s*'nexus\.(?:connect\.)?ota\./)
  })

  it('scopes BOTH writers of a key written from two components', () => {
    // nexus.spotlegend is toggled independently by BandMap and BandStrip. Migrating one
    // and not the other leaves a legend toggle that half-works across windows — the exact
    // shape of a partial migration, and invisible to a single-component test.
    for (const file of ['components/BandMap.tsx', 'components/BandStrip.tsx']) {
      const text = readFileSync(join(SRC, file), 'utf8')
      expect(text, file).toContain("surfaceGet('nexus.spotlegend')")
      expect(text, file).toContain("surfaceSet('nexus.spotlegend'")
      expect(text, file).not.toContain("localStorage.getItem('nexus.spotlegend')")
      expect(text, file).not.toContain("localStorage.setItem('nexus.spotlegend'")
    }
  })

  /**
   * The colour-role keys (Settings ▸ Appearance ▸ Colours) reach `localStorage` as
   * `role.storage`, a property of the role table, so the literal scan above cannot see them
   * (the useFieldMode.ts trap). The table is the one source of them, so it is checked here
   * directly: every role's key is classified, and SHARED lists none the table does not have.
   */
  it('classifies every colour-role key, from the role table itself', () => {
    const tableKeys = PALETTE_ROLES.map((r) => r.storage).sort()
    expect(tableKeys.length).toBeGreaterThan(0)
    const classified = [...PER_SURFACE, ...SHARED, ...DEDUPE, ...SESSION_SCOPED]
    expect(tableKeys.filter((k) => !classified.includes(k)), 'unclassified colour-role keys').toEqual([])
    expect(SHARED.filter((k) => k.startsWith('nexus-palette-')).sort()).toEqual(tableKeys)
  })

  /**
   * The scope scale records reach `localStorage` through `SCALE_KEYS[cockpit]`, a table lookup the
   * literal scan above cannot resolve, so the table itself is checked, as the colour roles' is: every
   * key in it is classified SHARED, and SHARED lists no `nexus.scope.` key the table does not have.
   */
  it('classifies every scope scale key, from the key table itself', () => {
    const tableKeys = Object.values(SCALE_KEYS).sort()
    expect(tableKeys.length).toBeGreaterThan(0)
    expect(tableKeys.filter((k) => !SHARED.includes(k)), 'scale keys not classified SHARED').toEqual([])
    expect(SHARED.filter((k) => k.startsWith('nexus.scope.')).sort()).toEqual(tableKeys)
    expect(tableKeys.filter((k) => routed.has(k)), 'a scale key routed per window').toEqual([])
  })

  it('classifies every key exactly once', () => {
    const seen = new Set<string>()
    for (const k of [...PER_SURFACE, ...SHARED, ...DEDUPE]) {
      expect(seen.has(k), `${k} is classified twice`).toBe(false)
      seen.add(k)
    }
  })
})
