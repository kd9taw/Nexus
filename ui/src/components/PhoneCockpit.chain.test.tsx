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

// ── DISABLED AND SAID, never gated by absence ─────────────────────────────────────────
//
// THE RULE the operator ruled for (2026-09-20): a control the radio cannot drive stays on
// screen, disabled, WITH THE REASON. The argument it overruled — "a control that does
// nothing is worse than none" — is answered in the rewritten assertions in
// PhoneCockpit.notch.test.tsx and PhoneCockpit.analoglevels.test.tsx; the short form is that
// a vanishing control is indistinguishable from one that was never built, so the operator
// cannot tell "your radio can't" from "Nexus didn't".
//
// ⚠️ EVERY CASE BELOW IS A PAIR. A disabled-with-reason assertion on its own passes against a
// component that renders a dead stub for everything, so each one states the ENABLED render
// too: same control, rig reporting, no mark, not disabled.
describe('a control the radio cannot drive is disabled and says why', () => {
  /** Present AND disabled AND the reason — three things, because any two of them pass on a
   *  half-built control. Returns the mark so a caller can read what it says. */
  function unavailable(el: HTMLElement, what: string) {
    expect(el, `${what}: not on screen at all — that is gating by absence`).toBeTruthy()
    expect((el as HTMLInputElement | HTMLButtonElement).disabled, `${what}: on screen and LIVE over a radio that cannot drive it`).toBe(true)
    const mark = el.closest('[data-chain]')?.querySelector('.ph-unavail')
    expect(mark, `${what}: disabled with no reason anywhere — the operator cannot tell "your radio can't" from "Nexus didn't"`).not.toBeNull()
    expect(mark!.textContent, `${what}: the mark is not TEXT`).toMatch(/\S/)
    return mark!
  }
  function available(el: HTMLElement, what: string) {
    expect(el, `${what}: not on screen`).toBeTruthy()
    expect((el as HTMLInputElement | HTMLButtonElement).disabled, `${what}: disabled over a radio that reports it`).toBe(false)
    expect(
      el.closest('[data-chain]')?.querySelector('.ph-unavail'),
      `${what}: marked unavailable over a radio that reports it`,
    ).toBeNull()
  }

  const SLIDERS: Array<[label: string, field: string, value: unknown]> = [
    ['RF gain', 'rfGain', 0.5],
    ['AF gain', 'afGain', 0.5],
    ['Squelch', 'squelch', 0.4],
    ['Noise-reduction level', 'nrLevel', 0.3],
    ['Manual notch frequency in hertz', 'notchFreqHz', 1500],
    ['Mic gain', 'micGain', 0.5],
    ['Speech processor depth', 'compLevel', 0.35],
  ]

  it.each(SLIDERS)('%s — disabled with a reason when the rig is silent, live when it reports', (label, field, value) => {
    mount({})
    unavailable(screen.getByLabelText(label), `${label} (rig silent)`)
    cleanup()
    mount({ [field]: value })
    available(screen.getByLabelText(label), `${label} (rig reporting)`)
  })

  const FUNCS = ['NB', 'NR', 'Auto notch', 'Manual notch', 'COMP', 'VOX'] as const
  const FUNC_FIELD: Record<string, string> = {
    NB: 'nb', NR: 'nr', 'Auto notch': 'notch', 'Manual notch': 'manualNotch', COMP: 'comp', VOX: 'vox',
  }

  it.each(FUNCS)('the %s toggle — disabled with a reason when the rig is silent, live when it reports', (name) => {
    mount({})
    unavailable(screen.getByRole('button', { name }), `${name} (rig silent)`)
    cleanup()
    mount({ [FUNC_FIELD[name]]: false })
    available(screen.getByRole('button', { name }), `${name} (rig reporting)`)
  })

  it('the AGC chips — disabled with a reason when the rig is silent, live when it reports', () => {
    mount({})
    const chips = [...document.querySelectorAll<HTMLButtonElement>('.ph-agc button')]
    expect(chips, 'the AGC row vanished on a rig that does not report AGC').toHaveLength(5)
    expect(chips.every((c) => c.disabled), 'an AGC chip is live over a radio that reports no AGC').toBe(true)
    expect(document.querySelector('[data-chain="AGC"] .ph-unavail'), 'AGC is dead and silent').not.toBeNull()
    cleanup()
    mount({ agc: 'fast' })
    expect([...document.querySelectorAll<HTMLButtonElement>('.ph-agc button')].every((c) => !c.disabled)).toBe(true)
    expect(document.querySelector('[data-chain="AGC"] .ph-unavail'), 'AGC marked unavailable over a rig that reports it').toBeNull()
  })

  // ⚠️ BW'S GATE IS NOT ITS READ-BACK, and pinning the wrong one here would have written a
  // regression into the suite. `filterWidthHz` null means "unknown or the rig's default" —
  // the stepper still commands the rig perfectly well from its 2.4 kHz base, and only the
  // READOUT is blank. What actually stops the control working is a dead CAT link, and what
  // makes it meaningless is FM, whose passband is fixed. So those are the two cases.
  const bwSteps = () => [...document.querySelectorAll<HTMLButtonElement>('.ph-filter-step')]

  it('BW — disabled with a reason when CAT is down, live when it is up', () => {
    mount({ catOk: false })
    expect(bwSteps(), 'the BW stepper vanished').toHaveLength(2)
    expect(bwSteps().every((b) => b.disabled)).toBe(true)
    expect(document.querySelector('[data-chain="BW"] .ph-unavail')).not.toBeNull()
    cleanup()
    mount(FULL_RIG)
    expect(bwSteps().every((b) => !b.disabled)).toBe(true)
    expect(document.querySelector('[data-chain="BW"] .ph-unavail')).toBeNull()
  })

  it('BW — disabled with a reason on FM, where the passband is fixed', () => {
    mount(FULL_RIG, 'fm')
    expect(bwSteps(), 'the BW stepper vanished on FM').toHaveLength(2)
    expect(bwSteps().every((b) => b.disabled)).toBe(true)
    const mark = document.querySelector('[data-chain="BW"] .ph-unavail')
    expect(mark, 'BW is dead on FM and does not say why').not.toBeNull()
    // …and the reason names FM rather than blaming the radio, because the radio is fine.
    expect(mark!.getAttribute('title')).toMatch(/FM/)
  })

  it('BW still steps on a rig that reports no width — a blank readout is not a dead control', () => {
    // The disconfirming case for the two above. A rig that never reports `filterWidthHz`
    // reads '—' and is STILL commandable; marking it unavailable would take away a control
    // that works, which is the same defect as hiding one, wearing the new clothes.
    mount({})
    expect(bwSteps().every((b) => !b.disabled), 'a blank BW readout disabled a working stepper').toBe(true)
    expect(document.querySelector('[data-chain="BW"] .ph-unavail')).toBeNull()
    expect(document.querySelector('.ph-filter-val')!.textContent).toBe('—')
  })

  it('the manual-notch FREQUENCY names the backend, not the radio', () => {
    // The distinction is not pedantry and it is not a Nexus gap: Hamlib's Icom backend has
    // no NOTCHF for ANY natively-driven model, so on those rigs the notch itself is real and
    // reachable from the radio's own knob while the FREQUENCY is not reachable over CAT.
    // A reason that said "your radio does not have this" would be false about the radio.
    mount({ manualNotch: true })
    const slider = screen.getByLabelText('Manual notch frequency in hertz') as HTMLInputElement
    expect(slider.disabled).toBe(true)
    const mark = document.querySelector('[data-chain="NOTCHF"] .ph-unavail')!
    expect(mark.getAttribute('title'), 'the notch-frequency reason does not name the CAT backend').toMatch(/backend/i)
  })

  it('CONTROL — a rig that reports everything wears no ⊘ mark anywhere in the two chains', () => {
    // Without this the whole file would pass against a cockpit that marks every control
    // unavailable forever, which is the failure mode of a negative-space assertion.
    //
    // ⚠️ SCOPED TO THE TWO PANES, and it was not at first: a document-wide sweep for
    // `.ph-unavail` also catches the HEADER's ATU mark, which is correct there (FULL_RIG
    // reports no `atu`) and has nothing to do with the receive and transmit chains. It is
    // named rather than widened — the ATU's own pair is in CockpitHeader.atu.test.tsx.
    mount(FULL_RIG)
    const inChains = [...document.querySelectorAll('[data-pane="receiver"] .ph-unavail, [data-pane="transmitter"] .ph-unavail')]
    expect(
      inChains.map((m) => m.closest('[data-chain]')?.getAttribute('data-chain')),
      'a fully-reporting rig is being told it cannot do something',
    ).toEqual([])
  })
})
