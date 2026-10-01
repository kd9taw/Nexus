// The solar-wind sample's age — one threshold for the station and the page.
//
// The station stops raising solar-wind insights from a sample older than SOLAR_WIND_STALE_SECS
// (crates/propagation/src/solar_wind.rs), and the Space Wx gauges say how old a reading is from the
// same moment. If the two drifted, the page would call a reading current that the station had already
// stopped speaking from, or the other way round — so the number is read out of the Rust source.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { SOLAR_WIND_STALE_SECS, solarWindAgeSecs } from './propViz'
import type { SolarWind } from './types'

const SOLAR_WIND_RS = readFileSync(
  fileURLToPath(new URL('../../crates/propagation/src/solar_wind.rs', import.meta.url)),
  'utf8',
)

const NOON = Date.UTC(2026, 8, 29, 12, 0, 0)
const dated = (timeUnix: number): SolarWind => ({ bzNt: -3, btNt: 5, speedKms: null, density: null, timeUnix })

describe('the solar-wind sample age', () => {
  it("is called old at the station's own threshold", () => {
    const m = /pub const SOLAR_WIND_STALE_SECS:\s*i64\s*=\s*([0-9_\s*]+);/.exec(SOLAR_WIND_RS)
    expect(m, 'solar_wind.rs declares SOLAR_WIND_STALE_SECS').not.toBeNull()
    const rust = m![1]
      .split('*')
      .map((f) => Number(f.trim().replace(/_/g, '')))
      .reduce((a, b) => a * b, 1)
    expect(SOLAR_WIND_STALE_SECS).toBe(rust)
  })

  it("is measured from the sample's own time", () => {
    expect(solarWindAgeSecs(dated(NOON / 1000 - 45 * 60), NOON)).toBe(45 * 60)
    // A sample stamped a little ahead of this computer's clock is fresh, never a negative age.
    expect(solarWindAgeSecs(dated(NOON / 1000 + 30), NOON)).toBe(0)
  })

  it('is unknown for an older station, which sends no time (and 0 for a missing speed)', () => {
    expect(solarWindAgeSecs({ bzNt: -3, btNt: 5, speedKms: 0, density: 0 }, NOON)).toBeNull()
  })
})
