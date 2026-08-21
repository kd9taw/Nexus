// Feature registry — the single source of truth for Nexus's modular features.
//
// A "feature" is either a SECTION (a nav destination / View) or a CAPABILITY
// (a cross-cutting behaviour like the Now-Bar or the gamification layer). Each
// carries the metadata the toggle system, profiles, and the (future) goal-driven
// wizard + adaptive reveal all resolve against.
//
// This module is pure data + pure helpers (no React, no storage) so it is fully
// unit-testable in node.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). A feature's `label`
// and `oneLine` reach the operator in Settings ▸ Features and in the setup wizard, and they
// resolve THROUGH GETTERS: this is a module constant a dozen surfaces index directly, so
// looking the words up at import time would freeze whichever locale loaded first and no
// re-render could move it (the `features/needVisuals.ts` lesson). The record's SHAPE is
// unchanged — every consumer still reads `.label` / `.oneLine`, so nothing downstream moved.
//
// THE LABELS ARE MIXED, and the split is the invariant-token rule (`i18n/index.ts`): a
// section named for a MODE keeps its name here (CW, Phone, RTTY, PSK, SSTV, APRS), as do the
// two named for an EVENT or a PROGRAMME (Field Day, POTA / SOTA) — a translated mode or
// programme name names nothing. Every other label, and every one-liner, is prose.

import { t, type MessageKey } from '../i18n'

/** The view the user is looking at. Lives here because features ARE the views;
 * `ModeNav` and `App` import it from here. */
export type View =
  | 'operate'
  | 'cw'
  | 'phone'
  | 'rtty'
  | 'psk'
  | 'sstv'
  | 'aprs'
  | 'connect'
  | 'dxped'
  | 'sats'
  | 'needed'
  | 'spots'
  | 'chat'
  | 'fieldDay'
  | 'logbook'
  | 'awards'
  | 'stats'
  | 'pota'
  | 'memories'
  | 'program'
  | 'settings'

/** Every section id is a `View`; capabilities add a few cross-cutting ids. */
export type FeatureId = View | 'nowBar' | 'gamification'

/** Operator goals a profile is built from (a feature surfaces in a profile when
 * its `intents` intersect the profile's). */
export type Intent = 'casual' | 'dx' | 'contest' | 'pota' | 'vhf'

export type FeatureCategory =
  | 'Operate'
  | 'DX & Awards'
  | 'Contesting'
  | 'POTA/SOTA'
  | 'Propagation'
  | 'Logging'
  | 'System'

export interface FeatureDef {
  id: FeatureId
  label: string
  kind: 'section' | 'capability'
  category: FeatureCategory
  /** Core features are always on and cannot be disabled (the app's spine). */
  core: boolean
  /** Features this one needs on; enabling pulls them in, disabling one cascades
   * off its dependents. (A DAG — validated by tests.) */
  dependsOn: FeatureId[]
  /** Goal profiles that surface this feature by default. */
  intents: Intent[]
  /** For `section` features, the View this renders (=== id). */
  view?: View
  /** Operate MODE this section is specific to: `'dx'` = the FT8/FT4 cockpit and its
   * features; `'msg'` = the Tempo two-way-calling cockpit and its features. Omitted
   * = GLOBAL: shown in both modes (Connect, Map, Propagation, Logbook, Awards,
   * Settings). The FT8/FT4 ⇄ Tempo switch only swaps the mode-specific sections. */
  workspace?: 'dx' | 'msg'
  /** Achievement id whose unlock *suggests* enabling this (adaptive reveal —
   * a follow-on; recorded here so the data model is ready). */
  revealOn?: string
  /** Ships hidden: excluded from profile resolution (including 'everything') and
   * defaults off for new AND upgrading users — only the explicit Settings ▸
   * Features toggle turns it on. For features staged behind an external gate. */
  defaultOff?: boolean
  /** One-line "why you'd want it", shown in Settings + the wizard. */
  oneLine: string
}

/**
 * The heading a category group carries in Settings ▸ Features.
 *
 * The union value is the KEY — it is what groups the registry — and the WORD on screen is
 * prose, so the two are no longer the same string. `POTA/SOTA` is the exception the rule
 * expects: two programme names, carried verbatim.
 */
export function featureCategoryLabel(cat: FeatureCategory): string {
  switch (cat) {
    case 'Operate':
      return t('features.category.operate')
    case 'DX & Awards':
      return t('features.category.dxAwards')
    case 'Contesting':
      return t('features.category.contesting')
    case 'Propagation':
      return t('features.category.propagation')
    case 'Logging':
      return t('features.category.logging')
    case 'System':
      return t('features.category.system')
    case 'POTA/SOTA':
      // The two programmes' own names — key and word are the same string here.
      return cat
  }
}

/**
 * One feature, with its words looked up when they are READ rather than at import (see the
 * file header). Used by every entry whose LABEL is prose; the eight named for a mode, an
 * event or a programme are written out below with the name in place and a getter for the
 * one-liner alone.
 */
function feature(
  v: Omit<FeatureDef, 'label' | 'oneLine'> & { labelKey: MessageKey; oneLineKey: MessageKey },
): FeatureDef {
  const { labelKey, oneLineKey, ...rest } = v
  return {
    ...rest,
    get label() {
      return t(labelKey)
    },
    get oneLine() {
      return t(oneLineKey)
    },
  }
}

/**
 * The catalog. Only features that actually exist today are listed (so the
 * "every section has a real view" invariant holds); future modules
 * (POTA/SOTA, opening detection, DXpedition board, need-aware spotting) slot in
 * here when built.
 */
export const FEATURES: FeatureDef[] = [
  // ---- Core spine (always on) ----
  feature({
    id: 'operate',
    labelKey: 'features.operate.label',
    kind: 'section',
    category: 'Operate',
    core: true,
    dependsOn: [],
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    view: 'operate',
    workspace: 'dx',
    oneLineKey: 'features.operate.oneLine',
  }),
  {
    id: 'cw',
    // The mode's own name.
    label: 'CW',
    kind: 'section',
    category: 'Operate',
    core: false, // opt-in: turn on if you operate CW (Settings ▸ Features / wizard)
    dependsOn: [],
    // Mode, not a goal — chosen explicitly in the wizard's "which modes?" step (and
    // toggleable in Settings ▸ Features), so a goal profile never auto-enables it.
    intents: [],
    view: 'cw',
    // Global (no workspace): the CW operating cockpit — keyboard + macros key the rig.
    get oneLine() {
      return t('features.cw.oneLine')
    },
  },
  {
    id: 'phone',
    label: 'Phone',
    kind: 'section',
    category: 'Operate',
    core: false, // opt-in: turn on if you operate voice (Settings ▸ Features / wizard)
    dependsOn: [],
    // Mode, not a goal — chosen explicitly in the wizard's "which modes?" step (and
    // toggleable in Settings ▸ Features), so a goal profile never auto-enables it.
    intents: [],
    view: 'phone',
    // Global (no workspace): the Phone (SSB/FM) cockpit — PTT + rig control + logging.
    get oneLine() {
      return t('features.phone.oneLine')
    },
  },
  {
    id: 'rtty',
    label: 'RTTY',
    kind: 'section',
    category: 'Operate',
    core: false, // toggleable in Settings ▸ Features (on by default, like every non-staged section)
    dependsOn: [],
    // Mode, not a goal — same doctrine as CW/Phone: a goal profile never auto-enables it.
    intents: [],
    view: 'rtty',
    // Global (no workspace): entering the skeleton asserts nothing on the rig; the
    // FSK/AFSK rig-mode policy lands with the TX wiring.
    get oneLine() {
      return t('features.rtty.oneLine')
    },
  },
  {
    id: 'psk',
    label: 'PSK',
    kind: 'section',
    category: 'Operate',
    core: false, // toggleable in Settings ▸ Features (on by default, like every non-staged section)
    dependsOn: [],
    // Mode, not a goal — same doctrine as CW/Phone/RTTY: a goal profile never auto-enables it.
    intents: [],
    view: 'psk',
    // Global (no workspace — RX-only this phase): entering asserts nothing on the
    // rig; the receiver auto-arms (with the SSTV/APRS decline memory) and the
    // waterfall click tunes the DECODER. Transmit arrives with Keyboard Modes
    // Phase 2.
    get oneLine() {
      return t('features.psk.oneLine')
    },
  },
  {
    id: 'sstv',
    label: 'SSTV',
    kind: 'section',
    category: 'Operate',
    core: false, // toggleable in Settings ▸ Features (on by default, like every non-staged section)
    dependsOn: [],
    // Mode, not a goal — same doctrine as CW/Phone: a goal profile never auto-enables it.
    intents: [],
    view: 'sstv',
    // Global (no workspace — RX-first): viewing the gallery never touches the rig.
    get oneLine() {
      return t('features.sstv.oneLine')
    },
  },
  {
    id: 'aprs',
    label: 'APRS',
    kind: 'section',
    category: 'Operate',
    core: false, // toggleable in Settings ▸ Features (on by default, like every non-staged section)
    dependsOn: [],
    // Mode, not a goal — same doctrine as CW/Phone/RTTY: a goal profile never auto-enables it.
    intents: [],
    view: 'aprs',
    // Global (no workspace — RX-first): monitoring decodes packets; a beacon is an explicit send.
    get oneLine() {
      return t('features.aprs.oneLine')
    },
  },
  feature({
    id: 'logbook',
    labelKey: 'features.logbook.label',
    kind: 'section',
    category: 'Logging',
    core: true,
    dependsOn: [],
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    view: 'logbook',
    oneLineKey: 'features.logbook.oneLine',
  }),
  feature({
    id: 'settings',
    labelKey: 'features.settings.label',
    kind: 'section',
    category: 'System',
    core: true,
    dependsOn: [],
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    view: 'settings',
    oneLineKey: 'features.settings.oneLine',
  }),
  feature({
    id: 'nowBar',
    labelKey: 'features.nowBar.label',
    kind: 'capability',
    category: 'System',
    core: true,
    dependsOn: [],
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    oneLineKey: 'features.nowBar.oneLine',
  }),

  // ---- Optional sections ----
  feature({
    id: 'chat',
    labelKey: 'features.chat.label',
    kind: 'section',
    category: 'Operate',
    core: true, // the spine of the MSG area — the original Tempo TempoFast/TempoDeep chat, always available
    dependsOn: [],
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    view: 'chat',
    workspace: 'msg',
    oneLineKey: 'features.chat.oneLine',
  }),
  {
    // NOTE: Field Day VISIBILITY is not driven by this persisted feature flag — it is
    // owned by the Field Day master switch `settings.fdActive` (a persisted backend bool,
    // toggled in Settings ▸ Features). App.tsx overrides `enabled.fieldDay` with `fdActive`
    // for the nav + view-redirect, so the two can never diverge. This entry stays only so
    // Field Day remains a real registry section (view/landing/profile semantics).
    id: 'fieldDay',
    // The ARRL event's own name.
    label: 'Field Day',
    kind: 'section',
    category: 'Contesting',
    core: false,
    dependsOn: [],
    intents: ['contest'],
    view: 'fieldDay',
    workspace: 'dx',
    get oneLine() {
      return t('features.fieldDay.oneLine')
    },
  },
  feature({
    id: 'connect',
    labelKey: 'features.connect.label',
    kind: 'section',
    category: 'Propagation',
    core: true, // global situational-awareness surface — present in both modes
    dependsOn: [],
    intents: ['casual', 'dx', 'vhf', 'pota'],
    view: 'connect',
    // global (no workspace): Connect is shared across FT8/FT4 and Tempo.
    oneLineKey: 'features.connect.oneLine',
  }),
  feature({
    id: 'needed',
    labelKey: 'features.needed.label',
    kind: 'section',
    category: 'DX & Awards',
    core: true, // flagship situational board — global, always available
    dependsOn: [],
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    view: 'needed',
    // global (no workspace): what you need, on the air now, in both modes.
    oneLineKey: 'features.needed.oneLine',
  }),
  feature({
    id: 'spots',
    labelKey: 'features.spots.label',
    kind: 'section',
    category: 'DX & Awards',
    core: false, // opt-in raw firehose view (the curated Needed board is the default)
    dependsOn: [],
    intents: ['dx', 'contest'],
    view: 'spots',
    // global (no workspace): every spot on the air (all modes), filter client-side.
    oneLineKey: 'features.spots.oneLine',
  }),
  feature({
    id: 'dxped',
    labelKey: 'features.dxped.label',
    kind: 'section',
    category: 'Propagation',
    core: false,
    dependsOn: [],
    intents: ['dx', 'vhf'],
    view: 'dxped',
    // global (no workspace — never touches the rig): the expedition board. The old
    // standalone Propagation section merged into Connect; its DXped pieces live here.
    oneLineKey: 'features.dxped.oneLine',
  }),
  feature({
    id: 'sats',
    labelKey: 'features.sats.label',
    kind: 'section',
    category: 'Propagation',
    core: false,
    dependsOn: [],
    intents: ['casual', 'vhf'],
    view: 'sats',
    // global (no workspace — read-only until the operator arms a rotor track):
    // pass schedule for the ★ favorites, per-bird polar plot + frequencies.
    oneLineKey: 'features.sats.oneLine',
  }),
  feature({
    id: 'memories',
    labelKey: 'features.memories.label',
    kind: 'section',
    category: 'Operate',
    core: false,
    dependsOn: [],
    // Everyone saves frequencies — repeaters, nets, calling freqs — so every
    // goal profile surfaces it (still toggleable in Settings ▸ Features).
    intents: ['casual', 'dx', 'contest', 'pota', 'vhf'],
    view: 'memories',
    // Global (no workspace): a manager view — never touches the rig on entry;
    // only an explicit Tune (recall) retunes.
    oneLineKey: 'features.memories.oneLine',
  }),
  feature({
    id: 'program',
    labelKey: 'features.program.label',
    kind: 'section',
    category: 'Operate',
    core: false,
    dependsOn: [],
    intents: ['casual', 'pota', 'vhf'],
    view: 'program',
    // global (no workspace) — a programming workbench, never touches the rig on entry.
    // On by default: it works today on the open hearham.com repeater data (no key), so
    // it no longer waits on RepeaterBook approval — the RB proxy is a data-quality
    // upgrade that layers in transparently when activated, not a gate on shipping this.
    oneLineKey: 'features.program.oneLine',
  }),
  feature({
    id: 'awards',
    labelKey: 'features.awards.label',
    kind: 'section',
    category: 'DX & Awards',
    core: false,
    dependsOn: ['logbook'],
    intents: ['dx'],
    view: 'awards',
    // global (no workspace): awards/log progress is shared across modes. Combines the
    // for-fun Journey layer (firsts/ladders/collections) with the official DXCC/WAS/…
    // tracker under one tabbed section. Reveal-nudged on the first QSO (not auto-on in
    // the lean starter surface) so a beginner is invited to the Journey tab early.
    revealOn: 'qso-1',
    oneLineKey: 'features.awards.oneLine',
  }),
  feature({
    id: 'stats',
    labelKey: 'features.stats.label',
    kind: 'section',
    category: 'DX & Awards',
    core: false,
    dependsOn: ['logbook'],
    intents: ['dx'],
    view: 'stats',
    // Global (no workspace): descriptive analytics over the whole log — QSOs by band/mode/year/
    // hour, top DXCC entities, WAS states, confirmation rate. Complements Awards (official credit)
    // + Journey (gamified goals) with a plain "here's my log, sliced" dashboard.
    revealOn: 'qso-1',
    oneLineKey: 'features.stats.oneLine',
  }),
  {
    id: 'pota',
    // The two programmes' own names.
    label: 'POTA / SOTA',
    kind: 'section',
    category: 'POTA/SOTA',
    core: false,
    dependsOn: ['logbook'],
    intents: ['pota'],
    view: 'pota',
    workspace: 'dx',
    get oneLine() {
      return t('features.pota.oneLine')
    },
  },

  // ---- Optional capabilities ----
  feature({
    id: 'gamification',
    labelKey: 'features.gamification.label',
    kind: 'capability',
    category: 'DX & Awards',
    core: false,
    // Independent of the Awards *view*: toasts fire on milestones even when the
    // full Awards console is hidden (the badge grid only shows if Awards is on).
    dependsOn: [],
    intents: ['casual', 'dx'],
    revealOn: 'qso-1',
    oneLineKey: 'features.gamification.oneLine',
  }),
]

const BY_ID: Map<FeatureId, FeatureDef> = new Map(FEATURES.map((f) => [f.id, f]))

export function featureById(id: FeatureId): FeatureDef | undefined {
  return BY_ID.get(id)
}

/** All section (nav-destination) features, in registry order. */
export function sectionFeatures(): FeatureDef[] {
  return FEATURES.filter((f) => f.kind === 'section')
}

/** All feature ids. */
export function allFeatureIds(): FeatureId[] {
  return FEATURES.map((f) => f.id)
}

/** Add `id` and all of its (transitive) dependencies to `set` (mutates). */
export function addWithDependencies(set: Set<FeatureId>, id: FeatureId): void {
  set.add(id)
  for (const dep of featureById(id)?.dependsOn ?? []) {
    if (!set.has(dep)) addWithDependencies(set, dep)
  }
}

/** Features that directly depend on `id`. */
export function directDependents(id: FeatureId): FeatureId[] {
  return FEATURES.filter((f) => f.dependsOn.includes(id)).map((f) => f.id)
}

/** Remove `id` and everything that (transitively) depends on it from `set`. */
export function removeWithDependents(set: Set<FeatureId>, id: FeatureId): void {
  set.delete(id)
  for (const dep of directDependents(id)) {
    if (set.has(dep)) removeWithDependents(set, dep)
  }
}

/**
 * Validate the registry's structural invariants. Returns a list of human-readable
 * problems (empty = healthy). Exercised by `registry.test.ts` so a malformed
 * registry fails the build.
 */
export function validateRegistry(): string[] {
  const errs: string[] = []
  const ids = new Set<FeatureId>()
  for (const f of FEATURES) {
    if (ids.has(f.id)) errs.push(`duplicate feature id: ${f.id}`)
    ids.add(f.id)
  }
  for (const f of FEATURES) {
    // every dependency resolves
    for (const dep of f.dependsOn) {
      if (!BY_ID.has(dep)) errs.push(`${f.id} dependsOn unknown feature ${dep}`)
    }
    // a feature cannot depend on itself
    if (f.dependsOn.includes(f.id)) errs.push(`${f.id} depends on itself`)
    // sections have a view equal to their id; capabilities have none
    if (f.kind === 'section') {
      if (f.view !== (f.id as View)) errs.push(`section ${f.id} must have view === id`)
    } else if (f.view !== undefined) {
      errs.push(`capability ${f.id} must not declare a view`)
    }
    // core features may only depend on other core features (the spine is closed)
    if (f.core) {
      for (const dep of f.dependsOn) {
        if (!BY_ID.get(dep)?.core) errs.push(`core ${f.id} depends on non-core ${dep}`)
      }
    }
  }
  // acyclic (DFS with a recursion stack)
  const WHITE = 0
  const GRAY = 1
  const BLACK = 2
  const color = new Map<FeatureId, number>(FEATURES.map((f) => [f.id, WHITE]))
  const visit = (id: FeatureId): boolean => {
    color.set(id, GRAY)
    for (const dep of featureById(id)?.dependsOn ?? []) {
      const c = color.get(dep) ?? WHITE
      if (c === GRAY) return true // back-edge → cycle
      if (c === WHITE && visit(dep)) return true
    }
    color.set(id, BLACK)
    return false
  }
  for (const f of FEATURES) {
    if ((color.get(f.id) ?? WHITE) === WHITE && visit(f.id)) {
      errs.push(`dependency cycle involving ${f.id}`)
      break
    }
  }
  return errs
}
