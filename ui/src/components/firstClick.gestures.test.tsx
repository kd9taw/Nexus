// @vitest-environment jsdom
//
// Every gesture starts afresh: a double-click acts on the row its OWN first click landed on, never on
// a row recorded by an earlier, finished gesture (the first version of this rule kept its record
// across gestures, and only a click on a row replaced it).
//   · The Stations cards: a double-click anywhere on a card (its Work button, its body, its path line)
//     begins on that card, not on the card an earlier click selected.
//   · The roster and the decode panes: a gesture whose first click lands on no row (blank space, a gap)
//     works nothing, even when a row slides under its second click. A browser fires the first click's
//     press and click on that blank space (detail 1), which is what the list records the gesture by.
import { it, expect, vi, afterEach } from 'vitest'
import { render, cleanup, fireEvent } from '@testing-library/react'
import { StationList } from './StationList'
import { OperateDecodes } from './OperateDecodes'
import { OperateRoster } from './OperateRoster'
import type { DecodeRow, NeedTag, Station } from '../types'

vi.mock('../api', async (orig) => ({ ...((await orig()) as object), getDeclination: vi.fn(() => Promise.resolve(0)) }))
afterEach(cleanup)

const st = (call: string, extra: Partial<Station> = {}): Station =>
  ({ call, grid: 'JN57', snr: -15, lastHeardSlot: 10, heardCount: 1, presence: 'active', worked: false, ...extra }) as Station

function cards(stations: Station[]) {
  const onCall = vi.fn(), onSelect = vi.fn()
  const view = render(<StationList stations={stations} myGrid="EN52" currentSlot={10} activePeer={null} unreadByPeer={{}}
      needByCall={new Map<string, NeedTag>()} onSelect={onSelect} onCall={onCall} conversations={[]}
      onArchive={() => {}} bandActive={false} bandUnread={0} onSelectBand={() => {}} />)
  const card = (call: string) => [...view.container.querySelectorAll<HTMLElement>('.station-card')].find((x) => x.querySelector('.station-call')?.textContent === call)!
  return { onCall, onSelect, card }
}

/** The first click of a gesture, as a browser sends it: a press and a click, click count 1. */
function firstClickOn(el: Element) {
  fireEvent.mouseDown(el, { detail: 1 })
  fireEvent.mouseUp(el, { detail: 1 })
  fireEvent.click(el, { detail: 1 })
}

it("a card's Work button double-clicked after another card was selected works that card, every time", () => {
  const { onCall, card } = cards([st('AAAA'), st('BBBB')])
  fireEvent.click(card('AAAA').querySelector('.station-open')!, { detail: 1 }) // an earlier, separate selection
  const w = card('BBBB').querySelector('.station-work')!
  fireEvent.click(w, { detail: 1 }); fireEvent.click(w, { detail: 2 }); fireEvent.doubleClick(w, { detail: 2 })
  expect(onCall.mock.calls.map((c) => c[0])).toEqual(['BBBB', 'BBBB', 'BBBB'])
})

it("a card's body double-clicked after another card was selected works that card", () => {
  const { onCall, card } = cards([st('AAAA'), st('BBBB')])
  fireEvent.click(card('AAAA').querySelector('.station-open')!, { detail: 1 })
  firstClickOn(card('BBBB'))
  fireEvent.click(card('BBBB'), { detail: 2 })
  fireEvent.doubleClick(card('BBBB'), { detail: 2 })
  expect(onCall.mock.calls.map((c) => c[0])).toEqual(['BBBB'])
})

const row = (message: string, freqHz: number): DecodeRow =>
  ({ from: message.split(' ')[1], snr: -10, dtSec: 0.1, freqHz, message, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: -1 }) as DecodeRow

it('decodes: a gesture begun on blank feed space works nothing, though a row slides under its second click', () => {
  const A = row('CQ KM7CAB DN17', 900), B = row('CQ KB2DJJ FN20', 1200)
  const onCall = vi.fn(), onSelectDecode = vi.fn()
  const props = { rxOffsetHz: 1500, band: '20m', tier: 'FT8' as const, harqRescues: 0, onCall, onSelectDecode, onToggleIgnore: vi.fn() }
  const v = render(<OperateDecodes decodes={[A]} slot={11} clearTick={0} {...props} />)
  const rf = (m: string) => [...v.container.querySelectorAll<HTMLElement>('.decode-row')].find((r) => r.textContent?.includes(m))!
  fireEvent.click(rf('CQ KM7CAB'), { detail: 1 }) // minutes earlier: KM7CAB selected
  firstClickOn(v.container.querySelector('[role=listbox]')!) // the gesture's first click, on blank space
  v.rerender(<OperateDecodes decodes={[A, B]} slot={11} clearTick={0} {...props} />) // KB2DJJ lands under the pointer
  fireEvent.click(rf('CQ KB2DJJ'), { detail: 2 })
  fireEvent.doubleClick(rf('CQ KB2DJJ'), { detail: 2 })
  expect(onCall).not.toHaveBeenCalled()
  expect(onSelectDecode.mock.calls.map((c) => c[0])).toEqual(['KM7CAB'])
})

it('roster: a gesture begun on blank grid space works nothing, though a row slides under its second click', () => {
  const s = (c: string) => ({ call: c, grid: 'EN52', snr: -10, lastHeardSlot: 100, heardCount: 1, presence: 'active', worked: false }) as Station
  const onCall = vi.fn(), onSelect = vi.fn()
  const { container } = render(<OperateRoster stations={['K0PT', 'KB2DJJ'].map(s)} myGrid="EN52" currentSlot={100}
    needByCall={new Map()} selectedCall={null} onSelect={onSelect} onCall={onCall} onToggleIgnore={vi.fn()} />)
  const r = (call: string) => [...container.querySelectorAll('.or-row[role=row]')].find((x) => x.getAttribute('aria-label')?.startsWith(call))!
  fireEvent.click(r('K0PT'), { detail: 1 }) // an earlier, separate selection
  firstClickOn(container.querySelector('.or-grid')!) // the gesture's first click, on blank space
  fireEvent.click(r('KB2DJJ'), { detail: 2 })
  fireEvent.dblClick(r('KB2DJJ'), { detail: 2 })
  expect(onCall).not.toHaveBeenCalled()
  expect(onSelect.mock.calls.map((c) => c[0])).toEqual(['K0PT'])
})

it("roster: a gesture begun on a row's QRZ link is not begun on the row", () => {
  const s = (c: string) => ({ call: c, grid: 'EN52', snr: -10, lastHeardSlot: 100, heardCount: 1, presence: 'active', worked: false }) as Station
  const onCall = vi.fn()
  const { container } = render(<OperateRoster stations={['K0PT', 'KB2DJJ'].map(s)} myGrid="EN52" currentSlot={100}
    needByCall={new Map()} selectedCall={null} onSelect={vi.fn()} onCall={onCall} onToggleIgnore={vi.fn()} />)
  const r = (call: string) => [...container.querySelectorAll('.or-row[role=row]')].find((x) => x.getAttribute('aria-label')?.startsWith(call))!
  fireEvent.click(r('KB2DJJ'), { detail: 1 }) // an earlier, separate selection
  fireEvent.mouseDown(r('K0PT').querySelector('.qrz-link')!, { detail: 1 }) // the gesture's first press, on K0PT's link
  fireEvent.click(r('KB2DJJ'), { detail: 2 })
  fireEvent.dblClick(r('KB2DJJ'), { detail: 2 })
  expect(onCall).not.toHaveBeenCalled()
})
