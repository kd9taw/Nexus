// @vitest-environment jsdom
//
// THE RTTY WATERFALL'S CALLSIGN TAGS, SPAN AND SPEED, in the cockpit.
//
// A call the decoder copies as a sender is tagged on the waterfall at the frequency the decoder
// was listening on (the netted centre plus the AFC). A click on the tag nets there, through the
// same refusal a click on the picture takes: nothing while keyed, and a fresh read of the engine
// first. In a running contest a call already in the log is dimmed. The span and the speed are
// buttons on the waterfall's header, and both are kept.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, waitFor, cleanup, fireEvent, act } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import * as api from '../api'
import type { AppSnapshot, RttyState } from '../types'
import type { SpectrumRenderer } from '../spectrum'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', () => ({
  getRttyState: vi.fn(),
  rttyArm: vi.fn(),
  rttyAutoArm: vi.fn(),
  getLicensedBandPlan: vi.fn(),
  rttySend: vi.fn(),
  rttySetLatched: vi.fn(),
  rttyType: vi.fn(),
  rttyStop: vi.fn(),
  rttyClear: vi.fn(),
  rttyAfcReset: vi.fn(),
  rttyNet: vi.fn(),
  haltTx: vi.fn(),
  getSpectrumRow: vi.fn(),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./RotorStrip', () => ({ RotorStrip: () => null }))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// A renderer that draws nothing, so the waterfall's row loop runs in jsdom.
vi.mock('../spectrum', async (importActual) => {
  const actual = await importActual<typeof import('../spectrum')>()
  return {
    ...actual,
    createSpectrumRenderer: (): SpectrumRenderer => ({
      backend: 'canvas2d',
      reason: '',
      canvas: null,
      rows: 0,
      resize() {},
      commitRow() {},
      rowAt: () => null,
      clearHistory() {},
      draw() {},
      destroy() {},
    }),
  }
})

const getRttyState = api.getRttyState as ReturnType<typeof vi.fn>
const rttyAutoArm = api.rttyAutoArm as ReturnType<typeof vi.fn>
const getLicensedBandPlan = api.getLicensedBandPlan as ReturnType<typeof vi.fn>
const getSpectrumRow = api.getSpectrumRow as ReturnType<typeof vi.fn>
const rttyNet = api.rttyNet as ReturnType<typeof vi.fn>

// Netted on 1415/1585 (centre 1500), the AFC 3 Hz up: the decoder listens on 1503 Hz.
const ARMED: RttyState = {
  armed: true,
  afcHz: 3,
  afcLocked: true,
  text: 'RYRYRY ',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  markHz: 1415,
  spaceHz: 1585,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
}
const HEARD = { ...ARMED, text: 'RYRYRY CQ CQ DE W1AW W1AW K\n' }

const snapWith = (over: Record<string, unknown> = {}, fieldDay: unknown = null) =>
  ({
    mycall: 'KD9TAW',
    fieldDay,
    radio: {
      dialMhz: 14.08,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      transmitting: false,
      txEnabled: true,
      tuning: false,
      txAllowed: true,
      ...over,
    },
  }) as unknown as AppSnapshot

// The waterfall's frame clock, turned by hand.
let clock = 0
let queue: FrameRequestCallback[] = []
let realRaf: typeof requestAnimationFrame
async function frames(n: number, ms: number) {
  for (let i = 0; i < n; i++) {
    clock += ms
    const due = queue
    queue = []
    await act(async () => {
      for (const cb of due) cb(clock)
      for (let k = 0; k < 5; k++) await Promise.resolve()
    })
  }
}

beforeEach(() => {
  localStorage.clear()
  clock = 1000
  queue = []
  getRttyState.mockReset().mockResolvedValue(ARMED)
  rttyAutoArm.mockReset().mockImplementation(() => api.getRttyState())
  getLicensedBandPlan.mockReset().mockResolvedValue([])
  getSpectrumRow.mockReset().mockResolvedValue({ row: [], loHz: 0, hiHz: 4000, source: 'audio' })
  rttyNet.mockReset().mockImplementation(async (hz: number) => ({ ...HEARD, markHz: hz - 85, spaceHz: hz + 85, afcHz: 0 }))
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => null)
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  realRaf = globalThis.requestAnimationFrame
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => queue.push(cb)) as typeof requestAnimationFrame
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener: () => {}, removeEventListener: () => {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(() => {
  cleanup()
  globalThis.requestAnimationFrame = realRaf
  vi.restoreAllMocks()
})

const settle = (ms: number) => act(() => new Promise((r) => setTimeout(r, ms)))

/** Mount on ARMED, then let the decoder copy `HEARD` on the next poll (500 ms on). */
async function mountHeard(snapOver: Record<string, unknown> = {}, fieldDay: unknown = null) {
  const r = render(<RttyCockpit snap={snapWith(snapOver, fieldDay)} />)
  await waitFor(() => expect(r.container.querySelector('.waterfall-canvas')).not.toBeNull())
  await settle(50)
  getRttyState.mockResolvedValue(HEARD)
  await waitFor(() => expect(screen.getByRole('button', { name: 'W1AW' })).toBeTruthy(), { timeout: 3000 })
  return r
}
const tag = () => screen.getByRole('button', { name: 'W1AW' }) as HTMLButtonElement

describe('a decoded call is tagged at its frequency', () => {
  it('W1AW, sent as DE W1AW, sits at 1503 Hz: 46.5 % across the 200–3000 Hz view', async () => {
    await mountHeard()
    expect(parseFloat(tag().style.left)).toBeCloseTo(((1503 - 200) / 2800) * 100, 6)
    expect(tag().title).toBe('Net onto W1AW at 1503 Hz. This moves the transmit tones too.')
    expect(tag().className).toBe('wf-tag')
  })

  it('history in the ring when the cockpit opens is not tagged (it was copied wherever the decoder was)', async () => {
    getRttyState.mockResolvedValue(HEARD)
    render(<RttyCockpit snap={snapWith()} />)
    await settle(1200)
    expect(screen.queryByRole('button', { name: 'W1AW' })).toBeNull()
  })
})

describe('a click on the tag nets there — and does nothing while keyed', () => {
  it('idle: the click nets the decoder on 1503 Hz', async () => {
    await mountHeard()
    fireEvent.click(tag())
    await waitFor(() => expect(rttyNet).toHaveBeenCalledWith(1503))
    expect(rttyNet).toHaveBeenCalledTimes(1)
  })

  it('an over that keyed since the last poll: the click re-reads the engine and does nothing', async () => {
    await mountHeard()
    expect(tag().disabled).toBe(false)
    getRttyState.mockResolvedValue({ ...HEARD, sending: true })
    fireEvent.click(tag())
    await settle(100)
    expect(rttyNet, 'a tag moved the transmit tones mid-over').not.toHaveBeenCalled()
  })

  for (const [what, state, snapOver] of [
    ['an over on the air', { ...HEARD, sending: true }, {}],
    ['continuous TX latched', { ...HEARD, latched: true }, {}],
    ['a tune carrier', HEARD, { tuning: true }],
  ] as const) {
    it(`${what}: the tag is disabled and a click nets nowhere`, async () => {
      await mountHeard(snapOver)
      getRttyState.mockResolvedValue(state)
      await waitFor(() => expect(tag().disabled).toBe(true), { timeout: 3000 })
      fireEvent.click(tag())
      await settle(100)
      expect(rttyNet).not.toHaveBeenCalled()
    })
  }
})

describe('in a running contest a call already worked is dimmed', () => {
  const contest = (log: unknown[]) => ({ running: true, log, club: { syncState: 'synced', dupes: [], board: [] } })

  it('W1AW in the log on 20 m RTTY: dimmed, and the title says why', async () => {
    await mountHeard({}, contest([{ call: 'W1AW', class: '', section: '', band: '20m', mode: 'DIG', submode: 'RTTY' }]))
    expect(tag().className).toBe('wf-tag dim')
    expect(tag().title).toBe('W1AW at 1503 Hz is already in the contest log. Click to net onto it.')
  })

  it('worked on another band: not dimmed', async () => {
    await mountHeard({}, contest([{ call: 'W1AW', class: '', section: '', band: '40m', mode: 'DIG', submode: 'RTTY' }]))
    expect(tag().className).toBe('wf-tag')
  })

  it('no contest running: the same log does not dim it', async () => {
    await mountHeard({}, { ...contest([{ call: 'W1AW', band: '20m', mode: 'DIG' }]), running: false })
    expect(tag().className).toBe('wf-tag')
  })
})

describe('span and speed are buttons, and both are kept', () => {
  it('the speed: Slow draws a quarter of the rows Fast does, and is still Slow after a remount', async () => {
    vi.spyOn(performance, 'now').mockImplementation(() => clock)
    const { container } = render(<RttyCockpit snap={snapWith()} />)
    await waitFor(() => expect(container.querySelector('.waterfall-canvas')).not.toBeNull())
    const speed = screen.getByRole('group', { name: 'Waterfall speed' })
    const btn = (label: string) => Array.from(speed.querySelectorAll('button')).find((b) => b.textContent === label)!
    expect(btn('Fast').getAttribute('aria-pressed')).toBe('true')
    await frames(2, 50 / 3)
    getSpectrumRow.mockClear()
    await frames(120, 50 / 3) // 2 s
    expect(getSpectrumRow.mock.calls.length).toBe(40)
    fireEvent.click(btn('Slow'))
    expect(localStorage.getItem('nexus.rtty.waterfall.speed')).toBe('slow')
    await frames(2, 50 / 3)
    getSpectrumRow.mockClear()
    await frames(120, 50 / 3)
    expect(getSpectrumRow.mock.calls.length).toBe(10)
    cleanup()
    render(<RttyCockpit snap={snapWith()} />)
    await waitFor(() => expect(screen.getByRole('group', { name: 'Waterfall speed' })).toBeTruthy())
    const again = screen.getByRole('group', { name: 'Waterfall speed' })
    expect(Array.from(again.querySelectorAll('button')).find((b) => b.textContent === 'Slow')!.getAttribute('aria-pressed')).toBe('true')
  })

  it('the span: buttons in place of the zoom select; 1k puts the tag at 50.3 % of 1000–2000 Hz, and is kept', async () => {
    await mountHeard()
    expect(screen.queryByRole('combobox', { name: 'Waterfall zoom span' })).toBeNull()
    const span = screen.getByRole('group', { name: 'Waterfall span' })
    fireEvent.click(Array.from(span.querySelectorAll('button')).find((b) => b.textContent === '1k')!)
    expect(localStorage.getItem('nexus.waterfall.rtty.zoom')).toBe('1000')
    expect(parseFloat(tag().style.left)).toBeCloseTo(50.3, 6)
  })
})
