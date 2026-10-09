// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { renderHook } from '@testing-library/react'
import { readFileSync, readdirSync } from 'node:fs'
import { resolve, join } from 'node:path'
import { fitScale, fieldFitScale, naturalFor } from './useScale'
import { useTextSize } from './useTextSize'
import { useDensity } from './useDensity'
import { useTheme } from './useTheme'
import { useNight } from './useNight'
import { useSkin } from './useSkin'
import { PALETTE_ROLES, attrValueOf } from './features/paletteRoles'
import { SKINS } from './features/skins'
import { usePaneWidths } from './usePaneWidths'

// THE BUDGET (2026-10-09). The slowest case here, "seeds exactly the widths the hook publishes, for every…", takes
// 0.28 s and 0.25 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

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

  it('stored rail widths still replay clamped against THIS window', () => {
    setWin(1366, 768) // fit 85 → ew ≈ 1607; 60% ceiling ≈ 964
    localStorage.setItem('tempo-right-rail-w', '2064') // legal on 3440, not here
    runPreseed()
    // Its own ceiling is 964, but the pair shares ⌊1607 − 520⌋ = 1087 (the conversation keeps its
    // 360): the stored rail keeps what it can, the stations rail its 220 floor.
    expect(railVar('--right-rail-w')).toBe('867px')
    expect(railVar('--left-rail-w')).toBe('220px')
    // Storage is never rewritten by the seed — the big-monitor preference survives.
    expect(localStorage.getItem('tempo-right-rail-w')).toBe('2064')
  })

  it('a stored PAIR from a wider window is seeded as a pair: the conversation is never squeezed (L1-1)', () => {
    setWin(1366, 768)
    localStorage.setItem('tempo-left-rail-w', '900')
    localStorage.setItem('tempo-right-rail-w', '1400')
    runPreseed()
    // Alone, 643 + 964 = the whole window. As a pair, in proportion into 1087.
    expect([railVar('--left-rail-w'), railVar('--right-rail-w')]).toEqual(['434px', '652px'])
    localStorage.setItem('tempo-rail-last', 'right')
    localStorage.setItem('tempo-left-rail-w', '700')
    localStorage.setItem('tempo-right-rail-w', '699')
    runPreseed()
    // The rail set last keeps its width; the other gives way.
    expect([railVar('--left-rail-w'), railVar('--right-rail-w')]).toEqual(['388px', '699px'])
  })
})

// The rails are seeded by a COPY of usePaneWidths' fit (the seed runs before any module loads),
// so the copy is held to the hook itself: for every window shape and every stored state —
// nothing, one rail, a pair that fits, a pair that does not with and without a last-set rail,
// junk — the widths the seed paints are the widths the hook then publishes. Any drift is a
// first-paint jump, and a pair squeezed on frame one that the hook then fixes is still a flash.
describe('index.html preseed: the Tempo rails, in lockstep with usePaneWidths', () => {
  const WINDOWS: [number, number][] = [[1024, 768], [1280, 800], [1366, 768], [1600, 900], [1920, 1080], [2560, 1440], [3440, 1440]]
  const STORED: Array<Record<string, string>> = [
    {},
    { 'tempo-left-rail-w': '300' },
    { 'tempo-right-rail-w': '500' },
    { 'tempo-left-rail-w': '250', 'tempo-right-rail-w': '280' },
    { 'tempo-left-rail-w': '900', 'tempo-right-rail-w': '1400' },
    { 'tempo-left-rail-w': '900', 'tempo-right-rail-w': '1400', 'tempo-rail-last': 'left' },
    { 'tempo-left-rail-w': '900', 'tempo-right-rail-w': '1400', 'tempo-rail-last': 'right' },
    { 'tempo-left-rail-w': '5000', 'tempo-right-rail-w': '5000', 'tempo-rail-last': 'left' },
    { 'tempo-left-rail-w': '314.003', 'tempo-right-rail-w': '700.5', 'tempo-rail-last': 'right' },
    { 'tempo-rail-last': 'right' },
    { 'tempo-left-rail-w': 'junk', 'tempo-right-rail-w': '-5', 'tempo-rail-last': 'sideways' },
  ]

  it('seeds exactly the widths the hook publishes, for every window and every stored state', () => {
    const bad: string[] = []
    for (const [w, h] of WINDOWS) {
      for (const stored of STORED) {
        const seed = () => {
          localStorage.clear()
          for (const [k, v] of Object.entries(stored)) localStorage.setItem(k, v)
          document.documentElement.removeAttribute('style')
          window.history.replaceState(null, '', '/')
          setWin(w, h)
        }
        seed()
        runPreseed()
        const seeded = [railVar('--left-rail-w'), railVar('--right-rail-w')]
        const zoom = document.documentElement.style.getPropertyValue('--ui-zoom')
        seed()
        document.documentElement.style.setProperty('--ui-zoom', zoom) // what the seed left for React
        const hook = renderHook(() => usePaneWidths())
        const published = [railVar('--left-rail-w'), railVar('--right-rail-w')]
        hook.unmount()
        if (JSON.stringify(seeded) !== JSON.stringify(published))
          bad.push(`${w}×${h} ${JSON.stringify(stored)}: seed ${seeded} ≠ hook ${published}`)
      }
    }
    expect(bad).toEqual([])
  })

  it('the parity above can fail: a seed that ignored the pair would not match (the positive control)', () => {
    // The squeezed pair is the case that separates a pair-aware seed from the old per-rail one:
    // per rail it would be 643 / 964 here, and the hook publishes 434 / 652.
    localStorage.setItem('tempo-left-rail-w', '900')
    localStorage.setItem('tempo-right-rail-w', '1400')
    setWin(1366, 768)
    runPreseed()
    expect([railVar('--left-rail-w'), railVar('--right-rail-w')]).not.toEqual(['643px', '964px'])
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

  it('high contrast alone seeds the attribute and NOT the bump', () => {
    // #215's whole point, at first paint: the standing preference reaches the tokens and
    // leaves the zoom alone. This is the test that tells the split apart from a second name
    // for field mode — an OR that also fed the zoom seed passes the attribute half of the
    // case above and fails the `fitScale` assertion here.
    for (const [w, h] of [[1024, 768], [1366, 768], [1920, 1080]] as const) {
      localStorage.clear()
      localStorage.setItem('nexus-high-contrast', '1')
      document.documentElement.removeAttribute('style')
      document.documentElement.removeAttribute('data-contrast')
      window.history.replaceState(null, '', '/')
      setWin(w, h)
      runPreseed()
      expect(document.documentElement.getAttribute('data-contrast'), `${w}x${h}`).toBe('high')
      expect(
        document.documentElement.style.getPropertyValue('--ui-zoom'),
        `${w}x${h}: high contrast moved the zoom — that is field mode's half, not this one`,
      ).toBe(String(fitScale(w, h) / 100))
      // The positive control for that negative result: at this window the two fits really
      // do differ, so "unchanged" is a fact about the seed and not about the arithmetic.
      expect(fieldFitScale(w, h), `${w}x${h}: the two fits agree — this case proves nothing`)
        .not.toBe(fitScale(w, h))
    }
  })

  it('field mode still bumps the zoom when BOTH are set', () => {
    // The mirror: an implementation that keyed the zoom on the new preference instead of on
    // field mode would pass every other case in this file.
    localStorage.clear()
    localStorage.setItem('nexus-field-mode', '1')
    localStorage.setItem('nexus-high-contrast', '1')
    document.documentElement.removeAttribute('style')
    document.documentElement.removeAttribute('data-contrast')
    window.history.replaceState(null, '', '/')
    setWin(1366, 768)
    runPreseed()
    expect(document.documentElement.getAttribute('data-contrast')).toBe('high')
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe(
      String(fieldFitScale(1366, 768) / 100),
    )
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

describe('index.html preseed: text size and density (#215)', () => {
  // Text size moves every font on screen, so a seed that disagreed with the hooks would
  // first-paint the whole app at one size and then jump to another. Held by PARITY rather
  // than by constants: for every stored value, including none and nonsense, the seed must
  // leave exactly the attributes useTextSize and useDensity then write.
  const ATTRS = ['data-text-size', 'data-density', 'data-touch'] as const
  const root = document.documentElement
  const read = () => Object.fromEntries(ATTRS.map((a) => [a, root.getAttribute(a)]))
  const clear = () => ATTRS.forEach((a) => root.removeAttribute(a))

  it('seeds exactly the attributes the hooks write, for every stored value', () => {
    const TEXT = [null, 'normal', 'large', 'larger', 'huge']
    const DENSITY = [null, 'guided', 'standard', 'dense', 'touch', 'jumbo']
    const bad: string[] = []
    for (const t of TEXT) {
      for (const den of DENSITY) {
        localStorage.clear()
        if (t !== null) localStorage.setItem('nexus-text-size', t)
        if (den !== null) localStorage.setItem('nexus-density', den)
        clear()
        runPreseed()
        const seeded = read()
        clear()
        const hooks = renderHook(() => {
          useTextSize()
          useDensity()
        })
        const written = read()
        hooks.unmount()
        if (JSON.stringify(seeded) !== JSON.stringify(written))
          bad.push(`text=${t} density=${den}: seed ${JSON.stringify(seeded)} ≠ hooks ${JSON.stringify(written)}`)
      }
    }
    expect(bad).toEqual([])
  })

  it('Larger + Touch seeds the attributes the sheet keys on (the case the parity loop could share a bug with)', () => {
    // Parity alone passes if seed and hooks are wrong the same way; this pins the values.
    localStorage.setItem('nexus-text-size', 'larger')
    localStorage.setItem('nexus-density', 'touch')
    clear()
    runPreseed()
    expect(read()).toEqual({ 'data-text-size': 'larger', 'data-density': 'guided', 'data-touch': '1' })
  })
})

describe('index.html preseed: the theme, System included', () => {
  // System resolves through prefers-color-scheme, in the seed and in useTheme alike. A seed that
  // disagreed would paint the first frame in one theme and snap to the other. The page's own
  // <html data-theme="dark"> is the default when the seed writes nothing, so "what the seed
  // leaves" is the attribute or, absent that, dark.
  const QUERY = '(prefers-color-scheme: dark)'
  const root = document.documentElement
  const os = (setting: 'dark' | 'light' | 'none') => {
    if (setting === 'none') {
      delete (window as { matchMedia?: unknown }).matchMedia
      return
    }
    window.matchMedia = ((q: string) => ({
      matches: q === QUERY && setting === 'dark',
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia
  }
  const painted = () => root.getAttribute('data-theme') ?? 'dark'

  it('seeds the theme useTheme paints, for every stored value and OS setting', () => {
    const bad: string[] = []
    for (const stored of [null, 'light', 'dark', 'system', 'amber', 'sepia']) {
      for (const setting of ['dark', 'light', 'none'] as const) {
        os(setting)
        localStorage.clear()
        if (stored !== null) localStorage.setItem('tempo-theme', stored)
        root.removeAttribute('data-theme')
        runPreseed()
        const seeded = painted()
        localStorage.clear()
        if (stored !== null) localStorage.setItem('tempo-theme', stored)
        root.removeAttribute('data-theme')
        const hook = renderHook(() => useTheme())
        const written = root.getAttribute('data-theme')
        hook.unmount()
        if (seeded !== written) bad.push(`stored=${stored} os=${setting}: seed ${seeded} ≠ hook ${written}`)
      }
    }
    delete (window as { matchMedia?: unknown }).matchMedia
    root.removeAttribute('data-theme')
    expect(bad).toEqual([])
  })

  it('System seeds the OS setting (the case parity could share a bug with), and unset stays Dark', () => {
    os('light')
    localStorage.setItem('tempo-theme', 'system')
    root.removeAttribute('data-theme')
    runPreseed()
    expect(root.getAttribute('data-theme')).toBe('light')
    // New installs start Dark (the operator's pick), even on a computer set to light.
    localStorage.clear()
    root.removeAttribute('data-theme')
    runPreseed()
    expect(painted()).toBe('dark')
    delete (window as { matchMedia?: unknown }).matchMedia
    root.removeAttribute('data-theme')
  })
})

describe('index.html preseed: colour roles', () => {
  // The first-paint copy of usePaletteRoles (Settings ▸ Appearance ▸ Colours): one attribute per
  // role, set only for a preset the table knows and never for the default, which is NO attribute.
  // Without it every launch with a picked colour flashes the stock one first. The seed carries its
  // own copy of the preset ids (it runs before any module loads), so it is compared to the hook's
  // own answer — `attrValueOf` — for every preset of every role, a value no preset has, and none.
  const clearRoles = () => {
    for (const r of PALETTE_ROLES) document.documentElement.removeAttribute(r.attr)
  }
  beforeEach(clearRoles)

  it('seeds exactly the attribute the hook would, role by role and preset by preset', () => {
    const bad: string[] = []
    for (const r of PALETTE_ROLES) {
      for (const stored of [...r.presets.map((p) => p.id), 'no-such-preset', null]) {
        localStorage.clear()
        clearRoles()
        if (stored !== null) localStorage.setItem(r.storage, stored)
        setWin(1366, 768)
        runPreseed()
        const want = attrValueOf(r, stored ?? '')
        const got = document.documentElement.getAttribute(r.attr)
        if (got !== want) bad.push(`${r.id} stored ${JSON.stringify(stored)}: seed ${JSON.stringify(got)}, hook ${JSON.stringify(want)}`)
        for (const other of PALETTE_ROLES) {
          if (other !== r && document.documentElement.getAttribute(other.attr) !== null) {
            bad.push(`${r.id} stored ${JSON.stringify(stored)} also seeded ${other.attr}`)
          }
        }
      }
    }
    expect(bad).toEqual([])
  })

  it('seeds every role at once, and leaves the zoom exactly where it was', () => {
    for (const r of PALETTE_ROLES) localStorage.setItem(r.storage, r.presets[r.presets.length - 1].id)
    setWin(1366, 768)
    runPreseed()
    for (const r of PALETTE_ROLES) {
      expect(document.documentElement.getAttribute(r.attr), r.id).toBe(r.presets[r.presets.length - 1].id)
    }
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe(String(fitScale(1366, 768) / 100))
  })
})

describe('index.html preseed: Night', () => {
  // The first-paint copy of useNight (Settings ▸ Appearance ▸ Workspace ▸ Night). On paints Night
  // from the first frame. Auto needs the station's grid square, which only the running app knows
  // (it arrives with the first snapshot), so the seed leaves Auto to the hook: the page paints
  // with Night off and the hook turns it on the moment the grid arrives, if the sun is down there.
  // Parity is therefore against the hook as it stands before the grid is known.
  const root = document.documentElement
  beforeEach(() => root.removeAttribute('data-night'))

  it('seeds exactly the attribute useNight writes before the grid is known, for every stored value', () => {
    const bad: string[] = []
    for (const stored of [null, 'off', 'on', 'auto', 'dim']) {
      localStorage.clear()
      if (stored !== null) localStorage.setItem('nexus-night', stored)
      root.removeAttribute('data-night')
      setWin(1366, 768)
      runPreseed()
      const seeded = root.getAttribute('data-night')
      root.removeAttribute('data-night')
      const hook = renderHook(() => useNight(''))
      const written = root.getAttribute('data-night')
      hook.unmount()
      if (seeded !== written) bad.push(`stored=${stored}: seed ${JSON.stringify(seeded)} ≠ hook ${JSON.stringify(written)}`)
    }
    root.removeAttribute('data-night')
    expect(bad).toEqual([])
  })

  it('On seeds data-night, Auto and Off do not (the case parity could share a bug with), and the zoom stays put', () => {
    localStorage.setItem('nexus-night', 'on')
    setWin(1366, 768)
    runPreseed()
    expect(root.getAttribute('data-night')).toBe('1')
    expect(document.documentElement.style.getPropertyValue('--ui-zoom')).toBe(String(fitScale(1366, 768) / 100))
    for (const stored of ['auto', 'off']) {
      localStorage.clear()
      localStorage.setItem('nexus-night', stored)
      root.removeAttribute('data-night')
      runPreseed()
      expect(root.getAttribute('data-night'), stored).toBeNull()
    }
  })
})

describe('index.html preseed: the built-in themes', () => {
  // The first-paint copy of useSkin (Settings ▸ Appearance ▸ Theme): `data-skin` for a theme the
  // table knows, and only while the page is that theme's base; the standard themes are NO
  // attribute. Without it every launch on a theme flashes the standard colours first, and a theme
  // seeded on the wrong base would paint that page's wells in its own colours. The seed carries its
  // own copy of the ids (it runs before any module loads), so it is compared to the hook's own
  // answer for every theme, an id no theme has, and none, on every stored theme and OS setting.
  const QUERY = '(prefers-color-scheme: dark)'
  const root = document.documentElement
  const os = (setting: 'dark' | 'light') => {
    window.matchMedia = ((q: string) => ({
      matches: q === QUERY && setting === 'dark',
      media: q,
      addEventListener: () => {},
      removeEventListener: () => {},
    })) as unknown as typeof window.matchMedia
  }
  const reset = () => {
    root.removeAttribute('data-skin')
    root.removeAttribute('data-theme')
  }
  beforeEach(reset)

  it('seeds exactly the attribute useSkin writes, theme by theme, for every stored theme and OS setting', () => {
    const bad: string[] = []
    for (const stored of [...SKINS.map((x) => x.id), 'no-such-theme', null]) {
      for (const theme of [null, 'dark', 'light', 'system']) {
        for (const setting of ['dark', 'light'] as const) {
          const seed = () => {
            localStorage.clear()
            if (stored !== null) localStorage.setItem('nexus-skin', stored)
            if (theme !== null) localStorage.setItem('tempo-theme', theme)
            reset()
          }
          os(setting)
          seed()
          setWin(1366, 768)
          runPreseed()
          const seeded = root.getAttribute('data-skin')
          seed()
          const hook = renderHook(() => {
            const [painted] = useTheme()
            return useSkin(painted)
          })
          const written = root.getAttribute('data-skin')
          hook.unmount()
          if (seeded !== written) bad.push(`skin=${stored} theme=${theme} os=${setting}: seed ${JSON.stringify(seeded)} ≠ hook ${JSON.stringify(written)}`)
        }
      }
    }
    delete (window as { matchMedia?: unknown }).matchMedia
    reset()
    expect(bad).toEqual([])
  })

  it('seeds a theme on its own base and never on the other (the case parity could share a bug with), and the zoom stays put', () => {
    for (const x of SKINS) {
      for (const theme of ['dark', 'light'] as const) {
        localStorage.clear()
        localStorage.setItem('nexus-skin', x.id)
        localStorage.setItem('tempo-theme', theme)
        reset()
        setWin(1366, 768)
        runPreseed()
        expect(root.getAttribute('data-skin'), `${x.id} on ${theme}`).toBe(theme === x.base ? x.id : null)
        expect(root.style.getPropertyValue('--ui-zoom'), `${x.id} on ${theme}`).toBe(String(fitScale(1366, 768) / 100))
      }
    }
    reset()
  })
})
