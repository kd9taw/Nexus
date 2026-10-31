// @vitest-environment jsdom
//
// The aurora layer row says what NOAA's OVATION oval is: a FORECAST, for the time in its own
// `Forecast Time` stamp, made from the solar wind measured at its `Observation Time` (hover). It
// was drawn as if it were the oval now. A forecast time long gone dims and says how long ago.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { AuroraForecastNote } from './MapView'
import { wxStampLabel } from '../propViz'
import { t } from '../i18n'

const NOW_MS = Date.UTC(2026, 9, 30, 2, 46, 0)
const NOW = NOW_MS / 1000
beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(NOW_MS)
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('the aurora row names its forecast time', () => {
  it('a forecast ahead reads "forecast for" its time, with the observation time on hover', () => {
    const forecastAt = Date.UTC(2026, 9, 30, 4, 4) / 1000
    const observedAt = Date.UTC(2026, 9, 30, 2, 39) / 1000
    const note = render(<AuroraForecastNote forecastAt={forecastAt} observedAt={observedAt} />).container
      .firstElementChild as HTMLElement
    expect(note.textContent).toBe(t('map.layer.aurora.forecast', { time: wxStampLabel(forecastAt) }))
    expect(note.textContent).toContain('04:04Z')
    expect(note.title).toContain('02:39Z')
    expect(note.hasAttribute('data-stale')).toBe(false)
  })

  it('a forecast time two hours gone dims and says how long ago', () => {
    const forecastAt = NOW - 2 * 3600
    const note = render(<AuroraForecastNote forecastAt={forecastAt} observedAt={null} />).container
      .firstElementChild as HTMLElement
    expect(note.hasAttribute('data-stale')).toBe(true)
    expect(note.textContent).toBe(
      t('map.layer.aurora.forecast.past', {
        time: wxStampLabel(forecastAt),
        ago: t('prop.opening.ago.hours', { hours: 2 }),
      }),
    )
  })
})
