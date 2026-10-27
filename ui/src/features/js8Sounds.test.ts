// JS8Call's notification sounds (features/js8Sounds): which frame says what, the first poll learning
// what is already there, a new call waiting for the log, and only the enabled sounds, once each; a
// message filed for me; the operator's WAV file for a sound, the built-in tone when it cannot play, and
// what is said when nothing plays (the output is a TX audio device).
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { Js8ActivityRow, Js8Heard, Js8InboxEntry } from '../types'
import {
  JS8_SOUND_EVENTS,
  Js8SoundWatch,
  __resetJs8SoundsForTest,
  js8FrameEvent,
  js8SoundFileName,
  js8SoundsToPlay,
  playJs8Sound,
} from './js8Sounds'
import * as api from '../api'
import * as toast from '../toast'

vi.mock('../api', () => ({ playJs8SoundHere: vi.fn() }))
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

const row = (text: string, o: Partial<Js8ActivityRow> = {}): Js8ActivityRow => ({
  atMs: 1,
  speed: 'normal',
  freqHz: 1500,
  snrDb: -5,
  dtS: 0,
  from: text.split(':')[0],
  text,
  directedToMe: false,
  mine: false,
  complete: true,
  lowConf: false,
  flags: { first: true, last: true, assembled: false },
  ...o,
})
const heard = (call: string): Js8Heard => ({
  call,
  grid: null,
  snrDb: 0,
  freqHz: 1500,
  speed: 'normal',
  lastMs: 1,
  lastHb: false,
  lastCq: false,
  storedMsgs: 0,
})

describe('what a frame announces', () => {
  it('names a CQ, a heartbeat, an ACK for me and a message to me or my group, and nothing else', () => {
    const ev = (text: string, o: Partial<Js8ActivityRow> = {}) => js8FrameEvent(row(text, o), 'KD9TAW', ['@FUN'])
    expect(ev('W1AW: @ALLCALL CQ CQ CQ FN31 ')).toBe('cq')
    expect(ev('W1AW: @HB HEARTBEAT FN31 ')).toBe('hb')
    expect(ev('W1AW: KD9TAW ACK ', { directedToMe: true })).toBe('ack')
    expect(ev('W1AW: KD9TAW SNR? ', { directedToMe: true })).toBe('directed')
    expect(ev('W1AW: KD9TAW/P MSG ', { directedToMe: true }), 'my call portable').toBe('directed')
    expect(ev('W1AW: @FUN SNR? ', { directedToMe: true })).toBe('directed')
    expect(ev('W1AW: @ALLCALL SNR? ', { directedToMe: true }), '@ALLCALL is not directed to me').toBeNull()
    expect(ev('W1AW: K1ABC SNR? ')).toBeNull()
    expect(ev('KD9TAW: @ALLCALL CQ CQ CQ EN61 ', { mine: true }), 'my own').toBeNull()
    expect(ev('HELLO WORLD', { from: '' }), 'a data frame').toBeNull()
    expect(ev('W1AW: @ALLCALL CQ', { flags: { first: false, last: false, assembled: true } }), 'the reassembled line').toBeNull()
  })
})

describe('the watch', () => {
  it('learns what the first poll holds, then says each new frame once', () => {
    const w = new Js8SoundWatch()
    const cq = row('W1AW: @ALLCALL CQ CQ CQ FN31 ')
    expect(w.frames([cq], 'KD9TAW', [])).toEqual([])
    const hb = row('K1ABC: @HB HEARTBEAT FN42 ', { atMs: 2 })
    expect(w.frames([cq, hb], 'KD9TAW', [])).toEqual(['hb'])
    expect(w.frames([cq, hb], 'KD9TAW', [])).toEqual([])
  })

  it('says a new call once the log has answered, old or new, and not the calls already listed', () => {
    const w = new Js8SoundWatch()
    expect(w.newCalls([heard('W1AW')], () => true)).toEqual([])
    const answers = new Map<string, boolean | null>([['K1ABC', null], ['N0NEW', false]])
    expect(w.newCalls([heard('W1AW'), heard('K1ABC'), heard('N0NEW')], (c) => answers.get(c) ?? null)).toEqual(['callNew'])
    answers.set('K1ABC', true)
    expect(w.newCalls([heard('W1AW'), heard('K1ABC'), heard('N0NEW')], (c) => answers.get(c) ?? null)).toEqual(['callOld'])
    expect(w.newCalls([heard('W1AW'), heard('K1ABC'), heard('N0NEW')], (c) => answers.get(c) ?? null)).toEqual([])
  })
})

it('plays the enabled sounds of a poll, each once, in JS8Call’s table order; none when none is on', () => {
  expect(JS8_SOUND_EVENTS).toEqual(['cq', 'hb', 'ack', 'directed', 'inbox', 'callNew', 'callOld'])
  expect(js8SoundsToPlay(['inbox', 'cq', 'cq', 'hb'], ['cq', 'inbox'])).toEqual(['cq', 'inbox'])
  expect(js8SoundsToPlay(['inbox', 'cq'], [])).toEqual([])
})

describe('a message filed for me', () => {
  const entry = (id: number, state: Js8InboxEntry['state'] = 'unread'): Js8InboxEntry => ({
    id,
    from: 'W1AW',
    to: 'KD9TAW',
    text: 'HELLO',
    path: ['W1AW'],
    state,
    atMs: id,
    freqHz: 1500,
    snrDb: -5,
  })

  it('learns what the first poll holds, then says "inbox" once for each poll that files an unread message', () => {
    const w = new Js8SoundWatch()
    expect(w.inbox([entry(1)]), 'mail from before the watch started').toEqual([])
    expect(w.inbox([entry(1), entry(2), entry(3)]), 'two filed in one poll: one sound').toEqual(['inbox'])
    expect(w.inbox([entry(1), entry(2), entry(3)])).toEqual([])
    expect(w.inbox([entry(1), entry(2), entry(3), entry(4, 'read')]), 'one already read').toEqual([])
    expect(w.inbox([entry(2), entry(3), entry(4, 'read')]), 'one deleted').toEqual([])
  })
})

describe('the sound a sound plays: the operator’s WAV file, else the built-in tone, on the operator’s output', () => {
  const play = vi.mocked(api.playJs8SoundHere)
  beforeEach(() => {
    __resetJs8SoundsForTest()
    vi.mocked(toast.pushToast).mockReset()
    play.mockReset()
    play.mockResolvedValue({ fileRefused: null })
  })
  const said = () => vi.mocked(toast.pushToast).mock.calls.map((c) => String(c[0]))

  it('asks this computer to play the event with its file, or with none for the tone', async () => {
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    await playJs8Sound('inbox', '  ')
    await playJs8Sound('hb')
    expect(play.mock.calls).toEqual([
      ['cq', '/home/op/sounds/cq.wav'],
      ['inbox', null],
      ['hb', null],
    ])
    expect(toast.pushToast).not.toHaveBeenCalled()
  })

  it('a file that fell back to the tone is named once, with why', async () => {
    play.mockResolvedValue({ fileRefused: 'missing' })
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    expect(said()).toEqual(['cq.wav could not be played (it is not there), so the built-in tone played instead.'])
    expect(vi.mocked(toast.pushToast).mock.calls[0].slice(1)).toEqual(['error', 12_000])
    // Another sound with the same file is named once too, and Settings' Test names it every time.
    await playJs8Sound('hb', '/home/op/sounds/cq.wav')
    await playJs8Sound('hb', '/home/op/sounds/cq.wav', true)
    expect(toast.pushToast).toHaveBeenCalledTimes(3)
  })

  it('names each reason in the operator’s words', async () => {
    const reason = async (fileRefused: string) => {
      vi.mocked(toast.pushToast).mockClear()
      play.mockResolvedValue({ fileRefused })
      await playJs8Sound('ack', 'C:\\Sounds\\ack.wav', true)
      return said()[0]
    }
    expect(await reason('unreadable')).toContain('(this computer would not let Nexus read it)')
    expect(await reason('tooLarge')).toContain('(it is larger than 10 MB)')
    expect(await reason('notWav')).toContain('(it is not a WAV file)')
    expect(await reason('notAFile')).toContain('(it is not a file)')
    expect(await reason('badFormat')).toContain('(its format is not one Nexus can play, or it is cut short)')
    expect(await reason('notPicked')).toContain('(it was not chosen for this sound in Settings)')
  })

  it('nothing played because the output is a TX audio device: said once, every time from Test, never another output', async () => {
    play.mockRejectedValue('txDevice')
    await playJs8Sound('cq')
    await playJs8Sound('hb', '/home/op/sounds/hb.wav')
    expect(said()).toEqual([
      'A JS8 sound did not play: its output is a device your radio transmits from, and a sound there could key the radio on VOX. Choose another output in Settings ▸ Digital ▸ JS8 ▸ Sounds.',
    ])
    expect(play, 'asked once per sound, never again elsewhere').toHaveBeenCalledTimes(2)
    await playJs8Sound('cq', null, true)
    expect(toast.pushToast).toHaveBeenCalledTimes(2)
    play.mockRejectedValue('noDevice')
    await playJs8Sound('cq')
    expect(said()[2]).toContain('its output is not on this computer now')
  })

  it('names a file by its last part, from either kind of path', () => {
    expect(js8SoundFileName('/home/op/sounds/cq.wav')).toBe('cq.wav')
    expect(js8SoundFileName('C:\\Sounds\\mail.wav')).toBe('mail.wav')
    expect(js8SoundFileName('hb.wav')).toBe('hb.wav')
  })
})
