// @vitest-environment jsdom
//
// #343: THE NEEDLE TX METERS — Power, ALC and SWR as analog faces with a peak marker, drawn by
// TxMeters in place of those three bars when Settings ▸ Workspace ▸ TX meters is Needle.
//
// What is held here: the peak hold (fast attack, a one-second hold, a slow fall; the last over's
// peak between overs), that the faces read the SAME readings and the SAME zones as the bars, that
// Bars stays the default and draws no needle, and that what the needles replace is exactly PO, ALC
// and SWR — COMP keeps its bar and Phone's processor controls, the temperatures keep theirs.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import { alcBar, swrBar, TxMeters } from './TxMeters'
import { needleZones, PEAK_FALL_PER_S, PEAK_HOLD_MS, peakAt, peakStep } from './TxNeedles'
import { TX_METER_STYLE_KEY } from '../txMeterStyle'
import type { RadioStatus } from '../types'

let now = 1_000_000
beforeEach(() => {
  now = 1_000_000
  vi.spyOn(Date, 'now').mockImplementation(() => now)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  localStorage.clear()
})

const needleOn = () => localStorage.setItem(TX_METER_STYLE_KEY, 'needle')
const keyed = (r: Partial<RadioStatus>) =>
  ({ rigKeyed: true, swrScaleVerified: true, ratedWatts: 100, ...r }) as unknown as RadioStatus
const idle = { rigKeyed: false } as unknown as RadioStatus

const faces = (c: HTMLElement) => [...c.querySelectorAll<HTMLElement>('.ph-txneedle')]
const face = (c: HTMLElement, key: string) => c.querySelector<HTMLElement>(`.ph-txneedle[data-meter="${key}"]`)!
const needleOf = (c: HTMLElement, key: string) => Number(face(c, key).querySelector('.ph-txneedle-needle')!.getAttribute('data-frac'))
const peakOf = (c: HTMLElement, key: string) => {
  const p = face(c, key).querySelector('.ph-txneedle-peak')
  return p == null ? null : Number(p.getAttribute('data-frac'))
}
const valueOf = (c: HTMLElement, key: string) => face(c, key).querySelector('.ph-txneedle-value')?.textContent
const barLabels = (c: HTMLElement) => [...c.querySelectorAll('.ph-txmeter-label')].map((l) => l.textContent)

describe('the peak hold: a fast attack, a one-second hold, a slow fall', () => {
  it('jumps to a higher reading at once and holds it through the hold', () => {
    const p = peakStep(null, 0.8, '80%', 0)
    expect(peakAt(p, 0)).toBe(0.8)
    // A lower reading does not take it (the hold), however soon or late within the hold.
    expect(peakStep(p, 0.3, '30%', 500)).toBe(p)
    expect(peakAt(p, PEAK_HOLD_MS)).toBe(0.8)
  })

  it('then falls at its rate, and a reading that catches it takes it and restarts the hold', () => {
    const p = peakStep(null, 0.8, '80%', 0)
    const t = PEAK_HOLD_MS + 500
    expect(peakAt(p, t)).toBeCloseTo(0.8 - PEAK_FALL_PER_S * 0.5)
    // Under the fallen marker: still the old peak, still falling.
    expect(peakStep(p, 0.5, '50%', t)).toBe(p)
    // At or above it: a fresh peak.
    const q = peakStep(p, 0.6, '60%', t)
    expect(q).toEqual({ frac: 0.6, at: t, value: '60%' })
    expect(peakAt(q, t + PEAK_HOLD_MS)).toBe(0.6)
  })

  it('never falls below zero', () => {
    expect(peakAt(peakStep(null, 0.4, '40%', 0), 60_000)).toBe(0)
  })
})

describe("the faces' coloured arcs are the bars' own zones", () => {
  // The zone a fraction falls in: the LAST arc reaching it (arcs run up the scale, and a boundary
  // belongs to the arc above it, as the bars' `>=` thresholds do); none is green.
  const zoneAt = (arcs: ReturnType<typeof needleZones>, f: number) =>
    [...arcs].reverse().find(([a, b]) => f >= a && f <= b)?.[2] ?? 'ok'
  it.each([1, 0.5])('ALC with the radio’s zone ending at %s of the bar', (zoneTop) => {
    const arcs = needleZones('alc', { swrVerified: true, alcZoneTop: zoneTop })
    for (const v of [0.1, 0.39, 0.4, 0.47, 0.475, 0.5, 0.6, 0.79, 0.8, 0.94, 0.95, 1]) {
      expect(zoneAt(arcs, alcBar(v, zoneTop).frac), `ALC ${v}`).toBe(alcBar(v, zoneTop).zone)
    }
  })

  it('SWR on a verified scale', () => {
    const arcs = needleZones('swr', { swrVerified: true, alcZoneTop: 1 })
    for (const v of [1, 1.3, 1.49, 1.5, 1.8, 1.99, 2, 2.5, 3, 4]) {
      expect(zoneAt(arcs, swrBar(v).frac), `SWR ${v}`).toBe(swrBar(v).zone)
    }
  })

  it('no zones where the SWR scale is unverified, and none on power', () => {
    expect(needleZones('swr', { swrVerified: false, alcZoneTop: 1 })).toEqual([])
    expect(needleZones('po', { swrVerified: true, alcZoneTop: 1 })).toEqual([])
  })
})

describe('Bars stays the default', () => {
  it('with nothing picked, the meters are bars and there is no needle', () => {
    const { container } = render(<TxMeters radio={keyed({ txAlc: 0.6, txPoW: 50, txSwr: 1.3 })} pinned />)
    expect(faces(container)).toHaveLength(0)
    expect(barLabels(container)).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
  })
})

describe('Needle draws PO, ALC and SWR as faces, and only those', () => {
  it('three faces, in the meter switch’s order, in place of their three bars; COMP stays a bar', () => {
    needleOn()
    const { container } = render(<TxMeters radio={keyed({ txAlc: 0.6, txPoW: 50, txSwr: 1.3, txCompDb: 10 })} pinned />)
    expect(faces(container).map((f) => f.dataset.meter)).toEqual(['po', 'alc', 'swr'])
    expect(barLabels(container)).toEqual(['COMP'])
    for (const f of faces(container)) expect(f.querySelector('.ph-txneedle-face')!.classList).toContain('well')
  })

  it('in every host: the dock, Operate’s cell once keyed, and the TX strip', () => {
    needleOn()
    for (const props of [{ pinned: true }, { inline: true }, { strip: true }]) {
      const { container } = render(<TxMeters radio={keyed({ txAlc: 0.5 })} {...props} />)
      expect(faces(container), JSON.stringify(props)).toHaveLength(3)
      cleanup()
    }
  })

  it('Operate’s cell still shows its hint alone until the rig first keys, as with bars', () => {
    needleOn()
    const { container } = render(<TxMeters radio={idle} inline />)
    expect(faces(container)).toHaveLength(0)
    expect(container.querySelector('.ph-txmeters-hint')).not.toBeNull()
  })

  it('Phone’s COMP row keeps its processor level, PROC and TX bandwidth beside the needles', () => {
    needleOn()
    const radio = keyed({ txCompDb: 8, compLevel: 0.4, comp: true, txProcSwitch: true, txBw: 'MID', txBwSteps: ['WIDE', 'MID'] })
    const controls = { onCompLevel: () => {}, onProc: () => {}, onTxBw: () => {} }
    const { container } = render(<TxMeters radio={radio} strip controls={controls} />)
    expect(faces(container)).toHaveLength(3)
    expect(container.querySelector('[role="slider"]')).not.toBeNull()
    expect(container.querySelector('[data-ctl="proc"]')).not.toBeNull()
    expect(container.querySelector('[data-ctl="txbw"]')).not.toBeNull()
  })

  it('Operate’s fixed-width cell holds the three faces only: its COMP slot goes; the strip keeps COMP', () => {
    needleOn()
    const cell = render(<TxMeters radio={keyed({ txAlc: 0.5, txCompDb: 4 })} inline />).container
    expect(faces(cell)).toHaveLength(3)
    expect(barLabels(cell)).toEqual([])
    cleanup()
    const strip = render(<TxMeters radio={keyed({ txAlc: 0.5, txCompDb: 4 })} strip />).container
    expect(barLabels(strip)).toEqual(['COMP'])
    cleanup()
    // The control: with bars, Operate's cell keeps all four, COMP among them.
    localStorage.clear()
    const bars = render(<TxMeters radio={keyed({ txAlc: 0.5, txCompDb: 4 })} inline />).container
    expect(barLabels(bars)).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
  })

  it('the temperatures stay bars', () => {
    needleOn()
    const { container } = render(<TxMeters radio={keyed({ txAlc: 0.5, paTempC: 45 })} pinned />)
    expect(container.querySelector('.ph-txmeter.temp[data-meter="rigTemp"]')).not.toBeNull()
  })
})

describe('the faces read what the bars read', () => {
  it('each needle stands at its bar’s fraction and prints its bar’s value', () => {
    needleOn()
    const { container } = render(<TxMeters radio={keyed({ txAlc: 0.6, txPoW: 50, txSwr: 2 })} pinned />)
    expect(needleOf(container, 'alc')).toBeCloseTo(0.6)
    expect(valueOf(container, 'alc')).toBe('60%')
    expect(needleOf(container, 'po')).toBeCloseTo(0.5)
    expect(valueOf(container, 'po')).toBe('50 W')
    expect(needleOf(container, 'swr')).toBeCloseTo(0.5)
    expect(valueOf(container, 'swr')).toBe('2.0:1')
  })

  it('power is scaled to the radio’s rated output, as its bar is', () => {
    needleOn()
    const { container } = render(<TxMeters radio={keyed({ txPoW: 5, ratedWatts: 10 })} pinned />)
    expect(needleOf(container, 'po')).toBeCloseTo(0.5)
  })

  it('the ALC face’s red arc starts where the radio’s ALC zone says (half scale on the Yaesu bar)', () => {
    needleOn()
    const { container } = render(<TxMeters radio={keyed({ txAlc: 0.3, txAlcZone: 0.5 })} pinned />)
    const hot = face(container, 'alc').querySelector('.ph-txneedle-zone[data-zone="hot"]')!
    expect(Number(hot.getAttribute('data-from'))).toBeCloseTo(0.475)
    expect(Number(hot.getAttribute('data-to'))).toBe(1)
    expect(hot.getAttribute('style')).toContain('--state-weak')
  })

  it('an unverified SWR scale paints no zone, and the face says SWR?', () => {
    needleOn()
    const { container } = render(<TxMeters radio={keyed({ txSwr: 2.5, swrScaleVerified: false })} pinned />)
    expect(face(container, 'swr').querySelectorAll('.ph-txneedle-zone')).toHaveLength(0)
    expect(face(container, 'swr').querySelector('.ph-txneedle-label')!.textContent).toBe('SWR?')
    // The positive control: verified, the same reading has its arcs.
    cleanup()
    const ok = render(<TxMeters radio={keyed({ txSwr: 2.5 })} pinned />).container
    expect(face(ok, 'swr').querySelectorAll('.ph-txneedle-zone')).toHaveLength(2)
  })
})

describe('the peak marker on the air, and between overs', () => {
  it('holds the highest reading for a second, then falls, while the needle follows the reading', () => {
    needleOn()
    const { container, rerender } = render(<TxMeters radio={keyed({ txAlc: 0.8 })} pinned />)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.8)
    now += 500
    rerender(<TxMeters radio={keyed({ txAlc: 0.3 })} pinned />)
    expect(needleOf(container, 'alc')).toBeCloseTo(0.3)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.8)
    now += 1000 // 0.5 s past the hold
    rerender(<TxMeters radio={keyed({ txAlc: 0.3 })} pinned />)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.8 - PEAK_FALL_PER_S * 0.5)
    rerender(<TxMeters radio={keyed({ txAlc: 0.9 })} pinned />)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.9)
  })

  it('between overs the needle rests and the marker and value hold the last over’s peak', () => {
    needleOn()
    const { container, rerender } = render(<TxMeters radio={keyed({ txAlc: 0.7, txSwr: 1.4 })} pinned />)
    now += 300
    rerender(<TxMeters radio={keyed({ txAlc: 0.4, txSwr: 1.6 })} pinned />)
    now += 5000
    rerender(<TxMeters radio={idle} pinned />)
    expect(needleOf(container, 'alc')).toBe(0)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.7)
    expect(valueOf(container, 'alc')).toBe('70%')
    expect(valueOf(container, 'swr')).toBe('1.6:1')
    // …however long the gap: it does not fall away between overs.
    now += 60_000
    rerender(<TxMeters radio={idle} pinned />)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.7)
  })

  it('the next over starts its peaks afresh', () => {
    needleOn()
    const { container, rerender } = render(<TxMeters radio={keyed({ txAlc: 0.9 })} pinned />)
    now += 10_000
    rerender(<TxMeters radio={idle} pinned />)
    now += 10_000
    rerender(<TxMeters radio={keyed({ txAlc: 0.2 })} pinned />)
    expect(peakOf(container, 'alc')).toBeCloseTo(0.2)
  })

  it('before any over there is no marker and no value', () => {
    needleOn()
    const { container } = render(<TxMeters radio={idle} pinned />)
    expect(peakOf(container, 'alc')).toBeNull()
    expect(needleOf(container, 'alc')).toBe(0)
    expect(valueOf(container, 'alc')).toBe('—')
  })
})

describe('a meter the rig never reported', () => {
  it('after an over that read others, its face carries the ⊘ the bar row would', () => {
    needleOn()
    const { container, rerender } = render(<TxMeters radio={keyed({ txAlc: 0.5, txPoW: 40 })} pinned />)
    expect(face(container, 'swr').querySelector('.ph-unavail')).toBeNull()
    rerender(<TxMeters radio={idle} pinned />)
    expect(face(container, 'swr').querySelector('.ph-unavail')).not.toBeNull()
    expect(face(container, 'alc').querySelector('.ph-unavail')).toBeNull()
  })
})
