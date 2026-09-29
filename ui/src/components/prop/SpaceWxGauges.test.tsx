// @vitest-environment jsdom
//
// The Space Wx gauges' solar-wind speed.
//
// The speed rides in the snapshot's `solarWind` beside Bz. Its one trap is the producer's:
// `solar_wind::assemble` (crates/propagation/src/solar_wind.rs) fills speed with 0 when the
// DSCOVR plasma product did not answer, and keeps Bz from the magnetometer. A gauge that printed
// that 0 would tell the operator the solar wind had stopped, so a speed of 0 is "no reading" —
// the Sun's wind never blows below ~250 km/s.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { SpaceWxGauges } from './SpaceWxGauges'
import type { SolarWind, SpaceWxView } from '../../types'

afterEach(cleanup)

const wx = (solarWind?: SolarWind | null): SpaceWxView => ({
  sfi: 97,
  kp: 2,
  aIndex: 7,
  xrayClass: 'B5',
  flare: false,
  solarWind,
})
const wind = (speedKms: number): SolarWind => ({ bzNt: -1.2, btNt: 5, speedKms, density: 4 })

/** The gauges' labels, in order, and each one's value — read off the rendered strip. */
function gauges(container: HTMLElement) {
  return [...container.querySelectorAll('.swx-gauge')].map((g) => ({
    label: g.querySelector('.swx-k')?.textContent ?? '',
    value: g.querySelector('.swx-v')?.textContent ?? '',
    unit: g.querySelector('.swx-u')?.textContent ?? '',
  }))
}

describe('the solar-wind speed gauge', () => {
  it('shows the speed in km/s beside Bz', () => {
    const { container } = render(<SpaceWxGauges wx={wx(wind(432))} />)
    const g = gauges(container)
    const speed = g.find((x) => x.unit === 'km/s')
    expect(speed, 'a gauge carries the km/s unit').toBeTruthy()
    expect(speed!.value).toBe('432')
    // Control: Bz, from the same sample, is on the strip too.
    expect(g.some((x) => x.label === 'Bz' && x.value === '-1.2')).toBe(true)
  })

  it('draws no speed when the plasma feed did not answer (the producer sends 0)', () => {
    const { container } = render(<SpaceWxGauges wx={wx(wind(0))} />)
    const g = gauges(container)
    expect(g.some((x) => x.unit === 'km/s')).toBe(false)
    // Bz came from the magnetometer and is still real.
    expect(g.some((x) => x.label === 'Bz' && x.value === '-1.2')).toBe(true)
  })

  it('draws no speed, and no Bz, when there is no solar wind at all', () => {
    const { container } = render(<SpaceWxGauges wx={wx(null)} />)
    const g = gauges(container)
    expect(g.some((x) => x.unit === 'km/s')).toBe(false)
    expect(g.some((x) => x.label === 'Bz')).toBe(false)
    // Control: the strip itself rendered.
    expect(g.some((x) => x.label === 'SFI' && x.value === '97')).toBe(true)
  })
})
