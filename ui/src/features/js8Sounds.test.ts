// JS8Call's notification sounds (features/js8Sounds): which frame says what, the first poll learning
// what is already there, a new call waiting for the log, and only the enabled sounds, once each; a
// message filed for me; the operator's WAV file for a sound, and the built-in tone when it cannot play.
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
import * as alerts from '../alerts'
import * as api from '../api'
import * as toast from '../toast'

vi.mock('../alerts', () => ({ beep: vi.fn(), doubleBeep: vi.fn(), ensureCtx: vi.fn(() => null) }))
vi.mock('../api', () => ({ readJs8SoundFile: vi.fn() }))
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

describe('the sound a sound plays: the operator’s WAV file, else the built-in tone', () => {
  const started: unknown[] = []
  const clip = (bytes: ArrayBuffer) => ({ fromBytes: new Uint8Array(bytes)[0] })
  const ctx = {
    destination: { id: 'speakers' },
    decodeAudioData: vi.fn(async (bytes: ArrayBuffer) => {
      if (new Uint8Array(bytes)[0] === 0xff) throw new DOMException('Unable to decode audio data', 'EncodingError')
      return clip(bytes)
    }),
    createBufferSource: () => {
      const node = {
        buffer: null as unknown,
        connected: null as unknown,
        connect(to: unknown) {
          node.connected = to
        },
        start() {
          started.push([node.buffer, node.connected])
        },
      }
      return node
    },
  }
  const bytes = (first: number) => new Uint8Array([first, 1, 2, 3]).buffer

  beforeEach(() => {
    __resetJs8SoundsForTest()
    started.length = 0
    vi.mocked(alerts.beep).mockClear()
    vi.mocked(alerts.doubleBeep).mockClear()
    vi.mocked(toast.pushToast).mockClear()
    vi.mocked(api.readJs8SoundFile).mockReset()
    ctx.decodeAudioData.mockClear()
    vi.mocked(alerts.ensureCtx).mockReturnValue(ctx as unknown as AudioContext)
  })

  it('plays the file through the alerts’ audio and no tone; each file is read once', async () => {
    vi.mocked(api.readJs8SoundFile).mockResolvedValue(bytes(7))
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    expect(api.readJs8SoundFile).toHaveBeenCalledWith('/home/op/sounds/cq.wav')
    expect(started).toEqual([[{ fromBytes: 7 }, { id: 'speakers' }]])
    expect(alerts.beep, 'no tone over the file').not.toHaveBeenCalled()
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    expect(started).toHaveLength(2)
    expect(api.readJs8SoundFile, 'read once').toHaveBeenCalledTimes(1)
    expect(toast.pushToast).not.toHaveBeenCalled()
  })

  it('no file: the built-in tone, a message for me ringing twice', async () => {
    await playJs8Sound('cq')
    await playJs8Sound('inbox', '  ')
    expect(alerts.beep).toHaveBeenCalledTimes(1)
    expect(alerts.doubleBeep).toHaveBeenCalledTimes(1)
    expect(api.readJs8SoundFile).not.toHaveBeenCalled()
    expect(started).toEqual([])
  })

  it('a file that cannot play plays the tone instead and says so once, naming the file and why', async () => {
    vi.mocked(api.readJs8SoundFile).mockRejectedValue('missing')
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    await playJs8Sound('cq', '/home/op/sounds/cq.wav')
    expect(alerts.beep, 'the tone each time').toHaveBeenCalledTimes(2)
    expect(started).toEqual([])
    expect(toast.pushToast).toHaveBeenCalledTimes(1)
    expect(vi.mocked(toast.pushToast).mock.calls[0]).toEqual([
      'cq.wav could not be played (it is not there), so the built-in tone played instead.',
      'error',
      12_000,
    ])
    // Another sound with the same file is named once too, and Settings' Test names it every time.
    await playJs8Sound('hb', '/home/op/sounds/cq.wav')
    await playJs8Sound('hb', '/home/op/sounds/cq.wav', true)
    expect(toast.pushToast).toHaveBeenCalledTimes(3)
  })

  it('names each reason in the operator’s words', async () => {
    const said = async (read: () => Promise<ArrayBuffer>) => {
      vi.mocked(toast.pushToast).mockClear()
      vi.mocked(api.readJs8SoundFile).mockImplementation(read)
      await playJs8Sound('ack', 'C:\\Sounds\\ack.wav', true)
      return String(vi.mocked(toast.pushToast).mock.calls[0]?.[0])
    }
    expect(await said(() => Promise.reject('unreadable'))).toContain('(this computer would not let Nexus read it)')
    expect(await said(() => Promise.reject('tooLarge'))).toContain('(it is larger than 10 MB)')
    expect(await said(() => Promise.reject('notWav'))).toContain('(it is not a WAV file)')
    expect(await said(() => Promise.resolve(bytes(0xff)))).toContain('(this computer could not decode it)')
    expect(await said(() => Promise.resolve(bytes(0xff)))).toMatch(/^ack\.wav could not be played/)
  })

  it('names a file by its last part, from either kind of path', () => {
    expect(js8SoundFileName('/home/op/sounds/cq.wav')).toBe('cq.wav')
    expect(js8SoundFileName('C:\\Sounds\\mail.wav')).toBe('mail.wav')
    expect(js8SoundFileName('hb.wav')).toBe('hb.wav')
  })
})
