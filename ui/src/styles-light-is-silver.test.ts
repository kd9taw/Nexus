// LIGHT IS SILVER, AND THE OLD LIGHT IS A THEME OF ITS OWN (operator, 2026-10-13: "your default light
// mode is far too light, but you have another mode, i think its called silver that is a bit warmer
// white mode, that should classify as the new light mode"; picked "Light becomes Silver, keep old").
//
// So the standard Light, which System also paints on a light computer, takes Silver's surfaces, inks,
// accent family and readout, by day and at night, and its display wells take Silver's accent and readout.
// The bright Light it replaces stays one click away as the Bright theme in the gallery, on the light
// base, painting exactly what Light painted. Silver leaves the gallery: it would be Light twice.
//
// Every value here is the cascade winner (cssCascade.ts), never a regex over the sheet, and every table
// is the value the shipped sheet painted (SILVER under `data-skin='silver'`, OLD_LIGHT with no theme),
// resolved from styles.css as it was before the change. High contrast and the colour-role picks keep
// beating the palette, and the locked colours resolve exactly as before in every mode.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  BASE_MODES,
  baseTheme,
  expandWith,
  parseRules,
  rootTokensFrom,
  tokensAt,
  type El,
  type Mode,
  type Rule,
} from './cssCascade'
import { PALETTE_ROLES, isLockedToken } from './features/paletteRoles'
import { SKINS, SKIN_TOKENS, skinOf } from './features/skins'

const blank = (s: string) => s.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const read = (name: string) => readFileSync(fileURLToPath(new URL(`./${name}`, import.meta.url)), 'utf8')
const RULES = parseRules(blank(read('styles.css')) + '\n' + blank(read('cockpit-panes.css')))

const WELL: El[] = [{ tag: 'div', classes: ['well'], attrs: {} }]
type Scope = 'root' | 'well'

/** Every theme token visible in `scope` under `mode`, var()s expanded. */
function tokens(rules: Rule[], mode: Mode, scope: Scope): Record<string, string> {
  const map = scope === 'well' ? tokensAt(rules, mode, WELL) : rootTokensFrom(rules, mode)
  return Object.fromEntries(SKIN_TOKENS.map((t) => [t, expandWith(map, `var(${t})`).trim().toLowerCase()]))
}

/** One look, as the sheet painted it: at <html> and inside a display well, by day and at night. */
interface Look {
  day: Record<string, string>
  well: Record<string, string>
  night: Record<string, string>
  nightWell: Record<string, string>
}

// A display well declares the dark palette on itself, and a light look gives it only an accent and a
// readout; these are the dark palette's, by day and at night, which every light look's well shares.
const WELL_DAY = {
  '--bg': '#0b0f17', '--panel': '#111722', '--bg-elev': '#131925', '--bg-elev-2': '#1a2231', '--border': '#243044',
  '--border-soft': '#1b2433', '--text': '#e7edf6', '--text-dim': '#9aa8bd', '--text-faint': '#7c8ca3',
  '--bubble-mine': '#1f5d73', '--bubble-mine-text': '#eaf7fc', '--bubble-theirs': '#1a2231', '--bubble-theirs-text': '#e7edf6',
  '--well-bg': '#0b0f17', '--well-ink': '#e7edf6', '--well-grid': '#334155',
}
const WELL_NIGHT = {
  '--bg': '#090604', '--panel': '#0f0b08', '--bg-elev': '#110d0a', '--bg-elev-2': '#191410', '--border': '#302822',
  '--border-soft': '#201b16', '--text': '#bfb4a5', '--text-dim': '#9d9284', '--text-faint': '#8a8073',
  '--bubble-mine': '#124658', '--bubble-mine-text': '#bdc9ce', '--bubble-theirs': '#191410', '--bubble-theirs-text': '#bfb4a5',
  '--well-bg': '#090604', '--well-ink': '#bfb4a5', '--well-grid': '#372f28',
}
const WELL_TRIO_DAY = { '--well-bg': '#0b0f17', '--well-ink': '#e7edf6', '--well-grid': '#334155' }
const WELL_TRIO_NIGHT = { '--well-bg': '#090604', '--well-ink': '#bfb4a5', '--well-grid': '#372f28' }

/** Silver chassis, as it shipped (`data-theme='light' data-skin='silver'`). */
const SILVER: Look = {
  day: {
    '--bg': '#e2e6eb', '--panel': '#f9fafb', '--bg-elev': '#f9fafb', '--bg-elev-2': '#f3f5f7', '--border': '#9aa3ad',
    '--border-soft': '#b9c1ca', '--text': '#1b1f24', '--text-dim': '#4a525c', '--text-faint': '#5b636d', '--accent': '#3b5a7a',
    '--accent-ink': '#ffffff', '--focus-ring': '#3b5a7a', '--bubble-mine': '#3b5a7a', '--bubble-mine-text': '#ffffff',
    '--bubble-theirs': '#e6e9ed', '--bubble-theirs-text': '#1b1f24', '--readout': '#143d6b', ...WELL_TRIO_DAY,
  },
  well: { ...WELL_DAY, '--accent': '#a9c0d8', '--accent-ink': '#0b1a2a', '--focus-ring': '#a9c0d8', '--readout': '#9ccbff' },
  night: {
    '--bg': '#eae4da', '--panel': '#fcf9f1', '--bg-elev': '#fcf9f1', '--bg-elev-2': '#f8f4ed', '--border': '#a39a90',
    '--border-soft': '#bfb7ae', '--text': '#201c17', '--text-dim': '#554f48', '--text-faint': '#5f5851', '--accent': '#325171',
    '--accent-ink': '#ffffff', '--focus-ring': '#325171', '--bubble-mine': '#325171', '--bubble-mine-text': '#ffffff',
    '--bubble-theirs': '#e2dfdb', '--bubble-theirs-text': '#201c17', '--readout': '#0c3664', ...WELL_TRIO_NIGHT,
  },
  nightWell: { ...WELL_NIGHT, '--accent': '#879db4', '--accent-ink': '#0b1a2a', '--focus-ring': '#879db4', '--readout': '#78a6d8' },
}

/** The standard Light as it shipped until now (`data-theme='light'`, no theme). */
const OLD_LIGHT: Look = {
  day: {
    '--bg': '#e5eaf0', '--panel': '#fbfcfe', '--bg-elev': '#fbfcfe', '--bg-elev-2': '#f4f6f9', '--border': '#c4cedb',
    '--border-soft': '#d8dfe9', '--text': '#14202e', '--text-dim': '#46556e', '--text-faint': '#57647a', '--accent': '#0174ab',
    '--accent-ink': '#ffffff', '--focus-ring': '#0174ab', '--bubble-mine': '#0174ab', '--bubble-mine-text': '#ffffff',
    '--bubble-theirs': '#eef2f7', '--bubble-theirs-text': '#14202e', '--readout': '#0174ab', ...WELL_TRIO_DAY,
  },
  well: { ...WELL_DAY, '--accent': '#4cc9f0', '--accent-ink': '#06222c', '--focus-ring': '#4cc9f0', '--readout': '#4cc9f0' },
  night: {
    '--bg': '#eae4da', '--panel': '#fcf9f1', '--bg-elev': '#fcf9f1', '--bg-elev-2': '#f8f4ed', '--border': '#c5bcb0',
    '--border-soft': '#d6cec3', '--text': '#26211a', '--text-dim': '#554f48', '--text-faint': '#5f5851', '--accent': '#006fae',
    '--accent-ink': '#ffffff', '--focus-ring': '#006fae', '--bubble-mine': '#006fae', '--bubble-mine-text': '#ffffff',
    '--bubble-theirs': '#efe9df', '--bubble-theirs-text': '#26211a', '--readout': '#006fae', ...WELL_TRIO_NIGHT,
  },
  nightWell: { ...WELL_NIGHT, '--accent': '#42a7c7', '--accent-ink': '#001620', '--focus-ring': '#42a7c7', '--readout': '#42a7c7' },
}

/** What `look` says each of the four places paints, and the mode that paints it with `theme` on. */
const places = (look: Look, theme = '') =>
  [
    ['light', 'root', look.day],
    ['light', 'well', look.well],
    ['light-night', 'root', look.night],
    ['light-night', 'well', look.nightWell],
  ].map(([m, scope, want]) => ({ mode: `${m}${theme && ` skin=${theme}`}` as Mode, scope: scope as Scope, want: want as Record<string, string> }))

/** Every token where the sheet paints something other than `want`. */
function offTable(rules: Rule[], look: Look, theme = ''): string[] {
  const off: string[] = []
  for (const { mode, scope, want } of places(look, theme)) {
    const got = tokens(rules, mode, scope)
    for (const t of SKIN_TOKENS) if (got[t] !== want[t]) off.push(`${mode} ${scope} ${t}: ${got[t]}, not ${want[t]}`)
  }
  return off
}

describe('the standard Light is Silver', () => {
  it("paints Silver's look: every theme token, at <html> and in a display well, by day and at night", () => {
    expect(offTable(RULES, SILVER)).toEqual([])
  })

  it('and no longer the old bright one (the tables differ where the looks do)', () => {
    const light = tokens(RULES, 'light', 'root')
    for (const t of ['--bg', '--panel', '--text', '--accent', '--readout']) expect(light[t], t).not.toBe(OLD_LIGHT.day[t])
  })

  it('the gallery no longer offers Silver, and no light theme in it paints what Light paints', () => {
    expect(skinOf('silver')).toBeUndefined()
    const light = tokens(RULES, 'light', 'root')
    for (const s of SKINS.filter((x) => x.base === 'light')) {
      expect(tokens(RULES, `light skin=${s.id}` as Mode, 'root'), `${s.id} duplicates Light`).not.toEqual(light)
    }
  })
})

describe('the old Light is the Bright theme', () => {
  it('a gallery theme on the light base, with the Auto waterfall the old Light had', () => {
    const bright = skinOf('bright')
    expect(bright?.base).toBe('light')
    expect(bright?.family).toBe('modern')
    expect(bright?.waterfall).toBe('cividis')
  })

  it('its table is the old Light, value for value', () => {
    const bright = skinOf('bright')!
    const pick = (look: Record<string, string>, keys: string[]) => Object.fromEntries(keys.map((k) => [k, look[k]]))
    expect(bright.day).toEqual(pick(OLD_LIGHT.day, Object.keys(bright.day)))
    expect(bright.night).toEqual(pick(OLD_LIGHT.night, Object.keys(bright.night)))
    expect(bright.well).toEqual(pick(OLD_LIGHT.well, Object.keys(bright.well!)))
    expect(bright.nightWell).toEqual(pick(OLD_LIGHT.nightWell, Object.keys(bright.nightWell!)))
  })

  it('the sheet paints exactly the old Light under it, at <html> and in a well, by day and at night', () => {
    expect(offTable(RULES, OLD_LIGHT, 'bright')).toEqual([])
  })
})

describe('what beat the light palette still beats it', () => {
  // High contrast owns the surfaces and the inks, whatever the theme (D4), so Light under it is the one
  // high-contrast look it always was: the HC block's values, by day and at night.
  const HIGH = {
    'light-high': {
      '--bg': '#dfe5ec', '--panel': '#ffffff', '--bg-elev': '#ffffff', '--bg-elev-2': '#f2f5f9', '--border': '#7d8899',
      '--border-soft': '#a8b3c2', '--text': '#000000', '--text-dim': '#1c2733', '--text-faint': '#26323f',
    },
    'light-night-high': {
      '--bg': '#eae3db', '--panel': '#fcf9f1', '--bg-elev': '#fcf9f1', '--bg-elev-2': '#f8f4ed', '--border': '#8e8479',
      '--border-soft': '#b2a99f', '--text': '#080503', '--text-dim': '#272019', '--text-faint': '#312a23',
    },
  } as const
  it.each(Object.entries(HIGH))('%s: the high-contrast surfaces and inks, on Light and on Bright', (mode, want) => {
    for (const m of [mode, `${mode} skin=bright`] as Mode[]) {
      const got = tokens(RULES, m, 'root')
      for (const [t, v] of Object.entries(want)) expect(got[t], `${m} ${t}`).toBe(v)
    }
  })

  // A colour-role pick beats Light's accent and readout, at <html> and inside a well, by day and at
  // night: the light value at <html>, the dark value in a well (a well shows a preset's dark value).
  it('every Accent and Readout pick paints its own table value on Light, at <html> and in a well, by day and at night', () => {
    const off: string[] = []
    for (const role of PALETTE_ROLES.filter((r) => r.dimsAtNight))
      for (const p of role.presets.slice(1))
        for (const night of [false, true]) {
          const mode = `${night ? 'light-night' : 'light'} ${role.id}=${p.id}` as Mode
          const atRoot = night ? p.night!.light : p.light
          const inWell = night ? p.night!.dark : p.dark
          const root = tokens(RULES, mode, 'root')
          const well = tokens(RULES, mode, 'well')
          for (const t of role.tokens) {
            if (root[t] !== atRoot[t]) off.push(`${mode} root ${t}: ${root[t]}, not ${atRoot[t]}`)
            if (t in well && WELL_PICKS.includes(t) && well[t] !== inWell[t]) off.push(`${mode} well ${t}: ${well[t]}, not ${inWell[t]}`)
          }
        }
    expect(off).toEqual([])
  })
})
/** The tokens a light look gives its display wells. */
const WELL_PICKS = ['--accent', '--accent-ink', '--focus-ring', '--readout']

describe('the locked colours do not move', () => {
  // The TX red, the ON AIR sign, the critical orange and the status and need inks resolve exactly as the
  // sheet resolved them before Light changed, in every mode and on Bright. The ON AIR rim is the theme's
  // full-strength ink by its own definition (`var(--text)`), so it is held to that, not to a hex.
  const LOCKED = {
    light: {
      '--alert-critical': '#9c0000', '--need-band': '#a16207', '--need-confirm': '#64748b', '--need-dxped': '#0369a1',
      '--need-entity': '#c2187f', '--need-grid': '#047857', '--need-ink': '#ffffff', '--need-iota': '#be123c', '--need-mode': '#0d6f8a',
      '--need-mode-ft4': '#0f7038', '--need-mode-rtty': '#8a5200', '--need-pota': '#01541c', '--need-sota': '#6d28d9',
      '--need-state': '#0f766e', '--need-watch': '#3f6212', '--need-zone': '#7e22ce',
      '--on-air-bg': 'color-mix(in srgb, #a50000 90%, #000000)', '--on-air-ink': '#ffffff', '--status-confirmed': '#008d3c',
      '--status-dupe': '#98a0a5', '--status-new-band': '#bb8805', '--status-new-entity': '#a30000', '--status-new-mode': '#7c44c3',
      '--status-worked': '#5c7f98', '--tx': '#a50000',
    },
    dark: {
      '--alert-critical': '#ff6100', '--need-band': '#f59e0b', '--need-confirm': '#9ca3af', '--need-dxped': '#38bdf8',
      '--need-entity': '#f23ec0', '--need-grid': '#34d399', '--need-ink': '#10131a', '--need-iota': '#fb7185', '--need-mode': '#22d3ee',
      '--need-mode-ft4': '#a7f3d0', '--need-mode-rtty': '#f5a524', '--need-pota': '#4ade80', '--need-sota': '#a78bfa',
      '--need-state': '#2dd4bf', '--need-watch': '#a3e635', '--need-zone': '#c084fc',
      '--on-air-bg': 'color-mix(in srgb, #e64343 90%, #000000)', '--on-air-ink': '#ffffff', '--status-confirmed': '#70e093',
      '--status-dupe': '#747b81', '--status-new-band': '#ccb10d', '--status-new-entity': '#ffb35b', '--status-new-mode': '#bb8aef',
      '--status-worked': '#7a96ab', '--tx': '#e64343',
    },
  } as const
  const MODES_SWEPT: Mode[] = [...BASE_MODES, ...BASE_MODES.filter((m) => m.startsWith('light')).map((m) => `${m} skin=bright` as Mode)]

  it.each(MODES_SWEPT)('%s', (mode) => {
    const root = rootTokensFrom(RULES, mode)
    const well = tokensAt(RULES, mode, WELL)
    const at = (map: Map<string, string>, t: string) => expandWith(map, `var(${t})`).trim().toLowerCase()
    const theme = LOCKED[baseTheme(mode)]
    const off: string[] = []
    const locked = [...root.keys()].filter((t) => isLockedToken(t) && t !== '--on-air-rim')
    expect(locked.sort(), 'the locked set itself').toEqual(Object.keys(theme).sort())
    for (const t of locked) {
      if (at(root, t) !== theme[t as keyof typeof theme]) off.push(`<html> ${t}: ${at(root, t)}`)
      // In a well every locked ink is the dark theme's, but the ON AIR fill, which is resolved on <html>
      // from the page's own TX red and inherited as that colour.
      const inWell = t === '--on-air-bg' ? theme[t] : LOCKED.dark[t as keyof typeof LOCKED.dark]
      if (at(well, t) !== inWell) off.push(`well ${t}: ${at(well, t)}`)
    }
    if (at(root, '--on-air-rim') !== at(root, '--text')) off.push(`the ON AIR rim is not the page's ink: ${at(root, '--on-air-rim')}`)
    expect(off).toEqual([])
  })
})
