// The road to a station on this network, as the stream view uses it: what goes out on the page's
// socket, and what each of the station's messages reaches. The page holds no key: its offer goes
// unsigned (this computer's Nexus signs it on its way), and the answer is checked twice, there and
// here, against the key this computer pinned.
import { afterEach, expect, it, vi } from 'vitest'
import { LanConnection, LAN_OPERATION_VERSION, lanStream } from './connection'
import { harness, LEASE, ANSWER, answerChecked } from '../remote-web/stream-link.testkit'
import { parseFrame } from '../remote-monitor/protocol'
import type { LanRoad } from './protocol'

// THE BUDGET (2026-10-09). The slowest case here, "asks the station at its lane’s own operation version…", takes
// 1.01 s and 0.76 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const ROAD: LanRoad = {
  stationId: '60000000-0000-4000-8000-000000000001',
  deviceId: '1d2b7c3a-4e5f-8a6b-9c7d-0e1f2a3b4c5d',
  sessionId: '50000000-0000-4000-8000-000000000002',
  stationKey: `3059301306072a8648ce3d020106082a8648ce3d03010703420004${'ab'.repeat(64)}`,
  address: '192.168.1.20:42075',
}

afterEach(() => vi.useRealTimers())

function road() {
  const sent: Record<string, unknown>[] = []
  const h = harness()
  const lan = new LanConnection(text => sent.push(JSON.parse(text)), ROAD, h.env)
  return { lan, sent, h }
}

it('asks the station at its lane’s own operation version, on the page’s socket', async () => {
  const { lan, sent } = road()
  // The operation client reads the state once it opens, on its own clock.
  await vi.waitFor(() => expect(sent.length).toBeGreaterThan(0), { timeout: 2000 })
  expect(sent[0]).toMatchObject({ type: 'operationRequest', operationVersion: LAN_OPERATION_VERSION, request: { type: 'state' } })
  expect(LAN_OPERATION_VERSION).toBe(4)
  lan.dispose()
})

it('sends the offer unsigned and asks for no relay, for this computer’s Nexus to sign it on its way', async () => {
  const sent: Record<string, unknown>[] = []
  const relay = vi.fn(async () => ({ urls: ['turn:turn.example.invalid:3478'], username: 'u', credential: 'c' }))
  const h = harness({ relay })
  const lan = new LanConnection(text => sent.push(JSON.parse(text)), ROAD, h.env)
  await lan.stream.start(LEASE)
  const offer = sent.find(m => m.type === 'streamSignal') as { leaseId: string; payload: Record<string, unknown> }
  expect(offer.leaseId).toBe(LEASE)
  expect(offer.payload.kind).toBe('offer')
  expect(offer.payload).not.toHaveProperty('publicKey')
  expect(offer.payload).not.toHaveProperty('signature')
  expect(relay).not.toHaveBeenCalled()
  expect(h.peers).toHaveLength(1)
  lan.dispose()
})

it('builds the browser’s peer with no ICE server at all: no STUN, no relay, nothing off this network', () => {
  const built: unknown[] = []
  vi.stubGlobal('RTCPeerConnection', class { constructor(config: unknown) { built.push(config) } })
  try {
    lanStream().peer([{ urls: 'stun:stun.cloudflare.com:3478' }])
    expect(built).toHaveLength(1)
    expect((built[0] as { iceServers: unknown[] }).iceServers).toEqual([])
  } finally { vi.unstubAllGlobals() }
})

it('checks the station’s answer itself too, against the key this computer pinned, and refuses one it cannot hold', async () => {
  const { lan } = road()
  await lan.stream.start(LEASE)
  // The contract's answer was never signed by this road's pinned key.
  lan.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }, 0)
  await answerChecked()
  await vi.waitFor(() => expect(lan.stream.getSnapshot().phase).toBe('ended'))
  expect(lan.stream.getSnapshot().reason).toBe('stationNotSigned')
  lan.dispose()
})

it('ends the stream saying why when this computer’s Nexus refused the answer before the page saw it', async () => {
  const { lan } = road()
  await lan.stream.start(LEASE)
  expect(lan.stream.getSnapshot().phase).toBe('connecting')
  lan.answerRefused('stationKeyMismatch')
  expect(lan.stream.getSnapshot()).toMatchObject({ phase: 'ended', reason: 'stationKeyMismatch' })
  lan.dispose()
})

it('hands the view the station’s status line as the one reading a frame carries, and nothing before it', async () => {
  const { lan } = road()
  const signal = new AbortController().signal
  await expect(lan.source.read(signal)).rejects.toThrow()
  lan.receive({ type: 'status', rigKeyed: true }, 0)
  const keyed = parseFrame(await lan.source.read(signal), 'native')
  expect(keyed.station.radio.rigKeyed).toBe(true)
  lan.receive({ type: 'status', rigKeyed: false }, 0)
  const next = parseFrame(await lan.source.read(signal), 'native')
  expect(next.station.radio.rigKeyed).toBe(false)
  expect(next.sequence).toBeGreaterThan(keyed.sequence)
  lan.receive({ type: 'status', rigKeyed: null }, 0)
  expect(parseFrame(await lan.source.read(signal), 'native').station.radio.rigKeyed).toBeNull()
  lan.dispose()
})

it('says nothing more once the road has closed, and every part knows it', async () => {
  const { lan, sent } = road()
  lan.closed()
  expect(lan.operations.getSnapshot().connected).toBe(false)
  const before = sent.length
  await expect(lan.operations.stopTransmit()).rejects.toThrow()
  await lan.stream.start(LEASE)
  expect(sent.length).toBe(before)
  await expect(lan.source.read(new AbortController().signal)).rejects.toThrow()
  lan.dispose()
})
