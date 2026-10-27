// @vitest-environment jsdom
//
// HELD MESSAGES IN THE JS8 COCKPIT: JS8Call's "Store Message…" on a heard station, and the
// Inbox's notes of what a cap took out. Neither keys anything: storing asks the engine to hold a
// message (`js8_store_message`), which goes on the air only when that station asks for it, through
// every automatic-reply gate; the notes are read-only until the operator clears them.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, waitFor } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import { Js8InboxDropped } from './Js8InboxDropped'
import * as api from '../api'
import type { AppSnapshot, Js8InboxDrop, Js8InboxEntry, Js8State } from '../types'
import { t } from '../i18n'
import { utcClock } from '../js8Vocab'

// THE BUDGET: the house's 15 s, as the other cockpit suites carry it.
vi.setConfig({ testTimeout: 15_000 })

const row = (over: Partial<Js8InboxEntry> = {}): Js8InboxEntry => ({
  id: 7,
  from: 'W1AW',
  to: 'K1ABC',
  text: 'NET AT EIGHT',
  path: [],
  state: 'store',
  atMs: 1_757_000_000_000,
  freqHz: 1750,
  snrDb: -5,
  ...over,
})
const drop = (why: 'count' | 'bytes', over: Partial<Js8InboxEntry> = {}): Js8InboxDrop => ({
  atMs: 1_757_000_100_000,
  why,
  entry: row(over),
})

const base = (): Js8State => ({
  speed: 'normal',
  rxSpeeds: 15,
  txEnabled: false,
  sending: false,
  hbOn: false,
  hbNextAtMs: null,
  hbIntervalMin: 0,
  cqOn: false,
  cqNextAtMs: null,
  cqIntervalMin: 0,
  autoreply: true,
  relay: true,
  hbAck: false,
  armed: { autoreply: false, relay: false, hbAck: false, hb: false, cq: false },
  notArmed: { autoreply: 'txOff', relay: 'txOff', hbAck: null, hb: null, cq: null },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [
    { call: 'W1AW', grid: 'FN31', snrDb: -3, freqHz: 1500, speed: 'normal', lastMs: Date.now(), lastHb: true, lastCq: false, storedMsgs: 0 },
  ],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: base() }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  const s = () => state.current
  return {
    ...auto,
    getJs8State: vi.fn(async () => s()),
    js8Enter: vi.fn(async () => s()),
    js8Composer: vi.fn(async () => null),
    js8LocatorRefusal: vi.fn(async () => null),
    js8StoreMessage: vi.fn(async () => s()),
    js8InboxDroppedClear: vi.fn(async () => s()),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
const toast = vi.hoisted(() => ({ pushToast: vi.fn() }))
vi.mock('../toast', () => ({
  pushToast: toast.pushToast,
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const js8StoreMessage = api.js8StoreMessage as ReturnType<typeof vi.fn>
const js8InboxDroppedClear = api.js8InboxDroppedClear as ReturnType<typeof vi.fn>

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: { dialMhz: 14.078, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, txAllowed: true, rxOffsetHz: 1500, txOffsetHz: 1500, txLevel: 0.5 },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = base()
  js8StoreMessage.mockReset()
  js8StoreMessage.mockImplementation(async () => state.current)
  js8InboxDroppedClear.mockClear()
  toast.pushToast.mockClear()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit() {
  const r = render(<Js8Cockpit snap={snap} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

const storeButton = () => screen.getByRole('button', { name: t('js8.station.store.label') })
const messageField = () => screen.getByRole('textbox', { name: t('js8.store.aria', { call: 'W1AW' }) })

describe('Store message… on a heard station', () => {
  it('opens for that station, upper-cases as JS8Call does, and asks the engine to hold it', async () => {
    await renderCockpit()
    expect(storeButton().getAttribute('title')).toBe(t('js8.station.store.title', { call: 'W1AW' }))
    expect(screen.queryByRole('dialog')).toBeNull()
    fireEvent.click(storeButton())
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect(screen.getByText(t('js8.store.label', { call: 'W1AW' }))).toBeTruthy()
    const save = screen.getByRole('button', { name: t('js8.store.save') })
    expect((save as HTMLButtonElement).disabled, 'nothing to store yet').toBe(true)
    fireEvent.change(messageField(), { target: { value: 'see you friday' } })
    expect((messageField() as HTMLInputElement).value).toBe('SEE YOU FRIDAY')
    fireEvent.click(screen.getByRole('button', { name: t('js8.store.save') }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(js8StoreMessage.mock.calls).toEqual([['W1AW', 'SEE YOU FRIDAY']])
    expect(toast.pushToast).toHaveBeenCalledWith(t('js8.store.done', { call: 'W1AW' }), 'success')
  })

  it('stores on Enter, and shows the engine\'s refusal in the box with the text kept', async () => {
    js8StoreMessage.mockImplementation(async () => {
      throw 'Handing that message over needs 30 frames; the cap at this speed is 19.'
    })
    await renderCockpit()
    fireEvent.click(storeButton())
    fireEvent.change(messageField(), { target: { value: 'a long one' } })
    fireEvent.keyDown(messageField(), { key: 'Enter' })
    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toBe(
        'Handing that message over needs 30 frames; the cap at this speed is 19.',
      ),
    )
    expect(screen.getByRole('dialog')).toBeTruthy()
    expect((messageField() as HTMLInputElement).value).toBe('A LONG ONE')
  })

  it('Cancel closes it and stores nothing', async () => {
    await renderCockpit()
    fireEvent.click(storeButton())
    fireEvent.change(messageField(), { target: { value: 'never mind' } })
    fireEvent.click(screen.getByRole('button', { name: t('js8.store.cancel') }))
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(js8StoreMessage).not.toHaveBeenCalled()
  })
})

describe('the Inbox says which message a cap took out, and why', () => {
  it('shows each note with the message and its reason, and clears them on the operator\'s word', async () => {
    state.current = { ...base(), inboxDropped: [drop('count'), drop('bytes', { id: 8, from: 'N0XYZ', to: 'KD9TAW', text: 'HI', state: 'unread' })] }
    await renderCockpit()
    const notes = document.querySelector('.js8-inbox-dropped')
    expect(notes?.getAttribute('role')).toBe('status')
    const rows = Array.from(notes?.querySelectorAll('.js8-inbox-row') ?? []).map((r) => r.textContent)
    expect(rows).toEqual([
      `${t('js8.inbox.dropped.chip')}${utcClock(1_757_000_100_000)}${t('js8.inbox.state.store')}W1AW → K1ABCNET AT EIGHT${t('js8.inbox.dropped.count.held')}`,
      `${t('js8.inbox.dropped.chip')}${utcClock(1_757_000_100_000)}${t('js8.inbox.state.unread')}N0XYZ → KD9TAWHI${t('js8.inbox.dropped.bytes.mine')}`,
    ])
    fireEvent.click(screen.getByRole('button', { name: t('js8.inbox.dropped.clear.label') }))
    await waitFor(() => expect(js8InboxDroppedClear).toHaveBeenCalledTimes(1))
  })

  it('says nothing while there are no notes', async () => {
    await renderCockpit()
    expect(document.querySelector('.js8-inbox-dropped')).toBeNull()
  })
})

describe('each reason in its own words', () => {
  it('names the cap and whose mail it was', () => {
    render(
      <Js8InboxDropped
        drops={[
          drop('count'),
          drop('bytes', { id: 2, state: 'delivered' }),
          drop('count', { id: 3, state: 'read' }),
          drop('bytes', { id: 4, state: 'unread' }),
        ]}
        onClear={() => {}}
      />,
    )
    const reasons = Array.from(document.querySelectorAll('.js8-inbox-row .js8-path')).map((e) => e.textContent)
    expect(reasons).toEqual([
      t('js8.inbox.dropped.count.held'),
      t('js8.inbox.dropped.bytes.held'),
      t('js8.inbox.dropped.count.mine'),
      t('js8.inbox.dropped.bytes.mine'),
    ])
  })
})
