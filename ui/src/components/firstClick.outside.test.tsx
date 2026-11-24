// @vitest-environment jsdom
//
// A gesture's first press ANYWHERE begins it, not only a press inside the list. A double-click whose
// first press landed outside the list (the roster's filter bar, the Stations header) and whose second
// click landed on a row, because the list moved between them, must not act on the row an earlier,
// finished gesture recorded. And the ordinary gesture, as a browser sends it, still works its row.
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

const row = (message: string, freqHz: number): DecodeRow =>
  ({ from: message.split(' ')[1], snr: -10, dtSec: 0.1, freqHz, message, isCq: true, directedToMe: false, worked: false, tier: 'FT8', rv: -1 }) as DecodeRow

it('roster: a gesture begun on the filter bar, outside the grid, works nothing when a row slides under its second click', () => {
  const s = (c: string) => ({ call: c, grid: 'EN52', snr: -10, lastHeardSlot: 100, heardCount: 1, presence: 'active', worked: false }) as Station
  const onCall = vi.fn()
  const { container } = render(<OperateRoster stations={['K0PT', 'KB2DJJ'].map(s)} myGrid="EN52" currentSlot={100}
    needByCall={new Map()} selectedCall={null} onSelect={vi.fn()} onCall={onCall} onToggleIgnore={vi.fn()} />)
  const r = (call: string) => [...container.querySelectorAll('.or-row[role=row]')].find((x) => x.getAttribute('aria-label')?.startsWith(call))!
  fireEvent.mouseDown(r('K0PT'), { detail: 1 }); fireEvent.click(r('K0PT'), { detail: 1 }) // earlier, finished gesture
  const bar = container.querySelector('.or-filters strong')!
  fireEvent.mouseDown(bar, { detail: 1 }); fireEvent.click(bar, { detail: 1 }) // new gesture begins outside the grid
  fireEvent.mouseDown(r('KB2DJJ'), { detail: 2 }); fireEvent.click(r('KB2DJJ'), { detail: 2 })
  fireEvent.dblClick(r('KB2DJJ'), { detail: 2 })
  expect(onCall.mock.calls.map((c) => c[0])).toEqual([])
})

it('cards: a gesture begun in the Stations header, outside the list, works nothing when a card slides under its second click', () => {
  const { onCall, card } = cards([st('AAAA'), st('BBBB')])
  const o = card('AAAA').querySelector('.station-open')!
  fireEvent.mouseDown(o, { detail: 1 }); fireEvent.click(o, { detail: 1 })
  const hdr = document.querySelector('.station-list')!.firstElementChild!
  fireEvent.mouseDown(hdr, { detail: 1 }); fireEvent.click(hdr, { detail: 1 })
  fireEvent.mouseDown(card('BBBB'), { detail: 2 }); fireEvent.click(card('BBBB'), { detail: 2 })
  fireEvent.doubleClick(card('BBBB'), { detail: 2 })
  expect(onCall.mock.calls.map((c) => c[0])).toEqual([])
})

it('roster, as a browser sends it: press, click, press, click, double-click on one row works it (and Alt ignores it)', () => {
  const s = (c: string) => ({ call: c, grid: 'EN52', snr: -10, lastHeardSlot: 100, heardCount: 1, presence: 'active', worked: false, freqHz: 700 }) as Station
  const onCall = vi.fn(), onIgn = vi.fn()
  const { container } = render(<OperateRoster stations={['K0PT', 'KB2DJJ'].map(s)} myGrid="EN52" currentSlot={100}
    needByCall={new Map()} selectedCall={null} onSelect={vi.fn()} onCall={onCall} onToggleIgnore={onIgn} />)
  const r = (call: string) => [...container.querySelectorAll('.or-row[role=row]')].find((x) => x.getAttribute('aria-label')?.startsWith(call))!
  const cell = r('KB2DJJ').querySelector('.or-gridc')!
  for (const alt of [false, true]) {
    fireEvent.mouseDown(cell, { detail: 1 }); fireEvent.click(cell, { detail: 1 })
    fireEvent.mouseDown(cell, { detail: 2 }); fireEvent.click(cell, { detail: 2 })
    fireEvent.dblClick(cell, { detail: 2, altKey: alt })
  }
  expect(onCall.mock.calls).toEqual([['KB2DJJ', 'EN52', undefined, undefined, 700]])
  expect(onIgn.mock.calls).toEqual([['KB2DJJ']])
})

it('decodes: a row reached by the keyboard, then double-clicked by the mouse, is worked', () => {
  const A = row('CQ KM7CAB DN17', 900), B = row('CQ KB2DJJ FN20', 1200)
  const onCall = vi.fn()
  const props = { rxOffsetHz: 1500, band: '20m', tier: 'FT8' as const, harqRescues: 0, onCall, onSelectDecode: vi.fn(), onToggleIgnore: vi.fn() }
  const v = render(<OperateDecodes decodes={[A, B]} slot={11} clearTick={0} {...props} />)
  const rf = (m: string) => [...v.container.querySelectorAll<HTMLElement>('.decode-row')].find((r) => r.textContent?.includes(m))!
  fireEvent.keyDown(rf('CQ KM7CAB'), { key: 'ArrowDown' })
  const t = rf('CQ KB2DJJ').firstElementChild ?? rf('CQ KB2DJJ')
  fireEvent.mouseDown(t, { detail: 1 }); fireEvent.click(t, { detail: 1 })
  fireEvent.mouseDown(t, { detail: 2 }); fireEvent.click(t, { detail: 2 })
  fireEvent.doubleClick(t, { detail: 2 })
  expect(onCall.mock.calls).toEqual([['KB2DJJ', undefined, 'CQ KB2DJJ FN20', -10, 1200]])
})
