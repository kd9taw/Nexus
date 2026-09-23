// @vitest-environment jsdom
//
// THE TRANSMIT CONTRACT — "what goes out when I key?", answered in one line that never moves.
//
// A voice operator's first question has never had one place to read the answer. The dial is
// in the header, the split offset is a chip beside it, XIT is in the tuning strip, the power
// is a slider in the actions cluster, and NONE of them is the emission: under split the RX
// dial is not a conservative stand-in for the TX frequency, it is an unrelated number, and
// XIT moves the transmitter without moving anything on screen at all.
//
// ⭐ IT IS A READOUT OF THE ENGINE'S OWN VERDICT, AND IT COMPUTES NOTHING. `Engine::
// tx_freq_verdict` is the one decision — which frequency the next over is emitted on, and how
// sure we are — and `tx_emission_mhz` is what it resolves to, XIT folded in. A UI that added
// a dial to an offset here would be a SECOND answer to that question, free to disagree with
// the one the licence gate actually judges. So the number is printed verbatim and the only
// thing decided here is how well each part of it is KNOWN.
//
// ⚠️ THE HONESTY MECHANISM, which is the point of the whole strip:
//   ✓rig  — a genuine read-back.
//   ⌁cmd  — Nexus commanded it and CANNOT read it back. RIT, XIT and VFO A/B are write-only
//           (engine.rs:5558, :18262), and so is a split Nexus set: the snapshot carries
//           `splitTxMhz` but not the verdict that says whether the rig acknowledged it.
//   ⊘     — unavailable, and why.
// Text marks, never colour alone; and a commanded value is never rendered as though it were
// read, which is the assertion the XIT cases below exist for.
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { getSettings } from '../api'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANEL_IDS } from '../features/panelState'
import type { PanelLayoutApi, PhonePanelId } from '../features/panelState'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [] }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

function snapWith(radio: Record<string, unknown>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.226,
      band: '20m',
      sideband: 'USB',
      catOk: true,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      splitTxMhz: null,
      ...radio,
    },
  } as unknown as AppSnapshot
}

/** `phoneMode` is a PROP, not a snapshot field — it is what resolves the cockpit's commanded
 *  mode to FM, and passing it is the only way to reach the FM branch. */
const mount = (radio: Record<string, unknown> = {}, panels?: PanelLayoutApi<PhonePanelId>, phoneMode?: string) =>
  render(<PhoneCockpit snap={snapWith(radio)} theme="dark" panels={panels} phoneMode={phoneMode} />)

const strip = () => document.querySelector('.cockpit-txdock .ph-txcontract')
/** One cell of the strip, by the part of the contract it answers for. */
const cell = (name: string) => document.querySelector(`.ph-txcontract [data-txc="${name}"]`)
const markIn = (name: string) => cell(name)?.querySelector('.ph-truth')

function fakePanels(removed: PhonePanelId[]): PanelLayoutApi<PhonePanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  }
}

describe('the transmit contract is always on screen and never moves', () => {
  it('renders in the dock, ABOVE the meters and above the PTT row', () => {
    // The dock is bottom-anchored, so anything that GROWS below the PTT row pushes the
    // button out from under a held pointer mid-over and `onPointerLeave` unkeys the rig.
    // The strip is one fixed line, but its position is the belt to that brace: above the
    // meters, which are above the row.
    mount({ transmitting: true, txSwr: 1.3 })
    const s = strip()
    expect(s, 'no transmit-contract strip in the dock').not.toBeNull()
    const meters = document.querySelector('.cockpit-txdock .ph-txmeters')!
    const ptt = document.querySelector('.cockpit-txdock .ph-ptt-row')!
    expect(s!.compareDocumentPosition(meters) & Node.DOCUMENT_POSITION_FOLLOWING, 'the strip is below the meters').toBeTruthy()
    expect(s!.compareDocumentPosition(ptt) & Node.DOCUMENT_POSITION_FOLLOWING, 'the strip is below the PTT row').toBeTruthy()
  })

  it('survives every ⊞ tick, singly and all at once — it has no id to hide it by', () => {
    for (const id of PHONE_PANEL_IDS) {
      mount({}, fakePanels([id]))
      expect(strip(), `hiding "${id}" took the transmit contract with it`).not.toBeNull()
      cleanup()
    }
    mount({}, fakePanels([...PHONE_PANEL_IDS]))
    expect(strip(), 'hiding the whole vocabulary took the transmit contract with it').not.toBeNull()
  })

  it('is on screen while receiving, not only while keyed', () => {
    // A contract you can only read while holding the mic key is one you can never check
    // BEFORE keying, which is the only time checking it would have helped.
    mount({ transmitting: false })
    expect(strip()).not.toBeNull()
  })

  it('carries no button at all', () => {
    // Stop-line hygiene, asserted rather than assumed: this is a READOUT. A control here
    // would be a transmit control living outside the ⊞ vocabulary and outside both sweeps,
    // and the next person to add one would have no test telling them not to.
    mount({ transmitting: true, txSwr: 1.3 })
    expect(strip()!.querySelectorAll('button, input, select')).toHaveLength(0)
  })
})

describe('the frequency cell is the ENGINE’s emission, not a dial the UI added up', () => {
  it('prints txEmissionMhz, even when it differs from the dial', () => {
    // ⭐ THE LOAD-BEARING CASE. Under split the RX dial and the emission are unrelated
    // numbers; the engine already decided which one the licence gate judges. A strip that
    // printed the dial here would be a second answer, and the two would disagree exactly
    // when it mattered. The two values differ deliberately so this can fail.
    mount({ dialMhz: 14.226, txEmissionMhz: 14.231, splitTxMhz: 14.231 })
    expect(cell('freq')!.textContent).toContain('14.2310')
    expect(cell('freq')!.textContent, 'the strip printed the RX dial as the emission').not.toContain('14.2260')
  })

  it('says ⊘ when the station reports no emission at all', () => {
    // An older station, or one that has not answered yet. Printing the dial instead would
    // be inventing the one number the whole strip exists to state.
    mount({ txEmissionMhz: null })
    const mark = markIn('freq')
    expect(mark, 'no emission and no mark — the strip is silently showing something else').not.toBeNull()
    expect(mark!.textContent).toMatch(/⊘/)
    expect(cell('freq')!.textContent).not.toContain('14.226')
  })

  it('marks the emission ✓rig on a CAT link with no clarifier offset', () => {
    mount({ txEmissionMhz: 14.226, xitHz: 0 })
    expect(markIn('freq')!.textContent).toContain('rig')
    expect(markIn('freq')!.textContent).not.toContain('cmd')
  })

  it('DEMOTES the emission to ⌁cmd as soon as XIT is non-zero', () => {
    // XIT rides INTO tx_emission_mhz (engine.rs) and the XIT path is write-only and
    // optimistic — an offset dialled on the radio's own clarifier knob is invisible here.
    // So the moment XIT is in play, the emission contains a component nothing read back,
    // and calling the whole number a read-back would be the exact lie the marks exist to
    // stop. Paired with the case above, which is the same snapshot with xitHz 0.
    mount({ txEmissionMhz: 14.2265, xitHz: 500 })
    expect(markIn('freq')!.textContent, 'an emission carrying a write-only XIT offset is claimed as read').toContain('cmd')
  })

  it('marks it ⌁cmd with no CAT link, because nothing can be read back', () => {
    mount({ txEmissionMhz: 14.226, catOk: false })
    expect(markIn('freq')!.textContent).toContain('cmd')
  })
})

// ⛔ FM THROUGH A REPEATER — the one case where `tx_emission_mhz` is NOT the emission.
//
// `Engine::tx_emission_mhz` (engine.rs:17677) is `verdict base + xit_offset` and nothing
// else: there is no `rptr` anywhere in it. The repeater shift lives in `settings.rptr_shift`
// / `rptr_offset_hz()` and is applied on the rig-CONFIGURATION path (:7720, :15339), so on
// FM through a repeater with a +600 kHz shift the figure the engine reports is the dial the
// operator is LISTENING on and the transmitter is 600 kHz away.
//
// A panel whose entire job is "what happens when I key" cannot state a number that is wrong
// by the whole shift on exactly the mode where the answer is least obvious. So it states
// nothing there, and says why.
//
// ⚠️ IT CANNOT SIMPLY ADD THE SHIFT ON. `rptr_offset_hz()` is an override or a BAND-CONVENTION
// TABLE in Rust (settings.rs:360) — re-deriving that here would be a second copy of engine
// arithmetic, free to drift, which is the same defect as printing the wrong number slower.
// The fix belongs in `tx_emission_mhz`; until it lands, silence is the honest answer and the
// report says so.
describe('FM through a repeater: no confident wrong number', () => {
  /** Mount and let the settings read (the only place the shift is visible) resolve. */
  async function mountFm(shift: unknown, radio: Record<string, unknown> = {}) {
    vi.mocked(getSettings).mockResolvedValue(
      (shift === undefined ? {} : { rptrShift: shift }) as never,
    )
    const r = mount({ txEmissionMhz: 146.94, rigMode: 'FM', ...radio }, undefined, 'fm')
    await act(async () => { await Promise.resolve() })
    return r
  }

  it('states NO transmit frequency on a repeater shift, and says why', async () => {
    await mountFm('plus')
    expect(cell('freq')!.textContent, 'the dial was printed as the emission on a repeater').not.toContain('146.94')
    const mark = markIn('freq')
    expect(mark, 'no frequency and no reason').not.toBeNull()
    expect(mark!.textContent).toMatch(/⊘/)
    expect(mark!.getAttribute('title'), 'the reason does not name the repeater shift').toMatch(/repeater/i)
  })

  it('states it normally on FM SIMPLEX — the shift is what disqualifies it, not the mode', async () => {
    // THE PAIR. Without it, "omit on FM" would pass by omitting on 146.520 too, which would
    // take the contract away from every simplex FM operator to fix a repeater bug.
    await mountFm('simplex')
    expect(cell('freq')!.textContent, 'FM simplex lost its transmit frequency').toContain('146.94')
    expect(markIn('freq')!.textContent).not.toMatch(/⊘/)
  })

  it('states NOTHING while the shift is still unknown — unread is not simplex', async () => {
    // The fail-safe direction. A settings read that has not landed, or a station whose
    // settings carry no shift at all, must not be resolved to "simplex" — that is the one
    // default that puts the confident wrong number back.
    await mountFm(undefined)
    expect(cell('freq')!.textContent).not.toContain('146.94')
    expect(markIn('freq')!.textContent).toMatch(/⊘/)
  })

  it('leaves SSB alone, whatever the shift setting says', async () => {
    // The shift is an FM setting; a stale 'plus' left in settings must not blank the
    // contract on 20 m sideband.
    vi.mocked(getSettings).mockResolvedValue({ rptrShift: 'plus' } as never)
    mount({ txEmissionMhz: 14.226, rigMode: 'USB' })
    await act(async () => { await Promise.resolve() })
    expect(cell('freq')!.textContent).toContain('14.2260')
    expect(markIn('freq')!.textContent).not.toMatch(/⊘/)
  })
})

describe('the mode, split and XIT cells each say how well they are known', () => {
  it('the mode is ✓rig when the radio read it back', () => {
    mount({ rigMode: 'USB', catOk: true })
    expect(cell('mode')!.textContent).toContain('USB')
    expect(markIn('mode')!.textContent).toContain('rig')
  })

  it('the mode is ⌁cmd when the radio said nothing', () => {
    // The pair. `commandedMode` is what Nexus asked for — with no read-back it is the only
    // thing we have, and it must not wear the read-back mark.
    mount({ rigMode: '', catOk: true })
    expect(markIn('mode')!.textContent).toContain('cmd')
  })

  it('split off reads simplex, and claims nothing', () => {
    mount({ splitTxMhz: null })
    expect(cell('split')!.textContent).toMatch(/simplex/i)
  })

  it('split on prints the offset and is ⌁cmd — the snapshot cannot prove the rig took it', () => {
    // ⚠️ AND THAT IS A GAP IN THE DTO, not a judgement about the radio. The engine DOES know
    // — `tx_freq_verdict` distinguishes a split we commanded and the rig acknowledged from
    // one it merely reported, and refuses outright when it cannot say where. None of that
    // reaches the snapshot: `txEmissionMhz` collapses SplitUnverified onto the dial and the
    // verdict itself is not a field. Until it is, the honest mark for a split is "Nexus
    // commanded this", and that is what is pinned here — deliberately, so the day the field
    // lands this test is what says the mark may be promoted.
    mount({ dialMhz: 14.226, splitTxMhz: 14.231, txEmissionMhz: 14.231 })
    expect(cell('split')!.textContent).toContain('+5')
    expect(markIn('split')!.textContent).toContain('cmd')
  })

  it('XIT is ⌁cmd even at zero, because the clarifier knob is on the radio', () => {
    // Write-only means Nexus cannot see an offset the operator dialled at the rig, so "+0"
    // is a statement about what Nexus commanded and not about where the transmitter is.
    mount({ xitHz: 0 })
    expect(cell('xit')!.textContent).toContain('0')
    expect(markIn('xit')!.textContent).toContain('cmd')
  })

  it('XIT prints the offset Nexus commanded', () => {
    mount({ xitHz: 500 })
    expect(cell('xit')!.textContent).toContain('500')
    expect(markIn('xit')!.textContent).toContain('cmd')
  })

  it('a radio with no XIT has no XIT cell — the IC-9700 has no clarifier knob to account for', () => {
    // The "+0 ⌁cmd" above exists because an offset may be dialled on the radio's own XIT
    // knob. A radio with no XIT has no such knob, so the cell would describe nothing real.
    mount({ xitUnsupported: true, xitHz: 0 })
    expect(cell('xit'), 'an XIT cell on a radio that has none').toBeNull()
    expect(cell('split'), 'control: the rest of the contract is still drawn').not.toBeNull()
  })
})

describe('power, and who holds the transmitter', () => {
  it('shows the power setting and the watts last measured', () => {
    mount({ rfPower: 0.75, txPoW: 87 })
    expect(cell('power')!.textContent).toContain('75')
    expect(cell('power')!.textContent, 'the last measured output is not on the contract').toContain('87')
  })

  it('shows no watts when the rig has never reported any', () => {
    mount({ rfPower: 0.75, txPoW: null })
    expect(cell('power')!.textContent).toContain('75')
    expect(cell('power')!.textContent).not.toMatch(/\bW\b/)
  })

  it('names the transmit owner, in the engine’s own words', () => {
    // `txBusyReason` is the arbiter's answer and covers all seven TX owners — not a flag
    // pair for the UI to re-derive. It is printed verbatim for the same reason the emission
    // is: a second wording here could disagree with the one that actually holds the rig.
    mount({ txBusyReason: 'the voice keyer is sending' })
    expect(cell('busy')!.textContent).toContain('the voice keyer is sending')
  })

  it('warns on the contract while VOX is on — Stop TX cannot unkey that', () => {
    // A SAFETY line rather than a status one. With VOX on the operator does not key at all:
    // the radio does, from the microphone, and Stop TX halts what Nexus is doing. It belongs
    // on the strip because it changes the answer to "what happens when I key".
    mount({ vox: true })
    expect(cell('vox'), 'VOX is on and the contract says nothing').not.toBeNull()
    expect(cell('vox')!.getAttribute('title')).toMatch(/Stop TX cannot unkey/i)
  })

  it('says nothing about VOX when it is off, and nothing when the rig has none', () => {
    // The pair, both arms: a standing VOX warning would be noise on every SSB contact, and
    // `null` (a rig that does not report VOX at all) is not `true`.
    mount({ vox: false })
    expect(cell('vox'), 'VOX off is being warned about').toBeNull()
    cleanup()
    mount({})
    expect(cell('vox'), 'a rig that reports no VOX is being warned about').toBeNull()
  })

  it('says nothing about the owner when nobody holds the transmitter', () => {
    mount({ txBusyReason: null })
    expect(cell('busy'), 'an idle transmitter is being announced as busy').toBeNull()
  })
})
