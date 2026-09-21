// @vitest-environment jsdom
//
// THE TWO QUESTIONS, ONE REGION EACH (operator, 2026-09-20).
//
// A voice operator asks two things of a radio and nothing else: "what goes out when I key?"
// and "what am I hearing?". The pre-rebuild cockpit answered neither in one place — the RX
// filter width sat in the HEADER, the AF gain sat in the header beside it, the mic gain (a
// TRANSMIT control) sat between them, and the rest of the receive chain was split across two
// panes named after their implementation ("DSP functions", "RX DSP levels") rather than after
// the question they answer.
//
// So: one pane per question, and the receive one in SIGNAL ORDER — front end, IF, DSP, audio
// — because that is the order the operator's own hand moves in when a signal is bad.
//
// ⚠️ WHY NOT A FRONT-PANEL REPLICA, which is the obvious alternative and was rejected: the
// REMOTE operator is not three feet from a rig, and this same file is their only panel. A
// layout that assumes the radio is within reach fails exactly the operator who has no radio
// within reach.
//
// ⚠️ jsdom NEVER LAYS OUT. Every assertion here is structure, text or an attribute — DOM
// order, pane membership, `disabled`, accessible names. Nothing reads geometry, because
// nothing here could.
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANEL_IDS } from '../features/panelState'

// Auto-mock every api export rather than listing the ones this file uses: the REAL
// CockpitHeader is mounted (this file asserts what is NO LONGER in it, which a stub could
// never show), and mounting it pulls in RotorStrip and the band plan, whose api calls a
// hand-written mock silently omits.
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

/**
 * A Phone snapshot reporting exactly the surface given.
 *
 * ⚠️ NO DEFAULTED LEVELS — a JS default parameter fires on an explicit `undefined`, which
 * would quietly turn a "the rig reports nothing" case into a copy of the positive one.
 * `catOk` IS defaulted true, because every case here is about the RADIO's capability and a
 * dead CAT link is a different question with its own row (below).
 */
function snapWith(radio: Record<string, unknown>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      sideband: 'USB',
      catOk: true,
      rigMode: 'USB',
      transmitting: false,
      txEnabled: false,
      txAllowed: true,
      slot: 0,
      nextSlotMs: 0,
      ...radio,
    },
    link: {},
    qso: {},
    stations: [],
    conversations: [],
  } as unknown as AppSnapshot
}

const mount = (radio: Record<string, unknown>, phoneMode?: string) =>
  render(<PhoneCockpit snap={snapWith(radio)} theme="dark" phoneMode={phoneMode} />)

/** Everything a rig could report, so the ENABLED half of every pair below is real. */
const FULL_RIG = {
  filterWidthHz: 2400,
  rfGain: 1,
  afGain: 0.5,
  squelch: 0,
  micGain: 0.5,
  nrLevel: 0.3,
  notchFreqHz: 1500,
  compLevel: 0.35,
  agc: 'fast',
  nb: false,
  nr: false,
  notch: false,
  manualNotch: false,
  comp: true,
  vox: false,
}

/** The pane a control renders in — `null` when it is nowhere, so a moved control and a
 *  deleted one can never read the same. */
const paneOf = (el: Element | null) => el?.closest('[data-pane]')?.getAttribute('data-pane') ?? null

describe('one region per question', () => {
  it('the receive chain and the transmit chain are each ONE pane', () => {
    mount(FULL_RIG)
    expect(document.querySelector('[data-pane="receiver"]'), 'no Receiver pane').not.toBeNull()
    expect(document.querySelector('[data-pane="transmitter"]'), 'no Transmitter pane').not.toBeNull()
    // …and the two panes they replace are gone, not left behind as empty boxes with live ⊞
    // entries. A tick that hides nothing is the dead-checkbox defect from the other side.
    expect(document.querySelector('[data-pane="dsp"]'), 'the old DSP pane survived').toBeNull()
    expect(document.querySelector('[data-pane="dspLevels"]'), 'the old RX-DSP-levels pane survived').toBeNull()
  })

  it('both panes are in the ⊞ vocabulary, and the two they replace are not', () => {
    expect(PHONE_PANEL_IDS).toContain('receiver')
    expect(PHONE_PANEL_IDS).toContain('transmitter')
    expect(PHONE_PANEL_IDS).not.toContain('dsp')
    expect(PHONE_PANEL_IDS).not.toContain('dspLevels')
  })

  it('the RECEIVE chain renders in the approved order: filter → gain → DSP → AGC → audio', () => {
    // The order is the whole claim of the pane, and it is the one thing a reader of the JSX
    // can break without noticing. Read the rendered order off the DOM rather than trusting
    // the source — DOM order is what the operator's eye follows.
    //
    // ⚠️ ONE ANCHOR PER CONTROL, not per visual row, and that is not cosmetic: the ⊘ mark
    // hangs off the anchor, so two controls sharing one would make a rig that reports the
    // first and not the second indistinguishable from one that reports neither. The NR
    // toggle and the NR LEVEL are separately reported by Hamlib and get separate anchors for
    // exactly that reason — #95 was a report of the toggle arriving without the level.
    mount(FULL_RIG)
    const pane = document.querySelector('[data-pane="receiver"]')!
    expect(
      [...pane.querySelectorAll('[data-chain]')].map((r) => r.getAttribute('data-chain')),
      'the receive chain is not in the approved order',
    ).toEqual(['BW', 'RF', 'NB', 'NR', 'NRLVL', 'ANF', 'MN', 'NOTCHF', 'AGC', 'AF', 'SQL'])
  })

  it('the TRANSMIT chain holds the controls that shape what goes out', () => {
    mount(FULL_RIG)
    const pane = document.querySelector('[data-pane="transmitter"]')!
    expect([...pane.querySelectorAll('[data-chain]')].map((r) => r.getAttribute('data-chain')))
      .toEqual(['MIC', 'COMP', 'COMPLVL', 'VOX'])
  })
})

describe('the header stops holding receive controls', () => {
  // BW lived in the header, which already wraps at 1024, and it is an IF control that belongs
  // with the rest of the receive chain (operator ruling, 2026-09-20). AF and MIC went the same
  // way for a sharper reason: they were ADJACENT in the header — a receive level and a
  // transmit level, side by side, with nothing saying which was which.
  it('BW is in the receive chain, not the header', () => {
    mount(FULL_RIG)
    const bw = document.querySelector('.ph-filter')
    expect(bw, 'the BW stepper is nowhere at all').not.toBeNull()
    expect(paneOf(bw)).toBe('receiver')
    expect(bw!.closest('.cockpit-header'), 'BW is still in the header').toBeNull()
  })

  it('AF is in the receive chain and MIC is in the transmit chain', () => {
    mount(FULL_RIG)
    expect(paneOf(screen.getByLabelText('AF gain'))).toBe('receiver')
    expect(paneOf(screen.getByLabelText('Mic gain'))).toBe('transmitter')
    expect(screen.getByLabelText('AF gain').closest('.cockpit-header')).toBeNull()
    expect(screen.getByLabelText('Mic gain').closest('.cockpit-header')).toBeNull()
  })
})

// ── DISABLED AND SAID — AND THE THREE PLACES IT MUST NOT REACH ───────────────────────
//
// THE RULE (operator, 2026-09-20): a control the radio cannot drive stays on screen,
// disabled, WITH THE REASON. The argument it overruled — "a control that does nothing is
// worse than none" — is answered in the rewritten assertions in PhoneCockpit.notch.test.tsx
// and PhoneCockpit.analoglevels.test.tsx; the short form is that a vanishing control is
// indistinguishable from one that was never built.
//
// ⛔ THE RULE'S HOME GROUND IS "THE RIG CAN DO IT, THIS PATH CANNOT" — a control Nexus built,
// unreachable through the operator's transport. Three things are NOT that, and the ruling
// was bounded away from all three on 2026-09-21, because disabled-with-reason makes a
// cockpit fuller and without these it becomes clutter:
//
//   1. A control Nexus has built for NO path (TX bandwidth today; ATT, PRE and MON until the
//      sibling's fields land). OMITTED entirely — a row greyed on every radio in the fleet
//      reads as "broken" to a thousand operators, not as "not built yet" — and not named at
//      the foot either, because "not on this radio" would blame the radio for Nexus.
//   2. Absent hardware — no tuner, no amplifier. OMITTED, and the ATU is the case that
//      matters: it KEYS THE TRANSMITTER, so the precaution outranks the discoverability.
//      That one lives in CockpitHeader.atu.test.tsx, where the button is.
//   3. Family-wide feature absence. COLLAPSED to one line at the pane's foot, so an IC-7300
//      does not open to four grey rows and nothing vanishes silently.
//
// Mode-inapplicability is NOT one of them: BW on FM keeps its slot and says so.
describe('a control the radio cannot drive does not simply disappear', () => {
  const footLine = (pane: 'receiver' | 'transmitter') =>
    document.querySelector(`[data-pane="${pane}"] .ph-chain-absent`)?.textContent ?? ''
  const row = (chain: string) => document.querySelector(`[data-chain="${chain}"]`)

  const COLLAPSING: Array<[label: string, field: string, value: unknown, chain: string, plate: string, pane: 'receiver' | 'transmitter']> = [
    ['RF gain', 'rfGain', 0.5, 'RF', 'RF', 'receiver'],
    ['AF gain', 'afGain', 0.5, 'AF', 'AF', 'receiver'],
    ['Squelch', 'squelch', 0.4, 'SQL', 'SQL', 'receiver'],
    ['Noise-reduction level', 'nrLevel', 0.3, 'NRLVL', 'NR', 'receiver'],
    ['Manual notch frequency in hertz', 'notchFreqHz', 1500, 'NOTCHF', 'NOTCH', 'receiver'],
    ['Mic gain', 'micGain', 0.5, 'MIC', 'Mic', 'transmitter'],
    ['Speech processor depth', 'compLevel', 0.35, 'COMPLVL', 'COMP', 'transmitter'],
  ]

  it.each(COLLAPSING)('%s — named at the pane foot when the rig is silent, drawn when it reports', (label, field, value, chain, plate, pane) => {
    mount({})
    expect(row(chain), `${label}: still drawing a row this radio cannot drive`).toBeNull()
    expect(footLine(pane), `${label}: gone from the pane and named nowhere`).toContain(plate)
    cleanup()
    mount({ [field]: value })
    expect(screen.getByLabelText(label), `${label}: not drawn over a radio that reports it`).toBeTruthy()
    expect((screen.getByLabelText(label) as HTMLInputElement).disabled, `${label}: drawn but dead`).toBe(false)
  })

  const FUNCS: Array<[name: string, field: string, chain: string, plate: string, pane: 'receiver' | 'transmitter']> = [
    ['NB', 'nb', 'NB', 'NB', 'receiver'],
    ['NR', 'nr', 'NR', 'NR', 'receiver'],
    ['Auto notch', 'notch', 'ANF', 'Auto notch', 'receiver'],
    ['Manual notch', 'manualNotch', 'MN', 'Manual notch', 'receiver'],
    ['COMP', 'comp', 'COMP', 'COMP', 'transmitter'],
    ['VOX', 'vox', 'VOX', 'VOX', 'transmitter'],
  ]

  it.each(FUNCS)('the %s toggle — named at the pane foot when the rig is silent, live when it reports', (name, field, chain, plate, pane) => {
    mount({})
    expect(row(chain), `${name}: still drawing a row`).toBeNull()
    expect(footLine(pane), `${name}: named nowhere`).toContain(plate)
    cleanup()
    mount({ [field]: false })
    const b = screen.getByRole('button', { name }) as HTMLButtonElement
    expect(b.getAttribute('aria-disabled'), `${name}: dead over a radio that reports it`).toBeNull()
    expect(b.disabled).toBe(false)
  })

  it('the AGC chips collapse as one control, and come back as five', () => {
    mount({})
    expect(document.querySelectorAll('.ph-agc button'), 'a rig with no AGC still drew the chips').toHaveLength(0)
    expect(footLine('receiver')).toContain('AGC')
    cleanup()
    mount({ agc: 'fast' })
    const chips = [...document.querySelectorAll<HTMLButtonElement>('.ph-agc button')]
    expect(chips).toHaveLength(5)
    expect(chips.every((c) => !c.disabled)).toBe(true)
    expect(footLine('receiver')).not.toContain('AGC')
  })

  // ⛔ EXCEPTION 1 — a control with no path on ANY radio is named NOWHERE.
  it('a control Nexus has built no path for is not drawn and not blamed on the radio', () => {
    // ATT, PRE and MON are in the registry so the sibling's fields drop in, and `built:
    // false` is what keeps them off the screen meanwhile. Listing them at the foot would
    // tell a thousand operators their radio is missing something Nexus has not written.
    mount(FULL_RIG)
    for (const chain of ['ATT', 'PRE', 'MON']) {
      expect(row(chain), `${chain} drew a row with no path behind it`).toBeNull()
    }
    for (const plate of ['ATT', 'PRE', 'MON']) {
      expect(footLine('receiver'), `${plate} was blamed on the radio`).not.toContain(plate)
      expect(footLine('transmitter'), `${plate} was blamed on the radio`).not.toContain(plate)
    }
  })

  // ⛔ EXCEPTION 3 — the collapse itself, end to end.
  it('a bare rig gets ONE line per pane, not a wall of grey rows', () => {
    // The density requirement in its own words: an IC-7300 does not open to four grey rows.
    mount({})
    expect(document.querySelectorAll('[data-pane="receiver"] [data-chain]').length, 'the receive pane is a wall of dead rows').toBe(1)
    expect(row('BW'), 'the one row left should be BW, which is commandable with no read-back').not.toBeNull()
    expect(document.querySelectorAll('[data-pane="receiver"] .ph-chain-absent')).toHaveLength(1)
    expect(document.querySelectorAll('[data-pane="transmitter"] [data-chain]')).toHaveLength(0)
    expect(document.querySelectorAll('[data-pane="transmitter"] .ph-chain-absent')).toHaveLength(1)
  })

  // ── WHAT THE RULING STILL GOVERNS ──────────────────────────────────────────────────
  it('NO CAT: every row stays, dead, and the pane says it ONCE', () => {
    // The pane-wide version of "the rig can do it, this path cannot". Collapsing these into
    // "not on this radio" would tell the operator his radio lacks thirteen features when it
    // lacks none of them; a ⊘ per row would print one sentence thirteen times.
    mount({ catOk: false, ...FULL_RIG })
    const banner = document.querySelector('[data-pane="receiver"] .ph-chain-banner')
    expect(banner, 'no CAT and the pane says nothing').not.toBeNull()
    expect(banner!.textContent).toMatch(/CAT/)
    expect(footLine('receiver'), 'a dead link was read as a radio with no features').toBe('')
    expect(row('RF'), 'a row vanished on a dead link').not.toBeNull()
    expect((screen.getByLabelText('RF gain') as HTMLInputElement).disabled, 'a live control over a dead link').toBe(true)
    // …and the row points AT the banner, so a screen reader reaches the reason from the row.
    expect(screen.getByLabelText('RF gain').getAttribute('aria-describedby')).toBe(banner!.id)
    // ONE sentence, not thirteen.
    expect(document.querySelectorAll('[data-pane="receiver"] .ph-chain-banner')).toHaveLength(1)
    expect(document.querySelectorAll('[data-pane="receiver"] .ph-unavail')).toHaveLength(0)
  })

  it('BW on FM keeps its slot and names the MODE, not the radio', () => {
    // Mode-inapplicability is explicitly NOT one of the three exceptions: FM's passband is
    // fixed, which is a fact about the mode and says nothing about what the radio can do.
    mount(FULL_RIG, 'fm')
    expect(row('BW'), 'BW collapsed on FM as though the radio lacked it').not.toBeNull()
    expect(footLine('receiver'), 'FM was read as a missing feature').not.toContain('BW')
    const mark = document.querySelector('[data-chain="BW"] .ph-unavail')
    expect(mark, 'BW is dead on FM and says nothing').not.toBeNull()
    expect(mark!.textContent, 'the mark does not name the mode').toContain('FM')
    expect(mark!.getAttribute('title')).toMatch(/does not apply on FM/i)
    expect([...document.querySelectorAll<HTMLButtonElement>('.ph-filter-step')].every((b) => b.disabled)).toBe(true)
  })

  it('BW still steps on a rig that reports no width — a blank readout is not a dead control', () => {
    // The disconfirming case. A rig that never reports `filterWidthHz` reads '—' and is
    // STILL commandable; collapsing it would take away a control that works, which is the
    // same defect as hiding one, wearing the new clothes.
    mount({})
    expect([...document.querySelectorAll<HTMLButtonElement>('.ph-filter-step')].every((b) => !b.disabled)).toBe(true)
    expect(footLine('receiver'), 'a working BW was listed as missing').not.toContain('BW')
    expect(document.querySelector('.ph-filter-val')!.textContent).toBe('—')
  })

  it('CONTROL — a rig that reports everything gets no foot line and no marks', () => {
    // Without this the whole file passes against a cockpit that collapses every control on
    // every radio, which is the failure mode of a negative-space assertion.
    mount(FULL_RIG)
    expect(footLine('receiver'), 'a fully-reporting rig was told it is missing something').toBe('')
    expect(footLine('transmitter')).toBe('')
    expect(document.querySelectorAll('[data-pane="receiver"] .ph-unavail')).toHaveLength(0)
    expect(document.querySelectorAll('[data-pane="receiver"] .ph-chain-banner')).toHaveLength(0)
  })
})
