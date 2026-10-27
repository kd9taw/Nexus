// @vitest-environment jsdom
//
// JS8 TYPEAHEAD FROM A COMPUTER DRIVING A STATION. Words typed into the box while its message goes
// out are sent to the station as they are typed. A refusal there was dropped, so the words never
// went out and nothing said so. On such a computer it is shown, once for a run of keystrokes that
// meets the same refusal. At the station the answer stays quiet as it was: there a refusal is the
// message having just ended or moved on, which the cockpit's poll already shows.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'

const driving = vi.hoisted(() => ({ on: false, n: 0 }))
vi.mock('../features/durableStore', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.on,
}))
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../toast')>()),
  pushToast: vi.fn(),
}))
vi.mock('../api', () => ({
  js8TypeaheadState: vi.fn(async () => ({ sent: 'CQ ', unsent: 'DE', locked: false })),
  // Its own words per case, so the burst guard never carries over from the case before.
  js8Typeahead: vi.fn(() => Promise.reject(`The station has not heard this computer (${driving.n})`)),
}))

import { pushToast } from '../toast'
import { useJs8Typeahead } from './js8Typeahead'

const toast = pushToast as unknown as ReturnType<typeof vi.fn>

async function typeThree() {
  const box = renderHook(() => useJs8Typeahead('DE', () => {})).result
  await act(async () => {
    expect(await box.current.begin(), 'control: a message is going out').toBe(true)
  })
  for (const text of ['DE K', 'DE KD', 'DE KD9']) {
    await act(async () => {
      box.current.type(text)
      await new Promise((r) => setTimeout(r, 0))
    })
  }
}

beforeEach(() => {
  toast.mockClear()
  driving.n++
})
afterEach(() => {
  driving.on = false
})

describe('a refused JS8 typeahead', () => {
  it('is shown, once, on a computer driving a station', async () => {
    driving.on = true
    await typeThree()
    expect(toast).toHaveBeenCalledTimes(1)
    expect(toast).toHaveBeenCalledWith(`The station has not heard this computer (${driving.n})`, 'error')
  })

  it('stays quiet at the station, as it was', async () => {
    await typeThree()
    expect(toast).not.toHaveBeenCalled()
  })
})
