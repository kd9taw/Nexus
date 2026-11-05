// @vitest-environment jsdom
//
// #448 — JS8 TAKES A TYPED Rx/Tx OFFSET, as the FT8/FT4 header does (WSJT-X's Rx/Tx Hz
// spinners). Before this the JS8 offsets moved only by a waterfall click, a table double-click or
// a menu jump. Each field moves its own offset and nothing else, through the same engine calls
// the waterfall uses, rounded and clamped to 200–4000 Hz as FT's are. The TX field stands
// disabled exactly where the engine would refuse the move (a message waiting or going out, and
// "Allow TX offset changes while sending" off), so a typed value cannot sit in the box as taken.
// Nothing here keys or changes when anything is sent.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent } from '@testing-library/react'
import { Js8Cockpit } from './Js8Cockpit'
import * as api from '../api'
import { t } from '../i18n'
import type { AppSnapshot, Js8State } from '../types'

vi.setConfig({ testTimeout: 15_000 })

const js8 = (): Js8State => ({
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
  armed: { autoreply: false, cq: false, relay: false, hbAck: false, hb: false },
  idleMinutes: 0,
  idleLimitMin: 60,
  idleTripped: false,
  activity: [],
  stations: [],
  inbox: [],
  queue: [],
  pendingReply: null,
  lastError: null,
})
const state: { current: Js8State } = { current: js8() }

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getJs8State: vi.fn(async () => state.current),
    js8Enter: vi.fn(async () => state.current),
    getLicensedBandPlan: vi.fn(async () => []),
    setRxOffset: vi.fn(async () => ({})),
    setTxOffset: vi.fn(async () => ({})),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The fields stand right of the readout (`frequencyExtras`); the rest of the header is
// covered by its own suites.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: (p: { frequencyExtras?: ReactNode }) => <header className="cockpit-header">{p.frequencyExtras}</header>,
}))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN61',
  radio: {
    dialMhz: 7.078,
    band: '40m',
    catOk: true,
    sideband: 'USB',
    transmitting: false,
    txEnabled: false,
    txAllowed: true,
    rxOffsetHz: 1500,
    txOffsetHz: 1200,
    txLevel: 0.5,
  },
} as unknown as AppSnapshot

beforeEach(() => {
  state.current = js8()
  window.localStorage.clear()
  vi.clearAllMocks()
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

async function renderCockpit(props: Partial<Parameters<typeof Js8Cockpit>[0]> = {}) {
  const r = render(<Js8Cockpit snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}
const field = (label: 'Rx' | 'Tx') => {
  const el = document.querySelector<HTMLInputElement>(
    `.cockpit-header input[aria-label="${t('operate.header.df.aria', { label })}"]`,
  )
  expect(el, `no ${label} offset field in the JS8 header`).not.toBeNull()
  return el!
}
async function type(el: HTMLInputElement, text: string) {
  await act(async () => {
    fireEvent.focus(el)
    fireEvent.change(el, { target: { value: text } })
    fireEvent.blur(el)
  })
}

describe('JS8 typed audio offsets (#448)', () => {
  it('shows both offsets as the engine has them', async () => {
    await renderCockpit()
    expect(field('Rx').value).toBe('1500')
    expect(field('Tx').value).toBe('1200')
  })

  it('a typed Rx moves the RX offset only, and a typed Tx the TX offset only', async () => {
    await renderCockpit()
    await type(field('Rx'), '1234')
    expect(api.setRxOffset).toHaveBeenCalledWith(1234)
    expect(api.setTxOffset).not.toHaveBeenCalled()
    vi.mocked(api.setRxOffset).mockClear()
    await type(field('Tx'), '1750')
    expect(api.setTxOffset).toHaveBeenCalledWith(1750)
    expect(api.setRxOffset).not.toHaveBeenCalled()
  })

  it("rounds and clamps to FT's 200–4000 Hz, reverts garbage, and sends nothing for no change", async () => {
    await renderCockpit()
    await type(field('Tx'), '9000')
    expect(api.setTxOffset).toHaveBeenCalledWith(4000)
    await type(field('Rx'), '99.6')
    expect(api.setRxOffset).toHaveBeenCalledWith(200)
    vi.mocked(api.setRxOffset).mockClear()
    await type(field('Rx'), 'abc')
    expect(field('Rx').value).toBe('1500')
    await type(field('Rx'), '1500')
    expect(api.setRxOffset, 'the offset it already has is not a move').not.toHaveBeenCalled()
  })

  it('holds the TX field while a message waits to go, as the engine holds the offset; RX stays free', async () => {
    state.current = { ...js8(), queue: [{ origin: 'operator', display: 'X', first: true, last: true }] }
    await renderCockpit()
    expect(field('Tx').disabled).toBe(true)
    expect(field('Tx').closest('label')?.getAttribute('title')).toBe(t('js8.header.txOffset.held'))
    expect(field('Rx').disabled).toBe(false)
  })

  it('leaves the TX field free while sending when the operator allows offset changes then', async () => {
    state.current = { ...js8(), sending: true, queue: [{ origin: 'operator', display: 'X', first: true, last: true }] }
    await renderCockpit({ txOffsetWhileSending: true })
    expect(field('Tx').disabled).toBe(false)
  })
})
