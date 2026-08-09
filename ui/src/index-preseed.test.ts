// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve, join } from 'node:path'
import { fitScale, fieldFitScale, naturalFor } from './useScale'

// index.html's pre-paint seed script, executed for real: it is the only thing standing
// between launch and a first-paint flash, and it must mirror the React hooks EXACTLY
// (useScale fit model, useViewport --vh/vw-eff, usePaneWidths rail clamps). The rail
// case that motivated this file: with NOTHING stored the seed used to skip the rails
// entirely, so every launch of a never-dragged install first-painted the :root 300px
// fallback and then jumped to usePaneWidths' proportional default (~319px wider at
// 3440) — the first-paint-flash class (census F15) on EVERY launch, not just the first
// (review 2026-07-31).

// (import.meta.url is an http: URL under the jsdom environment — resolve from the
// vitest cwd, which is the ui/ project root.)
const html = readFileSync(resolve(process.cwd(), 'index.html'), 'utf8')
// The preseed is the first PLAIN <script> (the app entry is type="module").
const src = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]

function setWin(w: number, h: number) {
  Object.defineProperty(window, 'innerWidth', { value: w, configurable: true, writable: true })
  Object.defineProperty(window, 'innerHeight', { value: h, configurable: true, writable: true })
}

function runPreseed() {
  new Function(src!)()
}

const railVar = (name: string) => document.documentElement.style.getPropertyValue(name)

beforeEach(() => {
  localStorage.clear()
  window.history.replaceState(null, '', '/')
  document.documentElement.removeAttribute('style')
})

describe('index.html preseed', () => {
  it('has an inline preseed script at all', () => {
    expect(src, 'no plain <script> block found in index.html').toBeTruthy()
  })

  it('never-dragged install: rails seed usePaneWidths’ proportional default pre-paint', () => {
    setWin(3440, 1440) // auto mode, default cap 100 → zoom 1 → effective width 3440
    runPreseed()
    expect(railVar('--vh-eff')).not.toBe('') // sanity: the script ran
    // clampLeft(round(0.18·ew)) / clampRight(round(0.22·ew)) — usePaneWidths' exact
    // default formula. The :root fallbacks are 300/360px; anything else here flashes.
    expect(railVar('--left-rail-w')).toBe('619px')
    expect(railVar('--right-rail-w')).toBe('757px')
  })

  it('never-dragged install: the default seed respects the 220/260 px floors', () => {
    setWin(700, 500) // fit floor 65 → ew ≈ 1077 → raw defaults 194/237, under the floors
    runPreseed()
    expect(railVar('--left-rail-w')).toBe('220px')
    expect(railVar('--right-rail-w')).toBe('260px')
  })

  it('stored rail widths still replay clamped against THIS window (unchanged path)', () => {
    setWin(1366, 768) // fit 85 → ew ≈ 1607; 60% ceiling ≈ 964
    localStorage.setItem('tempo-right-rail-w', '2064') // legal on 3440, not here
    runPreseed()
    expect(railVar('--right-rail-w')).toBe('964px')
    // Storage is never rewritten by the seed — the big-monitor preference survives.
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('2064')
  })
})

// The seed carries its OWN copy of the per-surface natural table (it runs before any
// module loads — that is the point of it). A copy is a drift risk, so it is compared to
// the real fitScale/naturalFor rather than to hand-typed constants: every openable panel
// (scanned from the call sites, so a new pop-out is covered the day it appears) at every
// window shape a pop-out can take. Without the seed mirroring the table, every pop-out
// first-painted at the 65% floor and then jumped — the flash class this file exists for.
const OPENABLE = (() => {
  const dir = resolve(process.cwd(), 'src')
  const found = new Set<string>()
  for (const rel of readdirSync(dir, { recursive: true }) as string[]) {
    if (!/\.tsx?$/.test(rel) || /\.test\.tsx?$/.test(rel)) continue
    for (const m of readFileSync(join(dir, rel), 'utf8').matchAll(/openPanelWindow\(\s*'([A-Za-z0-9]+)'/g))
      found.add(m[1])
  }
  return [...found].sort()
})()

/** open_panel_window's default inner sizes, plus the shapes a dragged/docked pop-out
 *  takes. Parity must hold at every one of them, not just where a window opens. */
const SHAPES: [number, number][] = [
  [420, 780], // band map default
  [420, 360], // band map / generic min_inner_size
  [420, 1400], // band map docked as a full-height edge strip
  [900, 300], // waterfall default
  [380, 180], // waterfall min_inner_size
  [760, 660], // generic pop-out default
  [560, 760], // fieldday default
  [1140, 760], // operate default
  [1920, 1080], // a pop-out dragged to full screen
]

describe('index.html preseed: per-surface natural footprint', () => {
  it('seeds every openable pop-out at exactly the scale useScale would compute', () => {
    expect(OPENABLE.length).toBeGreaterThan(5) // the scan actually found call sites
    const bad: string[] = []
    for (const slug of OPENABLE) {
      for (const [w, h] of SHAPES) {
        localStorage.clear()
        document.documentElement.removeAttribute('style')
        window.history.replaceState(null, '', '/?panel=' + slug)
        setWin(w, h)
        runPreseed()
        const want = String(fitScale(w, h, 100, undefined, naturalFor(slug)) / 100)
        const got = document.documentElement.style.getPropertyValue('--ui-zoom')
        if (got !== want) bad.push(`${slug} @ ${w}×${h}: seed ${got} ≠ fitScale ${want}`)
      }
    }
    expect(bad).toEqual([])
  })

  it('seeds an inherited PIN at the panel’s own ceiling, not the 65 floor', () => {
    // The other half of the same bug, and the half that flashes: the seed caps a
    // pop-out's pin at this window's fit ceiling (mirroring capPinnedScale). Against the
    // cockpit footprint that ceiling was 65, so a pinned operator's band map painted at
    // 65 and then jumped when React published the real scale.
    window.history.replaceState(null, '', '/?panel=bandmapCw')
    localStorage.setItem('nexus-ui-scale-mode', '175') // main's pin, inherited (bare key)
    setWin(420, 780)
    runPreseed()
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe('1.1')
    // A MAIN-window pin still applies verbatim — no ceiling, no change.
    localStorage.clear()
    document.documentElement.removeAttribute('style')
    window.history.replaceState(null, '', '/')
    localStorage.setItem('nexus-ui-scale-mode', '175')
    setWin(900, 600)
    runPreseed()
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe('1.75')
  })

  it('an UNKNOWN panel seeds the generic pop-out natural, not the main cockpit’s', () => {
    window.history.replaceState(null, '', '/?panel=nosuchpanel')
    setWin(760, 660)
    runPreseed()
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe(
      String(fitScale(760, 660, 100, undefined, naturalFor('nosuchpanel')) / 100),
    )
  })

  it('the MAIN window seed is unchanged — no ?panel=, main cockpit footprint', () => {
    for (const [w, h] of [
      [1920, 1080],
      [1366, 768],
      [1200, 720],
      [900, 600],
    ] as const) {
      localStorage.clear()
      document.documentElement.removeAttribute('style')
      window.history.replaceState(null, '', '/')
      setWin(w, h)
      runPreseed()
      expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe(
        String(fitScale(w, h) / 100),
      )
    }
  })
})

describe('index.html preseed: field mode', () => {
  // BOTH halves must seed — the attribute (high-contrast tokens key off it) and the larger
  // fit arithmetic — or every launch in field mode flashes the indoor look, then snaps.
  it('seeds data-contrast AND the field fit, matching fieldFitScale exactly', () => {
    const cases: Array<[number, number]> = [[1024, 768], [1366, 768], [1536, 864], [1920, 1080]]
    for (const [w, h] of cases) {
      localStorage.clear()
      localStorage.setItem('nexus-field-mode', '1')
      document.documentElement.removeAttribute('style')
      document.documentElement.removeAttribute('data-contrast')
      window.history.replaceState(null, '', '/')
      setWin(w, h)
      runPreseed()
      expect(document.documentElement.getAttribute('data-contrast'), `${w}x${h}`).toBe('high')
      const want = String(fieldFitScale(w, h) / 100)
      expect(
        document.documentElement.style.getPropertyValue('--ui-zoom'),
        `${w}x${h}: seed disagrees with fieldFitScale — first-paint flash`,
      ).toBe(want)
    }
  })

  it('field OFF seeds neither the attribute nor the bump', () => {
    localStorage.clear()
    document.documentElement.removeAttribute('style')
    document.documentElement.removeAttribute('data-contrast')
    window.history.replaceState(null, '', '/')
    setWin(1366, 768)
    runPreseed()
    expect(document.documentElement.getAttribute('data-contrast')).toBeNull()
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe(
      String(fitScale(1366, 768) / 100),
    )
  })
})
