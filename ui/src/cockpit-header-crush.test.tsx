// @vitest-environment jsdom
//
// THE COCKPIT HEADER CANNOT BE CRUSHED BELOW ITS OWN CONTENT — in every host, by the
// same mechanism, without anyone opting in.
//
// THE SHIPPED BUG (styles.css ~5883, 1366x768 ⇒ ~1607x900 effective px at the ~85%
// fit-to-window zoom). `.cockpit-header` pinned `min-height: 44px` for cross-mode
// alignment. A NON-AUTO min-height forfeits the CSS automatic minimum size (flexbox
// §4.5, grid §6.6) — the one thing that stops a box shrinking past its content — so the
// header became the only child of a cockpit column allowed to shrink below its own
// content. Its rows wrapped legitimately to 2–3 lines, the column's whole vertical
// deficit landed on it, it was squeezed back to 44px, and the extra rows rendered
// OUTSIDE the border box where the opaque `.ph-scope-panel` painted over them: the keyer
// Speed slider and Tune / Stop TX / CAT vanished under solid paint. A stop control
// hidden under opaque paint is why this one is guarded and not merely noted.
//
// WHY THIS FILE REPLACED cw-header-shrink.test.ts. The first repair was `flex-shrink: 0`
// applied through a hand-maintained four-selector allowlist
// (`.cw-cockpit > .cockpit-header, .phone-cockpit > …, .rtty-cockpit > …, .sstv-view > …`),
// and that test pinned those four NAMES. Protection was therefore per-host opt-in: a
// seventh host was unprotected until someone remembered the list, and Operate was
// DELIBERATELY excluded — it survived only because `.cockpit-body { flex:1; min-height:0 }`
// happens to be a fully shrinkable sibling, an accident of its current children that was
// asserted nowhere, inside an `overflow: hidden` shell. `flex-shrink` also cannot help a
// grid host at all (Tempo's header is a grid item, safe by `grid-template-rows: auto
// minmax(0,1fr)` — a flex-shaped fix misses it entirely).
//
// THE FIX THIS GUARDS: the 44px alignment floor moved OFF the header box and onto
// `.ch-identity`, the region the component always renders. The floor then arrives as
// CONTENT — it raises the header's content height instead of overriding its minimum — so
// the header's own `min-height` stays `auto` and the automatic minimum size does the
// protecting, in flex and in grid alike, in every host including a seventh nobody has
// written yet. No allowlist, no per-host rule, nothing to remember to join.
//
// FOUR ANGLES, because no one of them is the invariant:
//   1. HOST CENSUS (source): every component that renders a CockpitHeader is registered
//      here with the chain it mounts in — a seventh host fails until it is.
//   2. PER-HOST CASCADE (computed): for each registered chain, the winning `min-height`
//      on `.cockpit-header` must be `auto`. Specificity + source order, both sheets,
//      every media context. Operate included, on the same footing as the rest.
//   3. SUBJECT CENSUS (fail-closed): no rule anywhere may give `.cockpit-header` a
//      min-height or a non-visible overflow — including selector shapes the chain
//      matcher cannot evaluate (attributes, pseudos), where angle 2 would fail OPEN.
//      Overflow matters as much as min-height: the automatic minimum size only applies
//      while the item's computed overflow is `visible`.
//   4. THE FLOOR IS STILL LIVE (rendered): `.ch-identity` carries the 44px, and the
//      component renders it unconditionally — including with an empty mode indicator,
//      the case that would otherwise silently drop the cross-mode alignment.

import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { CockpitHeader } from './components/CockpitHeader'
import type { AppSnapshot } from './types'

// jsdom's URL resolves a relative reference against the DOCUMENT base, not the module —
// so every path here is built with node:path from the module's own directory.
const SRC_DIR = dirname(fileURLToPath(import.meta.url))
const SHEETS = ['styles.css', 'cockpit-panes.css'] as const

interface Rule {
  selector: string // one selector (lists split on top-level commas)
  body: string
  order: number
  sheet: string
  media: string | null // non-null ⇒ conditional
}

/** Brace-aware rule walk (comments stripped first so prose cannot read as a declaration —
 *  the trap documented in cockpit-floors.test.ts). @media/@supports nest; other at-rule
 *  bodies (@keyframes) are skipped wholesale. Same shape as cockpit-shells.test.ts. */
function parseRules(sheet: string, name: string, startOrder: number): Rule[] {
  const out: Rule[] = []
  let i = 0
  let order = startOrder
  const n = sheet.length

  function skipBalanced(): void {
    let depth = 1
    while (i < n && depth > 0) {
      if (sheet[i] === '{') depth++
      else if (sheet[i] === '}') depth--
      i++
    }
  }

  function parseBlock(media: string | null): void {
    let selStart = i
    while (i < n) {
      const ch = sheet[i]
      if (ch === '}') {
        i++
        return
      }
      if (ch === '{') {
        const sel = sheet.slice(selStart, i).trim()
        i++
        if (sel.startsWith('@')) {
          if (/^@(media|supports)\b/.test(sel)) parseBlock(sel)
          else skipBalanced()
        } else {
          const bodyStart = i
          while (i < n && sheet[i] !== '}' && sheet[i] !== '{') i++
          const body = sheet.slice(bodyStart, i)
          if (sheet[i] === '}') i++
          order++
          for (const s of sel.split(',')) {
            const one = s.trim().replace(/\s+/g, ' ')
            if (one) out.push({ selector: one, body, order, sheet: name, media })
          }
        }
        selStart = i
      } else if (ch === ';') {
        i++
        selStart = i
      } else {
        i++
      }
    }
  }

  parseBlock(null)
  return out
}

const RULES: Rule[] = (() => {
  const all: Rule[] = []
  for (const rel of SHEETS) {
    const text = readFileSync(join(SRC_DIR, rel), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
    // main.tsx imports cockpit-panes.css AFTER styles.css, so its rules are later in the
    // cascade; the running order counter reproduces that.
    all.push(...parseRules(text, rel, all.length ? all[all.length - 1].order : 0))
  }
  return all
})()

/** One compound of class selectors ('.a.b') as its class list; null when it contains
 *  anything else. Rejecting is fail-safe for the WINNER walk (an unevaluatable selector
 *  must never win by accident) and is exactly why angle 3 exists to cover the same rules
 *  from the other side. */
function compoundClasses(compound: string): string[] | null {
  if (/[\s>+~:[#]/.test(compound)) return null
  const parts = compound.match(/\.[a-zA-Z0-9_-]+/g)
  if (!parts || parts.join('') !== compound) return null
  return parts.map((p) => p.slice(1))
}

/** Right-to-left match of a class-only selector against an explicit ancestor CHAIN
 *  (outermost → the element; each entry is that ancestor's class set). */
function matchesChain(selector: string, chain: Array<Set<string>>): boolean {
  const parts = selector.split(/\s*([>+~])\s*|\s+/).filter((p): p is string => !!p)
  const subj = compoundClasses(parts[parts.length - 1] ?? '')
  if (!subj || !subj.every((c) => chain[chain.length - 1].has(c))) return false
  let idx = chain.length - 2
  let childOnly = false
  for (let i = parts.length - 2; i >= 0; i--) {
    const p = parts[i]
    if (p === '>') {
      childOnly = true
      continue
    }
    if (p === '+' || p === '~') return false
    const comp = compoundClasses(p)
    if (!comp) return false
    if (childOnly) {
      if (idx < 0 || !comp.every((c) => chain[idx].has(c))) return false
      idx--
      childOnly = false
    } else {
      while (idx >= 0 && !comp.every((c) => chain[idx].has(c))) idx--
      if (idx < 0) return false
      idx--
    }
  }
  return true
}

/** Class-count specificity — every candidate that can win here is a class-only compound. */
function specificity(selector: string): number {
  return (selector.match(/\./g) ?? []).length
}

/** Subject (rightmost compound) of a selector. */
function subject(selector: string): string {
  const parts = selector.split(/\s*[>+~]\s*|\s+/)
  return parts[parts.length - 1]
}

/** Rules whose SUBJECT carries the given class — i.e. rules that style that element,
 *  whatever their ancestor part looks like. */
function rulesOn(cls: string): Rule[] {
  return RULES.filter((r) => {
    const parts: string[] = subject(r.selector).match(/\.[a-zA-Z0-9_-]+/g) ?? []
    return parts.includes(`.${cls}`)
  })
}

/** Final min-height a block computes (in-block declaration order; the logical spelling
 *  counts too). There is no shorthand that sets min-height. */
function blockMinHeight(body: string): string | null {
  let v: string | null = null
  for (const decl of body.split(';')) {
    const m = /^\s*min-(?:height|block-size)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (m) v = m[1].replace(/\s+/g, ' ')
  }
  return v
}

/** Every non-visible overflow value a block declares (longhand or shorthand, either axis). */
function nonVisibleOverflow(body: string): string[] {
  const out: string[] = []
  for (const decl of body.split(';')) {
    const m = /^\s*(overflow(?:-[xy])?)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (!m) continue
    for (const value of m[2].split(/\s+/)) {
      if (value !== 'visible') out.push(`${m[1]}: ${m[2].trim()}`)
    }
  }
  return [...new Set(out)]
}

/** Cascade winner of a per-block-computed property for the element at the end of `chain`
 *  (specificity, then source order — conditional rules participate when their condition is
 *  the active one). */
function winningValue<T>(
  chain: Array<Set<string>>,
  activeMedia: string | null,
  blockValue: (body: string) => T | null,
): { value: T; selector: string; sheet: string } | null {
  let win: { value: T; selector: string; sheet: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null && r.media !== activeMedia) continue
    if (!matchesChain(r.selector, chain)) continue
    const v = blockValue(r.body)
    if (v === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: v, selector: r.selector, sheet: r.sheet, spec, order: r.order }
    }
  }
  return win && { value: win.value, selector: win.selector, sheet: win.sheet }
}

/** Media conditions that carry a min-height rule matching the element — each is a cascade
 *  context the header's automatic minimum has to survive. */
function mediaContexts(chain: Array<Set<string>>): Array<string | null> {
  const out = new Set<string | null>([null])
  for (const r of RULES) {
    if (r.media !== null && matchesChain(r.selector, chain) && blockMinHeight(r.body) !== null) {
      out.add(r.media)
    }
  }
  return [...out]
}

const HEADER = new Set(['cockpit-header'])
/** App mounts every workspace under `.app` → `.shell` (App.tsx ~2336). */
const ROOT = [new Set(['app']), new Set(['shell'])]
const shellChain = (cls: string) => [...ROOT, new Set(['layout', 'single', cls]), HEADER]

/**
 * EVERY host that renders a CockpitHeader, with the ancestor chain its header mounts in.
 * Registering a host is the point: the chain is what angle 2 computes against. Add the
 * seventh here and its cascade is checked with the other six — that is the whole of what
 * "protection is a property of the component" has to mean to be checkable.
 */
const HOSTS: Array<{ file: string; what: string; chain: Array<Set<string>> }> = [
  { file: 'components/PhoneCockpit.tsx', what: 'Phone', chain: shellChain('phone-cockpit') },
  { file: 'components/CwCockpit.tsx', what: 'CW', chain: shellChain('cw-cockpit') },
  { file: 'components/RttyCockpit.tsx', what: 'RTTY', chain: shellChain('rtty-cockpit') },
  { file: 'components/SstvView.tsx', what: 'SSTV', chain: shellChain('sstv-view') },
  {
    // The one the allowlist deliberately excluded: `overflow: hidden` shell, so a crushed
    // header's wrapped rows are clipped rather than painted over — the same loss of
    // Tune / Stop TX / CAT by a different route. Checked here on the same footing.
    file: 'components/OperateCockpit.tsx',
    what: 'Operate',
    chain: shellChain('operate-cockpit'),
  },
  {
    // Tempo: the header is a GRID item (`.layout[data-three-pane].has-tempo-header`
    // → `grid-template-rows: auto minmax(0,1fr)`), inside `.grid-header` which is a row
    // flex. flex-shrink could never have protected it; the automatic minimum size does,
    // because grid honours it too (§6.6). The chain omits the [data-three-pane] attribute
    // — the matcher is class-only, and no rule reaches the header through it.
    file: 'components/TempoHeader.tsx',
    what: 'Tempo',
    chain: [...ROOT, new Set(['layout', 'has-tempo-header']), new Set(['grid-header']), HEADER],
  },
]

/** Non-test .tsx sources under src/, relative to src/. */
function sourceFiles(dir = '', out: string[] = []): string[] {
  for (const e of readdirSync(join(SRC_DIR, dir), { withFileTypes: true })) {
    const rel = dir ? `${dir}/${e.name}` : e.name
    if (e.isDirectory()) sourceFiles(rel, out)
    else if (e.name.endsWith('.tsx') && !e.name.includes('.test.')) out.push(rel)
  }
  return out
}

const SRC = new Map(sourceFiles().map((f) => [f, readFileSync(join(SRC_DIR, f), 'utf8')]))

describe('the host census is complete (a seventh cockpit header cannot arrive unregistered)', () => {
  it('every component that renders a CockpitHeader is registered with its chain', () => {
    const rendered = [...SRC.entries()]
      .filter(([f, text]) => f !== 'components/CockpitHeader.tsx' && /<CockpitHeader[\s/>]/.test(text))
      .map(([f]) => f)
      .sort()
    expect(
      rendered,
      'A component renders a CockpitHeader that this guard has never computed a cascade for. ' +
        'Add it to HOSTS with the ancestor chain its header mounts in — that is how the ' +
        "component-level protection stays a claim this file can check instead of one it assumes.",
    ).toEqual(HOSTS.map((h) => h.file).sort())
  })

  it('nothing hand-rolls a .cockpit-header outside the shared component', () => {
    // A hand-rolled header would carry the class (and the sheet rules) but not the
    // component's `.ch-identity` — i.e. not the alignment floor, and not the structure
    // angle 4 renders. The mocks in *.test.tsx do exactly this on purpose and are excluded.
    const rogue = [...SRC.entries()]
      .filter(([f, text]) => f !== 'components/CockpitHeader.tsx' && /["'`]cockpit-header/.test(text))
      .map(([f]) => f)
    expect(rogue, `${rogue.join(', ')} writes the .cockpit-header class directly`).toEqual([])
  })
})

describe('the header keeps its automatic minimum size in every host', () => {
  for (const host of HOSTS) {
    it(`${host.what}: the winning min-height on .cockpit-header is auto`, () => {
      for (const ctx of mediaContexts(host.chain)) {
        const win = winningValue(host.chain, ctx, blockMinHeight)
        expect(
          win === null || win.value === 'auto',
          `${host.what}${ctx ? ` (within \`${ctx}\`)` : ''}: the cascade winner is ` +
            `\`${win?.selector} { min-height: ${win?.value} }\` (${win?.sheet}). A non-auto ` +
            'min-height FORFEITS the automatic minimum size (flexbox §4.5, grid §6.6), which is ' +
            'the only thing stopping this box being squeezed below its own wrapped rows — and ' +
            'those rows then render outside the border box, under whatever paints next. That is ' +
            'the 1366x768 bug that buried the keyer Speed slider and Tune / Stop TX / CAT. ' +
            'A height the header must not go below belongs on its CONTENT (`.ch-identity` ' +
            'carries the 44px cross-mode floor), never on the header box.',
        ).toBe(true)
      }
    })
  }
})

describe('nothing may opt the header out (fail-closed subject census)', () => {
  // Angle 2 walks a chain, so it cannot evaluate a selector with an attribute or a pseudo
  // and treats it as non-matching — fail-OPEN for exactly the shapes a responsive rule
  // would use (`[data-viewport='sm'] .cockpit-header { min-height: … }`). This angle reads
  // every rule whose SUBJECT is the header, whatever the ancestor part looks like, and
  // admits none of them. Over-strict on purpose: there is no correct reason to put a
  // minimum height, or a clip, on this box.
  it('no rule in either sheet declares min-height on .cockpit-header', () => {
    const offenders = rulesOn('cockpit-header')
      .filter((r) => blockMinHeight(r.body) !== null)
      .map((r) => `${r.sheet}: ${r.selector} { min-height: ${blockMinHeight(r.body)} }`)
    expect(
      offenders,
      `A rule floors the header box itself:\n${offenders.join('\n')}\nThat forfeits the ` +
        'automatic minimum size and re-arms the crush in EVERY host at once. Put the height on ' +
        'the content (`.ch-identity`) so it raises the header instead of overriding its minimum.',
    ).toEqual([])
  })

  it('no rule in either sheet clips .cockpit-header', () => {
    // The automatic minimum size applies only while the item's computed overflow is
    // `visible`; a clip would ALSO cut the wrapped rows off directly.
    const offenders = rulesOn('cockpit-header')
      .flatMap((r) => nonVisibleOverflow(r.body).map((d) => `${r.sheet}: ${r.selector} { ${d} }`))
    expect(
      offenders,
      `A rule clips the header:\n${offenders.join('\n')}\nA non-visible overflow zeroes the ` +
        'automatic minimum size (flexbox §4.5) — the crush comes back — and cuts the wrapped ' +
        'rows off on its own account.',
    ).toEqual([])
  })
})

describe('the cross-mode alignment floor is live, on the content', () => {
  afterEach(cleanup)

  const snap = {
    radio: {
      dialMhz: 14.074,
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: false,
      tuning: false,
      txAllowed: true,
    },
  } as unknown as AppSnapshot

  it('.ch-identity carries a positive px floor (the 44px that used to sit on the header)', () => {
    const win = winningValue([...shellChain('cw-cockpit').slice(0, -1), HEADER, new Set(['ch-identity'])], null, blockMinHeight)
    expect(win, '.ch-identity has no min-height — the cross-mode alignment floor is gone, and ' +
      'the FT8 tier tiles / CW badge / Phone sideband group no longer share a header height.').not.toBeNull()
    expect(
      /^\d+(\.\d+)?px$/.test(win!.value) && parseFloat(win!.value) > 0,
      `.ch-identity min-height is \`${win!.value}\` (${win!.selector}) — the floor must be a ` +
        'positive length on the CONTENT; that is what makes the header at least this tall ' +
        'while leaving the header box free to be taller when it wraps.',
    ).toBe(true)
  })

  it('CockpitHeader always renders .ch-identity, even with an empty mode indicator', () => {
    // The floor lives on this box now, so "the component always renders it" is load-bearing
    // rather than incidental: a conditional `.ch-identity` would drop the alignment in
    // whichever cockpit passed no indicator, silently and only there.
    for (const indicator of [<span key="i">CW</span>, null]) {
      const { container } = render(
        <CockpitHeader snap={snap} modeIndicator={indicator} bandControl={<span>20m</span>} />,
      )
      expect(
        container.querySelector('.cockpit-header > .ch-identity'),
        'CockpitHeader rendered no .ch-identity — the box the cross-mode floor lives on.',
      ).not.toBeNull()
      cleanup()
    }
  })
})
