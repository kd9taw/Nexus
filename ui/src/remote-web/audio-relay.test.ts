import { expect, it, vi } from 'vitest'
import { AudioRelay } from './audio-relay'
import { AUDIO_MESSAGE_BYTES, audioPackets, parseAudioBundle } from './audio-protocol'

const lease = '8aa041cb-c642-459c-83f3-11a5b720647d'
const session = 'bbe7d95a-6fbd-47aa-95a7-ab1e4c083dd6'
const peer = () => ({ send: vi.fn<(s: string) => void>(), close: vi.fn<(c: number, r: string) => void>() })
// Three 2-byte packets, each behind its own big-endian length. Built rather than
// pasted so the positive control below is decoding real framing.
const PAYLOAD = btoa(String.fromCharCode(0, 2, 0xaa, 0xbb, 0, 2, 0xcc, 0xdd, 0, 2, 0xee, 0xff))
const bundle = (seq = 0, payload = PAYLOAD) =>
  ({ type: 'audioRx', sessionId: session, seq, epoch: '0000000000000001', firstFrameMs: seq * 60, frameMs: 20, count: 3, payload })
function setup(supported = true) {
  const station = peer(), browser = peer(), relay = new AudioRelay()
  relay.sync({ peer: station, supported }, [{ sessionId: session, deviceId: 'device', peer: browser }])
  return { station, browser, relay }
}

it('stamps the browser identity itself and never reads one off the message', () => {
  const { station, relay } = setup()
  relay.receiveBrowser(session, { type: 'audioListen', listening: true, leaseId: lease })
  expect(JSON.parse(station.send.mock.lastCall![0]))
    .toEqual({ type: 'audioListen', sessionId: session, deviceId: 'device', listening: true, leaseId: lease })
})

it('refuses a listen request that carries an identity of its own', () => {
  // The stamping above is only load-bearing if a browser cannot smuggle a second
  // identity past it. Exact-field parsing is what holds that down, so it is asserted
  // here rather than trusted: without it the stamped value could be overridden.
  const { station, browser, relay } = setup()
  relay.receiveBrowser(session, { type: 'audioListen', listening: true, leaseId: lease, deviceId: 'somebody-else' })
  expect(station.send).not.toHaveBeenCalled()
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidAudio')
})

it('tells a browser the lane is unavailable rather than handing an old station a message it cannot parse', () => {
  const { station, browser, relay } = setup(false)
  relay.receiveBrowser(session, { type: 'audioListen', listening: true, leaseId: lease })
  expect(station.send).not.toHaveBeenCalled()
  expect(JSON.parse(browser.send.mock.lastCall![0])).toEqual({ type: 'audioState', listening: false, reason: 'audioUnavailable' })
  expect(browser.close).not.toHaveBeenCalled()
})

it('forwards a bundle to its session with the routing id stripped, and never decodes the payload', () => {
  const { browser, relay } = setup()
  relay.receiveStation(bundle(7))
  const forwarded = JSON.parse(browser.send.mock.lastCall![0]) as Record<string, unknown>
  expect(forwarded.sessionId).toBeUndefined()
  expect(parseAudioBundle(forwarded).seq).toBe(7)
  // Positive control on the same instrument: the payload really is a decodable bundle,
  // so "the relay did not decode it" is a statement about the relay, not about the data.
  expect(audioPackets(Uint8Array.from(atob(forwarded.payload as string), c => c.charCodeAt(0)), 3)).toHaveLength(3)
})

it('refuses an oversized bundle instead of forwarding it', () => {
  const { station, browser, relay } = setup()
  relay.receiveStation(bundle(1, 'A'.repeat(AUDIO_MESSAGE_BYTES)))
  expect(browser.send).not.toHaveBeenCalled()
  expect(station.close).toHaveBeenCalledWith(1008, 'invalidAudio')
  // Positive control: the same shape inside the bound does forward.
  const fresh = setup()
  fresh.relay.receiveStation(bundle(1))
  expect(fresh.browser.send).toHaveBeenCalled()
  expect(fresh.station.close).not.toHaveBeenCalled()
})

it('drops audio for a listener that has gone rather than holding it', () => {
  const { browser, relay } = setup()
  relay.sync({ peer: peer(), supported: true }, [])
  relay.receiveStation(bundle(2))
  expect(browser.send).not.toHaveBeenCalled()
  expect(relay.dropped).toBe(1)
})

it('drops rather than queues when a browser cannot take data, and gives up on it bounded', () => {
  const { browser, relay } = setup()
  browser.send.mockImplementation(() => { throw Error('backpressure') })
  for (let seq = 0; seq < 49; seq++) relay.receiveStation(bundle(seq))
  expect(relay.dropped).toBe(49)
  expect(browser.close).not.toHaveBeenCalled()
  relay.receiveStation(bundle(49))
  expect(browser.close).toHaveBeenCalledWith(1011, 'audioBacklog')
})

it('forgets a delivery failure once the browser takes a bundle again', () => {
  const { browser, relay } = setup()
  browser.send.mockImplementationOnce(() => { throw Error('backpressure') })
  relay.receiveStation(bundle(0))
  relay.receiveStation(bundle(1))
  for (let seq = 2; seq < 51; seq++) relay.receiveStation(bundle(seq))
  expect(browser.close).not.toHaveBeenCalled()
})

it('closes a browser that sends a malformed listen request', () => {
  const { station, browser, relay } = setup()
  relay.receiveBrowser(session, { type: 'audioListen', listening: true, leaseId: 'not-a-lease' })
  expect(station.send).not.toHaveBeenCalled()
  expect(browser.close).toHaveBeenCalledWith(1008, 'invalidAudio')
})

it('closes a station that sends an unroutable or malformed frame', () => {
  const { station, relay } = setup()
  relay.receiveStation({ ...bundle(0), sessionId: 'nope' })
  expect(station.close).toHaveBeenCalledWith(1008, 'invalidAudio')
})

it('ignores audio from a session that is not a known browser without disturbing the station', () => {
  const { station, relay } = setup()
  relay.receiveBrowser('someone-else', { type: 'audioListen', listening: true, leaseId: lease })
  expect(station.send).not.toHaveBeenCalled()
  expect(station.close).not.toHaveBeenCalled()
})
