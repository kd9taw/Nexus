// @vitest-environment jsdom
//
// THE MEM STRIP'S CEILING CANNOT EAT A CHIP — in all three cockpit headers that mount
// it, at every UI zoom the app offers.
//
// THE SHIPPED BUG (styles.css, `.mem-strip`). The strip carried a hard device-px
// ceiling with the vertical axis explicitly clipped and no recovery:
//
//     .mem-strip { flex-wrap: nowrap; max-height: 26px; overflow-x: auto; overflow-y: hidden }
//
// Its content is written in the operator's TYPE, not in device px: `.mem-strip-save` /
// `.mem-strip-manage` are `font-size: 0.78rem` with 1px padding and a 1px border, and
// `.mem-chip` is `0.72rem` the same way. `rem` resolves against the ROOT font-size —
// `<html>`, which is outside `.app` and which this sheet never sets, so it is whatever
// the browser/OS says (16px by default, larger for anyone who has bumped it). On top of
// that the strip's own `overflow-x: auto` reserves a horizontal scrollbar, and the app
// styles that scrollbar `::-webkit-scrollbar { height: 10px }` — space-consuming in
// Chromium. So at the DEFAULT root font the one row already resolved to ~19px of button
// box plus 10px of scrollbar = ~29px, against a 26px ceiling on an axis with
// `overflow-y: hidden`. The bottom ~3px of every chip and of the ＋/≡ buttons was already
// outside the clip before any zoom, any font bump, or any theme with a thicker border —
// and being clipped rather than scrolled, there was no way to reach it.
//
// A px ceiling over em/rem content is wrong IN KIND, not by 3px: the two sides are
// denominated in different units, so no amount of re-tuning the constant makes the
// relationship hold. The repair is that the ceiling YIELDS to the strip's own content.
// The bound the ceiling was written for (the old MemoryBank list grew the header with
// every saved channel) is now STRUCTURAL — `flex-wrap: nowrap` means the strip is exactly
// one row however many favorites exist, and `max-width: 40rem` + `overflow-x: auto` bound
// it sideways. One row cannot grow with the favorite count, so nothing needs a cap in
// order for the header-growth bug to stay dead. Nor does the header get taller: the
// cross-mode alignment floor on `.ch-identity` (44px, see cockpit-header-crush.test.tsx)
// is already well above the strip's ~29px row.
//
// FIVE ANGLES, because no one of them is the invariant:
//   1. HOST CENSUS (source): every component that renders a MemoryStrip is registered
//      here with the chain it mounts in — a fourth host fails until it is.
//   2. THE CEILING YIELDS (computed, at 100% / 150% / 175% zoom): for each registered
//      chain, in each media context, the winning `max-height`/`height` on `.mem-strip`
//      must leave room for the strip's own resolved one-row box — the tallest item it
//      renders, plus the horizontal scrollbar it reserves — at every zoom.
//   3. SUBJECT CENSUS (fail-closed): the same predicate over every rule whose SUBJECT is
//      `.mem-strip`, whatever its selector shape — including shapes the chain matcher
//      cannot evaluate (attributes, pseudos) and where angle 2 would fail OPEN.
//   4. THE HORIZONTAL SCROLL SURVIVES (computed): `overflow-x` must still resolve to a
//      scrolling value. Horizontal scroll is the CORRECT behavior for a chip strip; a
//      "fix" that let the strip wrap or run off the header instead is not this fix.
//   5. THE BOXES ARE THE RENDERED ONES (rendered): MemoryStrip really does emit the
//      classes angle 2 measures, and emits NO item class angle 2 does not measure — so
//      the arithmetic cannot go stale behind a taller item nobody told it about.
//   6. THE CHIPS ARE WHAT SCROLLS (computed): angle 4's horizontal scroll only means
//      anything if the chips can OVERFLOW the strip. `.mem-chip` is a scroll container
//      (`overflow: hidden`, for its ellipsis), and per CSS Flexbox §4.5 a flex item that
//      is a scroll container has its automatic minimum size resolved to ZERO. With the
//      initial `flex-shrink: 1` the chips therefore shrink without limit: the line always
//      fits, `overflow-x: auto` never has anything to scroll, and the ellipsis eats the
//      names — while `MEM`/`＋`/`≡` are not scroll containers, keep their min-content
//      floor, and stay readable. That is the reported picture exactly (truncated chips
//      beside intact buttons), and `flex-shrink: 0` on the chip is the whole of the fix.
//
// ON THE ZOOM SWEEP. UI zoom is `zoom: var(--ui-zoom)` on `.app` (styles.css ~3274).
// Chromium has shipped two readings of how `rem` behaves under `zoom` — scaled with the
// element's zoom, or resolved against the unzoomed root and left alone — and they differ
// by a lot at 175%. This guard refuses to depend on which one is running: it resolves the
// ceiling AND the content under BOTH readings and demands the invariant hold in each. A
// verdict that flips with a browser-version detail would be worse than no guard.

import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync, readdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { MemoryStrip } from './components/MemoryStrip'
import { memoriesStore, saveFavoriteFromDial, STRIP_FAVORITE_LIMIT } from './features/memories'

// jsdom's URL resolves a relative reference against the DOCUMENT base, not the module —
// so every path here is built with node:path from the module's own directory.
const SRC_DIR = dirname(fileURLToPath(import.meta.url))
const SHEETS = ['styles.css', 'cockpit-panes.css'] as const

/** `rem` resolves against <html>, which is outside `.app` and which this sheet never
 *  sizes — so it is the UA default. An operator who has raised it gets BIGGER content
 *  against the same ceiling; 16 is therefore the friendliest case, not a safe one. */
const ROOT_FONT_PX = 16
/** `line-height: normal` is ~1.15–1.5 for the system-ui stack. Using the low end makes
 *  the computed content box an UNDER-estimate, so anything this guard reports is real. */
const NORMAL_LINE_HEIGHT = 1.2
/** The three UI zooms the ask names; useScale offers a wider range still. */
const ZOOMS = [1, 1.5, 1.75] as const

interface Rule {
  selector: string // one selector (lists split on top-level commas)
  body: string
  order: number
  sheet: string
  media: string | null // non-null ⇒ conditional
}

/** Brace-aware rule walk (comments stripped first so prose cannot read as a declaration —
 *  the trap documented in cockpit-floors.test.ts). @media/@supports nest; other at-rule
 *  bodies (@keyframes) are skipped wholesale. Same shape as cockpit-header-crush.test.tsx. */
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
 *  must never win by accident) and is exactly why angle 3 covers the same rules from the
 *  other side. */
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

/** Split a declaration value on TOP-LEVEL whitespace (never inside `var(…, …)`/`calc(…)`). */
function splitTop(value: string): string[] {
  const out: string[] = []
  let depth = 0
  let cur = ''
  for (const ch of value) {
    if (ch === '(') depth++
    else if (ch === ')') depth--
    if (depth === 0 && /\s/.test(ch)) {
      if (cur) out.push(cur)
      cur = ''
    } else cur += ch
  }
  if (cur) out.push(cur)
  return out
}

/** Declarations of a block, in source order, as [prop, value]. */
function declarations(body: string): Array<[string, string]> {
  const out: Array<[string, string]> = []
  for (const decl of body.split(';')) {
    const m = /^\s*([a-zA-Z-]+)\s*:\s*(\S[^]*?)\s*$/.exec(decl)
    if (m) out.push([m[1].toLowerCase(), m[2].replace(/\s+/g, ' ')])
  }
  return out
}

/** The vertical-box properties this arithmetic reads, as raw CSS values. */
interface BoxDecls {
  fontSize?: string
  lineHeight?: string
  padTop?: string
  padBottom?: string
  borderTop?: string
  borderBottom?: string
  maxHeight?: string
  height?: string
  overflowX?: string
  overflowY?: string
  flexShrink?: string
}

/** First token of a `border` / `border-top` shorthand, which is where the width sits
 *  when one is given (`1px solid var(--border)`); `none`/`0` read as zero width. */
function borderWidth(value: string): string {
  const first = splitTop(value)[0] ?? '0'
  return /^(none|hidden)$/i.test(first) ? '0' : first
}

/** Apply one block's declarations onto an accumulating box (in-block source order). */
function applyBlock(box: BoxDecls, body: string): void {
  for (const [prop, value] of declarations(body)) {
    const parts = splitTop(value)
    switch (prop) {
      case 'font-size':
        box.fontSize = value
        break
      case 'line-height':
        box.lineHeight = value
        break
      case 'padding':
        box.padTop = parts[0]
        box.padBottom = parts[2] ?? parts[0]
        break
      case 'padding-block':
        box.padTop = parts[0]
        box.padBottom = parts[1] ?? parts[0]
        break
      case 'padding-top':
      case 'padding-block-start':
        box.padTop = value
        break
      case 'padding-bottom':
      case 'padding-block-end':
        box.padBottom = value
        break
      case 'border':
        box.borderTop = box.borderBottom = borderWidth(value)
        break
      case 'border-width':
        box.borderTop = parts[0]
        box.borderBottom = parts[2] ?? parts[0]
        break
      case 'border-block':
        box.borderTop = box.borderBottom = borderWidth(value)
        break
      case 'border-top':
        box.borderTop = borderWidth(value)
        break
      case 'border-bottom':
        box.borderBottom = borderWidth(value)
        break
      case 'border-top-width':
        box.borderTop = value
        break
      case 'border-bottom-width':
        box.borderBottom = value
        break
      case 'max-height':
      case 'max-block-size':
        box.maxHeight = value
        break
      case 'height':
      case 'block-size':
        box.height = value
        break
      case 'overflow':
        box.overflowX = parts[0]
        box.overflowY = parts[1] ?? parts[0]
        break
      case 'overflow-x':
        box.overflowX = value
        break
      case 'overflow-y':
        box.overflowY = value
        break
      case 'flex-shrink':
        box.flexShrink = value
        break
      case 'flex': {
        // The shorthand's shrink is the SECOND unitless number; with fewer than two it
        // stays the shorthand default of 1 (`flex: 1` = `1 1 0%`). `none` is `0 0 auto`.
        const kw = value.trim().toLowerCase()
        if (kw === 'none') box.flexShrink = '0'
        else {
          const nums = parts.filter((p) => /^\d*\.?\d+$/.test(p))
          box.flexShrink = nums.length >= 2 ? nums[1] : '1'
        }
        break
      }
      default:
        break
    }
  }
}

/** Cascade the sheet onto the element at the end of `chain`: every matching rule, applied
 *  in (specificity, source order) — i.e. the winner of each declaration is the last one
 *  applied, exactly as the cascade resolves it, shorthands and longhands interleaved. */
function computeBox(chain: Array<Set<string>>, activeMedia: string | null): BoxDecls {
  const box: BoxDecls = {}
  const matching = RULES.filter(
    (r) => (r.media === null || r.media === activeMedia) && matchesChain(r.selector, chain),
  ).sort((a, b) => specificity(a.selector) - specificity(b.selector) || a.order - b.order)
  for (const r of matching) applyBlock(box, r.body)
  return box
}

/** Media conditions carrying a rule that touches this element's vertical box — each is a
 *  cascade context the invariant has to survive. */
function mediaContexts(chain: Array<Set<string>>): Array<string | null> {
  const out = new Set<string | null>([null])
  for (const r of RULES) {
    if (r.media === null || !matchesChain(r.selector, chain)) continue
    const probe: BoxDecls = {}
    applyBlock(probe, r.body)
    if (Object.keys(probe).length > 0) out.add(r.media)
  }
  return [...out]
}

/** A length in px under `zoom`. `remScales` is the open Chromium question: whether a
 *  `rem` inside a zoomed subtree carries the element's zoom or the unzoomed root's.
 *  Returns null when the value is not a plain length this arithmetic can resolve — the
 *  callers treat that as a LOUD failure, never as a pass. */
function lengthPx(value: string | undefined, zoom: number, remScales: boolean): number | null {
  if (value == null) return null
  const v = value.trim()
  if (/^0$/.test(v)) return 0
  let m = /^(-?\d*\.?\d+)px$/.exec(v)
  if (m) return parseFloat(m[1]) * zoom
  m = /^(-?\d*\.?\d+)rem$/.exec(v)
  if (m) return parseFloat(m[1]) * ROOT_FONT_PX * (remScales ? zoom : 1)
  return null
}

interface Unresolved {
  what: string
  value: string
}

/** Resolved border-box height of one strip item (a chip, the MEM label, ＋ or ≡). */
function itemHeightPx(
  cls: string,
  stripChain: Array<Set<string>>,
  media: string | null,
  zoom: number,
  remScales: boolean,
  bad: Unresolved[],
): number {
  const chain = [...stripChain, new Set([cls])]
  const box = computeBox(chain, media)

  const fontPx = lengthPx(box.fontSize, zoom, remScales)
  if (fontPx === null) {
    bad.push({ what: `.${cls} font-size`, value: box.fontSize ?? '(none)' })
    return 0
  }

  // `line-height: normal` (nothing declared, and nothing in the app declares one for
  // these) → the conservative low end of the system-ui range.
  let linePx: number
  if (box.lineHeight == null || box.lineHeight === 'normal') {
    linePx = fontPx * NORMAL_LINE_HEIGHT
  } else if (/^\d*\.?\d+$/.test(box.lineHeight)) {
    linePx = fontPx * parseFloat(box.lineHeight)
  } else {
    const abs = lengthPx(box.lineHeight, zoom, remScales)
    if (abs === null) {
      bad.push({ what: `.${cls} line-height`, value: box.lineHeight })
      return 0
    }
    linePx = abs
  }

  let extra = 0
  for (const [what, value] of [
    ['padding-top', box.padTop],
    ['padding-bottom', box.padBottom],
    ['border-top-width', box.borderTop],
    ['border-bottom-width', box.borderBottom],
  ] as Array<[string, string | undefined]>) {
    if (value == null) continue // absent ⇒ 0, which is what the UA computes
    const px = lengthPx(value, zoom, remScales)
    if (px === null) bad.push({ what: `.${cls} ${what}`, value })
    else extra += px
  }
  return linePx + extra
}

/** The horizontal scrollbar the strip's own `overflow-x` reserves, as this app styles it. */
function scrollbarPx(
  stripChain: Array<Set<string>>,
  media: string | null,
  zoom: number,
  bad: Unresolved[],
): number {
  const PSEUDO = '::-webkit-scrollbar'
  let win: { value: string; selector: string; spec: number; order: number } | null = null
  for (const r of RULES) {
    if (r.media !== null && r.media !== media) continue
    if (!r.selector.endsWith(PSEUDO)) continue
    const host = r.selector.slice(0, -PSEUDO.length).trim()
    // A bare `::-webkit-scrollbar` is the universal rule and reaches every scroller.
    if (host && !matchesChain(host, stripChain)) continue
    let height: string | null = null
    for (const [prop, value] of declarations(r.body)) if (prop === 'height') height = value
    if (height === null) continue
    const spec = specificity(r.selector)
    if (!win || spec > win.spec || (spec === win.spec && r.order >= win.order)) {
      win = { value: height, selector: r.selector, spec, order: r.order }
    }
  }
  if (!win) {
    bad.push({
      what: '::-webkit-scrollbar height for .mem-strip',
      value:
        '(no rule) — the arithmetic lost its scrollbar term. Chromium\'s unstyled scrollbar is ' +
        'LARGER than the 10px this app styles, so the strip needs more room, not less.',
    })
    return 0
  }
  const px = lengthPx(win.value, zoom, true)
  if (px === null) {
    bad.push({ what: `${win.selector} height`, value: win.value })
    return 0
  }
  return px
}

/** The classes MemoryStrip renders inside the strip — every box angle 2 measures.
 *  Angle 5 checks this list against what the component actually emits, DESCENDANTS
 *  included: `.mem-strip-more` (the over-cap favorite count) rides inside the ≡ button,
 *  so it is not a child of the strip, but it is still a box that can set the row's
 *  height and so still has to be measured. */
const ITEM_CLASSES = [
  'mem-strip-label',
  'mem-strip-save',
  'mem-chip',
  'mem-strip-manage',
  'mem-strip-more',
] as const

/** State modifiers — they restyle an item's box rather than adding one, so they are not
 *  items in their own right. Anything else the strip emits must be in ITEM_CLASSES. */
const MODIFIERS = new Set(['active'])

/** App mounts every workspace under `.app` → `.shell` (App.tsx ~2336); the cockpit shell
 *  chain is the one cockpit-header-crush.test.tsx computes against. */
const ROOT = [new Set(['app']), new Set(['shell'])]
const headerChain = (cockpit: string) => [
  ...ROOT,
  new Set(['layout', 'single', cockpit]),
  new Set(['cockpit-header']),
  // CockpitHeader wraps its `children` slot in `.ch-mode-extras`; all three hosts pass
  // the strip through that slot.
  new Set(['ch-mode-extras']),
]

/**
 * EVERY component that renders a MemoryStrip, with the ancestor chain the strip mounts
 * in. Registering a host is the point: the chain is what angle 2 computes against.
 */
const HOSTS: Array<{ file: string; what: string; chain: Array<Set<string>> }> = [
  {
    file: 'components/PhoneCockpit.tsx',
    what: 'Phone',
    chain: [...headerChain('phone-cockpit'), new Set(['mem-strip'])],
  },
  {
    file: 'components/CwCockpit.tsx',
    what: 'CW',
    chain: [...headerChain('cw-cockpit'), new Set(['mem-strip'])],
  },
  {
    // Operate nests the strip one deeper, inside the `.cockpit-meta` cluster.
    file: 'components/OperateCockpit.tsx',
    what: 'Operate',
    chain: [
      ...headerChain('operate-cockpit'),
      new Set(['cockpit-meta']),
      new Set(['mem-strip']),
    ],
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

/** The strip's own resolved one-row box: the tallest item it renders, plus the horizontal
 *  scrollbar it reserves. Anything the arithmetic cannot read lands in `bad`. */
function contentPx(
  chain: Array<Set<string>>,
  media: string | null,
  zoom: number,
  remScales: boolean,
  bad: Unresolved[],
): { total: number; tallest: string; item: number; scrollbar: number } {
  let tallest: string = ITEM_CLASSES[0]
  let item = 0
  for (const cls of ITEM_CLASSES) {
    const h = itemHeightPx(cls, chain, media, zoom, remScales, bad)
    if (h > item) {
      item = h
      tallest = cls
    }
  }
  const scrollbar = scrollbarPx(chain, media, zoom, bad)
  return { total: item + scrollbar, tallest, item, scrollbar }
}

describe('the host census is complete (a fourth MemoryStrip host cannot arrive unregistered)', () => {
  it('every component that renders a MemoryStrip is registered with its chain', () => {
    const rendered = [...SRC.entries()]
      .filter(([f, text]) => f !== 'components/MemoryStrip.tsx' && /<MemoryStrip[\s/>]/.test(text))
      .map(([f]) => f)
      .sort()
    expect(
      rendered,
      'A component renders a MemoryStrip that this guard has never computed a ceiling for. ' +
        'Add it to HOSTS with the ancestor chain the strip mounts in — three cockpit headers ' +
        'inherit this box, and a fourth would inherit whatever the ceiling has become.',
    ).toEqual(HOSTS.map((h) => h.file).sort())
  })

  it('nothing hand-rolls a .mem-strip outside the shared component', () => {
    const rogue = [...SRC.entries()]
      .filter(([f, text]) => f !== 'components/MemoryStrip.tsx' && /["'`]mem-strip/.test(text))
      .map(([f]) => f)
    expect(rogue, `${rogue.join(', ')} writes the .mem-strip class directly`).toEqual([])
  })
})

describe('the ceiling yields to the strip’s own content, at every UI zoom', () => {
  for (const host of HOSTS) {
    for (const zoom of ZOOMS) {
      it(`${host.what} at ${Math.round(zoom * 100)}% zoom: the resolved box fits under the resolved ceiling`, () => {
        for (const media of mediaContexts(host.chain)) {
          for (const remScales of [true, false]) {
            const bad: Unresolved[] = []
            const content = contentPx(host.chain, media, zoom, remScales, bad)
            const box = computeBox(host.chain, media)

            expect(
              bad,
              'This guard could not resolve part of the strip’s box from the sheet:\n' +
                bad.map((b) => `  ${b.what}: ${b.value}`).join('\n') +
                '\nIt fails rather than passes: an unreadable box is an unverified one.',
            ).toEqual([])

            for (const [prop, raw] of [
              ['max-height', box.maxHeight],
              ['height', box.height],
            ] as Array<[string, string | undefined]>) {
              if (raw == null || raw === 'none' || raw === 'auto') continue
              const ceiling = lengthPx(raw, zoom, remScales)
              expect(
                ceiling,
                `.mem-strip resolves \`${prop}: ${raw}\` — a ceiling this guard cannot evaluate, ` +
                  'so it cannot certify that a chip survives it.',
              ).not.toBeNull()
              expect(
                ceiling! >= content.total,
                `${host.what}${media ? ` (within \`${media}\`)` : ''} at ${Math.round(zoom * 100)}% ` +
                  `zoom, rem ${remScales ? 'scaling' : 'not scaling'} with zoom: .mem-strip caps ` +
                  `itself at \`${prop}: ${raw}\` = ${ceiling!.toFixed(2)}px, but its own single row ` +
                  `resolves to ${content.total.toFixed(2)}px (.${content.tallest} = ` +
                  `${content.item.toFixed(2)}px + ${content.scrollbar.toFixed(2)}px of horizontal ` +
                  `scrollbar). The overflow on that axis is \`${box.overflowY ?? 'visible'}\`, so the ` +
                  `missing ${(content.total - ceiling!).toFixed(2)}px is CLIPPED with no way to ` +
                  'reach it. A device-px ceiling over em/rem content is wrong in kind, not by a ' +
                  'few px — the two sides scale on different inputs (root font size, theme border ' +
                  'width, zoom), so no constant makes the relationship hold. Let the ceiling yield ' +
                  'to the content: `flex-wrap: nowrap` already bounds this strip to ONE row however ' +
                  'many favorites exist, which is the whole of what the cap was written for.',
              ).toBe(true)
            }
          }
        }
      })
    }
  }
})

describe('nothing may re-pin the ceiling through a selector shape the chain walk cannot read', () => {
  // Angle 2 walks a chain, so it treats a selector with an attribute or a pseudo as
  // non-matching — fail-OPEN for exactly the shapes a responsive rule would use
  // (`[data-viewport='sm'] .mem-strip { max-height: … }`). This angle reads every rule
  // whose SUBJECT is the strip, whatever the ancestor part looks like.
  it('no rule in either sheet caps .mem-strip below its own resolved row', () => {
    const offenders: string[] = []
    for (const r of rulesOn('mem-strip')) {
      const box: BoxDecls = {}
      applyBlock(box, r.body)
      for (const [prop, raw] of [
        ['max-height', box.maxHeight],
        ['height', box.height],
      ] as Array<[string, string | undefined]>) {
        if (raw == null || raw === 'none' || raw === 'auto') continue
        for (const zoom of ZOOMS) {
          for (const remScales of [true, false]) {
            const bad: Unresolved[] = []
            // The content is the same wherever the strip mounts; Phone's chain stands in.
            const content = contentPx(HOSTS[0].chain, r.media, zoom, remScales, bad)
            const ceiling = lengthPx(raw, zoom, remScales)
            if (ceiling === null || ceiling < content.total) {
              offenders.push(
                `${r.sheet}: ${r.selector} { ${prop}: ${raw} } → ` +
                  `${ceiling === null ? 'unevaluatable' : `${ceiling.toFixed(2)}px`} at ` +
                  `${Math.round(zoom * 100)}% zoom vs a ${content.total.toFixed(2)}px row`,
              )
            }
          }
        }
      }
    }
    expect(
      [...new Set(offenders)],
      `A rule caps the memory strip below the row it renders:\n${[...new Set(offenders)].join('\n')}\n` +
        'Three cockpit headers inherit this box at once, and its vertical axis is clipped — a ' +
        'chip that does not fit is simply gone. The strip is one row by construction ' +
        '(`flex-wrap: nowrap`); it does not need a cap, and a px one only ever clips.',
    ).toEqual([])
  })
})

describe('the horizontal scroll is still the strip’s overflow behavior', () => {
  for (const host of HOSTS) {
    it(`${host.what}: .mem-strip resolves overflow-x to a scrolling value`, () => {
      for (const media of mediaContexts(host.chain)) {
        const box = computeBox(host.chain, media)
        expect(
          box.overflowX === 'auto' || box.overflowX === 'scroll',
          `${host.what}${media ? ` (within \`${media}\`)` : ''}: .mem-strip resolves ` +
            `\`overflow-x: ${box.overflowX ?? 'visible'}\`. Scrolling the surplus chips sideways ` +
            'is the CORRECT behavior for a chip strip and is what bounds the header horizontally ' +
            'with the favorite count — removing the vertical clip must not have taken it with it.',
        ).toBe(true)
      }
    })
  }
})

describe('the chips are what the strip scrolls (they do not shrink to nothing)', () => {
  // Angle 4 proves the strip still SAYS `overflow-x: auto`. This angle proves there is
  // ever anything to scroll. `.mem-chip` clips itself for its ellipsis, which makes it a
  // scroll container, which (Flexbox §4.5) resolves its automatic minimum size to 0 —
  // so with the initial `flex-shrink: 1` the chips give up all their width, the row
  // always fits, and the scroll is dead. The ceiling arithmetic above cannot see this:
  // it is the horizontal axis and it is a FLEX behavior, not a declared size.
  for (const host of HOSTS) {
    it(`${host.what}: .mem-chip resolves a shrink factor of 0`, () => {
      const chipChain = [...host.chain, new Set(['mem-chip'])]
      for (const media of mediaContexts(chipChain)) {
        const box = computeBox(chipChain, media)
        const where = `${host.what}${media ? ` (within \`${media}\`)` : ''}`
        const why =
          `${where}: .mem-chip resolves \`flex-shrink: ${box.flexShrink ?? '1 (initial)'}\`. ` +
          'The chip clips itself (`overflow: hidden`) for its ellipsis, so per CSS ' +
          'Flexbox §4.5 its automatic minimum size is 0 and a non-zero shrink factor ' +
          'lets it collapse without limit: every name ellipses down to a few characters, ' +
          'the row always fits, and the strip’s `overflow-x: auto` never has anything to ' +
          'scroll. `MEM`, `＋` and `≡` are not scroll containers, keep their min-content ' +
          'floor and stay readable — which is why only the chips look truncated. Give the ' +
          'chip `flex: 0 0 auto`; its `max-width` still bounds a pathological name.'
        expect(box.flexShrink, why).toBeDefined()
        expect(parseFloat(box.flexShrink!), why).toBe(0)
      }
    })
  }
})

describe('the measured boxes are the rendered ones', () => {
  afterEach(cleanup)

  it('MemoryStrip emits every item class the arithmetic measures, and no other', () => {
    // Favorites have to exist for a chip to render at all — saved the same way the
    // component's own "＋" saves, so the fixture cannot drift from the real shape. One
    // PAST the strip's cap, so the over-cap count on ≡ renders too: every item class the
    // component can emit has to be present for this census to see it.
    for (let i = 0; i <= STRIP_FAVORITE_LIMIT; i++) {
      memoriesStore.update(
        (b) => saveFavoriteFromDial(b, { rxMhz: 146 + i * 0.01, mode: 'FM', kind: 'simplex' }).bank,
      )
    }
    const { container } = render(
      <MemoryStrip dialMhz={146.52} mode="FM" onRecall={() => {}} onManage={() => {}} />,
    )
    const strip = container.querySelector('.mem-strip')
    expect(strip, 'MemoryStrip rendered no .mem-strip — the box this whole guard is about.').not.toBeNull()

    const emitted = new Set<string>()
    // Descendants, not just children: a box nested inside an item still contributes to
    // the row height the ceiling has to clear.
    for (const el of Array.from(strip!.querySelectorAll('*'))) {
      for (const cls of Array.from(el.classList)) {
        if (!MODIFIERS.has(cls)) emitted.add(cls)
      }
    }
    expect(
      [...emitted].sort(),
      'The strip renders an item class the height arithmetic does not measure (or stopped ' +
        'rendering one it does). The tallest item is what sets the row height, so an unmeasured ' +
        'item is an unguarded one — add it to ITEM_CLASSES.',
    ).toEqual([...ITEM_CLASSES].sort())
  })
})
