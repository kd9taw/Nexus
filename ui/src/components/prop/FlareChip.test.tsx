// @vitest-environment jsdom
//
// The flare chip says where every number comes from: NOAA's D-RAP grid with its valid time or "estimated
// from GOES"; NOAA's began / peak / ended with NOAA's times, or a trend word marked estimated; NOAA's own
// recovery time or NOAA's empirical fit. A simulated flare says PREVIEW and nothing of NOAA's.
//
// And the chip stack it lives in: the STACK is the positioned box, so the flare chip unfolding pushes the
// proton chip down instead of covering it (resolved from the sheet with the shared cascade resolver, on the
// elements the component actually renders — not a regex over the CSS).
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { FlareChip } from './FlareChip'
import { flareFootprint, type FlareNoaa } from '../../flareNow'
import { flareHafAt, flareRecoveryMin } from '../../mapGeo'
import { t } from '../../i18n'
import { chainOf, parseRules, winnerAt } from '../../cssCascade'
import type { DrapGrid, FlareEvent } from '../../types'

afterEach(cleanup)

const NOW = Date.UTC(2026, 9, 30, 18, 0)
const NOW_S = NOW / 1000
const M5 = 5e-5

function grid(recoveryAt: number | null = null): DrapGrid {
  const lats = Array.from({ length: 90 }, (_, i) => 89 - 2 * i)
  const lons = Array.from({ length: 90 }, (_, i) => -178 + 4 * i)
  const haf: number[] = []
  for (const la of lats) for (const lo of lons) haf.push(flareHafAt(la, lo, NOW, M5))
  return { validAt: NOW_S - 60, recoveryAt, lats, lons, haf }
}
const event: FlareEvent = {
  at: NOW_S - 60,
  beginAt: Date.UTC(2026, 9, 30, 17, 38) / 1000,
  beginClass: 'C5.0',
  maxAt: Date.UTC(2026, 9, 30, 17, 52) / 1000,
  maxClass: 'M5.1',
  maxFlux: 5.1e-5,
  endAt: null,
  currentClass: 'M4.0',
}
const noaa = (o: Partial<FlareNoaa> = {}): FlareNoaa => ({
  xrayAt: NOW_S - 60,
  series: [
    [NOW_S - 3600, 2e-7],
    [NOW_S - 1800, 6e-5],
    [NOW_S - 60, 5e-5],
  ],
  event,
  drap: grid(),
  ...o,
})

function chip(n: FlareNoaa | null, extra: { preview?: boolean; trend?: 'rising' | 'falling' | null } = {}) {
  const foot = flareFootprint(NOW, M5, n?.drap)!
  return render(
    <div className="wx-chip-stack">
      <FlareChip
        xrayLong={M5}
        foot={foot}
        noaa={n}
        trend={extra.trend ?? null}
        qth={{ lat: 41.9, lon: -87.6 }}
        nowMs={NOW}
        preview={extra.preview}
      />
      <div className="flare-chip pca-chip" role="status">
        protons
      </div>
    </div>,
  )
}

describe('FlareChip words', () => {
  it("names NOAA's grid and its valid time, and NOAA's event times", () => {
    const { container } = chip(noaa())
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.src.noaa', { time: '17:59Z', xtime: '17:59Z' }))
    expect(text).toContain(t('map.flare.noaa.peakedClass', { begin: '17:38Z', peak: '17:52Z', cls: 'M5.1' }))
    expect(container.querySelector('.flare-chip-src')!.getAttribute('data-source')).toBe('noaa')
  })

  it('says "estimated from GOES" when there is no current NOAA grid, and uses the trend word', () => {
    const { container } = chip(noaa({ drap: null, event: null }), { trend: 'falling' })
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.src.goes', { xtime: '17:59Z' }))
    expect(text).toContain(t('map.flare.est.falling'))
    expect(text).not.toContain('NOAA D-RAP')
  })

  it('says how old the GOES sample is once it stops being now', () => {
    const { container } = chip(noaa({ drap: null, xrayAt: NOW_S - 7 * 60 }))
    expect(container.querySelector('.flare-chip-src')!.textContent).toContain('17:53Z, ')
  })

  it('unfolds to the bands, the recovery source and the sparkline', () => {
    const { container, getByRole } = chip(noaa({ drap: grid(NOW_S + 30 * 60) }))
    expect(container.querySelector('.flare-spark')).toBeNull()
    fireEvent.click(getByRole('button', { name: t('map.flare.toggle.aria') }))
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.recovery.noaa', { mins: 30 }))
    expect(container.querySelector('.flare-spark path')!.getAttribute('d')).toMatch(/^M0\.0,/)
    // NOAA's begin and peak are marked on the trace.
    expect(container.querySelectorAll('.flare-spark-mark')).toHaveLength(2)
  })

  it("falls back to NOAA's empirical fit, said so, without NOAA's recovery time", () => {
    const { container, getByRole } = chip(noaa({ drap: null }))
    fireEvent.click(getByRole('button', { name: t('map.flare.toggle.aria') }))
    expect(container.textContent).toContain(t('map.flare.recovery.fit', { mins: Math.round(flareRecoveryMin(M5)!) }))
  })

  it('a preview says PREVIEW and nothing of NOAA', () => {
    const { container } = chip(null, { preview: true })
    const text = container.textContent ?? ''
    expect(text).toContain(t('map.flare.phase.preview'))
    expect(text).not.toContain('NOAA')
    expect(container.querySelector('button')).toBeNull()
  })
})

describe('the chip stack in the cascade', () => {
  const RAW = readFileSync(resolve(process.cwd(), 'src', 'styles.css'), 'utf8')
  const RULES = parseRules(RAW.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')))

  it('positions the stack, not each chip, so an unfolded chip pushes the next one down', () => {
    const { container } = chip(noaa())
    const stack = container.querySelector('.wx-chip-stack')!
    const flare = container.querySelector('.flare-chip:not(.pca-chip)')!
    const pca = container.querySelector('.pca-chip')!
    expect(winnerAt(RULES, 'dark', chainOf(stack), 'position')?.value).toBe('absolute')
    for (const el of [flare, pca]) {
      expect(winnerAt(RULES, 'dark', chainOf(el), 'position')?.value ?? 'static').toBe('static')
      expect(winnerAt(RULES, 'dark', chainOf(el), 'top')).toBeNull()
    }
  })

  it('draws the proton chip with its own violet edge, not the flare orange', () => {
    const { container } = chip(noaa())
    const pca = container.querySelector('.pca-chip')!
    const win = winnerAt(RULES, 'dark', chainOf(pca), 'border-left', 'border-left-color')
    expect(win?.value).toContain('#a855f7')
  })
})
