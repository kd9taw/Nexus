// @vitest-environment jsdom
//
// #276 — BAND ACTIVITY "NEWEST ON TOP", an option. The default stays WSJT-X's order (oldest at
// the top, the newest period arriving at the bottom), so an operator who never touches the chip
// sees no change. On, the rows are drawn newest first and the pane follows the top.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { OperateDecodes } from './OperateDecodes'
import { DECODE_NEWEST_TOP_KEY } from '../operateFilters'
import type { DecodeRow } from '../types'
import { EN } from '../i18n'

vi.mock('../api', () => ({ openQrzPage: vi.fn() }))

const decode = (from: string, over: Partial<DecodeRow> = {}): DecodeRow => ({
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
  ...over,
})

function Pane(props: { decodes: DecodeRow[]; slot: number }) {
  return (
    <OperateDecodes
      decodes={props.decodes}
      slot={props.slot}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
    />
  )
}

/** Two periods: OLD1 in slot 100, NEW1 in slot 101. */
function mountTwoPeriods() {
  const view = render(<Pane decodes={[decode('OLD1')]} slot={100} />)
  view.rerender(<Pane decodes={[decode('NEW1', { freqHz: 1500 })]} slot={101} />)
  return view
}

const calls = () =>
  Array.from(document.querySelectorAll('.decode-row')).map((r) => r.getAttribute('aria-label')?.split(/[ ,]/)[0])
const chip = () => screen.getByRole('button', { name: EN['operate.decodes.newestTop.label'] })

beforeEach(() => localStorage.clear())
afterEach(cleanup)

describe('#276 Band Activity newest on top', () => {
  it('defaults to WSJT-X order: oldest first, newest at the bottom', () => {
    mountTwoPeriods()
    expect(calls()).toEqual(['OLD1', 'NEW1'])
    expect(chip().getAttribute('aria-pressed')).toBe('false')
  })

  it('the chip draws the newest first, and persists across a remount', () => {
    const view = mountTwoPeriods()
    fireEvent.click(chip())
    expect(calls()).toEqual(['NEW1', 'OLD1'])
    expect(chip().getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem(DECODE_NEWEST_TOP_KEY)).toBe('1')
    view.unmount()
    mountTwoPeriods()
    expect(calls()).toEqual(['NEW1', 'OLD1'])
  })

  it('a period separator still heads the period below it', () => {
    mountTwoPeriods()
    fireEvent.click(chip())
    const scroll = document.querySelector('.od-scroll')!
    const kids = Array.from(scroll.children).map((el) =>
      el.classList.contains('od-period-sep') ? 'sep' : el.getAttribute('aria-label')?.split(/[ ,]/)[0],
    )
    expect(kids).toEqual(['NEW1', 'sep', 'OLD1'])
  })

  it('only applies in Time order; another sort leaves the ranking alone and says why', () => {
    mountTwoPeriods()
    fireEvent.click(chip())
    const sort = screen.getByRole('combobox')
    fireEvent.change(sort, { target: { value: 'freq' } })
    expect(chip().hasAttribute('disabled')).toBe(true)
    expect(chip().getAttribute('title')).toBe(EN['operate.decodes.newestTop.title.idle'])
    // Freq ascending: OLD1 (1200 Hz) before NEW1 (1500 Hz), not reversed.
    expect(calls()).toEqual(['OLD1', 'NEW1'])
  })
})
