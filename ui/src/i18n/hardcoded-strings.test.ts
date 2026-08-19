// THE GUARD — a migrated surface cannot drift back to hardcoded English.
//
// Migrating 250 files is a long job, and every long job in this tree has the same failure: the
// migrated part decays while the rest is being done, because nothing stops the next feature
// from adding a plain `title="…"` next to a translated one. So this exists before the second
// batch, not after the last.
//
// IT COMPUTES. The detector PARSES each file with the TypeScript compiler (already a
// devDependency — no new one) and asks the syntax tree which strings reach an operator: JSX
// text, the handful of attributes that are read aloud or shown on hover, string literals used
// as JSX children, and the first argument of the calls that put prose on screen. It never
// greps for a phrase, and it never asserts a phrase is absent — the presence-matching CSS
// tests that let dead fixes ship twice are the reason that rule exists in this project.
//
// IT IS PROVEN TO FIRE ON EVERY RUN, not once by hand: `the detector fires` below parses a
// fixture written to break every rule and asserts it is caught, kind by kind. A guard that has
// only ever been green is a guard nobody has tested. (It was also driven red by hand against
// the real files while it was written — a `title="Stop"` added to SettingsStation.tsx produced
// `components/SettingsStation.tsx:114 attr:title "Stop"`.)
//
// ---------------------------------------------------------------------------------------
// ⚠️ SCOPE — WHAT THIS DOES NOT COVER, stated because a partial guard read as a total one is
// worse than none.
// ---------------------------------------------------------------------------------------
//
//   • It checks exactly the files in MIGRATED. Everything else — including the other 8,900
//     lines of SettingsPanel.tsx, which appears in PARTIAL for its keys alone — is
//     deliberately unchecked and still hardcoded English.
//   • The list only ever GROWS. Removing a file from it is how a surface silently un-migrates,
//     so removal needs the same scrutiny as the migration did.
//   • It cannot see prose that reaches the operator from Rust (~440 `format!` sites), from a
//     data registry (`features/registry.ts` labels, `settings/registry.ts` labels/keywords),
//     or from a string built with `+`. Those are phase-2 and phase-3 decisions.
//   • It cannot tell a WRONG translation from a right one, and it cannot tell that a key is
//     used in the right place. It only proves the English is not baked into the JSX.
//   • A prose string hidden behind a named constant (`const MSG = 'Saved'; <b>{MSG}</b>`)
//     passes. That is the deliberate escape hatch invariant TOKENS use (see STATION_EXAMPLES),
//     and it costs an author a conscious act.

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'
import { EN, type MessageKey } from './index'

/**
 * The migrated surfaces. ADD A FILE HERE THE MOMENT YOU MIGRATE IT — and not before, or CI
 * goes red on strings nobody has moved yet.
 *
 * The pilot is "Settings ▸ Station": the fieldset itself, the search box that is the way into
 * it, and the first-run banner that points at it — plus `RevealNudge.tsx`, which is here for
 * one reason: the Station surface has no emphasised prose, and the rich-text path needs to be
 * proven on a real shipped sentence rather than on a fixture.
 */
const MIGRATED = [
  'components/SettingsStation.tsx',
  'components/SettingsSearch.tsx',
  'components/OnboardingBanner.tsx',
  'components/RevealNudge.tsx',
  // Batch 1 (2026-08-18) — the getting-started guide and the app's own notices. Pure prose,
  // no radio state: the guide is documentation, the update banner and crash fallback are the
  // app talking about itself. Chosen as the first batch because GettingStartedGuide.tsx is
  // the densest markup surface in the tree, which is what proves the `<T>` marker path at
  // volume rather than on one shipped sentence.
  'components/GettingStartedGuide.tsx',
  'components/AssistanceNote.tsx',
  'components/UpdateBanner.tsx',
  'features/updateCheck.ts',
  'components/ErrorBoundary.tsx',
  'main.tsx',
  // Batch 2 (2026-08-18) — the logbook and QSO entry. The densest UNITS surface in the tree:
  // every callsign, RST, band, mode, frequency, park reference and ADIF field name in these
  // five files is invariant and stays in the code (LOG_EXAMPLES in Logbook.tsx and
  // LogEntry.tsx, PARK_PROGRAMS, and the Q-code/service labels beside them), while the prose
  // around them moves. It also carried nine of the tree's hand-rolled plurals, which is what
  // proves the `{{count}}` path on real shipped counts rather than on a fixture.
  'components/Logbook.tsx',
  'components/LogEntry.tsx',
  'components/LogConfirm.tsx',
  'components/StationCard.tsx',
  'components/StationList.tsx',
  // Batch 3 (2026-08-18) — awards, journey, stats and the needed board. Two things this
  // batch proves that the first two did not: the AWARD NAMES are invariant tokens exactly as
  // callsigns are (DXCC, WAZ, VUCC, IOTA name programmes an operator applies to — a
  // translated one names nothing), and a REGISTRY can be migrated without touching its
  // consumers. `needVisuals.ts` and `statusMeta.ts` are label tables a dozen surfaces index
  // directly; their words resolve through getters, so the record shape is unchanged and the
  // lookup happens when the string is read rather than when the module loads.
  'components/AwardsView.tsx',
  'components/AwardsJourney.tsx',
  'components/JourneyView.tsx',
  'components/NeededPanel.tsx',
  'components/StatsView.tsx',
  'features/needVisuals.ts',
  'statusMeta.ts',
  'useJourneyUnlocks.ts',
  // Batch 4 (2026-08-18) — the maps, the globes and the propagation panes. The WIDEST
  // batch (33 files) and the densest MEASUREMENT surface in the tree: grids, bearings,
  // distances, SFI/Kp/A/Bz indices, MUF in MHz, R/S/G scale letters and every band and
  // mode name on these screens is an invariant token and stays in the code (MUF_LABEL,
  // INDEX, PATH_SP/PATH_LP, ENGINE_P533, QRZ_LABEL and the RSG letters). It also draws the
  // line this phase does NOT cross: the prose these surfaces receive from the BACKEND —
  // workability words, band reasons, insight sentences, window headlines — is interpolated
  // as a value, never translated, and moves in phase 3.
  'components/MapView.tsx',
  'components/MapLegend.tsx',
  'components/Globe3D.tsx',
  'components/QsoGlobe.tsx',
  'propViz.ts',
  'openingAlert.ts',
  'components/DxpeditionsView.tsx',
  'features/dxpedChase.ts',
  'features/dxpedAlarm.ts',
  'features/chaseFeed.ts',
  'features/shareCard.ts',
  'components/prop/ActivityMatrix.tsx',
  'components/prop/BandAdvisor.tsx',
  'components/prop/BandConditionStrip.tsx',
  'components/prop/BeaconMonitor.tsx',
  'components/prop/BestBandTable.tsx',
  'components/prop/ChaseFeedPane.tsx',
  'components/prop/ChasePane.tsx',
  'components/prop/DxpedCalendar.tsx',
  'components/prop/DxpedDigest.tsx',
  'components/prop/DxpedMonth.tsx',
  'components/prop/dxpedLink.ts',
  'components/prop/GetoutCompass.tsx',
  'components/prop/GreylineWindow.tsx',
  'components/prop/InsightFeed.tsx',
  'components/prop/LikelihoodHeatmap.tsx',
  'components/prop/MapInsightRail.tsx',
  'components/prop/MeasuredMuf.tsx',
  'components/prop/OpeningStrip.tsx',
  'components/prop/OpeningsLogPane.tsx',
  'components/prop/ScalesAnnunciator.tsx',
  'components/prop/SpaceWxGauges.tsx',
  'components/prop/WorkNowCard.tsx',
  // Batch 5 (2026-08-18) — spots, the watch list, the display filters and the Settings
  // sections that configure them. The firehose is nearly ALL tokens: every callsign,
  // spotter, DXCC entity, US state, band, mode, submode, frequency and cluster comment on
  // these screens is data and stays in the code, as do the POTA/SOTA programme names, the
  // P/S/✈/B badge glyphs, `de`, `DXCC`, and the prefix/grid EXAMPLES the watch and hide
  // filters offer (WATCH_EXAMPLES, HIDE_EXAMPLES). It also proves the registry-by-getter
  // path a second time: `SpotLegend.tsx`'s two badge tables resolve their words when read,
  // so `BandStrip.tsx` — which this batch does not own — reads them unchanged.
  'components/SpotsPanel.tsx',
  'components/SpotDialog.tsx',
  'components/SpotLegend.tsx',
  'components/BandMap.tsx',
  'components/PounceBanner.tsx',
  'components/WatchlistPanel.tsx',
  'components/HideCallsPicker.tsx',
  'components/CountryExclude.tsx',
  'components/RoamPanel.tsx',
  'alerts.ts',
  // Batch 6 (2026-08-18) — POTA/SOTA, Field Day and the Settings ▸ Contesting sections that
  // set them up. The whole review here is the units rule, and it lands on the EXCHANGE: park
  // and summit references (K-1234, W7A/MN-001), ARRL/RAC section codes and their division
  // names, Field Day class and category strings (3A, 2O) with the H/I/M/O letters, the FD
  // mode codes (DIG/CW/PH), every score and multiplier, and the FD_BONUSES names — all of
  // them values an entry is submitted with, so all of them stay in the code. So do the
  // programme and event names themselves (POTA, SOTA, ARRL Field Day, Winter Field Day,
  // WFD): they name a thing an operator enters, exactly as an award name does, and a
  // translated one names nothing. What moved is the prose around them. Deliberately NOT
  // migrated and flagged in the file: the Summary and Dupe-sheet EXPORTS, which build a
  // fixed-width document rather than interface prose.
  'components/PotaSotaView.tsx',
  'components/FieldDayView.tsx',
  'components/ContestCalendarPane.tsx',
  'fdEvent.ts',
  // Batch 7 (2026-08-18) — the Satellites section, the Connect Passes pane and the nine
  // composers behind them. One 3,900-line planning surface plus nine small modules, and the
  // units rule lands on the SKY and the DIAL: bird names, NORAD ids, TLE epochs, every
  // uplink/downlink frequency and offset, the SatNOGS transponder descriptions with their
  // per-leg mode names, azimuths, elevations, ranges, altitudes and the compass letters all
  // stay in the code, as do the mode names the radio binding prints (MODE_FM/MODE_SSB), the
  // NORAD label and the passband plot's centre tick. Two things this batch proves the earlier
  // ones did not: an INSTRUMENT can have text that is not prose — the sky dome's `az 143°`
  // plates are sized from the string by viewBox arithmetic, so they are tick labels and are
  // deliberately not migrated — and a module-level LABEL TABLE with a wire value beside every
  // label (`satVfo.ts`, read by two components) migrates through getters exactly as
  // `needVisuals.ts` did, so Settings ▸ Radio reads it unchanged. It also carried the tree's
  // heaviest run of mid-sentence conditionals: the Doppler row's six states, the badge's
  // four, the readiness rail's uplink offers and the TX-sideband note are each ONE entry with
  // the variable clause interpolated.
  'components/SatellitesView.tsx',
  'features/satHealth.ts',
  'features/satLane.ts',
  'features/satVfo.ts',
  'features/satAlarm.ts',
  'features/issAutoArm.ts',
  'features/tleMessages.ts',
  'features/elementBands.ts',
  'features/satPassAlert.ts',
  'components/prop/SatPassesPane.tsx',
  // Batch 8 (2026-08-18) — memories, recall and radio programming. The densest UNITS surface
  // of the low-risk half: every string on these screens sits beside a dial frequency, a step,
  // a CTCSS tone, an offset or a band name, and all of those stay in the code — as do the
  // rig-model names in the Program section's "Max name" list (FT-60, Baofeng, Yaesu, Anytone
  // are tokens exactly as a callsign is), the mode and CTCSS datalists, and the `value` of
  // every <select>, whose LABEL moved while the stored token did not. Three things this batch
  // settles that the earlier seven did not. A string can be operator-visible AND legally
  // fixed: the two repeater directories' attribution lines are written verbatim into the
  // exported CSV as well as shown in the footer, so they are constants, not entries. A
  // FILE NAME must not be built from a translated word — the export slug is invariant now,
  // because the old ASCII squeeze would have reduced a non-Latin view name to
  // `nexus-memories-.csv`. And a name spliced into "Search …" was replaced by one whole
  // placeholder per view: lower-casing a translated noun is wrong wherever nouns capitalise.
  // The five hand-rolled plurals in MemoriesView became `{{count}}` entries; the two-count
  // reports (imported/skipped, added/refreshed, saved/already-there) are one entry per count.
  'components/MemoriesView.tsx',
  'components/RadioProgView.tsx',
  'components/MemoryStrip.tsx',
  'components/RecallPanel.tsx',
  'components/BandPicker.tsx',
  'components/BandStrip.tsx',
  'components/FrequencyControl.tsx',
  'components/RadioPicker.tsx',
  'components/RadioSwitcher.tsx',
  'rigFormChecks.ts',
]

/**
 * Files where ONE SECTION is migrated and the rest is not.
 *
 * They are scanned for KEYS (so the catalog checks below see the entries they use) but NOT
 * for hardcoded strings, because the un-migrated remainder of the file is still English by
 * design. `SettingsPanel.tsx` is 9,000 lines: its Spots & Alerts and Contesting sections were
 * migrated with the panels they configure, batch 9 (2026-08-19) took the SHELL — the panel
 * chrome, the tab rail, Save, the toasts/confirms its handlers raise — plus the whole
 * Appearance tab (Workspace + Features + Accessibility), batch 10 (2026-08-19) took the
 * Logging & Connectors sections down to the Confirmations fieldset: Connections, Worked-before
 * (B4) & dupes, Integrations & Feeds with its Antenna gain disclosure, DXKeeper, N3FJP, N1MM+,
 * the LoTW users list and the callsign→state database, and batch 11 (2026-08-19) took
 * Confirmations itself — LoTW, eQSL, QRZ, HamQTH, ClubLog, HRDLog, RepeaterBook and
 * Cloudlog/Wavelog. Batch 12 (2026-08-19) took the first three sections of the Radio tab —
 * the dual-radio roster with its band coverage and band+mode routing table, Profiles, and Rig
 * & CAT down to Test CAT, Advanced included. Putting the file on MIGRATED would report the
 * tabs still to come; leaving it off entirely would make every key those sections use look
 * like an orphan.
 *
 * ⚠️ THIS LIST IS A CONCESSION, NOT A HOME. A file belongs here only while a migration is
 * partial; when the last section moves it graduates to MIGRATED, and nothing else may be
 * added to it to dodge a failing check.
 */
const PARTIAL = ['components/SettingsPanel.tsx']

/** Attributes whose value a human reads — on hover, or through a screen reader. */
const VISIBLE_ATTRS = new Set([
  'title',
  'placeholder',
  'alt',
  'label',
  'aria-label',
  'aria-description',
  'aria-placeholder',
  'aria-roledescription',
  'aria-valuetext',
])

/** Calls whose first argument becomes visible prose. */
const PROSE_CALLS = new Set(['pushToast', 'withErrorToast', 'confirm', 'alert', 'confirmDialog'])

interface Finding {
  file: string
  line: number
  kind: string
  text: string
}

/** Two or more letters in a row — the difference between prose and `✕`, `×`, `—`, `{' '}`. */
const isProse = (s: string) => /\p{L}{2,}/u.test(s)

/** The literal text an expression contributes, or null when it is computed at runtime. */
function literalText(sf: ts.SourceFile, n: ts.Node | undefined): string | null {
  if (!n) return null
  if (ts.isStringLiteral(n) || ts.isNoSubstitutionTemplateLiteral(n)) return n.text
  if (ts.isTemplateExpression(n)) {
    return [n.head.text, ...n.templateSpans.map((s) => s.literal.text)].join(' ')
  }
  if (ts.isJsxExpression(n)) return literalText(sf, n.expression)
  if (ts.isParenthesizedExpression(n)) return literalText(sf, n.expression)
  if (ts.isConditionalExpression(n)) {
    // `title={ok ? 'Receiving audio' : 'No RX audio'}` — both arms are prose.
    const parts = [literalText(sf, n.whenTrue), literalText(sf, n.whenFalse)].filter(Boolean)
    return parts.length ? parts.join(' ') : null
  }
  return null
}

/** Every user-visible string literal in one source file. */
export function findHardcoded(file: string, src: string): Finding[] {
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.ES2020, true, ts.ScriptKind.TSX)
  const out: Finding[] = []
  const line = (n: ts.Node) => sf.getLineAndCharacterOfPosition(n.getStart(sf)).line + 1
  const add = (n: ts.Node, kind: string, text: string) =>
    out.push({ file, line: line(n), kind, text: text.trim().replace(/\s+/g, ' ').slice(0, 60) })

  const visit = (n: ts.Node): void => {
    if (ts.isJsxText(n)) {
      if (isProse(n.text)) add(n, 'jsx-text', n.text)
    } else if (ts.isJsxAttribute(n)) {
      const name = n.name.getText(sf)
      if (VISIBLE_ATTRS.has(name)) {
        const text = literalText(sf, n.initializer)
        if (text && isProse(text)) add(n, `attr:${name}`, text)
      }
    } else if (
      ts.isJsxExpression(n) &&
      n.parent &&
      (ts.isJsxElement(n.parent) || ts.isJsxFragment(n.parent))
    ) {
      // A literal used as a JSX child: `{'Save'}`, `{`Work ${call}`}`.
      const text = literalText(sf, n.expression)
      if (text && isProse(text)) add(n, 'jsx-child', text)
    } else if (ts.isCallExpression(n)) {
      const callee = ts.isPropertyAccessExpression(n.expression)
        ? n.expression.name.text
        : ts.isIdentifier(n.expression)
          ? n.expression.text
          : ''
      if (PROSE_CALLS.has(callee)) {
        const text = literalText(sf, n.arguments[0])
        if (text && isProse(text)) add(n, `call:${callee}`, text)
      }
    }
    n.forEachChild(visit)
  }
  visit(sf)
  return out
}

/** Every catalog key a file names — `t('…')` and `<T k="…">`. */
export function findKeys(file: string, src: string): string[] {
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.ES2020, true, ts.ScriptKind.TSX)
  const out: string[] = []
  const visit = (n: ts.Node): void => {
    if (ts.isCallExpression(n) && ts.isIdentifier(n.expression) && n.expression.text === 't') {
      const a = n.arguments[0]
      if (a && ts.isStringLiteral(a)) out.push(a.text)
    } else if (ts.isJsxAttribute(n) && n.name.getText(sf) === 'k') {
      const v = n.initializer
      if (v && ts.isStringLiteral(v)) out.push(v.text)
    } else if (ts.isPropertyAssignment(n)) {
      // Key tables: `{ labelKey: 'settings.station.callsign.label' }`.
      const name = n.name.getText(sf)
      if (/Key$/.test(name) && ts.isStringLiteral(n.initializer)) out.push(n.initializer.text)
      if (name === 'prose' && ts.isStringLiteral(n.initializer)) out.push(n.initializer.text)
    }
    n.forEachChild(visit)
  }
  visit(sf)
  return out
}

const read = (rel: string) => readFileSync(fileURLToPath(new URL(`../${rel}`, import.meta.url)), 'utf8')

// ── the guard fires (positive control) ────────────────────────────────────────────────

const VIOLATIONS = `
import { pushToast } from '../toast'
export function Bad({ ok, call }: { ok: boolean; call: string }) {
  return (
    <div title="Stop transmitting" aria-label="Dismiss alert">
      Hardcoded prose here
      {'Also hardcoded'}
      <input placeholder={\`QSY to \${call}\`} />
      <button title={ok ? 'Receiving audio' : 'No RX audio'} onClick={() => pushToast('Could not save')} />
      <span aria-hidden>✕</span>
      {' '}
    </div>
  )
}
`

const CLEAN = `
import { t } from '../i18n'
import { T } from '../i18n/T'
export function Good({ call }: { call: string }) {
  return (
    <div title={t('common.dismiss')} aria-label={t('settings.search.label')}>
      <T k="reveal.prompt" tags={{ b: <strong /> }} vals={{ achievement: call, feature: call }} />
      <input placeholder={t('settings.search.placeholder')} type="search" role="combobox" />
      <span aria-hidden>✕</span>{' '}
      <em>{t('reveal.enable')}</em>
    </div>
  )
}
`

describe('the detector fires', () => {
  const found = findHardcoded('fixture.tsx', VIOLATIONS)
  const kinds = found.map((f) => f.kind).sort()

  it('catches every kind of hardcoded operator-visible string', () => {
    expect(kinds).toEqual([
      'attr:aria-label',
      'attr:placeholder',
      'attr:title',
      'attr:title',
      'call:pushToast',
      'jsx-child',
      'jsx-text',
    ])
  })

  it('reports where, so the failure is actionable', () => {
    const text = found.find((f) => f.kind === 'jsx-text')
    expect(text?.text).toBe('Hardcoded prose here')
    expect(text?.line).toBeGreaterThan(0)
  })

  it('does NOT fire on glyphs, spacers or non-visible attributes — the other direction', () => {
    expect(findHardcoded('fixture.tsx', CLEAN)).toEqual([])
  })
})

// ── the migrated surfaces are clean (the guard proper) ────────────────────────────────

describe('migrated surfaces carry no hardcoded operator-visible strings', () => {
  it('has a non-empty scope — a guard over nothing proves nothing', () => {
    expect(MIGRATED.length).toBeGreaterThan(0)
    for (const rel of [...MIGRATED, ...PARTIAL])
      expect(read(rel).length, `${rel} is readable`).toBeGreaterThan(0)
  })

  it('never checks a PARTIAL file for hardcoded strings — that is the whole concession', () => {
    for (const rel of PARTIAL) expect(MIGRATED).not.toContain(rel)
  })

  for (const rel of MIGRATED) {
    it(`${rel}`, () => {
      const found = findHardcoded(rel, read(rel))
      expect(
        found.map((f) => `${f.file}:${f.line} ${f.kind} "${f.text}"`),
        'this file is on the migrated list — move the string into ui/src/i18n/en.ts and ' +
          'call t() / <T>, or, if it is a technical token (callsign, grid, frequency, mode, ' +
          'ADIF field), name it as a constant and say so',
      ).toEqual([])
    })
  }
})

// ── keys and catalog agree ────────────────────────────────────────────────────────────

describe('every key resolves, and every entry is used', () => {
  const used = new Set([...MIGRATED, ...PARTIAL].flatMap((rel) => findKeys(rel, read(rel))))

  it('finds keys at all — the extractor is not silently returning nothing', () => {
    expect(used.size).toBeGreaterThan(10)
  })

  it('names no key the English catalog lacks', () => {
    const missing = [...used].filter((k) => !(k in EN))
    expect(missing, 'a t()/<T> key with no catalog entry renders the key itself').toEqual([])
  })

  it('leaves no orphan entry behind', () => {
    const orphans = (Object.keys(EN) as MessageKey[]).filter((k) => !used.has(k))
    expect(
      orphans,
      'catalog entries nothing references — every one of these would be handed to a ' +
        'translator to translate for nobody',
    ).toEqual([])
  })
})
