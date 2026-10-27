// @vitest-environment jsdom
//
// WHERE THE TUNING SCOPE STANDS IN THE RTTY COCKPIT, rendered rather than read off a sheet:
//   · RIGHT of the waterfall, in the waterfall's own strip (the strip its height divider sizes), so the
//     waterfall keeps the common height and the scope keeps its size; the waterfall gives up the width;
//   · showing or hiding it never re-parents or remounts the waterfall (the strip is always there);
//   · it is the same with the decoded text hidden, and with the waterfall hidden it holds the strip alone;
//   · the decoded text carries nothing of it (no floor, no row): the transcript is the stream's own child;
//   · its only controls are the two nudges and its ✕. It hosts no stop and no sender (THE STOP LINE).
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, act, within } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import type { AppSnapshot, RttyState } from '../types'
import type { PanelLayoutApi, RttyPanelId } from '../features/panelState'

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

const ARMED = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: 'CQ TEST',
  charConf: [],
  baud: 45.45,
  shiftHz: 170,
  markHz: 2125,
  spaceHz: 2295,
  backend: 'afsk',
  sending: false,
  latched: false,
  keyerError: null,
  auto: false,
  seqState: 'idle',
  peer: null,
  peerExchange: [],
  heardCq: null,
} as unknown as RttyState

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => ARMED),
  rttyAutoArm: vi.fn(async () => ARMED),
  getRttyScope: vi.fn(async () => ({ live: true, seq: 1, peak: 0.4, xy: [], offsetHz: null })),
  getLicensedBandPlan: vi.fn(async () => []),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))

const snap = {
  mycall: 'W9XYZ',
  radio: { dialMhz: 14.083, band: '20m', catOk: true, sideband: 'LSB', transmitting: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

function panels(removed: RttyPanelId[]): PanelLayoutApi<RttyPanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id: RttyPanelId) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  } as unknown as PanelLayoutApi<RttyPanelId>
}

async function show(removed: RttyPanelId[]) {
  const r = render(<RttyCockpit snap={snap} panels={panels(removed)} onSetTxEnabled={() => {}} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

afterEach(cleanup)

describe('the RTTY tuning scope in its cockpit', () => {
  it('stands right of the waterfall, in the strip its divider sizes', async () => {
    const { container } = await show([])
    const strip = container.querySelector('main.rtty-cockpit > .rtty-wf-row') as HTMLElement
    expect(strip, 'no waterfall strip under the shell').not.toBeNull()
    expect([...strip.children].map((k) => k.className)).toEqual(['waterfall-wrap', 'rtty-tuning'])
    // The divider under the strip sizes the strip (it is the strip's next sibling).
    expect(strip.nextElementSibling?.matches('.pane-splitter.horizontal')).toBe(true)
    expect(container.querySelector('[data-pane="stream"] .rtty-tuning'), 'a second scope in the text pane').toBeNull()
    expect(container.querySelector('[data-pane="tuning"]'), 'no frame of its own').toBeNull()
  })

  it('showing and hiding it keeps the same waterfall element', async () => {
    const { container, rerender } = await show([])
    const wf = container.querySelector('.waterfall-wrap')
    const strip = container.querySelector('.rtty-wf-row')
    for (const removed of [['tuning'], [], ['tuning']] as RttyPanelId[][]) {
      rerender(<RttyCockpit snap={snap} panels={panels(removed)} onSetTxEnabled={() => {}} />)
      expect(container.querySelector('.waterfall-wrap'), `the waterfall remounted (hidden: ${removed})`).toBe(wf)
      expect(container.querySelector('.rtty-wf-row'), 'the strip remounted').toBe(strip)
      expect([...strip!.children].map((k) => k.className)).toEqual(removed.length ? ['waterfall-wrap'] : ['waterfall-wrap', 'rtty-tuning'])
    }
  })

  it('hosts the two nudges and its ✕, and nothing that starts or stops a transmission', async () => {
    await show([])
    const group = screen.getByRole('group', { name: 'Tuning' })
    const names = within(group)
      .getAllByRole('button')
      .map((b) => b.getAttribute('aria-label'))
    expect(names).toEqual(['Hide Tuning', 'Tune down 5 Hz', 'Tune up 5 Hz'])
  })

  it('with the text hidden, stays in the strip', async () => {
    const { container } = await show(['stream'])
    const strip = container.querySelector('main.rtty-cockpit > .rtty-wf-row') as HTMLElement
    expect([...strip.children].map((k) => k.className)).toEqual(['waterfall-wrap', 'rtty-tuning'])
    expect(within(strip).getAllByRole('button', { name: 'Hide Tuning' })).toHaveLength(1)
    expect(container.querySelector('[data-pane="tuning"]')).toBeNull()
  })

  it('with the waterfall hidden, holds the strip alone', async () => {
    const { container } = await show(['scope'])
    const strip = container.querySelector('main.rtty-cockpit > .rtty-wf-row') as HTMLElement
    expect(strip, 'the scope vanished with a waterfall the operator hid').not.toBeNull()
    expect([...strip.children].map((k) => k.className)).toEqual(['rtty-tuning'])
  })

  it('asks nothing of the decoded text: no floor, no row, the transcript is the stream\'s own child', async () => {
    const { container } = await show([])
    const shell = container.querySelector('main.rtty-cockpit') as HTMLElement
    const stream = container.querySelector('[data-pane="stream"]') as HTMLElement
    expect(shell.classList.contains('rtty-tuning-on')).toBe(false)
    expect(stream.style.minHeight).not.toContain('--rtty-stream-floor')
    expect(container.querySelector('.rtty-rx-row')).toBeNull()
    expect(stream.querySelector('.rtty-stream > .cw-decode-text')).not.toBeNull()
  })

  it('with both hidden, there is no strip and no divider', async () => {
    const { container } = await show(['scope', 'tuning'])
    expect(container.querySelector('.rtty-wf-row')).toBeNull()
    expect(container.querySelector('.rtty-tuning')).toBeNull()
  })
})
