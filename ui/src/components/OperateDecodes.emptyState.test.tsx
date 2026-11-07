// @vitest-environment jsdom
//
// AN EMPTY LIST HAS NOTHING TO FOLLOW. The decode list pins its bottom edge, the WSJT-X flow, which is
// right for rows and wrong for its "No decodes yet" block: in a list shorter than the block (Rx Frequency
// on a stock 1366×768 Roster, or any feed sharing a busy rail) the pin scrolled the heading out of the
// top of the box. Empty, the list stands at its top and scrolls down to the rest of the block.
// ⚠️ jsdom does not lay out, so the overflow is stubbed; the pixels are the real-Chrome pass's.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { saveDecodeNewestTop } from '../operateFilters'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { chainOf, parseRules, winnerAt } from '../cssCascade'
import type { DecodeRow } from '../types'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const decode = (from: string): DecodeRow => ({
  from,
  snr: -12,
  dtSec: 0.2,
  freqHz: 1200,
  message: `CQ ${from} FN31`,
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
})

const Pane = ({ decodes }: { decodes: DecodeRow[] }) => (
  <OperateDecodes decodes={decodes} slot={100} rxOffsetHz={1200} band="20m" tier="FT8" harqRescues={0} onCall={() => {}} />
)
const list = () => document.querySelector<HTMLElement>('.od-scroll')!

const realHeight = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollHeight')!
beforeEach(() => {
  localStorage.clear()
  // The list is shorter than what it holds, whatever it holds.
  Object.defineProperty(Element.prototype, 'scrollHeight', { configurable: true, get: () => 500 })
})
afterEach(() => {
  cleanup()
  Object.defineProperty(Element.prototype, 'scrollHeight', realHeight)
})

describe('the decode list while it is empty', () => {
  it('stands at its top, so the heading of "No decodes yet" is in view', () => {
    render(<Pane decodes={[]} />)
    expect(list().querySelector('.sb-title')).not.toBeNull()
    expect(list().scrollTop).toBe(0)
  })

  it('the control: with a row it follows the bottom, from the first row on', () => {
    const view = render(<Pane decodes={[]} />)
    view.rerender(<Pane decodes={[decode('W1AW')]} />)
    expect(list().querySelector('.decode-row')).not.toBeNull()
    expect(list().scrollTop).toBe(500)
  })

  it('reading down the block is not "reviewing": the first row is still followed', () => {
    const view = render(<Pane decodes={[]} />)
    list().scrollTop = 200
    fireEvent.scroll(list())
    expect(document.querySelector('.od-paused.on')).toBeNull()
    view.rerender(<Pane decodes={[decode('W1AW')]} />)
    expect(list().scrollTop).toBe(500)
  })

  it('and with rows, scrolling up to read still pauses the follow', () => {
    render(<Pane decodes={[decode('W1AW')]} />)
    list().scrollTop = 100
    fireEvent.scroll(list())
    expect(document.querySelector('.od-paused.on')).not.toBeNull()
  })
})

describe('a list emptied while the operator was reviewing', () => {
  const row = (from: string, over: Partial<DecodeRow> = {}): DecodeRow => ({ ...decode(from), ...over })
  const rx = (decodes: DecodeRow[], slot: number, rxOffsetHz: number) => (
    <OperateDecodes decodes={decodes} slot={slot} rxOffsetHz={rxOffsetHz} band="20m" tier="FT8" harqRescues={0} onCall={() => {}} lockedFilter="rx" />
  )
  const reviewing = () => {
    list().scrollTop = 100
    fireEvent.scroll(list())
    expect(document.querySelector('.od-paused.on')).not.toBeNull()
  }
  // What the browser does as the list shrinks under a scrolled view: the offset clamps and a scroll
  // event follows. Nothing else in these panes would end the "reviewing".
  const clamp = () => {
    list().scrollTop = 0
    fireEvent.scroll(list())
  }

  it('the RX offset moved to a quiet frequency: "reviewing" ends and the first decode there is followed', () => {
    const view = render(rx([row('W1AW')], 100, 1200))
    reviewing()
    view.rerender(rx([], 101, 2500))
    clamp()
    expect(list().querySelector('.decode-row')).toBeNull()
    expect(document.querySelector('.od-paused.on')).toBeNull()
    list().scrollTop = 0
    view.rerender(rx([row('K1ABC', { freqHz: 2500, message: 'CQ K1ABC FN42' })], 102, 2500))
    expect(list().querySelector('.decode-row')).not.toBeNull()
    expect(list().scrollTop).toBe(500)
    expect(document.querySelector('.od-paused.on')).toBeNull()
  })

  it('a chip hid every row: the same', () => {
    const view = render(<Pane decodes={[row('W1AW', { isCq: false, message: 'K1ABC W1AW -05' })]} />)
    reviewing()
    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    clamp()
    expect(list().querySelector('.decode-row')).toBeNull()
    expect(document.querySelector('.od-paused.on')).toBeNull()
    list().scrollTop = 0
    view.rerender(<OperateDecodes decodes={[decode('K1ABC')]} slot={101} rxOffsetHz={1200} band="20m" tier="FT8" harqRescues={0} onCall={() => {}} />)
    expect(list().querySelector('.decode-row')).not.toBeNull()
    expect(list().scrollTop).toBe(500)
    expect(document.querySelector('.od-paused.on')).toBeNull()
  })

  it('newest on top: the same, at the top edge', () => {
    // Locked panes keep the stock order, so this is Band Activity with its chip.
    saveDecodeNewestTop(true)
    const pane = render(<Pane decodes={[row('W1AW', { isCq: false, message: 'K1ABC W1AW -05' })]} />)
    list().scrollTop = 100
    fireEvent.scroll(list())
    expect(document.querySelector('.od-paused.on')).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    clamp()
    expect(document.querySelector('.od-paused.on')).toBeNull()
    pane.rerender(<OperateDecodes decodes={[decode('K1ABC')]} slot={101} rxOffsetHz={1200} band="20m" tier="FT8" harqRescues={0} onCall={() => {}} />)
    expect(list().querySelector('.decode-row')).not.toBeNull()
    expect(list().scrollTop).toBe(0)
    expect(document.querySelector('.od-paused.on')).toBeNull()
  })

  it('the control: a filter that leaves a row in the list does not end the reading', () => {
    render(<Pane decodes={[decode('W1AW')]} />)
    reviewing()
    fireEvent.click(screen.getByRole('button', { name: 'CQ' }))
    expect(list().querySelector('.decode-row')).not.toBeNull()
    expect(document.querySelector('.od-paused.on')).not.toBeNull()
  })
})

describe('the block fits a short list: its heading on the first line, its room only where there is some', () => {
  const sheet = (name: string) =>
    readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
  const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
  const win = (el: Element, prop: string) => winnerAt(RULES, 'dark', chainOf(el), prop)?.value.trim() ?? null

  it('no padding above the heading, the glyph beside it, and auto margins that collapse when short', () => {
    render(<Pane decodes={[]} />)
    const block = list().querySelector('.state-block')!
    expect(win(block, 'padding')).toBe('0 var(--space-4)')
    expect(win(block, 'margin-block')).toBe('auto')
    expect(win(block, 'flex-direction')).toBe('row')
    expect(win(block, 'flex-wrap')).toBe('wrap')
    expect(win(block, 'flex-shrink')).toBe('0')
  })

  it('the control: a state block anywhere else keeps its roomy column', () => {
    const { container } = render(<div className="state-block"><div className="sb-detail" /></div>)
    const block = container.querySelector('.state-block')!
    expect(win(block, 'flex-direction')).toBe('column')
    expect(win(block, 'padding')).toBe('var(--space-7) var(--space-4)')
  })
})
