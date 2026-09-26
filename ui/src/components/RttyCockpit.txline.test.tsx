// @vitest-environment jsdom
//
// THE TX LINE (#379) — "the field that shows what I'm transmitting", at the bottom of the RTTY
// cockpit. The report came from a CQ WW RTTY weekend: F-key overs went out with nothing on
// screen saying what they said, which is the uncertainty an unanswered call leaves.
//
// What this pins, against the component that ships:
//   · it renders the ENGINE's over (`txText` / `txKeyed` / `txCut`) — keyed part, still-to-go
//     part, and a stopped over's never-sent part — each by a class that carries a non-colour
//     mark in the sheet (RttyCockpit.sentstyle.test.tsx proves the transcript half of that);
//   · it sits in the TX dock's compose row, outside every pane, and is no control at all;
//   · continuous-TX typing still goes through the compose field with it on screen;
//   · a station too old to send the fields leaves it empty instead of throwing.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, waitFor } from '@testing-library/react'
import { RttyCockpit } from './RttyCockpit'
import * as api from '../api'
import type { AppSnapshot, RttyState } from '../types'
import type { PanelLayoutApi, RttyPanelId } from '../features/panelState'
import { RTTY_PANEL_IDS } from '../features/panelState'
import { t } from '../i18n'

globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver

const IDLE: RttyState = {
  armed: true,
  afcHz: 0,
  afcLocked: false,
  text: '',
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
}
const state: { current: RttyState } = { current: IDLE }

vi.mock('../api', () => ({
  getRttyState: vi.fn(async () => state.current),
  rttyAutoArm: vi.fn(async () => state.current),
  getLicensedBandPlan: vi.fn(async () => []),
  rttyType: vi.fn(async () => state.current),
  rttySend: vi.fn(async () => state.current),
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

const line = () => document.querySelector('.cockpit-txdock .rtty-txline') as HTMLElement
const plate = () => line().querySelector('.rtty-txline-plate')?.textContent
/** A part's visible text — the screen-reader label inside it excluded. */
const part = (cls: string) => {
  const el = line().querySelector(`.${cls}`)
  if (!el) return null
  return [...el.childNodes]
    .filter((n) => !(n instanceof HTMLElement && n.classList.contains('sr-only')))
    .map((n) => n.textContent)
    .join('')
}

async function show(s: Partial<RttyState>, props: Partial<Parameters<typeof RttyCockpit>[0]> = {}) {
  state.current = { ...IDLE, ...s }
  render(<RttyCockpit snap={snap} {...props} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

beforeEach(() => {
  state.current = IDLE
  ;(api.rttyType as ReturnType<typeof vi.fn>).mockClear()
})
afterEach(cleanup)

// The F2 contest exchange, framed for the air (a line of its own, ending in a space), with nine
// characters keyed: CR, LF and "W1AW 59".
const OVER = '\r\nW1AW 599 4 WI 4 WI '

describe('RTTY TX line (#379)', () => {
  it('shows the over going out, the keyed part apart from what is still to go', async () => {
    await show({ sending: true, txText: OVER, txKeyed: 9, txCut: false })
    expect(plate()).toBe(t('rtty.txline.sending'))
    expect(line().classList.contains('live')).toBe(true)
    // The on-air line break is drawn, not swallowed: it took air time, and it is what the
    // underline has already covered.
    expect(part('rtty-txline-keyed')).toBe('↵W1AW 59')
    expect(part('rtty-txline-pending')).toBe('9 4 WI 4 WI ')
    expect(line().querySelector('.rtty-txline-unsent')).toBeNull()
    // Named for a screen reader, which cannot see the underline.
    expect(line().querySelector('.rtty-txline-keyed .sr-only')?.textContent).toBe(`${t('rtty.txline.keyed.sr')} `)
    expect(line().querySelector('.rtty-txline-pending .sr-only')?.textContent).toBe(`${t('rtty.txline.pending.sr')} `)
  })

  it('keeps the last over once the air is quiet, all of it gone', async () => {
    await show({ sending: false, txText: OVER, txKeyed: [...OVER].length, txCut: false })
    expect(plate()).toBe(t('rtty.txline.sent'))
    expect(part('rtty-txline-keyed')).toBe('↵W1AW 599 4 WI 4 WI ')
    expect(line().querySelector('.rtty-txline-pending')).toBeNull()
  })

  it('marks what a Stop kept off the air, apart from what still-to-go would mean', async () => {
    await show({ sending: false, txText: OVER, txKeyed: 9, txCut: true })
    expect(plate()).toBe(t('rtty.txline.stopped'))
    expect(part('rtty-txline-keyed')).toBe('↵W1AW 59')
    expect(part('rtty-txline-unsent')).toBe('9 4 WI 4 WI ')
    expect(line().querySelector('.rtty-txline-pending')).toBeNull()
    expect(line().querySelector('.rtty-txline-unsent .sr-only')?.textContent).toBe(`${t('rtty.txline.unsent.sr')} `)
  })

  it('keeps the recent tail of a long over in front of the keying point', async () => {
    const long = 'CQ CQ CQ DE W9XYZ W9XYZ W9XYZ PSE K CQ CQ DE W9XYZ K '
    await show({ sending: true, txText: long, txKeyed: 50, txCut: false })
    const keyed = part('rtty-txline-keyed')!
    expect(keyed.startsWith('…')).toBe(true)
    expect(keyed.slice(1)).toBe(long.slice(50 - 32, 50))
    expect(part('rtty-txline-pending')).toBe(long.slice(50))
  })

  it('is empty before the first over — and for a station too old to send the fields', async () => {
    await show({}) // no txText at all: an older station's shape
    expect(plate()).toBe('TX')
    expect(line().textContent).toContain(t('rtty.txline.empty'))
    cleanup()
    await show({ sending: true }) // an older station mid-over: still no text to show
    expect(plate()).toBe(t('rtty.txline.sending'))
    expect(line().querySelector('.rtty-txline-keyed, .rtty-txline-pending, .rtty-txline-unsent')).toBeNull()
  })

  it('sits in the dock’s compose row, outside every pane, and is no control', async () => {
    const allHidden: PanelLayoutApi<RttyPanelId> = {
      layout: { v: 1, state: {}, share: {} },
      stateOf: () => 'removed',
      setPanelState: () => {},
      shareOf: () => 1,
      setShare: () => {},
      setShares: () => {},
      undo: () => {},
      canUndo: false,
      undoRemoves: [],
      reset: () => {},
    }
    // Every ⊞ id hidden at once: the line is not in the vocabulary, so it cannot go with them.
    expect(RTTY_PANEL_IDS).toEqual(['scope', 'stream'])
    await show({ sending: true, txText: OVER, txKeyed: 9 }, { panels: allHidden })
    const el = line()
    expect(el, 'no TX line in the dock').not.toBeNull()
    expect(el.parentElement?.classList.contains('cw-send')).toBe(true)
    expect(el.closest('.pane-frame')).toBeNull()
    // Nothing in it can be clicked, focused or typed into — it starts and stops nothing.
    expect(el.querySelectorAll('button, input, textarea, select, a, [tabindex]').length).toBe(0)
    expect(el.hasAttribute('tabindex')).toBe(false)
    // It sits BEFORE the compose field, which is still the row's input.
    const compose = screen.getByLabelText(t('rtty.compose.aria'))
    expect(el.compareDocumentPosition(compose) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('leaves continuous-TX typing in the compose field, with the line showing the engine’s account', async () => {
    await show({ latched: true, sending: true, txText: 'CQ CQ', txKeyed: 2 })
    const input = screen.getByLabelText(t('rtty.compose.aria')) as HTMLInputElement
    const insert = (data: string) =>
      input.dispatchEvent(
        new (window as unknown as { InputEvent: typeof InputEvent }).InputEvent('beforeinput', {
          inputType: 'insertText',
          data,
          bubbles: true,
          cancelable: true,
        }),
      )
    insert('D')
    insert('E')
    expect((api.rttyType as ReturnType<typeof vi.fn>).mock.calls.map((c: unknown[]) => c[0])).toEqual(['D', 'E'])
    await waitFor(() => expect(input.value).toBe('DE'))
    // The line is the engine's word on what went out, not an echo of the field.
    expect(part('rtty-txline-keyed')).toBe('CQ')
    expect(part('rtty-txline-pending')).toBe(' CQ')
  })
})
