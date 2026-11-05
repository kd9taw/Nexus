// @vitest-environment jsdom
//
// THE VERB EACH SCREEN'S RECALL TUNES WITH, AND THE ASK A POP-OUT MAKES OF THE MAIN WINDOW.
//
// Recall adds no way of tuning: each screen uses the verb that screen's own surfaces already use.
// The SSTV strip's `sstv_tune`, the APRS Tune button's `aprs_tune`, and `work_spot` for the rest,
// so a recall during TX meets the same `tune_dial` it always did. `work_spot` is also what ticks the
// nav hint every window follows, and the SSTV and APRS verbs tick nothing: that gap is what a pop-out's
// ask fills.
import { beforeEach, describe, expect, it, vi } from 'vitest'
import * as api from '../api'
import type { RecallPlan } from './memories'
import {
  askMainWindowToOpen,
  MEMORY_OPEN_SCREEN_KEY,
  modeNotCommanded,
  openScreenRequest,
  ridesPhone,
  tuneForRecall,
} from './memoryRecall'

vi.mock('../api', () => ({
  sstvTune: vi.fn(async () => 'sstv'),
  aprsTune: vi.fn(async () => 'aprs'),
  workSpot: vi.fn(async () => 'work'),
}))
const sstvTune = api.sstvTune as unknown as ReturnType<typeof vi.fn>
const aprsTune = api.aprsTune as unknown as ReturnType<typeof vi.fn>
const workSpot = api.workSpot as unknown as ReturnType<typeof vi.fn>

const plan = (view: RecallPlan['view'], mode: string, freqMhz = 144.8875): RecallPlan => ({
  view,
  mode,
  freqMhz,
  settingsPatch: null,
})

beforeEach(() => vi.clearAllMocks())

describe('tuneForRecall', () => {
  it('SSTV uses the SSTV verb, and never work_spot, whose nav hint would pull the window to Phone', async () => {
    await expect(tuneForRecall(plan('sstv', 'FM'), '2m')).resolves.toBe('sstv')
    expect(sstvTune).toHaveBeenCalledExactlyOnceWith(144.8875, '2m', 'FM')
    expect(workSpot).not.toHaveBeenCalled()
    expect(aprsTune).not.toHaveBeenCalled()
  })

  it('an NFM memory holds FM on SSTV, whose hold is armed by the word FM', async () => {
    await tuneForRecall(plan('sstv', 'NFM'), '2m')
    expect(sstvTune).toHaveBeenCalledExactlyOnceWith(144.8875, '2m', 'FM')
  })

  it('a sideband memory keeps its own word on SSTV', async () => {
    await tuneForRecall(plan('sstv', 'LSB', 7.171), '40m')
    expect(sstvTune).toHaveBeenCalledExactlyOnceWith(7.171, '40m', 'LSB')
  })

  it('APRS uses the APRS verb alone', async () => {
    await expect(tuneForRecall(plan('aprs', 'FM', 144.39), '2m')).resolves.toBe('aprs')
    expect(aprsTune).toHaveBeenCalledExactlyOnceWith(144.39)
    expect(workSpot).not.toHaveBeenCalled()
    expect(sstvTune).not.toHaveBeenCalled()
  })

  it.each([
    ['phone', 'phone'],
    ['cw', 'cw'],
    ['operate', 'digital'],
    ['rtty', 'rtty'],
    ['psk', 'keyboard'],
    ['jtty', 'jtty'],
  ] as const)('%s tells work_spot section %s, with no tier', async (view, section) => {
    await expect(tuneForRecall(plan(view, 'USB', 14.08), '20m')).resolves.toBe('work')
    expect(workSpot).toHaveBeenCalledExactlyOnceWith(section, 14.08, '20m', undefined, undefined)
    expect(sstvTune).not.toHaveBeenCalled()
    expect(aprsTune).not.toHaveBeenCalled()
  })

  it('JS8 names its tier on the same call, as a Needed row of JS8 does', async () => {
    await tuneForRecall(plan('js8', 'JS8', 14.078), '20m')
    expect(workSpot).toHaveBeenCalledExactlyOnceWith('js8', 14.078, '20m', undefined, 'JS8')
  })
})

describe('what riding Phone means for a recall', () => {
  it('Phone and SSTV take a sideband; nothing else does', () => {
    expect(ridesPhone('phone')).toBe(true)
    expect(ridesPhone('sstv')).toBe(true)
    for (const v of ['operate', 'cw', 'rtty', 'psk', 'jtty', 'js8', 'aprs'] as const) {
      expect(ridesPhone(v)).toBe(false)
    }
  })

  it.each([
    ['phone', 'USB', false],
    ['phone', 'LSB', false],
    ['phone', 'FM', false],
    ['phone', 'AM', true],
    ['phone', 'NFM', true], // today's word, unchanged: Phone does not hold FM for it
    ['sstv', 'USB', false],
    ['sstv', 'FM', false],
    ['sstv', 'NFM', false], // SSTV holds FM for it
    ['sstv', 'AM', true],
    ['aprs', 'FM', false],
    ['cw', 'CW', false],
    ['operate', 'FT8', false],
    ['rtty', 'RTTY', false],
  ] as const)('%s %s: told to set the mode by hand = %s', (view, mode, want) => {
    expect(modeNotCommanded(plan(view, mode))).toBe(want)
  })
})

describe('a pop-out asking the main window to open a screen', () => {
  beforeEach(() => localStorage.clear())

  it('the ask reads back as the screen', () => {
    askMainWindowToOpen('sstv')
    expect(openScreenRequest(localStorage.getItem(MEMORY_OPEN_SCREEN_KEY))).toBe('sstv')
    askMainWindowToOpen('aprs')
    expect(openScreenRequest(localStorage.getItem(MEMORY_OPEN_SCREEN_KEY))).toBe('aprs')
  })

  it('a second ask for the same screen is a new value, so another storage event fires', () => {
    const now = vi.spyOn(Date, 'now')
    now.mockReturnValue(1000)
    askMainWindowToOpen('sstv')
    const first = localStorage.getItem(MEMORY_OPEN_SCREEN_KEY)
    now.mockReturnValue(2000)
    askMainWindowToOpen('sstv')
    expect(localStorage.getItem(MEMORY_OPEN_SCREEN_KEY)).not.toBe(first)
    now.mockRestore()
  })

  it('anything else in the key opens nothing', () => {
    for (const stored of [null, '', 'sstv', '{', '{}', '{"screen":"settings"}', '{"screen":"phone"}', '{"screen":7}', 'null']) {
      expect(openScreenRequest(stored)).toBeNull()
    }
  })
})
