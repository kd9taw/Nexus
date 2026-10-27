// @vitest-environment jsdom
//
// JS8CALL'S SOUNDS FROM ANY SCREEN (components/Js8SoundPlayer). JS8Call plays a notification from its
// decode and command paths, whatever its window shows; the player reads the engine itself, so no
// screen decides whether a sound plays. It reads only while JS8 is the mode and a sound is on.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry, Js8State } from '../types'
import { Js8SoundPlayer } from './Js8SoundPlayer'
import { __resetJs8SoundsForTest } from '../features/js8Sounds'
import * as alerts from '../alerts'
import * as api from '../api'

vi.mock('../alerts', () => ({ beep: vi.fn(), doubleBeep: vi.fn(), ensureCtx: vi.fn(() => null) }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))
vi.mock('../api', () => ({
  getJs8State: vi.fn(),
  askLog: vi.fn(async () => ({})),
  readJs8SoundFile: vi.fn(),
}))

const frame = (from: string, text: string, atMs: number, o: Partial<Js8ActivityRow> = {}): Js8ActivityRow => ({
  atMs,
  speed: 'normal',
  freqHz: 1500,
  snrDb: -5,
  dtS: 0,
  from,
  text: `${from}: ${text}`,
  directedToMe: false,
  mine: false,
  complete: true,
  lowConf: false,
  flags: { first: true, last: true, assembled: false },
  ...o,
})
const heard = (call: string): Js8Heard => ({ call, grid: null, snrDb: 0, freqHz: 1500, speed: 'normal', lastMs: 1, lastHb: false, lastCq: false, storedMsgs: 0 })
const mail = (id: number): Js8InboxEntry => ({ id, from: 'W1AW', to: 'KD9TAW', text: 'HI', path: ['W1AW'], state: 'unread', atMs: id, freqHz: 1500, snrDb: -5 })
const engine = (o: Partial<Js8State> = {}): Js8State => ({ activity: [], stations: [], inbox: [], ...o }) as unknown as Js8State

let state: Js8State
const tones = () => [...vi.mocked(alerts.beep).mock.calls.map((c) => c[0]), ...vi.mocked(alerts.doubleBeep).mock.calls.map((c) => -c[0])]

beforeEach(() => {
  vi.useFakeTimers()
  __resetJs8SoundsForTest()
  vi.clearAllMocks()
  state = engine({ activity: [frame('W1AW', '@ALLCALL CQ CQ CQ FN31 ', 1)], stations: [heard('W1AW')], inbox: [mail(1)] })
  vi.mocked(api.getJs8State).mockImplementation(async () => state)
  vi.mocked(api.askLog).mockImplementation(async () => ({}))
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

/** One of the player's reads (once a second), and what it starts. */
async function poll() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000)
  })
}
async function mount(props: Partial<Parameters<typeof Js8SoundPlayer>[0]> = {}) {
  const r = render(<Js8SoundPlayer tier="JS8" sounds={['cq', 'hb', 'inbox']} files={{}} myCall="KD9TAW" groups={[]} logTick={0} {...props} />)
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0)
  })
  return r
}

describe('the JS8 sounds, from whichever screen is showing', () => {
  it('plays what is heard after it starts, once each, and nothing it found already there', async () => {
    await mount()
    await poll()
    expect(tones(), 'the CQ and the message from before').toEqual([])
    state = engine({
      ...state,
      activity: [...state.activity, frame('K1HB', '@HB HEARTBEAT FN42 ', 2), frame('N0CQ', '@ALLCALL CQ CQ CQ EM10 ', 3)],
      inbox: [mail(1), mail(2)],
    })
    await poll()
    // JS8Call's table order: CQ, HB, then the inbox (it rings twice).
    expect(tones()).toEqual([587, 466, -784])
    await poll()
    expect(tones(), 'nothing new: nothing again').toEqual([587, 466, -784])
  })

  it('plays only the sounds switched on', async () => {
    await mount({ sounds: ['hb'] })
    state = engine({ ...state, activity: [...state.activity, frame('K1HB', '@HB HEARTBEAT FN42 ', 2), frame('N0CQ', '@ALLCALL CQ CQ CQ EM10 ', 3)] })
    await poll()
    expect(tones()).toEqual([466])
  })

  it('reads nothing while every sound is off (the default) or JS8 is not the mode, and stops when that changes', async () => {
    const r = await mount({ sounds: [] })
    await poll()
    r.rerender(<Js8SoundPlayer tier="FT8" sounds={['cq']} files={{}} myCall="KD9TAW" groups={[]} logTick={0} />)
    await poll()
    expect(api.getJs8State, 'never asked').not.toHaveBeenCalled()
    r.rerender(<Js8SoundPlayer tier="JS8" sounds={['cq']} files={{}} myCall="KD9TAW" groups={[]} logTick={0} />)
    await poll()
    const asked = vi.mocked(api.getJs8State).mock.calls.length
    expect(asked, 'JS8 and a sound on: it reads').toBeGreaterThan(0)
    r.rerender(<Js8SoundPlayer tier="FT8" sounds={['cq']} files={{}} myCall="KD9TAW" groups={[]} logTick={0} />)
    await poll()
    await poll()
    expect(vi.mocked(api.getJs8State).mock.calls.length, 'left JS8: no more reads').toBe(asked)
  })

  it('a new call sounds once the log has said whether it is worked: new, or worked before', async () => {
    vi.mocked(api.askLog).mockImplementation((async (q: { kind: string; calls?: string[] }) =>
      q.kind === 'callsSummary' && q.calls?.includes('K1OLD') ? { K1OLD: { count: 2, lastUnix: 1, grid: '', name: '', comment: '' } } : {}) as unknown as typeof api.askLog)
    await mount({ sounds: ['callNew', 'callOld'] })
    await poll()
    expect(tones(), 'W1AW was listed before').toEqual([])
    state = engine({ ...state, stations: [heard('W1AW'), heard('N0NEW'), heard('K1OLD')] })
    await poll()
    await poll()
    expect(tones().sort()).toEqual([554, 698])
  })

  it('a call heard after a first read that listed nobody sounds too', async () => {
    state = engine({ ...state, stations: [] })
    await mount({ sounds: ['callNew'] })
    await poll()
    state = engine({ ...state, stations: [heard('N0NEW')] })
    await poll()
    await poll()
    expect(tones()).toEqual([698])
  })

  it('a sound with a WAV file plays the file', async () => {
    vi.mocked(api.readJs8SoundFile).mockRejectedValue('missing')
    await mount({ sounds: ['cq'], files: { cq: '/home/op/sounds/cq.wav' } })
    vi.mocked(alerts.ensureCtx).mockReturnValue({} as AudioContext)
    state = engine({ ...state, activity: [...state.activity, frame('N0CQ', '@ALLCALL CQ CQ CQ EM10 ', 3)] })
    await poll()
    expect(api.readJs8SoundFile).toHaveBeenCalledWith('/home/op/sounds/cq.wav')
    expect(tones(), 'and the tone when the file cannot play').toEqual([587])
  })
})
