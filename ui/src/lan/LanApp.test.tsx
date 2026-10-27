// @vitest-environment jsdom
//
// The Stations on this network window's page, against a socket of the test's own: every reason
// this computer's Nexus gives is said in a sentence, the pairing dialog sends what the operator
// typed, a station opens its stream, and control taken back by the station (a decision about the
// hosted road there) is acquired again once, by the page, when nobody else holds it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { LanApp, type PageSocket } from './LanApp'
import { CLOSED_REASONS, CONNECT_REASONS, PAIR_REASONS, SIGN_IN_REASONS, WHERE_REASONS } from './protocol'
import { EN } from '../i18n/en'
import { harness, last } from '../remote-web/stream-link.testkit'
// The shack's own form for a key (its approval question and Remote access ▸ Devices uses it), so
// the window shows each key exactly as the station does, for the operator to compare by eye.
import { shortFingerprint } from '../remote-web/stream-protocol'

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
/** This computer's key for the station's internet road, as its fingerprint: SHA-256, lowercase hex. */
const ROAD_KEY = '9f8e7d6c5b4a3928'.repeat(4)
/** A P-256 private key as a road's record holds it (PKCS#8, lowercase hex): never the page's. */
const PRIVATE = `308187020100301306072a8648ce3d020106082a8648ce3d030107046d306b0201010420${'5a'.repeat(32)}a14403420004${'cd'.repeat(64)}`
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

  it('says, once paired, to press the button the station’s row shows to operate it', () => {
    page()
    const station = { id: STATION, addresses: ['192.168.1.20:42075'], key: '5966'.repeat(16) }
    socket.tell({ type: 'paired', station })
    socket.tell({ type: 'stations', computer: 'DEN-PC', stations: [station] })
    const operate = screen.getByRole('button', { name: EN['lanWindow.operate'] })
    expect(operate.textContent).toBeTruthy()
    expect(screen.getByRole('status').textContent).toContain(operate.textContent)
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
    // The window's own look on opening, and the pairing card's.
    expect(socket.sent.filter(m => m.type === 'find')).toHaveLength(2)
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.findAgain'] }))
    expect(socket.sent.filter(m => m.type === 'find')).toHaveLength(3)
    expect(screen.getByText(EN['lanWindow.finding'])).toBeTruthy()
    socket.tell({ type: 'found', available: false, shacks: [] })
    expect(screen.getByText(EN['remote.lan.find.unavailable'])).toBeTruthy()
    expect(screen.queryByRole('button', { name: EN['lanWindow.findAgain'] })).toBeNull()
  })

  it('shows each paired station by its address and the first 128 bits of its key, never more, as the station shows it', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    expect(screen.getByText(EN['lanWindow.stationAt'].replace('{{address}}', '192.168.1.20:42075'))).toBeTruthy()
    expect(shortFingerprint('0123456789abcdef'.repeat(4))).toBe('0123 4567 89AB CDEF 0123 4567 89AB CDEF')
    expect(screen.getByText(EN['lanWindow.stationKey'].replace('{{key}}', '0123 4567 89AB CDEF 0123 4567 89AB CDEF'))).toBeTruthy()
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
      fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.streamInstead'] }))
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
    expect(screen.getByRole('heading', { name: EN['lanWindow.connect.title'] })).toBeTruthy()
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

describe('Operate from this computer', () => {
  it('asks first, then names the station to the page’s socket; the answer says Nexus is starting again', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operate'] }))
    expect(screen.getByText(EN['lanWindow.operateConfirm'])).toBeTruthy()
    expect(socket.sent.some(m => m.type === 'operate'), 'asked first, nothing sent yet').toBe(false)
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateNo'] }))
    expect(screen.queryByText(EN['lanWindow.operateConfirm'])).toBeNull()
    expect(socket.sent.some(m => m.type === 'operate'), 'Not now sends nothing').toBe(false)
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operate'] }))
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(socket.sent).toContainEqual({ type: 'operate', stationId: STATION })
    socket.tell({ type: 'operating' })
    expect(screen.getByText(EN['lanWindow.operating'])).toBeTruthy()
  })

  it('a station this computer no longer knows is said, and nothing starts', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operate'] }))
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateYes'] }))
    socket.tell({ type: 'connectRefused', reason: 'unknownStation' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.reason.unknownStation'])
    expect((screen.getByRole('button', { name: EN['lanWindow.operate'] }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('operates at the address typed after nothing answered, asked first; the stream stays its quiet fallback', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.streamInstead'] }))
    socket.tell({ type: 'connectRefused', reason: 'unreachable' })
    fireEvent.change(screen.getByLabelText(EN['lanWindow.otherAddress']), { target: { value: ' 192.168.1.44:42080 ' } })
    const form = screen.getByLabelText(EN['lanWindow.otherAddress']).closest('form') as HTMLElement
    const buttons = within(form).getAllByRole('button')
    expect(buttons.map(b => b.textContent)).toEqual([EN['lanWindow.operateAt'], EN['lanWindow.stream']])
    expect(buttons[0].classList.contains('remote-button--quiet')).toBe(false)
    expect(buttons[1].classList.contains('remote-button--quiet')).toBe(true)
    fireEvent.click(buttons[0])
    expect(screen.getByText(EN['lanWindow.operateConfirm'])).toBeTruthy()
    expect(socket.sent.some(m => m.type === 'operate'), 'asked first, nothing sent yet').toBe(false)
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: STATION, address: '192.168.1.44:42080' })
    socket.tell({ type: 'connectRefused', reason: 'badAddress' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.reason.badAddress'])
    // CONTROL: the card's own Operate takes no typed address; the typed Stream still streams there.
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operate'] }))
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: STATION })
    socket.tell({ type: 'connectRefused', reason: 'unknownStation' })
    fireEvent.click(within(form).getByRole('button', { name: EN['lanWindow.stream'] }))
    expect(last(socket.sent)).toEqual({ type: 'connect', stationId: STATION, address: '192.168.1.44:42080' })
  })

  it('shows the address kept for Operate on the station’s row, changed or cleared there, after the main actions', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4), typedAddress: 'shack-pc.local:42075' }] })
    const card = screen.getByText(EN['lanWindow.stationAt'].replace('{{address}}', '192.168.1.20:42075')).closest('.lan-station') as HTMLElement
    expect(within(card).getByText(EN['lanWindow.typedFirst'].replace('{{address}}', 'shack-pc.local:42075'))).toBeTruthy()
    const buttons = within(card).getAllByRole('button').map(b => b.textContent)
    expect(buttons.slice(0, 2)).toEqual([EN['lanWindow.operate'], EN['lanWindow.streamInstead']])
    expect(buttons.slice(-2)).toEqual([EN['lanWindow.typedChange'], EN['lanWindow.typedClear']])
    fireEvent.click(within(card).getByRole('button', { name: EN['lanWindow.typedChange'] }))
    const field = within(card).getByLabelText(EN['lanWindow.typedLabel']) as HTMLInputElement
    expect(field.value).toBe('shack-pc.local:42075')
    fireEvent.change(field, { target: { value: ' radio-pc.local ' } })
    fireEvent.click(within(card).getByRole('button', { name: EN['lanWindow.typedSave'] }))
    expect(last(socket.sent)).toEqual({ type: 'keepAddress', stationId: STATION, address: 'radio-pc.local' })
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4), typedAddress: 'radio-pc.local:42075' }] })
    expect(within(card).getByText(EN['lanWindow.typedFirst'].replace('{{address}}', 'radio-pc.local:42075'))).toBeTruthy()
    fireEvent.click(within(card).getByRole('button', { name: EN['lanWindow.typedClear'] }))
    expect(last(socket.sent)).toEqual({ type: 'keepAddress', stationId: STATION, address: null })
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    // CONTROL: with none kept, the row shows none and offers neither.
    expect(within(card).queryByText(/shack-pc|radio-pc/)).toBeNull()
    expect(within(card).queryByRole('button', { name: EN['lanWindow.typedChange'] })).toBeNull()
    expect(within(card).queryByRole('button', { name: EN['lanWindow.typedClear'] })).toBeNull()
  })

  it('takes a host name where the station is now, and says when the name finds no computer', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.streamInstead'] }))
    socket.tell({ type: 'connectRefused', reason: 'noAnswer' })
    const field = screen.getByLabelText(EN['lanWindow.otherAddress']) as HTMLInputElement
    const name = `${'a'.repeat(240)}.local:42080`
    expect(field.maxLength).toBeGreaterThanOrEqual(name.length)
    expect(screen.getByText(EN['lanWindow.otherAddressHint'].replace('{{example}}', '192.168.1.20:42075').replace('{{name}}', 'shack-pc.local'))).toBeTruthy()
    fireEvent.change(field, { target: { value: name } })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateAt'] }))
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: STATION, address: name })
    socket.tell({ type: 'connectRefused', reason: 'nameUnresolved' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.reason.nameUnresolved'])
  })

  it('is each station’s main action, first and the only primary; the stream is the quiet fallback after it', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC',
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
    const card = screen.getByText(EN['lanWindow.stationAt'].replace('{{address}}', '192.168.1.20:42075')).closest('.lan-station') as HTMLElement
    const buttons = within(card).getAllByRole('button')
    expect(buttons.map(b => b.textContent).slice(0, 2)).toEqual([EN['lanWindow.operate'], EN['lanWindow.streamInstead']])
    expect(buttons.filter(b => b.classList.contains('remote-button--primary')).map(b => b.textContent)).toEqual([EN['lanWindow.operate']])
    expect(buttons[1].classList.contains('remote-button--quiet')).toBe(true)
    expect(within(card).getByText(EN['lanWindow.choiceHint'])).toBeTruthy()
    expect(screen.getByText(EN['lanWindow.connect.intro']).textContent).not.toMatch(/stream/i)
    fireEvent.click(buttons[1])
    expect(last(socket.sent)).toEqual({ type: 'connect', stationId: STATION })
  })
})

const AWAY = '20000000-0000-4000-8000-0000000000a1'

describe('Signing in', () => {
  const told = (account: { stationId: string; until: number; renew: boolean }[] = [], internet: { id: string; key: string }[] = []) =>
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet, account,
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
  const card = () => screen.getByText(EN['lanWindow.stationAt'].replace('{{address}}', '192.168.1.20:42075')).closest('.lan-station') as HTMLElement
  /** The window's own Sign in, beside the stations: the one that adds a station, and the only one
   *  so named. */
  const addButton = () => screen.getByRole('button', { name: EN['lanWindow.signIn'] }) as HTMLButtonElement

  it('is how a station is added: one press, the sign-in waits in the browser, and the station is said added', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], stations: [] })
    const add = addButton()
    expect(add.classList.contains('remote-button--primary')).toBe(true)
    expect(screen.getByText(EN['lanWindow.signInHint'])).toBeTruthy()
    fireEvent.click(add)
    expect(last(socket.sent)).toEqual({ type: 'signIn', stationId: null })
    expect(add.disabled, 'one thing at a time').toBe(true)
    expect(add.textContent).toBe(EN['lanWindow.internet.adding'])
    socket.tell({ type: 'signingIn' })
    expect(screen.getByText(EN['lanWindow.internet.signingIn'])).toBeTruthy()
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: false, away: false, key: ROAD_KEY })
    expect(screen.getByText(EN['lanWindow.signIn.done'])).toBeTruthy()
    expect(socket.sent.filter(m => m.type === 'stations').length).toBe(2)
    expect(addButton().disabled).toBe(false)
  })

  it('keeps pairing with a code beside it, as the quiet second way, for a station whose owner has not signed in', () => {
    page()
    told()
    const pair = screen.getByRole('button', { name: EN['lanWindow.pair'] })
    expect(pair.classList.contains('remote-button--quiet')).toBe(true)
    expect(screen.getByText(EN['lanWindow.pairCodeHint'])).toBeTruthy()
    fireEvent.click(pair)
    expect(screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })).toBeTruthy()
  })

  // The station asks "Approve it only if this key matches the one Nexus shows on that computer", so this
  // computer shows its key for the road: while the station waits to approve it, and on the station's row.
  it('shows this computer’s key for the road as the station shows it: in the line that asks to approve it there, and on the station’s row', () => {
    const shown = shortFingerprint(ROAD_KEY)
    expect(shown).toBe('9F8E 7D6C 5B4A 3928 9F8E 7D6C 5B4A 3928')
    page()
    told()
    fireEvent.click(within(card()).getByRole('button', { name: EN['lanWindow.signIn.link'] }))
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: true, away: false, key: ROAD_KEY })
    expect(screen.getByRole('status').textContent).toBe(EN['lanWindow.internet.waiting'].replace('{{key}}', shown))
    told([], [{ id: STATION, key: ROAD_KEY }])
    expect(within(card()).getByText(EN['lanWindow.internet.added'])).toBeTruthy()
    expect(within(card()).getByText(EN['lanWindow.internet.computerKey'].replace('{{key}}', shown))).toBeTruthy()
    // CONTROL: approved at once, nothing to compare at the station, and the line says only that it is done.
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: false, away: false, key: ROAD_KEY })
    expect(screen.getByRole('status').textContent).toBe(EN['lanWindow.signIn.done'])
    // A station added with the account has the road too: its row says until when, and this key.
    told([{ stationId: STATION, until: 1_802_592_000, renew: false }], [{ id: STATION, key: ROAD_KEY }])
    expect(within(card()).getByText(EN['lanWindow.internet.computerKey'].replace('{{key}}', shown))).toBeTruthy()
    expect(within(card()).queryByText(EN['lanWindow.internet.added'])).toBeNull()
    // CONTROL: a row with no road shows no key.
    told()
    expect(within(card()).queryByText(EN['lanWindow.internet.computerKey'].replace('{{key}}', shown))).toBeNull()
  })

  it('shows no key it was not told as a fingerprint: a private key is refused with the message that carried it', () => {
    page()
    for (const bad of [{ key: PRIVATE }, { key: ROAD_KEY, pkcs8: PRIVATE }]) {
      socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [{ id: STATION, ...bad }],
        stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
      socket.tell({ type: 'signedIn', stationId: STATION, waiting: true, away: false, ...bad })
    }
    expect(screen.getByText(EN['lanWindow.none']), 'the whole message refused: no station shown').toBeTruthy()
    expect(screen.queryByRole('status')).toBeNull()
    const text = (document.body.textContent ?? '').replace(/\s/g, '').toLowerCase()
    expect(text).not.toContain(PRIVATE.slice(0, 16))
    expect(text).not.toContain('5a'.repeat(8))
  })

  it('asks which station and where it is, and sends what the operator picks and types, or that it is not on this network', () => {
    const club = { id: '20000000-0000-4000-8000-0000000000c1', name: 'Club station' }
    page()
    told()
    fireEvent.click(addButton())
    socket.tell({ type: 'signingIn' })
    socket.tell({ type: 'chooseStation', stations: [club, { id: STATION, name: 'Shack' }] })
    const asking = screen.getByRole('region', { name: EN['lanWindow.internet.chooseTitle'] })
    expect(within(asking).getByText(EN['lanWindow.signIn.choose'])).toBeTruthy()
    fireEvent.click(within(asking).getByRole('button', { name: 'Shack' }))
    expect(last(socket.sent)).toEqual({ type: 'chooseStation', stationId: STATION })
    expect(screen.queryByRole('region', { name: EN['lanWindow.internet.chooseTitle'] })).toBeNull()

    socket.tell({ type: 'stationAddress', name: 'Shack', reason: 'keyChanged' })
    let where = screen.getByRole('region', { name: EN['lanWindow.signIn.whereTitle'] })
    expect(within(where).getByRole('alert').textContent).toBe(EN['lanWindow.signIn.reason.keyChanged'])
    const tryIt = () => within(screen.getByRole('region', { name: EN['lanWindow.signIn.whereTitle'] }))
      .getByRole('button', { name: EN['lanWindow.signIn.tryAddress'] }) as HTMLButtonElement
    expect(tryIt().disabled, 'nothing typed yet').toBe(true)
    fireEvent.change(within(where).getByLabelText(EN['lanWindow.address']), { target: { value: ' 192.168.1.20 ' } })
    fireEvent.click(tryIt())
    expect(last(socket.sent)).toEqual({ type: 'stationAddress', address: '192.168.1.20' })
    expect(screen.queryByRole('region', { name: EN['lanWindow.signIn.whereTitle'] })).toBeNull()

    socket.tell({ type: 'stationAddress', name: 'Shack' })
    where = screen.getByRole('region', { name: EN['lanWindow.signIn.whereTitle'] })
    expect(within(where).queryByRole('alert'), 'asked first: no reason yet').toBeNull()
    expect(within(where).queryByRole('button', { name: EN['lanWindow.cancel'] }), 'adding: not Cancel').toBeNull()
    fireEvent.click(within(where).getByRole('button', { name: EN['lanWindow.signIn.notHere'] }))
    expect(last(socket.sent)).toEqual({ type: 'stationAddress', address: null })
    // Kept by its service id: added, over the internet until this computer signs in on its network.
    socket.tell({ type: 'signedIn', stationId: AWAY, waiting: false, away: true, key: ROAD_KEY })
    expect(screen.getByRole('status').textContent).toBe(EN['lanWindow.away.added'])
    expect(last(socket.sent)).toEqual({ type: 'stations' })
  })

  it('CONTROL: signing in from the row of a station on this network, the where-card keeps Cancel', () => {
    page()
    told([], [{ id: STATION, key: ROAD_KEY }])
    fireEvent.click(within(card()).getByRole('button', { name: EN['lanWindow.signIn.link'] }))
    socket.tell({ type: 'stationAddress', name: '' })
    const where = screen.getByRole('region', { name: EN['lanWindow.signIn.whereTitle'] })
    expect(within(where).queryByRole('button', { name: EN['lanWindow.signIn.notHere'] })).toBeNull()
    fireEvent.click(within(where).getByRole('button', { name: EN['lanWindow.cancel'] }))
    expect(last(socket.sent)).toEqual({ type: 'stationAddress', address: null })
    socket.tell({ type: 'signInRefused', reason: 'notOnThisNetwork' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.signIn.reason.notOnThisNetwork'])
  })

  it('links a station paired with a code from its own row, and says when its internet road waits for Approve', () => {
    page()
    told([], [{ id: STATION, key: ROAD_KEY }])
    expect(within(card()).getByText(EN['lanWindow.internet.added'])).toBeTruthy()
    const link = within(card()).getByRole('button', { name: EN['lanWindow.signIn.link'] })
    expect(link.classList.contains('remote-button--quiet')).toBe(true)
    fireEvent.click(link)
    expect(last(socket.sent)).toEqual({ type: 'signIn', stationId: STATION })
    socket.tell({ type: 'chooseStation', stations: [{ id: '20000000-0000-4000-8000-0000000000c1', name: 'Club' }, { id: STATION, name: 'Shack' }] })
    expect(screen.getByText(EN['lanWindow.internet.choose'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Shack' }))
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: true, away: false, key: ROAD_KEY })
    expect(screen.getByText(EN['lanWindow.internet.waiting'].replace('{{key}}', shortFingerprint(ROAD_KEY)))).toBeTruthy()
  })

  it('shows until when a station added with the account takes this computer, and in its last week asks to sign in again', () => {
    page()
    // 1 802 592 000 s is 2027-02-14 UTC.
    told([{ stationId: STATION, until: 1_802_592_000, renew: false }], [{ id: STATION, key: ROAD_KEY }])
    expect(within(card()).getByText(EN['lanWindow.signIn.until'].replace('{{date}}', '2027-02-14'))).toBeTruthy()
    expect(within(card()).queryByRole('button', { name: EN['lanWindow.signIn.link'] }), 'signed in: nothing to link').toBeNull()
    expect(within(card()).queryByRole('button', { name: EN['remote.account.signInAgain'] }), 'not yet').toBeNull()
    told([{ stationId: STATION, until: 1_802_592_000, renew: true }], [{ id: STATION, key: ROAD_KEY }])
    expect(within(card()).getByText(EN['lanWindow.signIn.renew'].replace('{{date}}', '2027-02-14'))).toBeTruthy()
    fireEvent.click(within(card()).getByRole('button', { name: EN['remote.account.signInAgain'] }))
    expect(last(socket.sent)).toEqual({ type: 'signIn', stationId: STATION })
    socket.tell({ type: 'signingIn' })
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: false, away: false, key: ROAD_KEY })
    expect(screen.getByText(EN['lanWindow.signIn.renewed'])).toBeTruthy()
  })

  it('says, after a forget, that Nexus Remote could not be told, and where to remove this computer', () => {
    page()
    told()
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], stations: [], forgetUnreached: 'serviceUnavailable' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.internet.forgetUnreached'])
    cleanup()
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], stations: [] })
    expect(screen.queryByRole('alert'), 'CONTROL: a forget the service heard says nothing').toBeNull()
  })

  it('says why a sign-in added nothing, and why an address did not take this computer: every reason is a sentence', () => {
    page()
    told()
    fireEvent.click(addButton())
    socket.tell({ type: 'signInRefused', reason: 'stationNotProven' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.signIn.reason.stationNotProven'])
    expect(addButton().disabled).toBe(false)
    // `alreadyHere` with the one station here paired with a code goes on to link it: its own tests.
    for (const reason of SIGN_IN_REASONS.filter(reason => reason !== 'alreadyHere')) {
      socket.tell({ type: 'signInRefused', reason })
      const said = screen.getByRole('alert').textContent ?? ''
      expect(said.length, reason).toBeGreaterThan(10)
      expect(said, reason).not.toContain('lanWindow.')
    }
    for (const reason of WHERE_REASONS) {
      socket.tell({ type: 'stationAddress', name: '', reason })
      const said = within(screen.getByRole('region', { name: EN['lanWindow.signIn.whereTitle'] })).getByRole('alert').textContent ?? ''
      expect(said.length, reason).toBeGreaterThan(10)
      expect(said, reason).not.toContain('lanWindow.')
    }
  })

  it('says to confirm the email address when the issuer refused one not confirmed yet, never that the sign-in was cancelled', () => {
    page()
    told()
    fireEvent.click(addButton())
    socket.tell({ type: 'signInRefused', reason: 'emailUnverified' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.internet.reason.emailUnverified'])
    // CONTROL: the operator's own cancel is still said as one.
    fireEvent.click(addButton())
    socket.tell({ type: 'signInRefused', reason: 'signInRefused' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.internet.reason.signInRefused'])
  })

  it('names each sign-in by what it does, and says how each went under its own button, never under another', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], account: [],
      stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }],
      away: [{ id: AWAY, name: 'Shack', key: 'fedcba9876543210'.repeat(4) }] })
    const names = [EN['lanWindow.signIn'], EN['lanWindow.signIn.link'], EN['lanWindow.away.signIn']]
    expect(new Set(names).size).toBe(3)
    for (const name of names) expect(screen.getAllByRole('button', { name }), name).toHaveLength(1)
    const away = () => screen.getByText('Shack').closest('.lan-station') as HTMLElement
    // The code-paired row's link: its line on its row alone.
    fireEvent.click(within(card()).getByRole('button', { name: EN['lanWindow.signIn.link'] }))
    socket.tell({ type: 'signInRefused', reason: 'signInFailed' })
    expect(within(card()).getByRole('alert').textContent).toBe(EN['lanWindow.internet.reason.signInFailed'])
    expect(screen.getAllByRole('alert')).toHaveLength(1)
    // The add: its line by the add button, and the row's is gone.
    fireEvent.click(addButton())
    socket.tell({ type: 'signInRefused', reason: 'stationNotProven' })
    expect(within(card()).queryByRole('alert')).toBeNull()
    expect(within(away()).queryByRole('alert')).toBeNull()
    expect(screen.getAllByRole('alert')).toHaveLength(1)
    expect(addButton().parentElement?.parentElement?.contains(screen.getByRole('alert'))).toBe(true)
  })

  it('adding a station that is here already, paired with a code, goes on to link it when it is the one station here', () => {
    page()
    told()
    fireEvent.click(addButton())
    socket.tell({ type: 'signInRefused', reason: 'alreadyHere' })
    expect(last(socket.sent)).toEqual({ type: 'signIn', stationId: STATION })
    expect(within(card()).getByRole('status').textContent).toBe(EN['lanWindow.signIn.linking'])
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: false, away: false, key: ROAD_KEY })
    expect(within(card()).getByRole('status').textContent).toBe(EN['lanWindow.signIn.done'])
  })

  it('CONTROL: with more than one station here, adding one that is here already says which button links it, and signs in no more', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], account: [], stations: [
      { id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) },
      { id: '60000000-0000-4000-8000-000000000009', addresses: ['192.168.1.21:42075'], key: 'fedcba9876543210'.repeat(4) }] })
    fireEvent.click(addButton())
    const sent = socket.sent.length
    socket.tell({ type: 'signInRefused', reason: 'alreadyHere' })
    expect(socket.sent.length).toBe(sent)
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.signIn.reason.alreadyHere'])
    expect(EN['lanWindow.signIn.reason.alreadyHere']).toContain(EN['lanWindow.signIn.link'].replace('…', ''))
  })
})

describe('pairing again', () => {
  const told = () => socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], account: [],
    stations: [{ id: STATION, addresses: ['192.168.1.20:42075'], key: '0123456789abcdef'.repeat(4) }] })
  const card = () => screen.getByText(EN['lanWindow.stationAt'].replace('{{address}}', '192.168.1.20:42075')).closest('.lan-station') as HTMLElement

  it('offers Pair again on the row of a station that no longer knows this computer, or answers with another key, with its address filled in', () => {
    for (const reason of ['notPaired', 'keyChanged'] as const) {
      cleanup()
      page()
      told()
      fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.streamInstead'] }))
      socket.tell({ type: 'connectRefused', reason })
      expect(within(card()).getByRole('alert').textContent, reason).toBe(EN[`lanWindow.reason.${reason}`])
      fireEvent.click(within(card()).getByRole('button', { name: EN['lanWindow.pairAgain'] }))
      const pairing = screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })
      expect((within(pairing).getByLabelText(EN['lanWindow.address']) as HTMLInputElement).value, reason).toBe('192.168.1.20:42075')
      expect(last(socket.sent), 'it looks by name, as the pairing card always does').toEqual({ type: 'find' })
      fireEvent.change(within(pairing).getByLabelText(EN['lanWindow.code']), { target: { value: '0123 4567 89ab cdef' } })
      fireEvent.click(within(pairing).getByRole('button', { name: EN['lanWindow.pairSubmit'] }))
      expect(last(socket.sent)).toEqual({ type: 'pair', address: '192.168.1.20:42075', code: '0123 4567 89ab cdef', name: 'DEN-PC' })
    }
  })

  it('CONTROL: a station that did not answer is not offered Pair again', () => {
    page()
    told()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.streamInstead'] }))
    socket.tell({ type: 'connectRefused', reason: 'noAnswer' })
    expect(screen.queryByRole('button', { name: EN['lanWindow.pairAgain'] })).toBeNull()
  })

  it('opened to pair again with a station, opens the pairing card with that station’s address, once', () => {
    socket = new FakeSocket()
    render(<LanApp page={`${PAGE}&pairAgain=${STATION}`} open={() => socket} />)
    act(() => { socket.onopen?.() })
    told()
    const pairing = screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })
    expect((within(pairing).getByLabelText(EN['lanWindow.address']) as HTMLInputElement).value).toBe('192.168.1.20:42075')
    expect(screen.getByText(EN['lanWindow.pairAgainHint'])).toBeTruthy()
    fireEvent.click(within(pairing).getByRole('button', { name: EN['lanWindow.cancel'] }))
    told()
    expect(screen.queryByRole('region', { name: EN['lanWindow.pairTitle'] }), 'once').toBeNull()
  })

  it('CONTROL: opened to pair again with a station not listed here, opens nothing', () => {
    socket = new FakeSocket()
    render(<LanApp page={`${PAGE}&pairAgain=60000000-0000-4000-8000-0000000000ff`} open={() => socket} />)
    act(() => { socket.onopen?.() })
    told()
    expect(screen.queryByRole('region', { name: EN['lanWindow.pairTitle'] })).toBeNull()
  })
})

describe('a station signed in to from elsewhere', () => {
  const away = [{ id: AWAY, name: 'Shack', key: '0123456789abcdef'.repeat(4) }]
  const told = () => socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], account: [], stations: [], away })
  const row = () => screen.getByText('Shack').closest('.lan-station') as HTMLElement

  it('is listed by its name and key, with Operate (over the internet) first, Sign in for its network, and Forget; no stream', () => {
    page()
    told()
    expect(screen.queryByText(EN['lanWindow.none'])).toBeNull()
    expect(within(row()).getByText(EN['lanWindow.stationKey'].replace('{{key}}', '0123 4567 89AB CDEF 0123 4567 89AB CDEF'))).toBeTruthy()
    expect(within(row()).getByText(EN['lanWindow.away.status'])).toBeTruthy()
    const buttons = within(row()).getAllByRole('button')
    expect(buttons.map(b => b.textContent)).toEqual([EN['lanWindow.operate'], EN['lanWindow.away.signIn'], EN['lanWindow.forget']])
    expect(buttons.filter(b => b.classList.contains('remote-button--primary')).map(b => b.textContent)).toEqual([EN['lanWindow.operate']])
    expect(within(row()).queryByRole('button', { name: EN['lanWindow.streamInstead'] }), 'not on this network: no stream').toBeNull()
    fireEvent.click(buttons[0])
    expect(within(row()).getByText(EN['lanWindow.away.operateConfirm'])).toBeTruthy()
    expect(within(row()).queryByText(EN['lanWindow.operateConfirm'])).toBeNull()
    expect(socket.sent.some(m => m.type === 'operate'), 'asked first, nothing sent yet').toBe(false)
    fireEvent.click(within(row()).getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: AWAY })
  })

  it('forgets it only on a second press', () => {
    page()
    told()
    fireEvent.click(within(row()).getByRole('button', { name: EN['lanWindow.forget'] }))
    expect(socket.sent.some(m => m.type === 'forget')).toBe(false)
    fireEvent.click(within(row()).getByRole('button', { name: EN['lanWindow.forgetYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'forget', stationId: AWAY })
  })

  it('signs in for it from its row: not found, it says it stays over the internet; on its network, it is signed in there', () => {
    page()
    told()
    fireEvent.click(within(row()).getByRole('button', { name: EN['lanWindow.away.signIn'] }))
    expect(last(socket.sent)).toEqual({ type: 'signIn', stationId: AWAY })
    socket.tell({ type: 'signingIn' })
    socket.tell({ type: 'stationAddress', name: 'Shack' })
    const where = screen.getByRole('region', { name: EN['lanWindow.signIn.whereTitle'] })
    fireEvent.click(within(where).getByRole('button', { name: EN['lanWindow.signIn.notHere'] }))
    expect(last(socket.sent)).toEqual({ type: 'stationAddress', address: null })
    socket.tell({ type: 'signInRefused', reason: 'notOnThisNetwork' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.away.stillAway'])
    fireEvent.click(within(row()).getByRole('button', { name: EN['lanWindow.away.signIn'] }))
    socket.tell({ type: 'signedIn', stationId: STATION, waiting: false, away: false, key: ROAD_KEY })
    expect(screen.getByRole('status').textContent).toBe(EN['lanWindow.signIn.done'])
    expect(last(socket.sent)).toEqual({ type: 'stations' })
  })

  it('CONTROL: with no station of either kind, the window says none is here', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], account: [], stations: [], away: [] })
    expect(screen.getByText(EN['lanWindow.none'])).toBeTruthy()
  })
})

describe('the station’s key for each internet road', () => {
  const AWAY = '20000000-0000-4000-8000-0000000000a1'
  const KEY = '0123456789abcdef'.repeat(4)
  const station = { id: STATION, addresses: ['192.168.1.20:42075'], key: '5966'.repeat(16) }
  const away = [{ id: AWAY, name: 'Shack', key: 'fedcba9876543210'.repeat(4) }]
  const told = (held?: string) => socket.tell({ type: 'stations', computer: 'DEN-PC', stations: [station],
    internet: [{ id: STATION, key: ROAD_KEY }], account: [], away,
    stationKeys: [{ id: STATION, key: KEY }, { id: AWAY, key: KEY, ...(held ? { held } : {}) }] })
  const line = EN['lanWindow.internet.stationKey'].replace('{{key}}', shortFingerprint(KEY))

  it('shows on every road’s row, in the eight groups the station shows, and asks nothing of a road not held', () => {
    page()
    told()
    expect(screen.getAllByText(line)).toHaveLength(2)
    expect(shortFingerprint(KEY).split(' ')).toHaveLength(8)
    expect(screen.queryByRole('button', { name: EN['lanWindow.internet.keyMatches'] })).toBeNull()
  })

  it('held, asks for the check: It matches frees the road', () => {
    page()
    told('stationKeyUnconfirmed')
    expect(screen.getByText(EN['lanWindow.internet.held'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.internet.keyMatches'] }))
    expect(last(socket.sent)).toEqual({ type: 'keyMatches', stationId: AWAY })
  })

  it('held, It doesn’t match forgets the station, asked first as Forget is', () => {
    page()
    told('stationKeyUnconfirmed')
    const sent = socket.sent.length
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.internet.keyDiffers'] }))
    expect(socket.sent.length, 'forgotten without asking').toBe(sent)
    expect(screen.getByText(EN['lanWindow.forgetConfirm'])).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.forgetYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'forget', stationId: AWAY })
  })

  it('held, on a station paired here with a code, It doesn’t match removes the internet road alone, asked first', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', stations: [station], internet: [{ id: STATION, key: ROAD_KEY }],
      account: [], away: [], stationKeys: [{ id: STATION, key: KEY, held: 'stationKeyUnconfirmed', codePaired: true }] })
    expect(screen.getByText(EN['lanWindow.internet.heldCodePaired'])).toBeTruthy()
    expect(screen.queryByText(EN['lanWindow.internet.held'])).toBeNull()
    const sent = socket.sent.length
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.internet.keyDiffers'] }))
    expect(socket.sent.length, 'removed without asking').toBe(sent)
    expect(screen.getByText(EN['lanWindow.internet.dropRoadConfirm'])).toBeTruthy()
    expect(screen.queryByText(EN['lanWindow.forgetConfirm'])).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.internet.dropRoadYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'keyDiffers', stationId: STATION })
  })

  it('held, on a station Nexus does not name as paired with a code, It doesn’t match forgets it, with no sign-in line either', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', stations: [station], internet: [{ id: STATION, key: ROAD_KEY }],
      account: [], away: [], stationKeys: [{ id: STATION, key: KEY, held: 'stationKeyUnconfirmed' }] })
    expect(screen.getByText(EN['lanWindow.internet.held'])).toBeTruthy()
    expect(screen.queryByText(EN['lanWindow.internet.heldCodePaired'])).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.internet.keyDiffers'] }))
    expect(screen.queryByText(EN['lanWindow.internet.dropRoadConfirm'])).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.forgetYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'forget', stationId: STATION })
  })

  it('says, after the internet road alone was removed, that the pairing here was kept and Nexus Remote could not be told', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', stations: [station], internet: [], account: [], away: [], dropUnreached: 'serviceUnavailable' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.internet.dropRoadUnreached'])
    expect(EN['lanWindow.internet.dropRoadUnreached']).not.toContain('Forgotten')
  })

  it('held, on a station added with the account as a guest, It doesn’t match forgets the station', () => {
    page()
    socket.tell({ type: 'stations', computer: 'DEN-PC', stations: [station], internet: [{ id: STATION, key: ROAD_KEY }],
      account: [{ stationId: STATION, until: 1_900_000_000, renew: false }], away: [],
      stationKeys: [{ id: STATION, key: KEY, held: 'stationKeyUnconfirmed' }] })
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.internet.keyDiffers'] }))
    expect(screen.queryByText(EN['lanWindow.internet.dropRoadConfirm'])).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.forgetYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'forget', stationId: STATION })
  })

  it('disowned by the station on its network, says the road is off and offers no It matches', () => {
    page()
    told('stationKeyDisagrees')
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.internet.keyDisagrees'])
    expect(screen.queryByRole('button', { name: EN['lanWindow.internet.keyMatches'] })).toBeNull()
  })

  it('a link the service listed another key for is said as such', () => {
    page()
    socket.tell({ type: 'signInRefused', reason: 'stationKeyDisagrees' })
    expect(screen.getByRole('alert').textContent).toBe(EN['lanWindow.internet.reason.stationKeyDisagrees'])
  })
})

// Connect to my station: the window decides by itself, network first. A station found by name is a
// hint for which station to reach and at what address, never an identity: the road still proves the
// key pinned at pairing, and a code pairing still proves the station's.
describe('Connect to my station', () => {
  const KEY = '5966'.repeat(16)
  const mine = { id: STATION, addresses: ['192.168.1.20:42075'], key: KEY }
  const shack = (name: string, address: string, key: string) => ({ name, address, protocol: 1, key })
  const told = (stations: unknown[] = [], away: unknown[] = []) =>
    socket.tell({ type: 'stations', computer: 'DEN-PC', internet: [], account: [], stations, away })
  const connectCard = () => screen.getByRole('region', { name: EN['lanWindow.connect.title'] })

  it('looks on this network first, once the stations here are known', () => {
    page()
    expect(socket.sent).toEqual([{ type: 'stations' }])
    told([mine])
    expect(last(socket.sent)).toEqual({ type: 'find' })
    expect(within(connectCard()).getByText(EN['lanWindow.connect.looking'])).toBeTruthy()
  })

  it('found, and paired with this computer: asks once, then operates it', () => {
    page()
    told([mine])
    socket.tell({ type: 'found', available: true, shacks: [shack('SHACK', '192.168.1.20:42075', KEY.slice(0, 16))] })
    const card = connectCard()
    expect(within(card).getByText(EN['lanWindow.operateConfirm'])).toBeTruthy()
    expect(socket.sent.some(m => m.type === 'operate'), 'asked first').toBe(false)
    fireEvent.click(within(card).getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: STATION })
  })

  it('found, and not paired with this computer: the code pairing, with its address filled in; paired, it operates it', () => {
    page()
    told([])
    socket.tell({ type: 'found', available: true, shacks: [shack('SHACK', '192.168.1.30:42075', 'abcdabcdabcdabcd')] })
    const pairing = screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })
    expect((within(pairing).getByLabelText(EN['lanWindow.address']) as HTMLInputElement).value).toBe('192.168.1.30:42075')
    expect(socket.sent.some(m => m.type === 'signIn' || m.type === 'pair'), 'nothing sent before the code').toBe(false)
    fireEvent.change(within(pairing).getByLabelText(EN['lanWindow.code']), { target: { value: '0123 4567 89ab cdef' } })
    fireEvent.click(within(pairing).getByRole('button', { name: EN['lanWindow.pairSubmit'] }))
    expect(last(socket.sent)).toEqual({ type: 'pair', address: '192.168.1.30:42075', code: '0123 4567 89ab cdef', name: 'DEN-PC' })
    const station = { id: STATION, addresses: ['192.168.1.30:42075'], key: 'abcd'.repeat(16) }
    socket.tell({ type: 'paired', station })
    told([station])
    fireEvent.click(within(connectCard()).getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: STATION })
  })

  it('not found on this network: offers sign-in for the internet, and connects through the account', () => {
    page()
    told([])
    socket.tell({ type: 'found', available: true, shacks: [] })
    const card = connectCard()
    expect(within(card).getByText(EN['lanWindow.connect.notFound'])).toBeTruthy()
    fireEvent.click(within(card).getByRole('button', { name: EN['lanWindow.connect.signIn'] }))
    expect(last(socket.sent)).toEqual({ type: 'signIn', stationId: null })
    socket.tell({ type: 'signedIn', stationId: AWAY, waiting: true, away: true, key: ROAD_KEY })
    told([], [{ id: AWAY, name: 'Shack', key: KEY }])
    expect(within(connectCard()).getByText(EN['lanWindow.internet.waiting'].replace('{{key}}', shortFingerprint(ROAD_KEY)))).toBeTruthy()
    expect(within(connectCard()).getByText(EN['lanWindow.away.operateConfirm'])).toBeTruthy()
    fireEvent.click(within(connectCard()).getByRole('button', { name: EN['lanWindow.operateYes'] }))
    expect(last(socket.sent)).toEqual({ type: 'operate', stationId: AWAY })
  })

  it('several stations found: asks which, then decides for the one picked', () => {
    page()
    told([mine])
    socket.tell({ type: 'found', available: true, shacks: [
      shack('CLUB', '192.168.1.40:42075', 'abcdabcdabcdabcd'), shack('FIELD', '192.168.1.41:42075', '0123012301230123')] })
    const card = connectCard()
    expect(within(card).getByText(EN['lanWindow.connect.choose'])).toBeTruthy()
    expect(screen.queryByRole('region', { name: EN['lanWindow.pairTitle'] }), 'nothing chosen for the operator').toBeNull()
    fireEvent.click(within(card).getByRole('button', { name: 'FIELD at 192.168.1.41:42075' }))
    const pairing = screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })
    expect((within(pairing).getByLabelText(EN['lanWindow.address']) as HTMLInputElement).value).toBe('192.168.1.41:42075')
  })

  it('a refusal is said in the card, with Pair again for a station that no longer knows this computer', () => {
    page()
    told([mine])
    socket.tell({ type: 'found', available: true, shacks: [shack('SHACK', '192.168.1.20:42075', KEY.slice(0, 16))] })
    fireEvent.click(within(connectCard()).getByRole('button', { name: EN['lanWindow.operateYes'] }))
    socket.tell({ type: 'connectRefused', reason: 'notPaired' })
    const card = connectCard()
    expect(within(card).getByRole('alert').textContent).toBe(EN['lanWindow.reason.notPaired'])
    expect(screen.getAllByRole('alert'), 'said once').toHaveLength(1)
    fireEvent.click(within(card).getByRole('button', { name: EN['lanWindow.pairAgain'] }))
    const pairing = screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })
    expect((within(pairing).getByLabelText(EN['lanWindow.address']) as HTMLInputElement).value).toBe('192.168.1.20:42075')
  })

  it('keeps the stations, a typed address and the code under Advanced, collapsed', () => {
    page()
    told([mine])
    const advanced = screen.getByText(EN['lanWindow.connect.advanced']).closest('details') as HTMLDetailsElement
    expect(advanced.open).toBe(false)
    expect(advanced.contains(screen.getByRole('button', { name: EN['lanWindow.operate'] }))).toBe(true)
    expect(advanced.contains(screen.getByRole('button', { name: EN['lanWindow.pair'] }))).toBe(true)
    expect(advanced.contains(screen.getByRole('button', { name: EN['lanWindow.signIn'] }))).toBe(true)
  })

  it('CONTROL: an operator who started something under Advanced before the look answered is not moved', () => {
    page()
    told([mine])
    fireEvent.click(screen.getByRole('button', { name: EN['lanWindow.pair'] }))
    socket.tell({ type: 'found', available: true, shacks: [shack('SHACK', '192.168.1.20:42075', KEY.slice(0, 16))] })
    expect(screen.queryByText(EN['lanWindow.operateConfirm'])).toBeNull()
    expect(screen.getByRole('region', { name: EN['lanWindow.pairTitle'] })).toBeTruthy()
  })

  it('CONTROL: opened to pair again, it does not look for a station to connect to', () => {
    socket = new FakeSocket()
    render(<LanApp page={`${PAGE}&pairAgain=${STATION}`} open={() => socket} />)
    act(() => { socket.onopen?.() })
    told([mine])
    socket.tell({ type: 'found', available: true, shacks: [shack('SHACK', '192.168.1.20:42075', KEY.slice(0, 16))] })
    expect(screen.queryByText(EN['lanWindow.operateConfirm'])).toBeNull()
  })
})
