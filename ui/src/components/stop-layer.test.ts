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
