import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Guards the responsive VOCABULARY (2026-07-30 layout assessment, census
// responsive-vocab F2/F3/F6/F11 + verified V13):
//
//   1. UI zoom is `zoom: var(--ui-zoom)` on the non-root `.app`, so a raw
//      `@media (max-width: …)` measures the UNZOOMED window. It fired at the
//      900-px Tauri minimum (effective width there: a comfortable 1385 px) and
//      could NEVER fire for a pinned-zoom operator — the accessibility path.
//      The only legitimate signal is `[data-viewport]` on <html> (effective
//      width) plus `--vh-eff`/`--vw-eff` for lengths.
//   2. `classifyViewport` emits exactly xs|sm|md|lg|xl. Rules keyed on any other
//      value (`narrow`, `phone`) shipped dead at birth — and the test meant to
//      guard one of them was green BECAUSE the selector was dead.
//   3. Raw vh/vw lengths inside `.app` are wrong by the zoom factor (Blink bakes
//      zoom into viewport units). The one permanent exception: PORTALED surfaces
//      (.ui-dialog, .ui-tooltip render into document.body, OUTSIDE `.app`). The
//      portal-zoom fix zooms their CONTENT and deliberately leaves the BOX
//      unzoomed, so their raw vh/vw stay correct — this allowlist is the end
//      state, not a stopgap. Converting `.ui-dialog`'s 12vh/84vh to --vh-eff
//      would mis-position the dialog: --vh-eff is pre-divided for in-`.app` use.
//   4. A converted rule must not out-cascade what its @media body used to lose
//      to: `@media (…) { .mode-btn { } }` was (0,1,0); `[data-viewport='sm']
//      .mode-btn` is (0,2,0) and started beating `.mode-btn.sub`. Attribute keys
//      that stand in for a media CONDITION go in `:where()` (zero specificity).
//   5. Nothing keyed on [data-viewport] may declare a value that JS publishes as
//      an INLINE style on the same element (<html>) — inline always wins.
//
// Parser reused from cockpit-shells.test.ts (brace/comment-aware, @media-aware):
// a regex over raw text would read prose comments as declarations.

// BOTH sheets: cockpit-panes.css is a separate structural file (imported after
// styles.css) and must obey the same vocabulary — a raw vh/vw or size @media there
// would be exactly as zoom-blind, and until fix-round 2026-07-31 nothing scanned it.
const raw =
  readFileSync(fileURLToPath(new URL('./styles.css', import.meta.url)), 'utf8') +
  '\n' +
  readFileSync(fileURLToPath(new URL('./cockpit-panes.css', import.meta.url)), 'utf8')
const css = raw.replace(/\/\*[\s\S]*?\*\//g, '')

interface Rule {
  selector: string // one selector (lists are split on top-level commas)
  body: string
  media: string | null
}

/** Split a selector list on TOP-LEVEL commas only: `:where(a, b) .c` is ONE
 *  selector, and a naive split cut it in half (and halved the specificity the
 *  cascade test computes). */
function splitSelectorList(sel: string): string[] {
  const out: string[] = []
  let depth = 0
  let start = 0
  for (let k = 0; k < sel.length; k++) {
    const c = sel[k]
    if (c === '(') depth++
    else if (c === ')') depth--
    else if (c === ',' && depth === 0) {
      out.push(sel.slice(start, k))
      start = k + 1
    }
  }
  out.push(sel.slice(start))
  return out
}

/** Specificity of the selector shapes this sheet uses (classes, attributes,
 *  pseudo-classes, type selectors; `:where()` contributes zero by definition).
 *  Returned as a comparable number — no ID selector in this sheet gets near
 *  the 100s, and the sheet has no `!important` on the rules compared here. */
function specificity(sel: string): number {
  const s = sel.replace(/:where\([^()]*\)/g, ' ')
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const classes =
    (s.match(/\.[\w-]+/g) ?? []).length +
    (s.match(/\[[^\]]*\]/g) ?? []).length +
    (s.match(/(?<!:):[\w-]+(?:\([^()]*\))?/g) ?? []).length
  const types = (s.match(/(?:^|[\s>+~])([a-z][\w-]*)/g) ?? []).length
  return ids * 10000 + classes * 100 + types
}

/** Brace-aware rule walk. @media/@supports nest (rules inside carry the
 *  condition); other at-rule bodies (@keyframes etc.) are skipped wholesale. */
function parseRules(sheet: string): Rule[] {
  const out: Rule[] = []
  let i = 0
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
          for (const s of splitSelectorList(sel)) {
            const one = s.trim().replace(/\s+/g, ' ')
            if (one) out.push({ selector: one, body, media })
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

const RULES = parseRules(css)

describe('responsive vocabulary', () => {
  it("every [data-viewport=…] value is one classifyViewport can emit (xs|sm|md|lg|xl)", () => {
    // useViewport.ts sets exactly these five (mirrored by the index.html preseed).
    // Anything else matches no DOM ever — the rule is dead the day it ships.
    const LIVE = new Set(['xs', 'sm', 'md', 'lg', 'xl'])
    const offenders: string[] = []
    for (const r of RULES) {
      const re = /\[data-viewport=(?:'([^']*)'|"([^"]*)"|([^\]'"]+))\]/g
      let m: RegExpExecArray | null
      while ((m = re.exec(r.selector)) !== null) {
        const v = m[1] ?? m[2] ?? m[3]
        if (!LIVE.has(v)) offenders.push(`${r.selector}  (value '${v}')`)
      }
    }
    expect(
      offenders,
      `dead data-viewport vocabulary (classifyViewport never emits these):\n${offenders.join('\n')}`,
    ).toEqual([])
  })

  it('no size-based @media — the only legitimate conditions are motion preferences', () => {
    // A width/height @media measures the unzoomed window: it mis-fires at the
    // 900-px Tauri minimum and is dead for pinned-zoom operators. Narrowness is
    // [data-viewport]'s job. (Scanned over the raw text too, so a media block
    // containing only @keyframes cannot slip past the rule walk.)
    const offenders: string[] = []
    const re = /@media\s*([^{]+)\{/g
    let m: RegExpExecArray | null
    while ((m = re.exec(css)) !== null) {
      const cond = m[1].trim()
      if (!/prefers-reduced-motion/.test(cond)) offenders.push(`@media ${cond}`)
    }
    expect(
      offenders,
      `size-based @media are zoom-blind — key on [data-viewport] instead:\n${offenders.join('\n')}`,
    ).toEqual([])
  })

  it('no raw vh/vw lengths outside the portaled surfaces (use --vh-eff/--vw-eff)', () => {
    // Inside `.app` a raw viewport unit resolves against the UNZOOMED window, so
    // every length is wrong by the zoom factor. The zoom-corrected idiom is
    // calc(N * var(--vh-eff, 100vh)) — the raw unit is permitted ONLY as the
    // var() fallback (first paint, before the index.html preseed value exists).
    // Portal allowlist: .ui-dialog/.ui-tooltip render into document.body,
    // OUTSIDE `.app`, and the portal-zoom fix zooms their CONTENT while leaving
    // the BOX unzoomed — so raw vh/vw are correct there permanently. Do NOT
    // convert those to --vh-eff: it is pre-divided by the zoom for in-`.app`
    // use, and the dialog would sit at the wrong offset at every zoom level.
    const PORTALED = /^\.ui-(dialog|tooltip)/
    const offenders: string[] = []
    for (const r of RULES) {
      if (PORTALED.test(r.selector)) continue
      const scrubbed = r.body
        .replace(/var\(\s*--vh-eff\s*,\s*100vh\s*\)/g, 'var(--vh-eff)')
        .replace(/var\(\s*--vw-eff\s*,\s*100vw\s*\)/g, 'var(--vw-eff)')
      const re = /\b\d[\d.]*v[hw]\b/g
      let m: RegExpExecArray | null
      while ((m = re.exec(scrubbed)) !== null) {
        offenders.push(`${r.selector} { … ${m[0]} … }`)
      }
    }
    expect(
      offenders,
      `raw viewport units are zoom-blind inside .app — use var(--vh-eff/--vw-eff):\n${offenders.join('\n')}`,
    ).toEqual([])
  })

  it('a [data-viewport] rule never declares a var that JS publishes INLINE on <html>', () => {
    // usePaneWidths publishes --left-rail-w/--right-rail-w as inline styles on
    // <html> on every mount, and index.html's preseed writes them inline before
    // first paint in EVERY window (stored value, or the proportional default when
    // nothing is stored). An inline declaration beats a stylesheet rule on the
    // same element, and the [data-viewport] attribute such a rule keys on is set
    // by those very same code paths — so the rule can never apply. A
    // `:root[data-viewport='xl']` rail-default block shipped exactly like this and
    // was dead on arrival; the ultrawide default actually comes from the hook.
    // (Rules that declare these on a DESCENDANT — `.layout[data-three-pane]` —
    // are live: a declaration on the element itself beats an inherited one.)
    const INLINE_PUBLISHED = /--(left|right)-rail-w\s*:/
    const offenders = RULES.filter(
      (r) => /^(:root|html)\S*\[data-viewport/.test(r.selector) && INLINE_PUBLISHED.test(r.body),
    ).map((r) => r.selector)
    expect(
      offenders,
      `these declare rail widths on <html>, where usePaneWidths' inline vars always win:\n${offenders.join('\n')}`,
    ).toEqual([])
  })

  it('the narrow collapse does not out-cascade the compact .mode-btn.sub variant', () => {
    // Conversion cascade fidelity. `@media (max-width:900px) { .mode-btn {…} }` was
    // (0,1,0) and LOST to `.mode-btn.sub` (0,2,0), so the Digital sub-mode buttons
    // kept min-height:0 / 6px·space-1 padding / 3px gap in the horizontal nav strip.
    // Rewritten as `[data-viewport='sm'] .mode-btn` the rule is (0,2,0) and, being
    // later in the file, started winning — refattening every sub button to a 44px
    // touch target and a wider gap, in the one layout where width is scarcest (and
    // which now only a pinned-zoom operator ever sees). `:where()` restores (0,1,0).
    const sub = RULES.find((r) => r.selector === '.mode-btn.sub')
    expect(sub, '.mode-btn.sub rule must exist').toBeTruthy()
    const narrow = RULES.filter(
      (r) => /\[data-viewport=/.test(r.selector) && /(^|\s)\.mode-btn$/.test(r.selector),
    )
    expect(narrow.length, 'the narrow .mode-btn collapse must exist').toBeGreaterThan(0)
    const offenders = narrow
      .filter((r) => specificity(r.selector) >= specificity(sub!.selector))
      .map((r) => `${r.selector}  (${specificity(r.selector)} ≥ .mode-btn.sub ${specificity(sub!.selector)})`)
    expect(
      offenders,
      `these beat .mode-btn.sub and undo its compact sizing — wrap the viewport key in :where():\n${offenders.join('\n')}`,
    ).toEqual([])
  })
})
