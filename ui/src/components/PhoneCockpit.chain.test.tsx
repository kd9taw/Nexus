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
import { describe, it, expect, afterEach, beforeAll, beforeEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'
import { PHONE_PANEL_IDS } from '../features/panelState'
import { setAttDb, setPreampDb } from '../api'

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

// The api module is auto-mocked above, so every export is already a `vi.fn`. These two are
// the ones a pad chip commands, and they are cleared per test because several cases below
// assert a call COUNT or that nothing was sent at all.
const mockSetAttDb = setAttDb as unknown as ReturnType<typeof vi.fn>
const mockSetPreampDb = setPreampDb as unknown as ReturnType<typeof vi.fn>
beforeEach(() => {
  mockSetAttDb.mockClear()
  mockSetPreampDb.mockClear()
})

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

/** Everything a rig could report, so the ENABLED half of every pair below is real.
 *
 *  ⚠️ THE PADS ARE TWO FIELDS, NOT ONE, and a fixture carrying only the reading would be a
 *  rig that has published no list — which is a DIFFERENT state (`noSteps`) and not the
 *  "reports everything" this constant is for. `attStepsDb` is what the radio DECLARED;
 *  `attDb` is which of those is in. Both, or the positive half of these cases is not real.
 *  The numbers are an IC-7610's (6/12/18 dB of pad, two preamp positions). */
const FULL_RIG = {
  filterWidthHz: 2400,
  attStepsDb: [6, 12, 18],
  attDb: 0,
  preampStepsDb: [1, 2],
  preampDb: 0,
  monitorGain: 0.4,
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
    // ATT and PRE joined on 2026-09-22 and they sit where the SIGNAL puts them — ahead of
    // RF gain, because the pad and the preamp act on the antenna before anything else in
    // this pane does, and an operator fighting a strong neighbour reaches for ATT first.
    ).toEqual(['BW', 'ATT', 'PRE', 'RF', 'NB', 'NR', 'NRLVL', 'ANF', 'MN', 'NOTCHF', 'AGC', 'AF', 'SQL'])
  })

  it('the TRANSMIT chain holds the controls that shape what goes out — MON included', () => {
    // ⚠️ MON IS HERE AND NOT IN THE RECEIVE PANE. It is the rig playing your own audio back
    // while you TALK: it shapes the over, and it is silent while receiving. Putting it under
    // "what you are hearing" would also sit it beside the AF slider — the exact adjacency
    // (a transmit level next to a receive level, nothing saying which) that split these two
    // panes apart in the first place.
    mount(FULL_RIG)
    const pane = document.querySelector('[data-pane="transmitter"]')!
    expect([...pane.querySelectorAll('[data-chain]')].map((r) => r.getAttribute('data-chain')))
      .toEqual(['MIC', 'COMP', 'COMPLVL', 'VOX', 'MON'])
    expect(paneOf(screen.getByLabelText('Monitor level')), 'MON drifted into the receive chain').toBe('transmitter')
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
    // MON collapses on the ordinary rule — it is a plain level with a reporting field, and
    // a rig that never reports one genuinely does not have a transmit monitor. The two
    // STEPPED controls are the exception and get their own block below.
    ['Monitor level', 'monitorGain', 0.4, 'MON', 'MON', 'transmitter'],
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

  // ⛔ EXCEPTION 3 — the collapse itself, end to end.
  it('a bare rig gets ONE line per pane, not a wall of grey rows', () => {
    // The density requirement in its own words: an IC-7300 does not open to four grey rows.
    //
    // ⚠️ THREE ROWS, NOT ONE, SINCE 2026-09-22, and the two extra are deliberate. A rig that
    // has published no pad list is not a rig without pads — so ATT and PRE keep their rows,
    // dead, saying which unknown that is (`noSteps`), while everything the rig genuinely
    // does not report still collapses. A rig that DOES publish (the FULL_RIG cases above)
    // shows no such row at all, so this is bounded at two and only on rigs Nexus could not
    // read. If that density ever reads wrong on a real radio, the fix is a ruling about
    // `noSteps`, not a quiet collapse back into "not on this radio".
    mount({})
    const rxRows = [...document.querySelectorAll('[data-pane="receiver"] [data-chain]')].map((r) => r.getAttribute('data-chain'))
    expect(rxRows, 'the receive pane is a wall of dead rows').toEqual(['BW', 'ATT', 'PRE'])
    expect(row('BW'), 'the one commandable row left should be BW, which needs no read-back').not.toBeNull()
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

// ── THE TWO STEPPED STAGES (2026-09-22) ──────────────────────────────────────────────
//
// `setAttDb` / `setPreampDb` shipped with the CAT path in 9271de6c and NO operator control
// at all. They are not sliders, and that is the whole of why they needed their own shape:
// an attenuator is the handful of pads THIS radio has — one 20 dB on an IC-7300, 6/12/18 on
// an IC-7610 — and `set_att_db` REJECTS any dB that is not on the published list, because a
// pad the rig does not hold is NAKed or silently rounded to a neighbour and the front end
// then moves by an amount nobody chose.
//
// So the capability answer is the LIST (`attStepsDb`), not the reading, and it is genuinely
// three-state. Each state is asserted here by what the operator SEES, and the three are
// mutually exclusive — a test that could not tell them apart would be no coverage at all.
//
// ⚠️ jsdom NEVER LAYS OUT. Whether two more chip groups still fit the receive pane's wrap at
// 1024 is a browser question and is answered nowhere in this file.
describe('the attenuator and the preamp are picked from the list the RADIO publishes', () => {
  const chips = (chain: 'ATT' | 'PRE') =>
    [...document.querySelectorAll<HTMLButtonElement>(`[data-chain="${chain}"] .theme-chip`)]
  const faces = (chain: 'ATT' | 'PRE') => chips(chain).map((c) => c.textContent)
  const footLine = (pane: 'receiver' | 'transmitter') =>
    document.querySelector(`[data-pane="${pane}"] .ph-chain-absent`)?.textContent ?? ''

  it('PUBLISHED: one chip per pad the rig declared, plus the Off every rig has', () => {
    // `attStepsDb` omits the implicit 0 deliberately (types.ts), so Off is prepended by the
    // cockpit — asserting the faces catches either half going wrong.
    mount({ ...FULL_RIG, attStepsDb: [6, 12, 18] })
    expect(faces('ATT'), 'the chips are not the pads this radio published').toEqual(['Off', '6 dB', '12 dB', '18 dB'])
    expect(chips('ATT').every((c) => !c.getAttribute('aria-disabled')), 'a published pad was dead').toBe(true)
  })

  it('commands the pad the operator picked, and only that one', () => {
    mount({ ...FULL_RIG, attStepsDb: [6, 12, 18], attDb: 0 })
    fireEvent.click(chips('ATT')[2]) // 12 dB
    expect(mockSetAttDb, 'the chip did not command the radio').toHaveBeenCalledTimes(1)
    expect(mockSetAttDb.mock.calls[0], 'a pad other than the one clicked was commanded').toEqual([12])
    expect(mockSetPreampDb, 'the attenuator drove the preamp').not.toHaveBeenCalled()
  })

  it('the chip that is IN is the one the radio reports, and none is when it reports nothing', () => {
    // UNKNOWN is not "off". A rig that publishes pads but has not said which is in has no
    // selection to announce, and `aria-pressed="false"` on every chip announces one.
    mount({ ...FULL_RIG, attStepsDb: [6, 12], attDb: 12 })
    expect(chips('ATT').map((c) => c.getAttribute('aria-pressed'))).toEqual(['false', 'false', 'true'])
    cleanup()
    mount({ ...FULL_RIG, attStepsDb: [6, 12], attDb: undefined })
    expect(chips('ATT').map((c) => c.getAttribute('aria-pressed')), 'an unread pad was announced as Off').toEqual([null, null, null])
  })

  it('EMPTY LIST: the rig says it has no pad — the row collapses to the foot line', () => {
    // The positive answer, and the one the backend is explicit about: "Empty is the
    // different, positive answer: no pad fitted" (`Engine::observe_rig_db_steps`). An IC-905
    // has no preamp at all, and this is the shape that says so without a grey row.
    mount({ ...FULL_RIG, attStepsDb: [], preampStepsDb: [] })
    expect(document.querySelector('[data-chain="ATT"]'), 'a rig with no pad still drew the chips').toBeNull()
    expect(footLine('receiver'), 'a declared absence was not named anywhere').toContain('ATT')
    expect(footLine('receiver')).toContain('PRE')
  })

  it('⛔ NO LIST: the row STAYS, dead, and says which unknown it is', () => {
    // THE ONE THIS DESIGN EXISTS FOR. A rig whose `\dump_state` Nexus could not read may
    // well have a 20 dB pad — so "Not on this radio: ATT" would be a confident wrong answer
    // about the radio, and vanishing would be the silence the ⊘ ruling overturned. Note the
    // reading IS present here (`attDb: 0`, pad out): before this landed that was enough to
    // draw a live control with no chips in it.
    mount({ ...FULL_RIG, attStepsDb: undefined, preampStepsDb: undefined, attDb: 0 })
    expect(document.querySelector('[data-chain="ATT"]'), 'the row vanished instead of saying why').not.toBeNull()
    expect(footLine('receiver'), 'a rig that published nothing was told it has nothing').not.toContain('ATT')
    const mark = document.querySelector('[data-chain="ATT"] .ph-unavail')
    expect(mark, 'the row is dead and says nothing').not.toBeNull()
    expect(mark!.getAttribute('title'), 'the reason does not say what is missing').toMatch(/publish/i)
    // Dead means dead: aria-disabled (a disabled button leaves the tab order and the reason
    // with it) AND the handler swallowed, which aria-disabled does not do by itself.
    expect(chips('ATT').every((c) => c.getAttribute('aria-disabled') === 'true'), 'the chips were live').toBe(true)
    fireEvent.click(chips('ATT')[0])
    expect(mockSetAttDb, 'a dead chip still commanded the radio').not.toHaveBeenCalled()
  })

  it('⚠️ THE PREAMP CARRIES NO UNIT — on an Icom its steps are NAMES, not decibels', () => {
    // `set_preamp_db` takes the LABEL this radio gives each position; on an IC-7300 they are
    // 1 and 2 (P.AMP1/P.AMP2). Printing "1 dB" beside a 1 that means "first preamp" is a
    // wrong number on the screen, not a cosmetic — so only ATT may say dB.
    mount({ ...FULL_RIG, preampStepsDb: [1, 2], attStepsDb: [6] })
    expect(faces('PRE'), 'the preamp positions were printed as gains').toEqual(['Off', '1', '2'])
    expect(faces('ATT'), 'the attenuator lost its unit').toEqual(['Off', '6 dB'])
  })

  it('NO CAT beats the lists — both rows stay, dead, under the pane’s ONE banner', () => {
    // Ordering, and it matters here more than elsewhere: a breaker trip CLEARS the step
    // lists (`clear_rig_db_steps`), so without noCat winning first every pad row would swap
    // the banner for "could not read the steps" at the moment the link died.
    mount({ ...FULL_RIG, catOk: false })
    expect(document.querySelector('[data-chain="ATT"]'), 'a pad row vanished on a dead link').not.toBeNull()
    expect(document.querySelectorAll('[data-chain="ATT"] .ph-unavail'), 'a dead link was reported as a missing list').toHaveLength(0)
    expect(footLine('receiver'), 'a dead link was read as a radio with no pads').toBe('')
    expect(chips('ATT').every((c) => c.getAttribute('aria-disabled') === 'true')).toBe(true)
  })
})
