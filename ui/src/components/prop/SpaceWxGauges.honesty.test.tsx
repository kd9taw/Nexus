// @vitest-environment jsdom
//
// The space-weather gauges draw only what NOAA measured, and say how old it is.
//
// When SWPC fails while the spot feeds answer (a "partial" snapshot), the station still runs its
// model on neutral stand-ins (SFI 120, Kp 2, A 8, B1.0). Those numbers used to reach these gauges
// as readings, in green. Each number now carries NOAA's own sample time (`sfiAt`, `kpAt`, `xrayAt`,
// `aDay`); a null stamp is a stand-in, drawn as "—" with no colour and no impact word, and a real
// reading older than its product's cadence is drawn with its age, dimmed. The A gauge is NOAA's
// daily planetary A with its day, the Kp-derived ap estimate only on hover.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { SpaceWxGauges } from './SpaceWxGauges'
import type { SpaceWxView } from '../../types'
import { aImpact, kpImpact, sfiImpact, WX_STALE_SECS } from '../../propViz'
import { t } from '../../i18n'
import { dayLabel } from './SolarTrends'

const NOW_MS = Date.UTC(2026, 9, 30, 2, 46, 0)
const NOW = NOW_MS / 1000
const DAY = Date.UTC(2026, 9, 29) / 1000
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW_MS)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** A whole live reading, as a station of this build sends it (the 30 Oct 2026 02:46Z sky). */
const live = (over: Partial<SpaceWxView> = {}): SpaceWxView => ({
  sfi: 89,
  kp: 0.67,
  aIndex: 4,
  xrayClass: 'B-class',
  flare: false,
  xrayLong: 2.13e-7,
  fetched: true,
  sfiAt: DAY + 20 * 3600,
  kpAt: NOW - 6 * 60,
  xrayAt: NOW - 4 * 60,
  aDay: DAY,
  apNow: 2.68,
  ...over,
})
/** SWPC failed, the spots answered: every number is the model's stand-in. */
const partial = (): SpaceWxView => ({
  sfi: 120,
  kp: 2,
  aIndex: 8,
  xrayClass: 'B-class',
  flare: false,
  xrayLong: 1e-7,
  fetched: false,
  sfiAt: null,
  kpAt: null,
  xrayAt: null,
  aDay: null,
})

function gauge(container: HTMLElement, label: string) {
  const g = [...container.querySelectorAll('.swx-gauge')].find((x) => x.querySelector('.swx-k')?.textContent === label)
  if (!g) throw new Error(`no ${label} gauge`)
  return {
    value: g.querySelector('.swx-v')?.textContent,
    numberDim: g.querySelector('.swx-v')?.hasAttribute('data-dim'),
    unit: g.querySelector('.swx-u')?.textContent ?? null,
    line: g.querySelector('.swx-impact')?.textContent,
    bar: (g.querySelector('.swx-bar-fill') as HTMLElement).style.background,
    inked: g.querySelector('.swx-impact')?.hasAttribute('data-state-ink'),
    title: g.querySelector('.swx-head')?.getAttribute('title') ?? '',
  }
}

describe('stand-ins never read as readings', () => {
  it('a partial snapshot draws a dash, no colour and no impact word on every index', () => {
    const { container } = render(<SpaceWxGauges wx={partial()} />)
    for (const k of ['SFI', 'Kp', 'A', 'X-ray']) {
      const g = gauge(container, k)
      expect(g.value, k).toBe('—')
      expect(g.bar, k).toBe('transparent')
      expect(g.inked, k).toBe(false)
      expect(g.line, k).toBe(t('prop.spaceWx.none'))
    }
    // None of the stand-in numbers is on the strip.
    expect(container.textContent).not.toMatch(/120|B1/)
  })

  it('one product failing dims only its own gauge', () => {
    const { container } = render(<SpaceWxGauges wx={live({ xrayAt: null, sfiAt: null })} />)
    expect(gauge(container, 'X-ray').value).toBe('—')
    expect(gauge(container, 'SFI').value).toBe('—')
    expect(gauge(container, 'Kp')).toMatchObject({ value: '1', line: kpImpact(0.67).text, inked: true })
  })
})

describe('a real reading carries its time, and its age once old', () => {
  it('reads fresh readings as before, each with its sample time on hover', () => {
    const { container } = render(<SpaceWxGauges wx={live()} />)
    expect(gauge(container, 'SFI')).toMatchObject({ value: '89', line: sfiImpact(89).text, inked: true })
    expect(gauge(container, 'Kp').title).toContain('02:40Z')
    expect(gauge(container, 'X-ray').title).toContain('02:42Z')
    expect(gauge(container, 'SFI').title).toContain('20:00Z')
  })

  it('a Kp past its cadence shows its age, dimmed, with no impact word', () => {
    const kpAt = NOW - WX_STALE_SECS.kp - 30 * 60
    const { container } = render(<SpaceWxGauges wx={live({ kp: 6, kpAt })} />)
    const g = gauge(container, 'Kp')
    expect(g.value).toBe('6')
    expect(g.bar).toBe('transparent')
    expect(g.inked).toBe(false)
    expect(g.line).toBe(t('prop.spaceWx.stale', { ago: t('prop.opening.ago.mins', { mins: 45 }) }))
    expect(g.line).not.toBe(kpImpact(6).text)
  })

  it('an old or missing number is dimmed with its line; a fresh one is not', () => {
    const { container } = render(<SpaceWxGauges wx={live({ kp: 6, kpAt: NOW - 2 * 3600, sfiAt: null })} />)
    expect(gauge(container, 'Kp').numberDim, 'stale').toBe(true)
    expect(gauge(container, 'SFI').numberDim, 'stand-in').toBe(true)
    expect(gauge(container, 'X-ray').numberDim, 'fresh').toBe(false)
  })

  it('the SFI hover gives the true measurement time and never calls another reading noon', () => {
    // Penticton's 17:00 UT reading, taken when the file carried no noon row.
    const { container } = render(<SpaceWxGauges wx={live({ sfiAt: DAY + 17 * 3600 })} />)
    const title = gauge(container, 'SFI').title
    expect(title).toContain('17:00Z')
    expect(title).not.toMatch(/noon/i)
  })

  it('just inside the cadence it is still a reading (the boundary)', () => {
    const { container } = render(<SpaceWxGauges wx={live({ kpAt: NOW - WX_STALE_SECS.kp + 1 })} />)
    expect(gauge(container, 'Kp').inked).toBe(true)
  })
})

describe('the A gauge is the daily planetary A', () => {
  it('shows the daily A with its day, and the ap estimate only on hover', () => {
    const { container } = render(<SpaceWxGauges wx={live()} />)
    const g = gauge(container, 'A')
    expect(g).toMatchObject({ value: '4', unit: dayLabel(DAY), line: aImpact(4).text })
    expect(g.title).toContain(t('prop.spaceWx.src.apNow', { ap: '3' }))
  })

  it('without a daily A from NOAA it is a dash, never the ap estimate', () => {
    const { container } = render(<SpaceWxGauges wx={live({ aDay: null, aIndex: 8 })} />)
    expect(gauge(container, 'A')).toMatchObject({ value: '—', unit: null, line: t('prop.spaceWx.none') })
  })
})

describe('an older station', () => {
  it('sends no stamps; its numbers read as they always did', () => {
    const old: SpaceWxView = { sfi: 97, kp: 2, aIndex: 7, xrayClass: 'B5', flare: false }
    const { container } = render(<SpaceWxGauges wx={old} />)
    expect(gauge(container, 'SFI')).toMatchObject({ value: '97', inked: true })
    expect(gauge(container, 'A')).toMatchObject({ value: '7', unit: null })
  })
})
