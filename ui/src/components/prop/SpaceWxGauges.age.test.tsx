// @vitest-environment jsdom
//
// The Bz gauge is honest about its solar-wind reading's age.
//
// The station keeps its last good sample while NOAA's DSCOVR feed is unreachable, and until now the
// gauge showed it with nothing to say it was old: a southward Bz from the morning read as the field
// right now. The sample carries its own time (the magnetometer reading's); past half an hour the gauge
// says when the reading was made instead of interpreting it, and with no sample at all it says so. An
// older station sends no time — its gauge reads exactly as it always did, because nothing can be known.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { SpaceWxGauges } from './SpaceWxGauges'
import type { SolarWind, SpaceWxView } from '../../types'
import { bzImpact, SOLAR_WIND_STALE_SECS } from '../../propViz'
import { t } from '../../i18n'

/** A fixed "now", so a boundary test cannot move under it. */
const NOW_MS = Date.UTC(2026, 8, 29, 12, 0, 0)
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW_MS)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const wx = (solarWind?: SolarWind | null): SpaceWxView => ({
  sfi: 97,
  kp: 2,
  aIndex: 7,
  xrayClass: 'B5',
  flare: false,
  solarWind,
})
/** A sample `ageSecs` old, as a station of this build sends it (dated); null = an older station's. */
const sample = (ageSecs: number | null, over: Partial<SolarWind> = {}): SolarWind => ({
  bzNt: -3.4,
  btNt: 6.1,
  speedKms: 487,
  density: 5.2,
  ...(ageSecs == null ? {} : { timeUnix: NOW_MS / 1000 - ageSecs }),
  ...over,
})

/** The Bz gauge as rendered: its value, its line, and the colour of its bar. */
function bz(container: HTMLElement) {
  const g = [...container.querySelectorAll('.swx-gauge')].find((x) => x.querySelector('.swx-k')?.textContent === 'Bz')
  if (!g) return null
  return {
    value: g.querySelector('.swx-v')?.textContent,
    line: g.querySelector('.swx-impact')?.textContent,
    bar: (g.querySelector('.swx-bar-fill') as HTMLElement | null)?.style.background,
  }
}

describe("the Bz gauge and its reading's age", () => {
  it('reads a fresh sample as it always has', () => {
    const { container } = render(<SpaceWxGauges wx={wx(sample(5 * 60))} />)
    expect(bz(container)).toMatchObject({ value: '-3.4', line: bzImpact(-3.4).text })
  })

  it('past half an hour, says when the reading was made instead of reading it as the field now', () => {
    const { container } = render(<SpaceWxGauges wx={wx(sample(45 * 60))} />)
    const g = bz(container)!
    expect(g.value, 'the number stays, with its age').toBe('-3.4')
    expect(g.line).toBe(t('prop.spaceWx.bz.stale', { ago: t('prop.opening.ago.mins', { mins: 45 }) }))
    expect(g.bar, 'an old reading gets no severity colour').toBe('transparent')
  })

  it('turns exactly at the threshold, not a second before', () => {
    const fresh = render(<SpaceWxGauges wx={wx(sample(SOLAR_WIND_STALE_SECS - 1))} />)
    expect(bz(fresh.container)!.line).toBe(bzImpact(-3.4).text)
    cleanup()
    const old = render(<SpaceWxGauges wx={wx(sample(SOLAR_WIND_STALE_SECS))} />)
    expect(bz(old.container)!.line).not.toBe(bzImpact(-3.4).text)
  })

  it('counts a long gap in hours', () => {
    const { container } = render(<SpaceWxGauges wx={wx(sample(3 * 3600))} />)
    expect(bz(container)!.line).toBe(t('prop.spaceWx.bz.stale', { ago: t('prop.opening.ago.hours', { hours: 3 }) }))
  })

  it('with no sample at all, says so instead of leaving Bz off the strip', () => {
    const { container } = render(<SpaceWxGauges wx={wx(null)} />)
    const g = bz(container)
    expect(g, 'a Bz gauge is drawn').not.toBeNull()
    expect(g!.value).toBe('—')
    expect(g!.line).toBe(t('prop.spaceWx.bz.none'))
    expect(g!.bar).toBe('transparent')
  })

  it("reads an older station's sample (no time, 0 for a missing speed) as it always did: no age is claimed", () => {
    const { container } = render(<SpaceWxGauges wx={wx(sample(null, { speedKms: 0, density: 0 }))} />)
    expect(bz(container)).toMatchObject({ value: '-3.4', line: bzImpact(-3.4).text })
  })
})
