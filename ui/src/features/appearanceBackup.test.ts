// Appearance in the Backup (features/appearanceBackup.ts) — operator pick of 2026-09-26,
// "Per computer + Backup": the appearance stays webview-local, and Settings ▸ Config ▸ Backup &
// reset carries it in the file and puts it back on a restore.
//
// What must hold, each against a bundle shaped exactly like the one the station writes
// (src-tauri `export_settings_bundle`: serde's pretty JSON, keys app · kind · schema · settings ·
// uiState):
//   · the section rides BESIDE the station's sections, never inside `settings` — it adds no
//     settings.json key — and every byte the station wrote is kept, a 64-bit number included;
//   · it round-trips;
//   · an old backup, without the section, reads as "nothing to restore", not as an error;
//   · a value it cannot read is skipped, never guessed at or defaulted over the operator's own;
//   · Field mode and UI scale are not in it, by design.
import { describe, expect, it } from 'vitest'
import { DEFAULT_SELECTION } from './paletteRoles'
import { appearanceIn, withAppearance, type Appearance } from './appearanceBackup'

/** A station bundle as serde_json::to_string_pretty writes it. `bigId` is past 2^53: JSON.parse
 *  would round it, so it proves the station's bytes are copied, not re-serialised. */
const STATION = `{
  "app": "1.16.0",
  "kind": "nexus-settings-backup",
  "schema": 1,
  "settings": {
    "bigId": 9007199254740993,
    "mycall": "KD9TAW",
    "txLevel": 1.0
  },
  "uiState": {
    "nexus.memory.bank.v2": "{\\"version\\":2}"
  }
}`

const FULL: Appearance = {
  theme: 'light',
  highContrast: true,
  night: 'auto',
  textSize: 'larger',
  density: 'touch',
  motion: 'reduce',
  colours: { ...DEFAULT_SELECTION, accent: 'violet', readout: 'amber' },
  waterfallPalette: 'cividis',
  ftWaterfallPalette: 'inferno',
  logbookGlobe: false,
}

describe('the backup carries the appearance', () => {
  it('beside the station’s sections, never inside settings', () => {
    const out = JSON.parse(withAppearance(STATION, FULL))
    expect(out.appearance).toEqual(FULL)
    expect(Object.keys(out.settings), 'the appearance reached settings.json').toEqual(['bigId', 'mycall', 'txLevel'])
    expect(out.kind).toBe('nexus-settings-backup')
    expect(out.schema).toBe(1)
  })

  it('keeps every byte the station wrote', () => {
    const out = withAppearance(STATION, FULL)
    const body = STATION.slice(0, STATION.lastIndexOf('}'))
    expect(out.startsWith(body.trimEnd()), 'the station part was rewritten').toBe(true)
    expect(out).toContain('"bigId": 9007199254740993')
    expect(out).toContain('"txLevel": 1.0')
  })

  it('round-trips', () => {
    expect(appearanceIn(withAppearance(STATION, FULL))).toEqual(FULL)
  })

  it('carries only what the host has — a partial section stays partial', () => {
    const out = withAppearance(STATION, { theme: 'system' })
    expect(JSON.parse(out).appearance).toEqual({ theme: 'system' })
    expect(appearanceIn(out)).toEqual({ theme: 'system' })
  })

  it('never carries Field mode or UI scale', () => {
    const section = JSON.parse(withAppearance(STATION, FULL)).appearance
    expect(Object.keys(section).sort()).toEqual(
      ['colours', 'density', 'ftWaterfallPalette', 'highContrast', 'logbookGlobe', 'motion', 'night',
        'textSize', 'theme', 'waterfallPalette'],
    )
  })
})

describe('a restore reads it back', () => {
  it('an old backup with no appearance section has nothing to restore, and is no error', () => {
    expect(appearanceIn(STATION)).toBeNull()
  })

  it('a value it cannot read is skipped, the rest restored', () => {
    const odd = JSON.stringify({
      ...JSON.parse(STATION),
      appearance: {
        theme: 'amber', // retired in 0.8.0
        highContrast: 'yes',
        night: 'auto',
        textSize: 'huge',
        density: 'dense',
        motion: 'off',
        colours: { accent: 'pink', readout: 'green', ok: 42 },
        waterfallPalette: 'rainbow',
        ftWaterfallPalette: 'grayscale',
        logbookGlobe: 0,
        fieldMode: true, // never restored, even when a file carries it
      },
    })
    expect(appearanceIn(odd)).toEqual({
      night: 'auto',
      density: 'dense',
      colours: { readout: 'green' },
      ftWaterfallPalette: 'grayscale',
    })
  })

  it('a palette the curated list no longer offers, but the app still paints, comes back', () => {
    // resolveColormap still accepts every WATERFALL_PALETTES value — a legacy pick is a real pick.
    const legacy = withAppearance(STATION, { waterfallPalette: 'linrad' })
    expect(appearanceIn(legacy)).toEqual({ waterfallPalette: 'linrad' })
  })

  it('a section that is not an object, or a file that is not JSON, has nothing to restore', () => {
    for (const appearance of [null, 'dark', 7, ['theme']]) {
      expect(appearanceIn(JSON.stringify({ ...JSON.parse(STATION), appearance }))).toBeNull()
    }
    expect(appearanceIn('not json')).toBeNull()
  })
})
