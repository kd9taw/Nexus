// The storm toast says which Kp it is and when: NOAA's measured 3-hour Kp with its period, or the
// 1-minute estimate with its time (capsNow `kpNow` chooses; App.stormKp.test.tsx pins the choice
// reaching here). The edge, hysteresis and loudness are stormAlert.test.ts's.
import { describe, it, expect, beforeEach, vi } from 'vitest'

const toast = vi.hoisted(() => ({ push: vi.fn() }))
vi.mock('./toast', () => ({ pushToast: toast.push }))
vi.mock('./alerts', () => ({ doubleBeep: vi.fn() }))

import { processStorm, resetStormAlerts } from './stormAlert'
import { t } from './i18n'

const P21 = Date.UTC(2026, 9, 30, 21) / 1000
const AT = Date.UTC(2026, 9, 31, 1, 28) / 1000

beforeEach(() => {
  toast.push.mockClear()
  resetStormAlerts()
})

describe('the storm toast names its Kp', () => {
  it('a measured Kp with its 3-hour period', () => {
    processStorm({ kp: 5.67, source: 'measured', at: P21, until: P21 + 3 * 3600 })
    expect(toast.push.mock.calls[0][0]).toBe(
      t('prop.stormAlert.now', { g: 1, kp: t('prop.kpNow.measured', { kp: '5.7', from: '21Z', to: '00Z' }) }),
    )
  })

  it('an estimate with its time', () => {
    processStorm({ kp: 6.33, source: 'estimated', at: AT })
    expect(toast.push.mock.calls[0][0]).toBe(t('prop.stormAlert.now', { g: 2, kp: t('prop.kpNow.est', { kp: '6.3', time: '01:28Z' }) }))
  })

  it('no Kp, no toast', () => {
    processStorm(null)
    expect(toast.push).not.toHaveBeenCalled()
  })
})
