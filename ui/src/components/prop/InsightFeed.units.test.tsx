// @vitest-environment jsdom
//
// THE SPORADIC-E WATCH GIVES ONE HOP'S REACH IN THE OPERATOR'S UNITS.
//
// Its technical line ended "Es is minutes-long, 500–2500 km", composed in Rust, so it read in
// kilometres on Imperial. The backend now sends the span as a km token (`km_range_token`,
// crates/propagation/src/geo.rs) and the feed writes it. The setting and the OS locale DISAGREE in
// each case, so a feed that read only one of them cannot pass. 500–2500 km is 311–1553 mi.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { Insight } from '../../types'
import { setUnitsMirror } from '../../units'
import { InsightFeed } from './InsightFeed'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  localStorage.clear()
})

const ES_WATCH = {
  kind: 'esWatch',
  level: 'info',
  plain: '6m: watch 50.313 for sudden DX (sporadic-E season)',
  technical: 'boreal Es season (solar declination > 15°); Es is minutes-long, {km:500-2500}',
  band: '6m',
} as unknown as Insight

function technical(setting: 'imperial' | 'metric', locale: string): string {
  vi.spyOn(navigator, 'language', 'get').mockReturnValue(locale)
  setUnitsMirror(setting)
  render(<InsightFeed insights={[ES_WATCH]} />)
  return document.querySelector('.if-tech')?.textContent ?? ''
}

describe("the sporadic-E watch's technical line", () => {
  it('Imperial beats a British locale: miles', () => {
    expect(technical('imperial', 'en-GB')).toBe(
      'boreal Es season (solar declination > 15°); Es is minutes-long, 311–1553 mi',
    )
  })

  it('Metric beats a US locale: kilometres', () => {
    expect(technical('metric', 'en-US')).toBe(
      'boreal Es season (solar declination > 15°); Es is minutes-long, 500–2500 km',
    )
  })
})
