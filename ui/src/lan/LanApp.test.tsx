// @vitest-environment jsdom
//
// The Stations on this network window's page, against a socket of the test's own: every reason
// this computer's Nexus gives is said in a sentence, the pairing dialog sends what the operator
// typed, a station opens its stream, and control taken back by the station (a decision about the
// hosted road there) is acquired again once, by the page, when nobody else holds it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { LanApp, type PageSocket } from './LanApp'
import { CLOSED_REASONS, CONNECT_REASONS, PAIR_REASONS } from './protocol'
import { EN } from '../i18n/en'
import { harness, last } from '../remote-web/stream-link.testkit'

// THE BUDGET (2026-10-05). The stream's lease tests wait on the page's own clocks: a heartbeat, the state
// reads after a refused one, and then 1.5 s more to show nothing is acquired. On a quiet box the slowest takes
// 4.5–4.8 s against vitest's 5 s default (CI timed it out at 5.0 s), and the waits it allows itself add up to
// about 13.5 s. 15 s covers them; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const PAGE = `http://127.0.0.1:42076/${'5e'.repeat(32)}/lan.html?lang=en`
const STATION = '60000000-0000-4000-8000-000000000001'
const ROAD = {
  stationId: STATION, deviceId: '1d2b7c3a-4e5f-8a6b-9c7d-0e1f2a3b4c5d', sessionId: '50000000-0000-4000-8000-000000000002',
  stationKey: `3059301306072a8648ce3d020106082a8648ce3d03010703420004${'ab'.repeat(64)}`, address: '192.168.1.20:42075',
}
const BOOT = '0f7d1c2e-5b3a-4c1d-9e8f-7a6b5c4d3e2f'
const LEASE = '30000000-0000-4000-8000-000000000003'

class FakeSocket implements PageSocket {
  sent: Record<string, any>[] = []
  url = ''
  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onclose: (() => void) | null = null
  send(text: string) { this.sent.push(JSON.parse(text)) }
  close() {}
  tell(value: Record<string, unknown>) { act(() => { this.onmessage?.({ data: JSON.stringify(value) }) }) }
  /** The operation requests sent so far, by type. */
  asked(type: string) { return this.sent.filter(m => m.type === 'operationRequest' && m.request.type === type) }
  answer(request: Record<string, any>, value: unknown) {
    this.tell({ type: 'operationResponse', requestId: request.request.requestId, value })
  }
}

function state(phase: 'available' | 'controlling' | 'occupied') {
  const owned = phase === 'controlling'
  return { stationBootId: BOOT, allowed: true, phase, leaseId: owned ? LEASE : null, revision: 1,
    commandWindowId: owned ? '70000000-0000-4000-8000-000000000007' : null, nextSequence: owned ? 1 : null,
    leaseRemainingMs: owned ? 5000 : null, actions: [], txArmed: false,
    transmitEpoch: phase === 'controlling' ? '000000000000002b' : null }
}

let socket: FakeSocket
function page(environment?: () => ReturnType<typeof harness>['env']) {
  socket = new FakeSocket()
  const utils = render(<LanApp page={PAGE} open={url => { socket.url = url; return socket }} environment={environment} />)
  act(() => { socket.onopen?.() })
  return utils
}

beforeEach(() => {
  Object.defineProperty(HTMLVideoElement.prototype, 'requestVideoFrameCallback', { configurable: true, value() { return 1 } })
  Object.defineProperty(HTMLVideoElement.prototype, 'cancelVideoFrameCallback', { configurable: true, value() {} })
})
afterEach(() => {
  cleanup()
  delete (HTMLVideoElement.prototype as { requestVideoFrameCallback?: unknown }).requestVideoFrameCallback
  delete (HTMLVideoElement.prototype as { cancelVideoFrameCallback?: unknown }).cancelVideoFrameCallback
})

it('opens its socket beside the page, with the launch secret in its path, and asks for the stations', () => {
  page()
  expect(socket.url).toBe(`ws://127.0.0.1:42076/${'5e'.repeat(32)}/socket`)
  expect(socket.sent).toEqual([{ type: 'stations' }])
})

describe('every reason, in a sentence', () => {
  const REACH = ['otherNetwork', 'refused', 'noAnswer']
  const sentences = (prefix: string, reasons: readonly string[]) => reasons.map(reason =>
    [reason, EN[(reason === 'disconnected' ? 'lanWindow.disconnected'
      : REACH.includes(reason) ? `remote.lan.reach.${reason}` : `${prefix}${reason}`) as keyof typeof EN]] as const)
  it.each(sentences('lanWindow.reason.', PAIR_REASONS))('a pairing refused %s', (reason, said) => {
    page()
    socket.tell({ type: 'pairRefused', reason })
    expect(said).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toBe(said)
  })
  it.each(sentences('lanWindow.reason.', CONNECT_REASONS))('a road refused %s', (reason, said) => {
    page()
    socket.tell({ type: 'connectRefused', reason })
    expect(said).toBeTruthy()
    expect(screen.getByRole('alert').textContent).toBe(said)
  })
  it.each(sentences('lanWindow.reason.', CLOSED_REASONS))('a road closed %s', (reason, said) => {
    page()
    socket.tell({ type: 'closed', reason })
    expect(said).toBeTruthy()
    expect(screen.getByRole(reason === 'disconnected' ? 'status' : 'alert').textContent).toBe(said)
  })
  it('and a reason this computer’s Nexus never gives is no sentence at all', () => {
    page()
    socket.tell({ type: 'pairRefused', reason: 'somethingElse' })
    expect(screen.queryByRole('alert')).toBeNull()
  })
})

describe('the pairing dialog', () => {
  it('sends the code as typed, capitals and spaces, with the name Nexus gave this computer', () => {
    page()
    socket.tell({ type: 'stations', stations: [], computer: 'DEN-PC' })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.pair'] }))
    const pair = screen.getByRole('button', { name: EN['lanWindow.pairSubmit'] }) as HTMLButtonElement
    fireEvent.change(screen.getByLabelText(EN['lanWindow.address']), { target: { value: ' 192.168.1.20:42075 ' } })
    fireEvent.change(screen.getByLabelText(EN['lanWindow.code']), { target: { value: '0A1B 2C3D 4E5F 607' } })
    expect(pair.disabled, 'fifteen characters').toBe(true)
    fireEvent.change(screen.getByLabelText(EN['lanWindow.code']), { target: { value: '0A1B 2C3D 4E5F 6071' } })
    expect((screen.getByLabelText(EN['lanWindow.name']) as HTMLInputElement).value).toBe('DEN-PC')
    expect(pair.disabled).toBe(false)
    fireEvent.click(pair)
    expect(last(socket.sent)).toEqual({ type: 'pair', address: '192.168.1.20:42075', code: '0A1B 2C3D 4E5F 6071', name: 'DEN-PC' })
    expect(screen.getByRole('button', { name: EN['lanWindow.pairing'] })).toBeTruthy()
    // Paired: the dialog closes, it says so, and the list is asked for again.
    socket.tell({ type: 'paired', station: { id: STATION, addresses: ['192.168.1.20:42075'], key: '5966'.repeat(16) } })
    expect(screen.getByRole('status').textContent).toBe(EN['lanWindow.pairedNow'])
    expect(screen.queryByRole('button', { name: EN['lanWindow.pairSubmit'] })).toBeNull()
    expect(last(socket.sent)).toEqual({ type: 'stations' })
  })

  it('offers the stations found by name in its address field, and a typed address still works', () => {
    const foundAt = (name: string, address: string) => EN['lanWindow.foundAt'].replace('{{name}}', name).replace('{{address}}', address)
    page()
    socket.tell({ type: 'stations', stations: [], computer: 'DEN-PC' })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.pair'] }))
    // Opening the dialog looks for stations by name, and says so while it does.
    expect(last(socket.sent)).toEqual({ type: 'find' })
    expect(screen.getByText(EN['lanWindow.finding'])).toBeTruthy()
    socket.tell({ type: 'found', available: true, shacks: [
      { name: 'Nexus 3F2A 9B1C', address: '192.168.1.20:42075', protocol: 1, key: '3f2a9b1c00c0ffee' },
      { name: 'Nexus 00C0 FFEE', address: '192.168.1.31:42075', protocol: 1, key: '00c0ffee00c0ffee' }] })
    expect(screen.queryByText(EN['lanWindow.finding'])).toBeNull()
    const address = screen.getByLabelText(EN['lanWindow.address']) as HTMLInputElement
    const offered = within(screen.getByRole('group', { name: EN['lanWindow.found'] }))
    expect(offered.getByRole('button', { name: foundAt('Nexus 3F2A 9B1C', '192.168.1.20:42075') })).toBeTruthy()
    const shack = offered.getByRole('button', { name: foundAt('Nexus 00C0 FFEE', '192.168.1.31:42075') })
    fireEvent.click(shack)
    expect(address.value).toBe('192.168.1.31:42075')
    expect(shack.getAttribute('aria-pressed')).toBe('true')
    fireEvent.change(screen.getByLabelText(EN['lanWindow.code']), { target: { value: '0a1b2c3d4e5f6071' } })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.pairSubmit'] }))
    expect(last(socket.sent)).toEqual({ type: 'pair', address: '192.168.1.31:42075', code: '0a1b2c3d4e5f6071', name: 'DEN-PC' })
    // Typed still works, at an address no look found.
    socket.tell({ type: 'pairRefused', reason: 'wrongCode' })
    fireEvent.change(address, { target: { value: '192.168.1.44' } })
    expect(shack.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.pairSubmit'] }))
    expect(last(socket.sent)).toEqual({ type: 'pair', address: '192.168.1.44', code: '0a1b2c3d4e5f6071', name: 'DEN-PC' })
  })

  it('says why a pairing reached nobody, in the words the card uses', () => {
    for (const reach of ['otherNetwork', 'refused', 'noAnswer'] as const) {
      cleanup()
      page()
      socket.tell({ type: 'pairRefused', reason: reach })
      expect(screen.queryByRole('alert')?.textContent, reach).toBe(EN[`remote.lan.reach.${reach}`])
    }
  })

  it('says why no station was found by name, and looks again only where it can', () => {
    page()
    socket.tell({ type: 'stations', stations: [], computer: 'DEN-PC' })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.pair'] }))
    socket.tell({ type: 'found', available: true, shacks: [] })
    expect(screen.getByText(EN['remote.lan.find.none'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.findAgain'] }))
    expect(socket.sent.filter(m => m.type === 'find')).toHaveLength(2)
    expect(screen.getByText(EN['lanWindow.finding'])).toBeTruthy()
    socket.tell({ type: 'found', available: false, shacks: [] })
    expect(screen.getByText(EN['remote.lan.find.unavailable'])).toBeTruthy()
    expect(screen.queryByRole('button', { name: EN['lanWindow.findAgain'] })).toBeNull()
  })

  it('shows each paired station by its address and the first 128 bits of its key, never more', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    expect(screen.getByText(EN['lanWindow.stationAt'].replace('{{address}}', '192.168.1.20:42075'))).toBeTruthy()
    expect(screen.getByText(EN['lanWindow.stationKey'].replace('{{key}}', '0123 4567 89ab cdef 0123 4567 89ab cdef'))).toBeTruthy()
  })

  it('forgets a station only on a second press', () => {
    page()
    socket.tell({ type: 'stations', computer: '', stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '00'.repeat(32) }] })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.forget'] }))
    expect(socket.sent.some(m => m.type === 'forget')).toBe(false)
    expect(screen.getByText(EN['lanWindow.forgetConfirm'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.forgetYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'forget', stationId: STATION })
  })

  it('says why nothing answered where the station was, and offers to type where it is now', () => {
    for (const reason of ['unreachable', 'otherNetwork', 'refused', 'noAnswer'] as const) {
      cleanup()
      page()
      socket.tell({ type: 'stations', computer: '', stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '00'.repeat(32) }] })
      fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.stream'] }))
      expect(last(socket.sent)).toEqual({ type: 'connect', stationId: STATION })
      socket.tell({ type: 'connectRefused', reason })
      expect(screen.getByRole('alert').textContent, reason)
        .toBe(EN[(reason === 'unreachable' ? 'lanWindow.reason.unreachable' : `remote.lan.reach.${reason}`) as keyof typeof EN])
      fireEvent.change(screen.getByLabelText(EN['lanWindow.otherAddress']), { target: { value: '192.168.1.44' } })
      fireEvent.click(last(screen.getAllByRole('button', { name: EN['lanWindow.stream'] }))!)
      expect(last(socket.sent), reason).toEqual({ type: 'connect', stationId: STATION, address: '192.168.1.44' })
    }
  })
})

describe('the stream', () => {
  it('opens the station’s stream on the road, with no Sign out, and Disconnect closes the road', async () => {
    page()
    socket.tell({ type: 'connected', ...ROAD })
    expect(await screen.findByRole('button', { name: 'Stop TX' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: EN['remote.signOut'] })).toBeNull()
    await waitFor(() => expect(socket.asked('state').length).toBeGreaterThan(0))
    expect(socket.asked('state')[0].operationVersion).toBe(4)
    fireEvent.click(screen.getByRole('button', { name: EN['remote.disconnect'] }))
    expect(last(socket.sent)).toEqual({ type: 'disconnect' })
    socket.tell({ type: 'closed', reason: 'disconnected' })
    expect(screen.getByRole('heading', { name: EN['lanWindow.title'] })).toBeTruthy()
  })

  /** Connected, in control and offering: the view started by itself (the Stream press), took the
   *  lease and sent its offer. */
  async function streaming() {
    const h = harness()
    page(() => h.env)
    socket.tell({ type: 'connected', ...ROAD })
    await waitFor(() => expect(socket.asked('state').length).toBe(1))
    socket.answer(socket.asked('state')[0], state('available'))
    await waitFor(() => expect(socket.asked('acquire').length).toBe(1))
    socket.answer(socket.asked('acquire')[0], state('controlling'))
    await waitFor(() => expect(socket.sent.some(m => m.type === 'streamSignal' && m.payload.kind === 'offer')).toBe(true))
    return h
  }

  /** The lease's next heartbeat, refused as the station refuses one it no longer holds, and the
   *  state read after it, answered with `phase`. */
  async function leaseGone(phase: 'available' | 'occupied') {
    await waitFor(() => expect(socket.asked('heartbeat').length).toBeGreaterThan(0), { timeout: 3000 })
    socket.tell({ type: 'operationResponse', requestId: last(socket.asked('heartbeat'))!.request.requestId, error: 'notController' })
    await waitFor(() => expect(socket.asked('state').length).toBeGreaterThan(1), { timeout: 3000 })
    socket.answer(last(socket.asked('state'))!, state(phase))
  }

  it('acquires again once when the station took control back, and only when control is free', async () => {
    await streaming()
    // The station ends this computer's lease as a side effect of a decision about the hosted road.
    socket.tell({ type: 'streamState', streaming: false, reason: 'notController' })
    await leaseGone('available')
    await waitFor(() => expect(socket.asked('acquire').length).toBe(2), { timeout: 3000 })
  })

  it('CONTROL: a stream that ended any other way is not started again', async () => {
    await streaming()
    socket.tell({ type: 'streamState', streaming: false, reason: 'connectionFailed' })
    await leaseGone('available')
    await new Promise(resolve => setTimeout(resolve, 1500))
    expect(socket.asked('acquire').length).toBe(1)
  })

  it('CONTROL: control another device took, and then let go of, is not taken back', async () => {
    await streaming()
    socket.tell({ type: 'streamState', streaming: false, reason: 'notController' })
    await leaseGone('occupied')
    await waitFor(() => expect(socket.asked('state').length).toBeGreaterThan(2), { timeout: 3000 })
    socket.answer(last(socket.asked('state'))!, state('available'))
    await new Promise(resolve => setTimeout(resolve, 1500))
    expect(socket.asked('acquire').length).toBe(1)
  })

  it('CONTROL: control another device holds now is not fought for', async () => {
    await streaming()
    socket.tell({ type: 'streamState', streaming: false, reason: 'notController' })
    await leaseGone('occupied')
    await new Promise(resolve => setTimeout(resolve, 1500))
    expect(socket.asked('acquire').length).toBe(1)
  })
})
