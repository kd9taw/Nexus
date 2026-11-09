// @vitest-environment jsdom
//
// THE WORDS FOR TRANSMIT LEFT OFF BY A CAT OUTAGE MOVE NOTHING ON THE STRIP. They once stood in the TX
// state, which grew with them, and on FT mid-contact at 1024 or 900 px wide the strip wrapped and Send,
// Log and the rest of the second row dropped 13-19 px. Now the state keeps its own words and the
// reason rides in a cell whose line-breaking width is zero: FT's absorber (`.cq-spacer`, already a
// zero-basis grower) and, on every other screen, a zero-basis cell whose gap is cancelled. These are
// the computed cascade winners over the real sheets, for the rendered elements.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import { CockpitTxStrip } from './CockpitTxStrip'
import { css, loadSheets } from '../cssCascade.testkit'
import { EN } from '../i18n'
import type { RadioStatus } from '../types'

const radio = {
  dialMhz: 14.074, band: '20m', catOk: false, sideband: 'USB', txAllowed: true, txEnabled: false,
  transmitting: false, tuning: false, txBusyReason: null, rigKeyed: false, holdTxFreq: false,
  txEven: true, txCycleAuto: true,
  catDetail: `14.0740 MHz CAT link unreachable (1/3) ${EN['settings.rigControl.catSilentTxOff']}`,
} as unknown as RadioStatus

beforeAll(() => loadSheets())
afterEach(cleanup)

const n = vi.fn()

describe('the CAT words take no room the strip lays out by', () => {
  it("FT's: they ride in the strip's one absorber, a zero-basis grower that cannot be held wide by its text", () => {
    const { container } = render(
      <OperateQsoStrip qso={null} radio={radio} onSetMode={n} onCallCq={n} onResend={n} onFreetext={n} onLog={n}
        onSetTxEnabled={n} onSetTune={n} onHaltTx={n} onSetHoldTxFreq={n} />,
    )
    const words = container.querySelector('.cat-txoff-note')!
    expect(words.textContent).toBe('CAT lost: press TX On')
    expect(container.querySelectorAll('.cq-spacer'), 'still exactly one absorber').toHaveLength(1)
    expect(words.classList.contains('cq-spacer')).toBe(true)
    expect(css(words, 'flex')).toBe('1 1 0%')
    expect(css(words, 'min-width')).toBe('0px')
    expect(css(words, 'white-space')).toBe('nowrap')
    expect(css(words, 'overflow')).toBe('hidden')
    expect(css(words, 'text-overflow')).toBe('ellipsis')
  })

  it("every other screen's: a zero basis, and its column gap cancelled, so the line sees zero width", () => {
    const { container } = render(<CockpitTxStrip radio={radio} onSetTxEnabled={n} onStopTx={n} />)
    const strip = container.querySelector('.cockpit-txstrip')!
    const words = container.querySelector('.cat-txoff-note')!
    expect(words.textContent).toBe('CAT lost: press TX On')
    expect(css(words, 'flex-basis')).toBe('0px')
    expect(css(words, 'min-width')).toBe('0px')
    // What the cell adds to the line the strip breaks by: the column gap before it, its margin, its border
    // and its padding (with border-box and a zero basis, padding is still width the breaker sees). Zero, or the
    // strip wraps and Stop TX moves (Phone, CW and RTTY in Russian at 175 %, 1002-1012 px wide).
    const px = (v: string | null) => {
      const m = /^calc\(\s*(-?[\d.]+)\s*\*\s*(-?[\d.]+)px\s*\)$/.exec(v ?? '')
      if (m) return Number(m[1]) * Number(m[2])
      const n = /^(-?[\d.]+)(px)?$/.exec((v ?? '0').trim())
      if (!n) throw new Error(`cannot read ${v}`)
      return Number(n[1])
    }
    const columnGap = px(css(strip, 'gap')!.split(/\s+/)[1])
    expect(columnGap, 'premise: the strip has a column gap').toBeGreaterThan(0)
    expect(
      columnGap + px(css(words, 'margin-left')) + px(css(words, 'padding-left')) + px(css(words, 'border-left-width')),
    ).toBe(0)
    // The space before the words is drawn by an indent, which no line breaker counts.
    expect(css(words, 'text-indent')).toBe('8px')
    expect(css(words, 'white-space')).toBe('nowrap')
    expect(css(words, 'overflow')).toBe('hidden')
  })

  // #489 (the transmit review): the words were left out beside a meters cell, which reduced the cause
  // to a tooltip on every screen with meters from md up. They stand IN the cell now, as its one block,
  // in place of the meters (blank while transmit is off): no item is added to the strip's line.
  it('with a meters cell the words are the cell, in place of the meters, and nothing is added beside the state', () => {
    const { container } = render(
      <CockpitTxStrip radio={radio} onSetTxEnabled={n} onStopTx={n} meters={<span className="meters-stub">meters</span>} />,
    )
    const notes = [...container.querySelectorAll('.cat-txoff-note')]
    expect(notes.map((e) => e.textContent)).toEqual([EN['operate.strip.state.txOffCatLost']])
    expect(notes[0].parentElement!.classList.contains('cockpit-txstrip-meters'), 'the meters cell holds the words').toBe(true)
    expect(notes[0].tagName, 'a block, so its ellipsis applies inside the cell').toBe('DIV')
    expect(container.querySelector('.meters-stub'), 'the meters give the cell up').toBeNull()
    const strip = container.querySelector('.cockpit-txstrip')!
    expect([...strip.children].filter((e) => e.classList.contains('cat-txoff-note')), 'no item of its own on the line').toEqual([])
    expect(container.querySelector('.cq-statecap')!.getAttribute('title')).toBe(EN['settings.rigControl.catSilentTxOff'])
  })
})
