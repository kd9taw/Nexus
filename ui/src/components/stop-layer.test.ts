// @vitest-environment jsdom
//
// THE STOP LAYER'S STACKING WINS OVER EVERY OVERLAY'S — computed from the stylesheets, never read off them.
//
// components/StopLayer.tsx mirrors a covered stop control into `.stop-layer`, and the mirror is clickable only because
// that layer is drawn above everything else. The layer is a child of <body> (no ancestor's stacking context caps it),
// so what decides it is z-index alone: the winner of `z-index` for a real `.stop-layer` element must be above the
// winner for every overlay the app draws, and above every z-index the sheets declare anywhere, so an overlay added
// next year at "z-index: 99999" goes red here instead of silently covering Stop TX again. Inline z-indexes in the
// app's source are counted too (the probe pages under src/probe are not the app).
import { describe, it, expect, beforeAll } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { css, loadSheets } from '../cssCascade.testkit'
import { parseRules } from '../cssCascade'

const SRC = resolve(__dirname, '..')
const SHEETS = ['styles.css', 'cockpit-panes.css']

/** A rendered element with these classes (and attributes), in a fresh body, for the cascade to resolve. */
function el(html: string): Element {
  const host = document.createElement('div')
  host.innerHTML = html
  document.body.appendChild(host)
  return host.firstElementChild!
}
const z = (e: Element) => Number(css(e, 'z-index'))

beforeAll(() => loadSheets(SHEETS))

describe('Stop TX above every overlay: the stop layer outranks them all', () => {
  it('the stop layer is fixed, takes no pointer events itself, and has a numeric z-index', () => {
    const layer = el('<div class="stop-layer"></div>')
    expect(css(layer, 'position')).toBe('fixed')
    expect(css(layer, 'pointer-events')).toBe('none')
    expect(Number.isFinite(z(layer))).toBe(true)
  })

  // The overlays that cover Stop TX today, each resolved as the app renders it.
  const OVERLAYS: [string, string][] = [
    ['ui/Dialog overlay', '<div class="ui-dialog-overlay"></div>'],
    ['ui/Dialog box', '<div class="ui-dialog"></div>'],
    ['a menu (ui/Menu, BandMenu, every MenuParts menu)', '<div class="ui-menu band-menu" role="menu"></div>'],
    ['the prompt to log and the Logbook scrims', '<div class="logconfirm-backdrop" role="dialog" aria-modal="true"></div>'],
    ['the launch radio picker', '<div class="radio-picker-overlay" role="dialog" aria-modal="true"></div>'],
    ['the memory packs', '<div class="mv-packs-overlay"></div>'],
  ]
  it.each(OVERLAYS)('wins over %s', (_name, html) => {
    const layer = el('<div class="stop-layer"></div>')
    const over = el(html)
    expect(Number.isFinite(z(over)), 'premise: the overlay has a z-index').toBe(true)
    expect(z(layer)).toBeGreaterThan(z(over))
  })

  it('wins over every z-index the stylesheets declare, and every inline one in the app', () => {
    const layer = z(el('<div class="stop-layer"></div>'))
    const declared: [string, number][] = []
    for (const name of SHEETS) {
      const sheet = readFileSync(join(SRC, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
      for (const r of parseRules(sheet)) {
        if (r.selector.includes('.stop-layer')) continue
        for (const d of r.decls) if (d.prop === 'z-index' && /^-?\d+/.test(d.value)) declared.push([`${name} ${r.selector}`, parseInt(d.value, 10)])
      }
    }
    const walk = (dir: string): string[] =>
      readdirSync(dir).flatMap((n) => {
        const p = join(dir, n)
        if (statSync(p).isDirectory()) return n === 'probe' ? [] : walk(p)
        return /\.tsx?$/.test(n) && !/\.test\.tsx?$/.test(n) ? [p] : []
      })
    for (const p of walk(SRC)) {
      for (const m of readFileSync(p, 'utf8').matchAll(/zIndex:\s*(-?\d+)/g)) declared.push([p.slice(SRC.length + 1), Number(m[1])])
    }
    expect(declared.length, 'premise: the sweep found the sheets').toBeGreaterThan(50)
    const above = declared.filter(([, v]) => v >= layer)
    expect(above, `z-indexes at or above the stop layer's ${layer}`).toEqual([])
  })
})

// A SCRIM MUST BE THE OVERLAY OR ITS ANCESTOR (StopLayer.tsx's header). The layer spares a stop control its hit tests
// when no part of an open overlay meets it — the element with the role or aria-modal, what it holds and what holds it —
// so a full-window scrim drawn BESIDE the dialog it dims would cover Stop TX unseen. Every full-window scrim the
// sheets declare (fixed, inset 0) is found here, and every place the app draws one must be the overlay itself, or
// hold one as its first child, or be a Radix modal dialog's own overlay (which locks the page's pointer events, and
// then every stop is hit-tested).
describe('every full-window scrim is an overlay, or holds one', () => {
  const OVERLAY_ATTR = /\brole=["{]?["']?(dialog|alertdialog|menu)\b|\baria-modal=(["']true["']|\{true\})/
  /** The single-class selectors whose rules draw a fixed box over the whole window. */
  function scrims(): string[] {
    const decls = new Map<string, Map<string, string>>()
    for (const name of SHEETS) {
      const sheet = readFileSync(join(SRC, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '')
      for (const r of parseRules(sheet)) {
        const sel = r.selector.trim()
        if (!/^\.[\w-]+$/.test(sel)) continue
        const m = decls.get(sel) ?? new Map<string, string>()
        for (const d of r.decls) m.set(d.prop, d.value.trim())
        decls.set(sel, m)
      }
    }
    return [...decls]
      .filter(([sel, d]) => sel !== '.stop-layer' && d.get('position') === 'fixed' &&
        (d.get('inset') === '0' || ['top', 'right', 'bottom', 'left'].every((k) => d.get(k) === '0')))
      .map(([sel]) => sel.slice(1))
      .sort()
  }
  /** The JSX opening tag that starts at `at` (a '<'), braces and quotes skipped, '>' at depth 0 ending it. */
  function tagAt(src: string, at: number): string {
    let depth = 0
    let quote = ''
    for (let i = at + 1; i < src.length; i++) {
      const c = src[i]
      if (quote) {
        if (c === quote) quote = ''
      } else if (c === '"' || c === "'" || c === '`') quote = c
      else if (c === '{') depth++
      else if (c === '}') depth--
      else if (c === '>' && depth === 0) return src.slice(at, i + 1)
    }
    return src.slice(at)
  }
  const appFiles = (dir: string): string[] =>
    readdirSync(dir).flatMap((n) => {
      const p = join(dir, n)
      if (statSync(p).isDirectory()) return n === 'probe' ? [] : appFiles(p)
      return /\.tsx$/.test(n) && !/\.test\.tsx$/.test(n) ? [p] : []
    })

  it('the scrims the sheets declare are the known ones (a new one is checked below once added here)', () => {
    expect(scrims()).toEqual(['logconfirm-backdrop', 'mv-packs-overlay', 'radio-picker-overlay', 'roam-modal', 'ui-dialog-overlay'])
  })

  it('every place the app draws a scrim, it is the overlay, holds one as its first child, or is a Radix modal overlay', () => {
    const all = scrims()
    const tokens = all.map((cls) => [cls, new RegExp(`(?<![\\w-])${cls}(?![\\w-])`)] as const)
    const found = new Map<string, number>()
    const bad: string[] = []
    for (const p of appFiles(SRC)) {
      const src = readFileSync(p, 'utf8')
      if (!all.some((cls) => src.includes(cls))) continue
      for (const [cls, token] of tokens) {
        for (const m of src.matchAll(/className=("[^"]*"|\{[^}]*\})/g)) {
          if (!token.test(m[1])) continue
          found.set(cls, (found.get(cls) ?? 0) + 1)
          const start = src.lastIndexOf('<', m.index)
          const tag = tagAt(src, start)
          const where = `${p.slice(SRC.length + 1)}:${src.slice(0, start).split('\n').length} (${cls})`
          if (OVERLAY_ATTR.test(tag)) continue
          // A Radix dialog's Overlay is drawn only for a modal dialog, which locks the page's pointer events.
          if (/^<(RD|Dialog)\.Overlay\b/.test(tag)) {
            if (/modal=\{false\}/.test(src)) bad.push(`${where}: a Radix Overlay in a file with a non-modal dialog`)
            continue
          }
          // The first child: the next JSX opening tag, past whitespace and {/* comments */}.
          const rest = src.slice(start + tag.length).replace(/^(\s|\{\/\*[\s\S]*?\*\/\})*/, '')
          if (rest.startsWith('<') && OVERLAY_ATTR.test(tagAt(rest, 0))) continue
          bad.push(`${where}: neither the overlay nor holding one`)
        }
      }
    }
    expect([...found.keys()].sort(), 'premise: every scrim is drawn somewhere').toEqual(all)
    expect(bad).toEqual([])
  })
})

