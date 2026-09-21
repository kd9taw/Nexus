// @vitest-environment jsdom
//
// FOUR ROWS, ALWAYS — so the dock stops moving (operator, 2026-09-20).
//
// The panel built its rows conditionally, one `if (radio.txSwr != null)` per meter, so its
// height was 0 to 4 rows depending on what the rig reports AND on whether it had keyed yet.
// It sits in a BOTTOM-ANCHORED dock above the PTT button: every one of those transitions
// moves the dock's top edge, and the ones that happen on key-down happen while the operator
// is holding the button. Fixed at four, there is nothing left to move.
//
// ⚠️ WHAT A DASH MEANS HAS TO STAY UNAMBIGUOUS, and four fixed rows is what makes that a
// question. A row reading '—' could mean "you are not transmitting" or "this radio has no
// ALC meter", which are different facts with different remedies. They are told apart by WHEN
// we can know:
//   · before the first over — nothing is known about any meter, because these fields are
//     populated only while transmitting. That is what the idle hint says, and no row may
//     claim the rig lacks a meter it has never had the chance to report.
//   · after an over — a meter that stayed silent through it is one this rig does not report,
//     and the row says so with the same ⊘ mark the cockpit's controls use.
//
// ⚠️ COMPACT IDLE, FULL KEYED is a VARIANT SWITCH over CSS that already exists (6px bars
// pinned, 10px bare) — not new geometry. jsdom never lays out, so what is asserted here is
// the class the sheet keys on, plus that the sheet still carries both heights.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'

afterEach(cleanup)

const radio = (over: Record<string, unknown> = {}) => ({ ...over }) as unknown as RadioStatus
/** Keyed, through the ARBITER rather than the FT slot flag — `rigKeyed` is what a voice over
 *  sets (#57), and it is how this panel knows it is on the air at all. */
const keyed = (over: Record<string, unknown> = {}) => radio({ rigKeyed: true, ...over })

const panel = () => document.querySelector('.ph-txmeters')!
const rows = () => [...document.querySelectorAll('.ph-txmeter')]
const labels = () => rows().map((r) => r.querySelector('.ph-txmeter-label')?.textContent)
const values = () => rows().map((r) => r.querySelector('.ph-txmeter-value')?.textContent)
const rowFor = (label: string) =>
  rows().find((r) => r.querySelector('.ph-txmeter-label')?.textContent?.startsWith(label))!

describe('the pinned panel is always four rows high', () => {
  it('a rig that has never keyed shows four rows and the hint', () => {
    render(<TxMeters radio={radio()} pinned />)
    expect(labels(), 'the panel is not four rows').toEqual(['SWR', 'ALC', 'PO', 'COMP'])
    expect(values()).toEqual(['—', '—', '—', '—'])
    expect(panel().textContent).toContain('readings appear on transmit')
  })

  it('a rig reporting ONE meter still shows four rows', () => {
    // The case the old build was 1 row high for. Nothing about the dock's height may depend
    // on which meters a radio happens to have.
    render(<TxMeters radio={keyed({ txSwr: 1.3, swrScaleVerified: true })} pinned />)
    expect(labels()).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
    expect(rowFor('SWR').querySelector('.ph-txmeter-value')!.textContent).toBe('1.3:1')
  })

  it('a rig reporting all four shows the same four rows', () => {
    // The pair, and the point of the whole change: these two renders are the SAME HEIGHT.
    render(<TxMeters radio={keyed({ txSwr: 1.3, txAlc: 0.6, txPoW: 87, txCompDb: 6, swrScaleVerified: true })} pinned />)
    expect(labels()).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
    expect(values()).toEqual(['1.3:1', '60%', '87 W', '6 dB'])
  })
})

describe("a dash says which kind of nothing it is", () => {
  it('before the first over it claims nothing about the radio', () => {
    // These fields exist only while transmitting, so a ⊘ here would be asserting that the
    // rig has no ALC meter on the evidence of never having asked it.
    render(<TxMeters radio={radio()} pinned />)
    expect(document.querySelectorAll('.ph-unavail'), 'a meter was written off before the rig ever keyed').toHaveLength(0)
    expect(panel().textContent).toContain('readings appear on transmit')
  })

  it('after an over, a meter that stayed silent is marked ⊘ and the others are not', () => {
    // One each way in ONE render: the rig keyed and reported SWR and Po, so ALC and COMP are
    // meters it does not have. An implementation that marked all four, or none, passes only
    // one half of this.
    const r = render(<TxMeters radio={keyed({ txSwr: 1.3, txPoW: 87, swrScaleVerified: true })} pinned />)
    // …and the mark survives the release, which is when the operator actually reads the panel.
    r.rerender(<TxMeters radio={radio()} pinned />)
    expect(rowFor('SWR').querySelector('.ph-unavail'), 'a meter the rig reported was written off').toBeNull()
    expect(rowFor('PO').querySelector('.ph-unavail'), 'a meter the rig reported was written off').toBeNull()
    expect(rowFor('ALC').querySelector('.ph-unavail'), 'a silent meter says nothing about why it is blank').not.toBeNull()
    expect(rowFor('COMP').querySelector('.ph-unavail')).not.toBeNull()
    // The hint has done its job and goes, exactly as before: the rows now carry their own
    // answers and a standing line saying "on transmit" would contradict the ⊘ beside them.
    expect(panel().textContent).not.toContain('readings appear on transmit')
  })

  it('the readings themselves are retained between overs, dimmed', () => {
    const r = render(<TxMeters radio={keyed({ txSwr: 2.5, swrScaleVerified: true })} pinned />)
    expect(panel().classList.contains('idle')).toBe(false)
    r.rerender(<TxMeters radio={radio()} pinned />)
    expect(rowFor('SWR').querySelector('.ph-txmeter-value')!.textContent, 'the last over was forgotten').toBe('2.5:1')
    expect(panel().classList.contains('idle'), 'a memory is being shown as a live needle').toBe(true)
  })
})

describe('compact when idle, full size when keyed', () => {
  it('the dock panel wears the compact class on receive and drops it on key-down', () => {
    const r = render(<TxMeters radio={keyed({ txSwr: 1.3, swrScaleVerified: true })} pinned />)
    expect(panel().classList.contains('pinned'), 'keyed, and still compact').toBe(false)
    r.rerender(<TxMeters radio={radio({ txSwr: null })} pinned />)
    expect(panel().classList.contains('pinned'), 'idle, and still full size').toBe(true)
  })

  it('the Operate inline cell stays compact THROUGH a key-down', () => {
    // The anti-bounce ruling: that cell is fixed-width chrome in a strip, and the whole
    // reason it exists is that the TX cycle must cause zero geometry change there. The
    // dock's idle/keyed switch must not leak into it.
    const r = render(<TxMeters radio={radio()} inline />)
    expect(panel().classList.contains('pinned')).toBe(true)
    r.rerender(<TxMeters radio={keyed({ txSwr: 1.3, swrScaleVerified: true })} inline />)
    expect(panel().classList.contains('pinned'), 'the inline cell grew on key-down').toBe(true)
  })

  it('the two sizes the class switches between are still in the sheet', () => {
    // The class is the mechanism — the sheet keys on it — but on its own it is a proxy for
    // a size claim that lives somewhere this test cannot see. So the sizes are read from the
    // artifact: delete either rule and "compact vs full" silently becomes one geometry.
    // ⚠️ NOT `new URL('../styles.css', import.meta.url)`, which is the idiom the CSS tests in
    // src/ use and which is WRONG under jsdom: jsdom's URL resolves the relative part
    // against the DOCUMENT's base, so a perfectly good file: base comes back as
    // http://localhost:3000/src/styles.css and fileURLToPath refuses it. Resolve the path as
    // a path.
    const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', 'styles.css'), 'utf8')
    expect(css, 'the full-size track height is gone').toMatch(/\.ph-txmeter-track\s*\{[^}]*height:\s*10px/)
    expect(css, 'the compact track height is gone').toMatch(/\.ph-txmeters\.pinned\s+\.ph-txmeter-track\s*\{[^}]*height:\s*6px/)
  })
})

describe('the default variant is unchanged', () => {
  it('renders nothing at all on receive', () => {
    // Phone and CW both pass `pinned` and Operate passes `inline`, so this variant has no
    // caller in the app — which is exactly why it is pinned here rather than quietly
    // inheriting the four-row contract that was decided for the dock.
    const { container } = render(<TxMeters radio={radio()} />)
    expect(container.querySelector('.ph-txmeters')).toBeNull()
  })

  it('renders the four rows while keyed', () => {
    render(<TxMeters radio={keyed({ txSwr: 1.3, swrScaleVerified: true })} />)
    expect(labels()).toEqual(['SWR', 'ALC', 'PO', 'COMP'])
  })
})
