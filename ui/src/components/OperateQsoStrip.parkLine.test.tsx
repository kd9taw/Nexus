// @vitest-environment jsdom
//
// FT'S STRIP PARKS ITS FIRST LINE AT THE FOOT OF THE SHELL (OperateQsoStrip useParkLine, cockpit-panes.css
// `.cockpit-ftstrip`). At 150–175 % on a small window FT's header alone ran past the window's foot, so Stop TX was off
// screen as FT opened (900×600 / 175 %: the header ends at y 991 in a 600 px window, measured in Chrome). The strip is a
// shell child now and sticky at the bottom; FT's strip is 300–400 px there, taller than the whole scrollport, so it
// parks only its first line, the TX cluster with Stop TX, and waits the rest of its height past the foot. That rest is
// what this measures: the strip's height under the cluster's line, in layout px (window px over the UI zoom), less the
// strip's own bottom padding and border, which park with the line. jsdom lays nothing out: the boxes are stubbed.
import { describe, it, expect, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { OperateQsoStrip } from './OperateQsoStrip'
import type { RadioStatus } from '../types'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const radio = {
  dialMhz: 14.074, band: '20m', catOk: true, sideband: 'USB', txAllowed: true, txEnabled: true, transmitting: false, tuning: false,
  txBusyReason: null, rigKeyed: false, holdTxFreq: false, txEven: true, txCycleAuto: true,
} as unknown as RadioStatus

const ZOOM = 1.75
/** The strip's box and its first line's, in window px from the window's top. */
const STRIP = { top: 400, height: 350 }
const LINE_BOTTOM = 400 + 105

function stub() {
  const box = (top: number, height: number) => ({ top, bottom: top + height, height, left: 0, right: 500, width: 500, x: 0, y: top, toJSON: () => ({}) }) as DOMRect
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains('cockpit-qso')) return box(STRIP.top, STRIP.height)
    if (this.classList.contains('cq-txctl')) return box(STRIP.top + 4, LINE_BOTTOM - STRIP.top - 4)
    return box(0, 0)
  })
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockImplementation(function (this: HTMLElement) {
    return this.classList.contains('cockpit-qso') ? STRIP.height / ZOOM : 0
  })
}

function Strip({ withRadio = true }: { withRadio?: boolean }) {
  const noop = vi.fn()
  return (
    <OperateQsoStrip qso={null} radio={withRadio ? radio : undefined} onSetMode={noop} onCallCq={noop} onResend={noop} onFreetext={noop}
      onLog={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetHoldTxFreq={noop} />
  )
}
const strip = () => document.querySelector<HTMLElement>('.cockpit-qso')!

describe("FT's strip parks only its first line, the line with Stop TX", () => {
  it('writes the height under the TX cluster’s line, in layout px', () => {
    stub()
    render(<Strip />)
    expect(strip().classList.contains('cockpit-ftstrip'), 'the strip lost its parking rule').toBe(true)
    expect(strip().querySelector('.cq-txctl .op-btn.stop'), 'control: Stop TX is in the parked line').not.toBeNull()
    expect(strip().style.getPropertyValue('--cockpit-ftstrip-rest')).toBe(`${Math.floor((STRIP.top + STRIP.height - LINE_BOTTOM) / ZOOM)}px`)
  })

  it('less the strip’s own bottom padding and border, which park with the line', () => {
    stub()
    const s = document.createElement('style')
    s.textContent = '.cockpit-qso { padding-bottom: 3px; border-bottom: 1px solid }'
    document.head.appendChild(s)
    render(<Strip />)
    expect(strip().style.getPropertyValue('--cockpit-ftstrip-rest')).toBe(`${Math.floor((STRIP.top + STRIP.height - LINE_BOTTOM) / ZOOM - 4)}px`)
    s.remove()
  })

  it('a strip with no box (a keep-alive host under [hidden]) writes nothing, and parks whole if it ever shows that way', () => {
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(0)
    render(<Strip />)
    expect(strip().style.getPropertyValue('--cockpit-ftstrip-rest')).toBe('')
  })

  it('with no radio there is no cluster, and nothing to park short of the whole strip', () => {
    stub()
    render(<Strip withRadio={false} />)
    expect(strip().style.getPropertyValue('--cockpit-ftstrip-rest')).toBe('')
  })
})
