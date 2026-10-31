// @vitest-environment jsdom
//
// The Kp outlook's hover: NOAA's forecasters' own rationale from the 3-day forecast, with its issue
// time — and no hover at all when that text is not known (never an empty or made-up one).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'

const api = vi.hoisted(() => ({ getKpForecast: vi.fn(), getArrivalOutlook: vi.fn() }))
vi.mock('../../api', () => api)

import { KpOutlookPane } from './KpOutlookPane'
import { wxStampLabel } from '../../propViz'

const FC = {
  points: [
    { timeUnix: 1_793_394_000, kp: 2.33, kind: 'observed', noaaScale: null },
    { timeUnix: 1_793_404_800, kp: 5, kind: 'predicted', noaaScale: 'G1' },
  ],
}
const ISSUED = 1_793_406_600 // 31 Oct 00:30Z
const TEXT = 'The geomagnetic field is likely to reach G1 (Minor) levels due to a positive polarity CH HSS.'

afterEach(cleanup)

async function lines() {
  const { container } = render(<KpOutlookPane />)
  await act(async () => {})
  return container.querySelector('.kp-lines')
}

describe('the Kp outlook rationale hover', () => {
  it("carries NOAA's rationale with its issue time", async () => {
    api.getKpForecast.mockResolvedValueOnce(FC)
    api.getArrivalOutlook.mockResolvedValueOnce({
      watch: null, scales: null, model: null, l1: null, rationale: { issued: ISSUED, text: TEXT },
    })
    const el = await lines()
    expect(el?.getAttribute('title')).toBe(`NOAA forecaster, issued ${wxStampLabel(ISSUED)}: ${TEXT}`)
  })

  it('has no hover when the rationale is not known', async () => {
    api.getKpForecast.mockResolvedValueOnce(FC)
    api.getArrivalOutlook.mockRejectedValueOnce(new Error('offline'))
    const el = await lines()
    expect(el).not.toBeNull()
    expect(el?.hasAttribute('title')).toBe(false)
  })
})
