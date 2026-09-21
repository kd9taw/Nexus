// @vitest-environment jsdom
//
// THE TX SWR METER MUST NOT ASSERT A THRESHOLD IT CANNOT SUPPORT (2026-09-20).
//
// `swrScaleVerified` answers whether this rig's SWR reading is on a scale Nexus can stand
// behind. The ENGINE takes it seriously: it refuses to arm the high-SWR cutoff without it.
// Settings takes it seriously (SettingsPanel.swrstop.test.tsx). This panel did not — it
// painted the bar `hot` at 2.0 and told the operator to "keep it under 2:1" over the very
// number the engine will not act on.
//
// #292 is the concrete case: a Xiegu reading 1.2:1 on its own front panel arrives here as
// 6:1. Mid-transmission that operator got a red bar and a numeric instruction, both of them
// wrong, on the loudest surface Nexus has.
//
// TWO CONSTRAINTS, PULLING AGAINST EACH OTHER, and this file pins BOTH:
//
//   • DO NOT HIDE IT. A reading that renders as nothing is indistinguishable from a reading
//     Nexus never built — "gating by absence", the central UI flaw this codebase was just
//     audited for. The raw figure is repeatable, so its CHANGES are real information even
//     when its absolute scale is not.
//   • DO NOT ASSERT A THRESHOLD. The ok/warn/hot zoning at 1.5 and 2.0, and the words "keep
//     it under 2:1", are the same claim about an absolute scale in two media. Neither is
//     supportable here, so both go.
//
// ⚠️ EVERY CHECK BELOW IS A PAIR. The verified render is not decoration: on the unfixed
// component both renders were IDENTICAL, so an unverified-only assertion would have passed
// against the bug and proved nothing about the branch.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'

afterEach(cleanup)

/** Keyed, one SWR reading and no other meter, so the SWR row is the only row on screen.
 *  `swrScaleVerified` is OPTIONAL on the DTO — `undefined` is a case the operator really
 *  reaches, and it is passed explicitly below rather than left to a default. */
function radio(swrScaleVerified: boolean | undefined, swr = 6.0): RadioStatus {
  return { rigKeyed: true, txSwr: swr, swrScaleVerified } as unknown as RadioStatus
}

/** The SWR row read straight off the DOM. */
function swrRow(root: HTMLElement) {
  const row = root.querySelector('.ph-txmeter') as HTMLElement
  const fill = row?.querySelector('.ph-txmeter-fill') as HTMLElement
  return {
    present: row != null,
    label: row?.querySelector('.ph-txmeter-label')?.textContent ?? '',
    value: row?.querySelector('.ph-txmeter-value')?.textContent ?? '',
    title: row?.getAttribute('title') ?? '',
    /** The RAW attribute React wrote: jsdom's CSSOM does not reliably round-trip a `var()`
     *  through the `background` shorthand, and the control below is what proves it here. */
    style: fill?.getAttribute('style') ?? '',
    width: fill?.style.width ?? '',
  }
}

describe('the TX SWR meter tells the truth about what it knows', () => {
  it('CONTROL — on a verified rig 6:1 is hot, and the tooltip names the 2:1 line', () => {
    // Everything below is stated as a DIFFERENCE from this render. If the zone colour or the
    // threshold wording ever stops arriving here, every negative assertion in this file goes
    // vacuous — so this one runs first and asserts them positively.
    const r = swrRow(render(<TxMeters radio={radio(true)} />).container)
    expect(r.label).toBe('SWR')
    expect(r.style, 'the hot zone really is painted').toContain('--state-weak')
    expect(r.title, 'and the threshold really is in the words').toContain('2:1')
  })

  it('on an UNVERIFIED rig the same 6:1 loses the zone and loses the threshold', () => {
    const r = swrRow(render(<TxMeters radio={radio(false)} />).container)
    expect(r.style, 'a red bar is a claim about an absolute scale').not.toContain('--state-weak')
    expect(r.style, 'neutral: Nexus does not know where this number sits').toContain(
      '--state-pending',
    )
    expect(r.title, '"keep it under 2:1" is that same claim in words').not.toContain('2:1')
    expect(r.title, 'and it says plainly why').toContain('no verified scale')
  })

  it('and says so VISIBLY — losing the colour cannot be the only signal', () => {
    // Dropping the zone silently is itself an absence: a grey bar reading 6.0:1 with nothing
    // to explain it is not more honest than a red one, just quieter. The mark rides on the
    // label, which costs no geometry in the fixed-width Operate cell.
    expect(swrRow(render(<TxMeters radio={radio(false)} />).container).label).toBe('SWR?')
  })

  it('an ABSENT verdict reads as UNVERIFIED — a missing field must never mean "verified"', () => {
    // `swrScaleVerified` is optional on the DTO and per-active-radio. The three readers in
    // SettingsPanel all test `!radio?.swrScaleVerified`, and this is the same direction for
    // the same reason: resolving absent to "verified" is the one way a missing value puts a
    // false red bar in front of a transmitting operator.
    const r = swrRow(render(<TxMeters radio={radio(undefined)} />).container)
    expect(r.label).toBe('SWR?')
    expect(r.style).toContain('--state-pending')
    expect(r.title).not.toContain('2:1')
  })

  it('NOT HIDDEN — the reading is still shown, and the bar still tracks it', () => {
    // The other half of the ruling. The figure is repeatable even when its scale is
    // meaningless, so the operator can watch it move while they tune; a meter that vanished
    // would take that away AND read as a feature Nexus never built.
    const low = swrRow(render(<TxMeters radio={radio(false, 1.4)} />).container)
    const high = swrRow(render(<TxMeters radio={radio(false, 3.0)} />).container)
    expect(low.present && high.present, 'the meter must still be on screen').toBe(true)
    expect(low.value).toBe('1.4:1')
    expect(high.value).toBe('3.0:1')
    expect(low.width, 'the bar moves with the reading').not.toBe(high.width)
  })

  it('BOTH COCKPITS — the Phone panel and the Operate inline cell', () => {
    // Phone/CW mount the default variant; the Operate strip mounts `inline`. Same component,
    // two geometries, and the honesty must not live in only one of them.
    for (const inline of [false, true]) {
      const where = `inline=${inline}`
      const bad = swrRow(render(<TxMeters radio={radio(false)} inline={inline} />).container)
      expect(bad.label, where).toBe('SWR?')
      expect(bad.style, where).toContain('--state-pending')
      const ok = swrRow(render(<TxMeters radio={radio(true)} inline={inline} />).container)
      expect(ok.label, where).toBe('SWR')
      expect(ok.style, where).toContain('--state-weak')
    }
  })

  it('touches ONLY the SWR row — ALC has no such verdict and keeps its zone', () => {
    // The scope guard. `swrScaleVerified` says nothing about the ALC, Po or COMP meters, and
    // a fix that greyed the whole strip out would be a different and larger claim.
    const snap = { rigKeyed: true, txSwr: 6.0, txAlc: 1.0, swrScaleVerified: false } as unknown as RadioStatus
    const rows = render(<TxMeters radio={snap} />).container.querySelectorAll('.ph-txmeter')
    const alc = [...rows].find((r) => r.querySelector('.ph-txmeter-label')?.textContent === 'ALC')!
    expect(alc.querySelector('.ph-txmeter-fill')?.getAttribute('style')).toContain('--state-weak')
  })
})
