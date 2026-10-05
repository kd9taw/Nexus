// @vitest-environment jsdom
//
// Remote over this network's card at the shack (Settings ▸ Station ▸ Remote access).
//
// ONLY AT THE SHACK (the operator's ruling of 2026-10-04): turning it on or off, making a code,
// removing a computer and resetting the key are refused while the press comes through the stream.
// The presses here come from the REAL stream dispatcher, so the mark under test is the one the
// stream makes; each refusal is paired with the same press made at the shack, which goes through.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { LAN_POLL_MS, LanStation } from './LanStation'
import { StreamInputDispatcher } from './stream-input'
import type { LanStatus, RemoteStationStatus } from './types'
import { EN, type PartialCatalog } from '../i18n'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'

const { getRemoteStationStatus, remoteStationAction } = vi.hoisted(() => ({
  getRemoteStationStatus: vi.fn(),
  remoteStationAction: vi.fn(),
}))
vi.mock('../api', () => ({ getRemoteStationStatus, remoteStationAction }))

const COMPUTER = '8a1f0c2e-77d3-8b41-9c0a-5e2b6d4f1a30'
const KEY = 'ab'.repeat(32)
const DEVICE_KEY = '0123456789abcdef'.repeat(4)

const status = (lan: LanStatus): RemoteStationStatus => ({
  phase: 'unpaired', origin: 'https://remote.invalid', stationId: null, accountId: null, pairingId: null,
  pairingCode: null, expiresAt: null, devices: [], error: null, lan,
})
const listening: LanStatus = {
  on: true, listening: '192.168.1.20:42075', key: KEY,
  devices: [{ id: COMPUTER, name: 'Den PC', key: DEVICE_KEY }],
}
const pairing: LanStatus = { ...listening, pairing: { code: '0123456789abcdef', closesAt: 0 } }
const NETWORKS = [{ address: '192.168.1.20', name: 'Wi-Fi' }, { address: '10.0.0.5', name: 'Ethernet' }]
const choosing: LanStatus = { on: true, reason: 'chooseAddress', networks: NETWORKS, devices: [] }

// jsdom never lays out: `elementFromPoint` does not exist. Each press says what is under the pointer.
let under: Element | null = null
let stream: StreamInputDispatcher | null = null
beforeEach(() => {
  under = null
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => under })
  stream = new StreamInputDispatcher(window)
})
afterEach(() => {
  stream?.dispose()
  stream = null
  cleanup()
  delete (document as { elementFromPoint?: unknown }).elementFromPoint
  getRemoteStationStatus.mockReset()
  remoteStationAction.mockReset()
})

const pointer = (action: 'down' | 'up') =>
  ({ type: 'pointer', action, x: 0.5, y: 0.5, button: 0, buttons: action === 'down' ? 1 : 0,
    modifiers: 0, pointerType: 'mouse', clicks: 1 })
/** A press and release over `element`, as the stream delivers them. */
const pressedThroughStream = async (element: Element) => {
  under = element
  act(() => { stream!.handle(pointer('down')); stream!.handle(pointer('up')) })
  await act(async () => { await Promise.resolve() })
}
const pressedAtShack = async (element: Element) => {
  fireEvent.click(element)
  await act(async () => { await Promise.resolve() })
}

async function showing(lan: LanStatus) {
  getRemoteStationStatus.mockResolvedValue(status(lan))
  remoteStationAction.mockResolvedValue(status(lan))
  render(<LanStation />)
  await screen.findByText(lan.on && lan.listening ? /Listening at/ : /Off|Not listening|Starting/)
}

describe('only at the shack: a press through the stream changes nothing', () => {
  const presses: [string, LanStatus, () => Element, unknown][] = [
    ['the switch', listening, () => screen.getByRole('switch'), { type: 'lanOff' }],
    ['Pair a computer', listening, () => screen.getByRole('button', { name: 'Pair a computer' }), { type: 'lanPair' }],
    ['Cancel pairing', pairing, () => screen.getByRole('button', { name: 'Cancel pairing' }), { type: 'lanCancel' }],
    ['Remove', listening, () => screen.getByRole('button', { name: 'Remove' }), { type: 'lanRevoke', deviceId: COMPUTER }],
    ['Reset network identity', listening, () => screen.getByRole('button', { name: 'Reset network identity' }), { type: 'lanReset' }],
  ]
  for (const [name, lan, control, sent] of presses) {
    it(`${name}: refused through the stream, and said why`, async () => {
      await showing(lan)
      await pressedThroughStream(control())
      expect(remoteStationAction, `${name} went to the station through the stream`).not.toHaveBeenCalled()
      expect(screen.getByRole('alert').textContent).toBe('Only at the station itself: this can’t be done through a stream.')
    })
    it(`CONTROL: ${name} pressed at the shack goes to the station`, async () => {
      await showing(lan)
      await pressedAtShack(control())
      expect(remoteStationAction).toHaveBeenCalledWith(sent)
      expect(screen.queryByText(/Only at the station itself/)).toBeNull()
    })
  }
  it('turning it on is refused through the stream too, and taken at the shack', async () => {
    await showing({ on: false, devices: [] })
    await pressedThroughStream(screen.getByRole('switch'))
    expect(remoteStationAction).not.toHaveBeenCalled()
    await pressedAtShack(screen.getByRole('switch'))
    expect(remoteStationAction).toHaveBeenCalledWith({ type: 'lanOn' })
  })
})

describe('what the card says', () => {
  it('names each reason it is off or not listening', async () => {
    const said: [LanStatus['reason'], boolean, string][] = [
      ['noKey', false, 'Off: this station has no network key it can read. Press Reset network identity under Network identity below, then turn this on again. A reset removes every paired computer, so each one has to be paired again.'],
      ['endedAtShack', false, 'Off: remote control was ended here. Turn this on again when you want it.'],
      ['addressGone', true, 'Not listening: 10.0.0.9 is not this computer’s address right now. Nexus listens there again when it is back, or choose another network.'],
      ['chooseAddress', true, 'Not listening: choose which of this computer’s networks to listen on.'],
      ['portInUse', true, 'Not listening: another program is using its port.'],
      ['noNetwork', true, 'Not listening: this computer is not on a private network.'],
      ['unavailable', true, 'Not listening: it could not start on this computer.'],
    ]
    for (const [reason, on, sentence] of said) {
      await showing({ on, reason, picked: '10.0.0.9', devices: [] })
      expect(screen.getByRole('status').textContent, reason).toBe(sentence)
      cleanup()
    }
    await showing({ on: false, devices: [] })
    expect(screen.getByRole('status').textContent).toBe('Off.')
  })

  it('shows where it listens, the code with its warning, and each computer by name and key', async () => {
    await showing(pairing)
    expect(screen.getByRole('status').textContent).toBe('Listening at 192.168.1.20:42075. Only computers paired here can connect.')
    expect(screen.getByText('0123 4567 89ab cdef')).toBeTruthy()
    expect(screen.getByText(/Whoever types it in time can operate this station, transmit included/)).toBeTruthy()
    expect(screen.getByText('This station: 192.168.1.20:42075, key ABAB ABAB ABAB ABAB ABAB ABAB ABAB ABAB')).toBeTruthy()
    expect(screen.getByText('Den PC')).toBeTruthy()
    expect(screen.getByText('Key 0123 4567 89AB CDEF 0123 4567 89AB CDEF')).toBeTruthy()
    // One window at a time: no second Pair a computer while one is open.
    expect(screen.queryByRole('button', { name: 'Pair a computer' })).toBeNull()
  })

  it('says what a refused press needs, by name', async () => {
    await showing(listening)
    remoteStationAction.mockRejectedValueOnce('lanFull')
    await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
    expect(screen.getByRole('alert').textContent).toBe('Eight computers are paired already. Remove one to pair another.')
    remoteStationAction.mockRejectedValueOnce('credentialStoreUnavailable')
    await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
    expect(screen.getByRole('alert').textContent).toBe('Nexus could not read or keep this station’s network key and paired computers in your operating system’s credential store. Press Reset network identity under Network identity below. A reset removes every paired computer, so each one has to be paired again.')
  })

  it('sends a key or a paired list it cannot read to Reset network identity only, named as the card names it', async () => {
    // The station cannot tell a store that would not answer from a record this Nexus cannot read (a newer
    // Nexus's). Both name the one way out, Reset network identity, and what it costs; neither offers
    // unlocking the credential store first (the operator's ruling, 2026-10-05).
    const WAY_OUT = 'Press Reset network identity under Network identity below'
    const COST = 'A reset removes every paired computer, so each one has to be paired again.'
    await showing({ on: false, reason: 'noKey', devices: [] })
    // CONTROL: the control the advice names is on this card, under that section.
    const reset = screen.getByRole('button', { name: 'Reset network identity' })
    expect(reset.closest('details')?.querySelector('summary')?.textContent).toBe('Network identity')
    const off = screen.getByRole('status').textContent ?? ''
    expect(off).toContain(WAY_OUT)
    expect(off).toContain(COST)
    cleanup()
    // Pair refused with no key, or with a paired list the station could not read.
    for (const refusal of ['noKey', 'credentialStoreUnavailable']) {
      await showing(listening)
      remoteStationAction.mockRejectedValueOnce(refusal)
      await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
      const alert = screen.getByRole('alert').textContent ?? ''
      expect(alert, refusal).toContain(WAY_OUT)
      expect(alert, refusal).toContain(COST)
      cleanup()
    }
    // In every catalog, both sentences name the button and its section by that language's own labels, and
    // neither says to unlock the store. CONTROL: each language's word for unlocking is found in the hosted
    // Remote's own advice, which does say to unlock it.
    const catalogs: Record<string, PartialCatalog> = { en: EN, de: DE, es: ES, fr: FR, ja: JA }
    const UNLOCK: Record<string, RegExp> = { en: /unlock/i, de: /entsperr/i, es: /desbloque/i, fr: /déverrouill/i, ja: /解除/ }
    for (const [locale, catalog] of Object.entries(catalogs)) {
      expect(catalog['remote.vaultFailed'], `control: ${locale} says unlock as ${UNLOCK[locale]}`).toMatch(UNLOCK[locale])
      for (const key of ['remote.lan.reason.noKey', 'remote.lan.vaultFailed'] as const) {
        expect(catalog[key], `${locale} ${key}`).toContain(catalog['remote.lan.reset'])
        expect(catalog[key], `${locale} ${key}`).toContain(catalog['remote.lan.resetTitle'])
        expect(catalog[key], `${locale} ${key} offers unlocking`).not.toMatch(UNLOCK[locale])
      }
    }
  })
})

describe('the network it listens on: picked at the shack', () => {
  // Labelled the way Settings labels its other pickers: the label, then the hint, inside one <label>.
  const picker = () => screen.getByRole('combobox', { name: /^Network/ }) as HTMLSelectElement

  it('offers each network by its adapter’s name, and Automatic, only when there is more than one', async () => {
    await showing(choosing)
    expect(within(picker()).getAllByRole('option').map(option => option.textContent))
      .toEqual(['Automatic', 'Wi-Fi, 192.168.1.20', 'Ethernet, 10.0.0.5'])
    expect(picker().value).toBe('')
    expect(screen.getByText(/^Virtual adapters and VPNs are not offered/)).toBeTruthy()
    cleanup()
    // CONTROL: one network, or LAN off, leaves nothing to pick.
    await showing({ ...listening, networks: [NETWORKS[0]] })
    expect(screen.queryByRole('combobox')).toBeNull()
    cleanup()
    await showing({ on: false, networks: NETWORKS, devices: [] })
    expect(screen.queryByRole('combobox')).toBeNull()
  })

  it('sends the pick, and Automatic as no pick, from the shack', async () => {
    await showing(choosing)
    fireEvent.change(picker(), { target: { value: '10.0.0.5' } })
    await act(async () => { await Promise.resolve() })
    expect(remoteStationAction).toHaveBeenLastCalledWith({ type: 'lanAddress', address: '10.0.0.5' })
    cleanup()
    await showing({ ...listening, networks: NETWORKS, picked: '10.0.0.5' })
    expect(picker().value).toBe('10.0.0.5')
    fireEvent.change(picker(), { target: { value: '' } })
    await act(async () => { await Promise.resolve() })
    expect(remoteStationAction).toHaveBeenLastCalledWith({ type: 'lanAddress' })
  })

  it('refuses a pick made through the stream, says why, and still shows the pick it had', async () => {
    await showing(choosing)
    await pressedThroughStream(picker())
    const list = document.querySelector('[data-stream-picker]')
    expect(list, 'premise: the stream opened its list').not.toBeNull()
    await pressedThroughStream(within(list as HTMLElement).getAllByRole('option')[2])
    expect(remoteStationAction, 'a pick went to the station through the stream').not.toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toBe('Only at the station itself: this can’t be done through a stream.')
    expect(picker().value).toBe('')
  })

  it('keeps a pick this computer does not have right now in the list, so it can be seen and undone', async () => {
    await showing({ on: true, reason: 'addressGone', picked: '10.0.0.9', networks: [NETWORKS[0]], devices: [] })
    expect(within(picker()).getAllByRole('option').map(option => option.textContent))
      .toEqual(['Automatic', 'Wi-Fi, 192.168.1.20', '10.0.0.9 (not on this computer now)'])
    expect(picker().value).toBe('10.0.0.9')
  })
})

describe('Windows\' own prompt, the firewall and the name', () => {
  it('says beforehand to allow Private networks only, and only while off', async () => {
    const first = /^The first time this is turned on, Windows asks whether Nexus may use networks\. Allow Private networks only\.$/
    await showing({ on: false, devices: [] })
    expect(screen.getByText(first)).toBeTruthy()
    cleanup()
    await showing(listening)
    expect(screen.queryByText(first)).toBeNull()
  })

  it('says what the firewall stands in the way with, one sentence for each, only while listening', async () => {
    const said: [NonNullable<LanStatus['firewall']>, RegExp][] = [
      ['ask', /^When Windows asks whether Nexus may use this network, allow Private networks only\.$/],
      ['public', /^Windows calls this network Public, so its firewall keeps other computers out\./],
      ['blocked', /^Windows Firewall blocks Nexus on this network, as it does after its question is cancelled\./],
      ['blocksAll', /^Windows Firewall blocks every incoming connection on this network/],
      ['managed', /set by an administrator’s policy/],
      ['silent', /^Windows Firewall blocks new programs on this network without asking\./],
    ]
    for (const [firewall, sentence] of said) {
      await showing({ ...listening, firewall })
      expect(screen.getByText(sentence), firewall).toBeTruthy()
      cleanup()
    }
    // CONTROL: nothing in the way, and not listening, say nothing of the firewall.
    await showing(listening)
    expect(screen.queryByText(/Windows Firewall|firewall/)).toBeNull()
    cleanup()
    await showing({ ...choosing, firewall: 'public' })
    expect(screen.queryByText(/Windows calls this network Public/)).toBeNull()
  })

  it('says when Windows will not name it, so the address is typed, and plainly what it cannot see', async () => {
    const unnamed = /^Windows won’t let other computers find this station by name here, so type its address on the other computer\.$/
    const guest = /^A guest network, or one that keeps its devices apart, stops other computers reaching this one, and Nexus can’t tell that from here\.$/
    await showing({ ...listening, named: false })
    expect(screen.getByText(unnamed)).toBeTruthy()
    expect(screen.getByText(guest)).toBeTruthy()
    cleanup()
    // CONTROL: named, or not yet said, it says nothing of the name; not listening, nothing of guests.
    for (const named of [true, undefined]) {
      await showing({ ...listening, named })
      expect(screen.queryByText(unnamed)).toBeNull()
      cleanup()
    }
    await showing(choosing)
    expect(screen.queryByText(guest)).toBeNull()
  })
})

describe('the pairing code leads the card while a window is open', () => {
  // The operator, 2026-10-05: at the shack the code was hard to find among the card's other lines.
  // After Pair a computer it is the card's first content, large, with the time it has left counting
  // down, its hint, warning and Cancel beside it; the press brings it into view and screen readers
  // hear it. A window already open when the card is shown moves nothing.
  const OPENED = Date.UTC(2026, 9, 5, 12, 0, 0)
  const open: LanStatus = { ...listening, pairing: { code: '0123456789abcdef', closesAt: OPENED + 10 * 60_000 } }
  // The card's own content follows its switch; the status line is always in it.
  const card = () => screen.getByRole('status').parentElement as HTMLElement
  const scrolled = vi.fn()
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date', 'setInterval', 'clearInterval'] })
    vi.setSystemTime(OPENED)
    scrolled.mockReset()
    // jsdom has no scrollIntoView.
    Object.defineProperty(Element.prototype, 'scrollIntoView', { configurable: true, value: scrolled })
  })
  afterEach(() => {
    vi.useRealTimers()
    delete (Element.prototype as { scrollIntoView?: unknown }).scrollIntoView
  })
  const second = async (ms = 1000) => { await act(async () => { vi.advanceTimersByTime(ms) }) }

  it('CONTROL: with no window open the status line comes first, and nothing is reserved for a code', async () => {
    await showing(listening)
    expect(card().firstElementChild).toBe(screen.getByRole('status'))
  })

  it('after Pair a computer: first in the card, announced, counting down, and brought into view', async () => {
    await showing(listening)
    remoteStationAction.mockResolvedValue(status(open))
    getRemoteStationStatus.mockResolvedValue(status(open))
    const focus = vi.spyOn(HTMLElement.prototype, 'focus')
    try {
      await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
      const region = card().firstElementChild as HTMLElement
      expect(region.contains(screen.getByText('0123 4567 89ab cdef')), 'the code is the card’s first content').toBe(true)
      expect(region.getAttribute('aria-live')).toBe('polite')
      // Its hint, its warning and Cancel stay beside it.
      expect(within(region).getByText(/^Type this code in Nexus on the other computer\./)).toBeTruthy()
      expect(within(region).getByText(/^Whoever types it in time can operate this station/)).toBeTruthy()
      expect(within(region).getByRole('button', { name: 'Cancel pairing' })).toBeTruthy()
      // The time left, counting down each second. It changes every second, so it is not read out.
      const left = within(region).getByRole('timer')
      expect(left.getAttribute('aria-live')).toBe('off')
      expect(left.textContent).toBe('Expires in 10:00')
      await second()
      expect(left.textContent).toBe('Expires in 9:59')
      await second(4 * 60_000 + 54_000)
      expect(left.textContent).toBe('Expires in 5:05')
      // Never below zero while the station's next read is on its way.
      await second(6 * 60_000)
      expect(left.textContent).toBe('Expires in 0:00')
      // The press brought it into view and moved focus to it (the button pressed is gone), without
      // the focus scrolling anything by itself.
      expect(scrolled).toHaveBeenCalledTimes(1)
      expect(scrolled).toHaveBeenCalledWith({ block: 'nearest' })
      expect(scrolled.mock.contexts[0]).toBe(region)
      expect(document.activeElement).toBe(region)
      expect(focus.mock.lastCall).toEqual([{ preventScroll: true }])
    } finally {
      focus.mockRestore()
    }
  })

  it('CONTROL: a window already open when the card is shown leads the card, and moves nothing', async () => {
    await showing(open)
    const region = card().firstElementChild as HTMLElement
    expect(region.contains(screen.getByText('0123 4567 89ab cdef'))).toBe(true)
    expect(within(region).getByRole('timer').textContent).toBe('Expires in 10:00')
    expect(scrolled).not.toHaveBeenCalled()
    expect(document.activeElement).toBe(document.body)
  })

  it('CONTROL: a Pair the station refuses, or one through the stream, moves nothing', async () => {
    await showing(listening)
    remoteStationAction.mockRejectedValueOnce('lanFull')
    await pressedAtShack(screen.getByRole('button', { name: 'Pair a computer' }))
    await pressedThroughStream(screen.getByRole('button', { name: 'Pair a computer' }))
    // A later read that finds a window open (made some other way) is not this press's.
    getRemoteStationStatus.mockResolvedValue(status(open))
    await second(LAN_POLL_MS)
    expect(screen.getByText('0123 4567 89ab cdef')).toBeTruthy()
    expect(scrolled).not.toHaveBeenCalled()
  })
})
