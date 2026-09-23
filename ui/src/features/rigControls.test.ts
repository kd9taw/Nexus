// THE REGISTRY'S DECISIONS, tested where they are pure — before any pane renders them.
//
// Everything here is the part of "disabled and said" that is a judgement rather than a
// drawing: which cause is the one to name, which controls get a row at all, and which get
// collapsed into the pane-foot line. The panes' own test asserts the rendered result; this
// one asserts the reasoning, so a wrong answer is caught at the size of a table.
import { describe, it, expect } from 'vitest'
import {
  RIG_CONTROLS,
  CAUSE_KEY,
  causeFor,
  rendersRow,
  absentPlates,
  chainControls,
  deadControlProps,
  guard,
  capStateFor,
  stepsFor,
  subCauseFor,
  subRendersRow,
  subChainControls,
  subUnconfirmedPlates,
  type ControlState,
  type RigControl,
  type SubState,
  type UnavailableCause,
} from './rigControls'
import type { ReceiversStatus, ReceiverStatus, StageOwner, SubCapability } from '../types'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const byId = (id: string) => RIG_CONTROLS.find((c) => c.id === id)!

/** A station state. `reported` defaults to "this radio reports everything", so each case
 *  states only the thing it is about — and NOTHING here defaults a capability to absent,
 *  which would make half the assertions below pass for the wrong reason. */
const state = (over: Partial<ControlState> = {}): ControlState => ({
  catOk: true,
  reported: () => true,
  mode: 'USB',
  ...over,
})

describe('the registry is a table, and the table is the contract', () => {
  it('every id is unique — the id is the row anchor and the join key', () => {
    const ids = RIG_CONTROLS.map((c) => c.id)
    expect(new Set(ids).size, 'a duplicate id would make two controls share one ⊘ anchor').toBe(ids.length)
  })

  it('every cause in the union has a catalogue key', () => {
    // The map is `Record<UnavailableCause, string>`, so a cause added without a wording does
    // not typecheck — this checks the other direction, that none of them is blank.
    for (const [cause, key] of Object.entries(CAUSE_KEY)) {
      expect(key, `${cause}: no catalogue key`).toMatch(/^phone\./)
    }
  })

  it('the receive chain is in signal order and the transmit chain follows it', () => {
    expect(RIG_CONTROLS.filter((c) => c.chain === 'rx').map((c) => c.id)).toEqual([
      'BW', 'ATT', 'PRE', 'RF', 'NB', 'NR', 'NRLVL', 'ANF', 'MN', 'NOTCHF', 'AGC', 'AF', 'SQL',
    ])
    // ⚠️ MON IS ON THE TRANSMIT CHAIN AND THAT IS NOT A FILING ACCIDENT. It is the rig
    // playing YOUR OWN audio back while you talk — heard only while transmitting — so it
    // shapes the over, not what the receiver hears, and it belongs beside MIC and COMP.
    // Moving it to `rx` would put transmitted audio in the pane labelled "what you are
    // hearing" and sit it next to AF gain, which is the adjacency the two panes were split
    // up to end.
    expect(RIG_CONTROLS.filter((c) => c.chain === 'tx').map((c) => c.id)).toEqual([
      'MIC', 'COMP', 'COMPLVL', 'VOX', 'MON',
    ])
  })

  it('every control has a path built behind it, and the stepped pair has its reading field', () => {
    // ATT, PRE and MON were the last three `built: false` slots and landed 2026-09-22. The
    // table is now wholly built — which is a fact worth pinning, because `built: false` is
    // an EXCEPTION whose test below now has to supply its own fixture.
    expect(RIG_CONTROLS.filter((c) => !c.built).map((c) => c.id)).toEqual([])
    // ⛔ AND `field` IS NOT OPTIONAL FOR THESE TWO. It is where the ACTIVE chip is read
    // from; pointing one at a neighbour's field, or leaving it off to satisfy the type, is
    // how a row starts answering for the wrong stage.
    expect(byId('ATT').field).toBe('attDb')
    expect(byId('PRE').field).toBe('preampDb')
    expect(byId('MON').field).toBe('monitorGain')
  })
})

describe('which cause is the one to name', () => {
  it('a radio that drives it has no cause at all', () => {
    expect(causeFor(byId('RF'), state())).toBeNull()
  })

  it('no CAT beats everything — one explanation for the whole pane', () => {
    // It is true of every control at once, so the pane says it once as a banner rather than
    // printing it thirteen times. That is only sound if it also WINS here.
    expect(causeFor(byId('RF'), state({ catOk: false }))).toBe('noCat')
    expect(causeFor(byId('BW'), state({ catOk: false, mode: 'FM' })), 'FM outranked a dead link').toBe('noCat')
    expect(causeFor(byId('RF'), state({ catOk: false, reported: () => false }))).toBe('noCat')
  })

  it('BW on FM is not-on-mode, and on SSB it is nothing', () => {
    // A fact about the MODE, not a fault in the radio — which is why it keeps its row
    // instead of collapsing into the "not on this radio" line with things the rig lacks.
    expect(causeFor(byId('BW'), state({ mode: 'FM' }))).toBe('notOnMode')
    expect(causeFor(byId('BW'), state({ mode: 'USB' })), 'BW is dead on sideband').toBeNull()
  })

  it('mode-inapplicability is BW’s alone — it does not leak to its neighbours', () => {
    // The scope guard. FM has an AF gain and a squelch like any other mode; a blanket
    // "FM disables the receive chain" would be a much larger and wrong claim.
    for (const id of ['RF', 'AF', 'SQL', 'NB', 'AGC']) {
      expect(causeFor(byId(id), state({ mode: 'FM' })), `${id} went dead on FM`).toBeNull()
    }
  })

  it('a field this radio has never reported is absent', () => {
    expect(causeFor(byId('NOTCHF'), state({ reported: () => false }))).toBe('absent')
  })

  it('a BUILT control with no capability field is available — BW is not its read-back', () => {
    // The registry's own version of a trap the panes already fell into once: `filterWidthHz`
    // null means "unknown or the rig's default", and the stepper works anyway. Reading a
    // blank READOUT as "this radio has no filter" would collapse a working control into the
    // foot line, which is the vanishing the whole ruling is against.
    expect(causeFor(byId('BW'), state({ reported: () => false }))).toBeNull()
    expect(absentPlates('rx', state({ reported: () => false })), 'BW was listed as missing').not.toContain('BW')
  })

  it('the caps masks, when they land, settle it WITHOUT waiting for a probe', () => {
    // The masks are what `\dump_state` gives and they do not depend on whether a read ever
    // came back — `present` settles a control whose value has never arrived, and `absent`
    // settles one whose probe will never answer.
    const s = state({ caps: { levelSet: ['RF'] }, reported: () => false })
    expect(causeFor(byId('RF'), s), 'a masked-present control was still waiting for a poll').toBeNull()
    expect(causeFor(byId('NOTCHF'), s), 'a level missing from a PRESENT mask is absent').toBe('absent')
  })

  it('⛔ UNKNOWN IS NOT ABSENT — a missing mask claims nothing', () => {
    // The whole reason the masks are positive and three-state. A control absent from a mask
    // that DOES NOT EXIST has not been judged at all, and treating that as "your radio does
    // not have it" is the same default that resolves an unread repeater shift to simplex.
    // Here the FUNC masks are supplied and the LEVEL masks are not, in one state.
    const s = state({ caps: { funcSet: ['NB'] }, reported: (c: RigControl) => c.id === 'RF' })
    expect(capStateFor(byId('RF'), s.caps), 'a missing mask was read as an empty one').toBe('unknown')
    expect(causeFor(byId('RF'), s), 'unknown fell through to the observed answer — RF was reported').toBeNull()
    expect(causeFor(byId('NRLVL'), s), 'unknown + never reported is still absent, from observation').toBe('absent')
    // …and the FUNC mask that IS present judges its own dimension.
    expect(capStateFor(byId('NB'), s.caps)).toBe('present')
    expect(capStateFor(byId('ANF'), s.caps), 'a func missing from a PRESENT func mask is absent').toBe('absent')
  })

  it('no caps at all is unknown for everything — today’s world, unchanged', () => {
    // The state the app is actually in until the backend lands. Nothing may be claimed from
    // the masks, and the cockpit's own "has this radio ever reported it" answers instead.
    for (const c of RIG_CONTROLS) expect(capStateFor(c, undefined), c.id).toBe('unknown')
  })

  it('the step lists are three-state too: chips, no pad fitted, or nothing known', () => {
    // ⚠️ WORDING CORRECTED 2026-09-22 against the shipped backend. This used to read "an
    // EMPTY list means the rig has the stage but offers no steps, which is a dB stepper",
    // and three sources say otherwise: `Engine::observe_rig_db_steps` ("Empty is the
    // different, positive answer: no pad fitted"), the same sentence on
    // `RadioStatus.attStepsDb`, and the CI-V broker, which refuses both the read and the
    // write of PREAMP when the model's list is empty — an IC-905 has no preamp at all.
    // `stepsFor` is unchanged: it is the ACCESSOR, and keeping `[]` and `undefined` apart is
    // exactly what lets `capStateFor` below read one as ABSENT and the other as UNKNOWN.
    expect(stepsFor(byId('ATT'), { attDb: [6, 12] })).toEqual([6, 12])
    expect(stepsFor(byId('ATT'), { attDb: [] }), 'an empty step list was read as unknown').toEqual([])
    expect(stepsFor(byId('ATT'), {}), 'a missing step list was read as empty').toBeUndefined()
    expect(stepsFor(byId('PRE'), { preampDb: [10] })).toEqual([10])
    expect(stepsFor(byId('RF'), { attDb: [6] }), 'a step list leaked to a control with no steps').toBeUndefined()
  })

  it('⛔ ATT/PRE ARE JUDGED BY THEIR STEP LIST, and the three states stay three', () => {
    // The question `capStateFor` answers is "may the operator drive it", and for a pad that
    // is the list of values the radio will accept — not a level mask, which only says the
    // token exists. Each of the three is asserted BY VALUE, so a collapse in either
    // direction fails here rather than at a rendered row.
    expect(capStateFor(byId('ATT'), { attDb: [6, 12, 18] }), 'a published pad list was not PRESENT').toBe('present')
    expect(capStateFor(byId('PRE'), { preampDb: [1, 2] })).toBe('present')
    // EMPTY is the positive "no pad fitted" — the IC-905's preamp, the rig with no ATT.
    expect(capStateFor(byId('ATT'), { attDb: [] }), 'an empty list was not read as no pad fitted').toBe('absent')
    expect(capStateFor(byId('PRE'), { preampDb: [] })).toBe('absent')
    // …and NOTHING PUBLISHED is UNKNOWN. This is the collapse the whole model exists to
    // stop: a rig whose `\dump_state` Nexus could not read may well have a 20 dB pad.
    expect(capStateFor(byId('ATT'), {}), 'unknown collapsed into absent').toBe('unknown')
    expect(capStateFor(byId('PRE'), { attDb: [6] }), 'ATT’s list answered for PRE').toBe('unknown')
    // CONTROL — a level mask does NOT settle a stepped control, in either direction. Without
    // this the assertions above would pass against a capStateFor that still read the masks.
    expect(capStateFor(byId('ATT'), { levelSet: ['ATT'] }), 'a level mask stood in for the pad list').toBe('unknown')
  })

  it('no list published: the row STAYS and says which unknown it is — never "not on this radio"', () => {
    // ⛔ THE ONE THAT COST A NEW CAUSE. `set_att_db` rejects any dB not on the published
    // list, so with no list there is no value to command — but that is a fact about what
    // Nexus could READ, and printing "Not on this radio: ATT" over a rig that has a pad is
    // the confident wrong answer the three-state model is built against.
    const s = state({ caps: {} })
    expect(causeFor(byId('ATT'), s)).toBe('noSteps')
    expect(rendersRow(byId('ATT'), s), 'the row vanished instead of saying why').toBe(true)
    expect(absentPlates('rx', s), 'a rig that published nothing was blamed for having nothing').not.toContain('ATT')

    // ⚠️ AND THE OBSERVED FALLBACK MUST NOT RESCUE IT. `reported` is true here — a rig can
    // report `attDb: 0` (pad out) all day and still have published no list — and the pre-
    // 2026-09-22 path would have called that available and drawn a live control with no
    // chips in it. This is the assertion that pins the difference.
    expect(causeFor(byId('ATT'), state({ caps: {}, reported: () => true }))).toBe('noSteps')

    // CONTROL — publish a list and it is live; publish an EMPTY one and it collapses to the
    // foot line as a genuine absence. Three different answers from one control.
    expect(causeFor(byId('ATT'), state({ caps: { attDb: [20] } })), 'a published pad was not offered').toBeNull()
    const none = state({ caps: { attDb: [], preampDb: [] } })
    expect(causeFor(byId('ATT'), none)).toBe('absent')
    expect(absentPlates('rx', none), 'a rig that says it has no pad should say so at the foot').toContain('ATT')
  })

  it('a dead CAT link still beats the step lists — one explanation, not three', () => {
    // Ordering, and it matters more now than it did: on a breaker trip the engine CLEARS the
    // step lists (`clear_rig_db_steps`), so without `noCat` winning first every pad row
    // would swap its banner for "could not read the steps" at the moment the link died.
    expect(causeFor(byId('ATT'), state({ catOk: false, caps: {} }))).toBe('noCat')
    expect(causeFor(byId('PRE'), state({ catOk: false, caps: { preampDb: [] } }))).toBe('noCat')
  })

  it('the four undetectable causes never fire today', () => {
    // ⚠️ THE POSITIVE CONTROL FOR A NEGATIVE CLAIM. `nativeCiv`, `backend`, `backendRawOnly`
    // and `silent` need a transport, a rig model and a last-asked age, none of which is on
    // RadioStatus. They are declared so the shape is settled, and this is what stops one
    // becoming reachable with no data and no wording behind it: the day a resolver starts
    // returning one, this goes red and names it.
    const unreachable: UnavailableCause[] = ['nativeCiv', 'backend', 'backendRawOnly', 'silent']
    const seen = new Set<UnavailableCause>()
    for (const c of RIG_CONTROLS)
      for (const catOk of [true, false])
        for (const mode of ['USB', 'FM', 'AM'])
          for (const reported of [() => true, () => false])
            for (const caps of [undefined, {}, { funcSet: [], levelSet: [] }, { funcSet: [c.token.hamlib], levelSet: [c.token.hamlib] }]) {
              const cause = causeFor(c, { catOk, mode, reported, caps })
              if (cause) seen.add(cause)
            }
    expect([...seen].filter((c) => unreachable.includes(c))).toEqual([])
    // …and the sweep really did exercise the reachable ones, or it proves nothing.
    //
    // ⚠️ `noSteps` JOINED THIS LIST 2026-09-22, deliberately and with a wording behind it
    // (`phone.unavail.noSteps`). None of the `caps` shapes swept above carries a step list,
    // so every pass over ATT/PRE lands on it — which is what this line is for: a cause
    // becoming reachable turns this red and names it, and the author then has to say
    // whether that was the intent. It was.
    expect([...seen].sort()).toEqual(['absent', 'noCat', 'noSteps', 'notOnMode'])
  })
})

describe('which controls get a row, and which collapse to the foot line', () => {
  it('a control Nexus has built no path for gets no row and NO mention', () => {
    // Exception 1. A row greyed on every radio in the fleet reads as "broken", and naming it
    // in a line that says "not on this radio" would blame the radio for Nexus.
    //
    // ⚠️ THE FIXTURE IS SYNTHETIC NOW, AND THAT IS THE HONEST SHAPE. This used to read the
    // real table for a `built: false` row and guarded itself with "the fixture has no
    // unbuilt control, so this proves nothing" — which fired when ATT/PRE/MON were built on
    // 2026-09-22 and left the table wholly built. The rule still governs the NEXT slot
    // declared ahead of its path, so the behaviour is tested against a control that has that
    // shape rather than deleted for want of a live example. That the real table currently
    // holds none is asserted separately, above, so the two facts cannot be confused.
    const unbuilt: RigControl = {
      id: 'SHIFT', plate: 'SHIFT', token: { hamlib: 'IF' }, kind: 'hz', chain: 'rx', built: false,
    }
    expect(causeFor(unbuilt, state()), 'an unbuilt control was not absent').toBe('absent')
    expect(rendersRow(unbuilt, state()), 'a control with no path behind it drew a row').toBe(false)
    // …and the foot line is computed off the REAL table, which must name neither an unbuilt
    // control nor, on a rig that reports everything, anything at all.
    expect(absentPlates('rx', state()), 'an unbuilt control was blamed on the radio').not.toContain('SHIFT')
    // CONTROL — the same control WITH a path built is judged on its merits instead, so the
    // assertions above are about `built` and not about the id being unknown to the table.
    expect(causeFor({ ...unbuilt, built: true }, state()), 'built made no difference').toBeNull()
  })

  it('a control this radio lacks gets no row, and its PLATE goes to the foot line', () => {
    // Exception 3, and both halves matter: the row goes (so an IC-7300 does not open to four
    // grey rows) and the plate appears (so nothing vanishes silently).
    const s = state({ reported: (c: RigControl) => !['NOTCHF', 'AGC'].includes(c.id) })
    expect(rendersRow(byId('NOTCHF'), s)).toBe(false)
    expect(absentPlates('rx', s)).toEqual(['NOTCH', 'AGC'])
    expect(chainControls('rx', s).map((c) => c.id)).not.toContain('NOTCHF')
    // CONTROL: the ones it does report still draw.
    expect(chainControls('rx', s).map((c) => c.id)).toContain('RF')
  })

  it('a dead CAT link keeps every row and empties the foot line', () => {
    // The banner explains the lot. Collapsing them into "not on this radio" instead would
    // tell the operator his radio lacks thirteen features when it lacks none of them.
    const s = state({ catOk: false, reported: () => false })
    expect(absentPlates('rx', s), 'a dead link was read as a radio with no features').toEqual([])
    expect(chainControls('rx', s).map((c) => c.id)).toContain('NOTCHF')
  })

  it('BW keeps its row on FM rather than collapsing', () => {
    const s = state({ mode: 'FM' })
    expect(rendersRow(byId('BW'), s)).toBe(true)
    expect(absentPlates('rx', s)).toEqual([])
  })
})

describe('the a11y primitive', () => {
  it('a dead BUTTON stays focusable, so the reason is reachable', () => {
    // A `disabled` button is out of the tab order, so a screen-reader operator never lands
    // on it and never hears the reason — which is the whole point of saying it.
    const p = deadControlProps('button', 'why-1')
    expect(p['aria-disabled']).toBe(true)
    expect(p.disabled, 'the button was taken out of the tab order').toBeUndefined()
    expect(p['aria-describedby']).toBe('why-1')
  })

  it('a dead INPUT is really disabled, because aria-disabled would leave it draggable', () => {
    const p = deadControlProps('input', 'why-2')
    expect(p.disabled).toBe(true)
    expect(p['aria-disabled'], 'a range input cannot be aria-only dead — it still drags').toBeUndefined()
  })

  it('guard swallows the handler on a dead control and passes it through on a live one', () => {
    // aria-disabled does not prevent activation, so the guard is the other half of it —
    // without this the focusable dead button would still command the radio.
    let fired = 0
    guard(true, () => { fired += 1 })()
    expect(fired, 'an aria-disabled control still fired').toBe(0)
    guard(false, () => { fired += 1 })()
    expect(fired, 'a live control was swallowed').toBe(1)
  })
})

// ── THE RECEIVER AXIS (dual-receiver programme) ──────────────────────────────────────────────
//
// The registry grew a second axis beside `chain`: WHICH RECEIVER a row is drawn for. Main's
// answers are `causeFor`'s, untouched. The Sub's come from three facts the snapshot carries —
// whether a Sub is offered at all, which stages the Sub may be credited with (D7), and whether
// the CAT path serving the radio can name the Sub — and one this table owns: which controls
// Nexus has BUILT a Sub path for.

/** A receiver in the snapshot's shape. The stages are required arguments — NO defaults — so a
 *  case can never quietly inherit "own" for the stage it is about. */
const rx = (id: 'main' | 'sub', frontEnd: StageOwner, dsp: StageOwner, audio: StageOwner): ReceiverStatus => ({
  id,
  stages: { frontEnd, dsp, audio },
})
const MAIN = rx('main', 'own', 'own', 'own')
/** The IC-7610's Sub by the vendor table: its own front end and AF, no documented DSP. */
const SUB_7610 = rx('sub', 'own', 'unknown', 'own')
/** The IC-9700's Sub: its own front end; neither DSP nor AF documented per receiver. */
const SUB_9700 = rx('sub', 'own', 'unknown', 'unknown')

const receivers = (
  sub: ReceiverStatus | null,
  subCommandable: boolean | null,
  subCapability: SubCapability = sub ? 'present' : 'unknown',
): ReceiversStatus => ({ main: MAIN, sub, subCapability, subCommandable })

const sub = (r: ReceiversStatus | null | undefined, catOk = true): SubState => ({ catOk, receivers: r })

describe('the receiver axis — which receiver a row is drawn for', () => {
  it('every RECEIVE row names its stage, and the stages are the Rust table’s, row for row', () => {
    // `dualrx::RxStage` documents the join: "The mapping onto the 13 chain: 'rx' rows of
    // ui/src/features/rigControls.ts". Read it out of the Rust source, so the two cannot drift.
    const src = readFileSync(resolve(process.cwd(), '../crates/tempo-app/src/dualrx.rs'), 'utf8')
    const rust: Record<string, string> = {}
    const word = { FrontEnd: 'frontEnd', Dsp: 'dsp', Audio: 'audio' } as const
    for (const m of src.matchAll(/^\/\/\/ \| \[`RxStage::(FrontEnd|Dsp|Audio)`\] \| (.+?) \|$/gm)) {
      for (const id of m[2].split(' · ')) rust[id.trim()] = word[m[1] as keyof typeof word]
    }
    // Parser sanity: a pattern that matched nothing would make the comparison below vacuous.
    expect(Object.keys(rust).length, 'the Rust stage table did not parse').toBe(13)
    const ours = Object.fromEntries(RIG_CONTROLS.filter((c) => c.chain === 'rx').map((c) => [c.id, c.stage]))
    expect(ours).toEqual(rust)
    // A transmit row belongs to no receiver: the radio has one transmitter.
    expect(RIG_CONTROLS.filter((c) => c.chain === 'tx' && c.stage !== undefined).map((c) => c.id)).toEqual([])
  })

  it('no Sub in the snapshot: no Sub control, on UNKNOWN, ABSENT and PRESENT-not-offered alike', () => {
    for (const cap of ['unknown', 'absent', 'present'] as const) {
      const s = sub(receivers(null, null, cap))
      expect(subChainControls(s), cap).toEqual([])
      expect(subUnconfirmedPlates(s), cap).toEqual([])
      for (const c of RIG_CONTROLS) expect(subCauseFor(c, s), `${cap} ${c.id}`).toBe('noSub')
    }
    // A station older than the field says nothing, and nothing is claimed for it either.
    expect(subChainControls(sub(undefined))).toEqual([])
    expect(subChainControls(sub(null))).toEqual([])
  })

  it('an offered Sub on a route that names it: the rows Nexus built for the Sub, in signal order', () => {
    const s = sub(receivers(SUB_7610, true))
    expect(subChainControls(s).map((c) => c.id)).toEqual(['RF', 'AF', 'SQL'])
    for (const id of ['RF', 'AF', 'SQL']) expect(subCauseFor(byId(id), s), id).toBeNull()
  })

  it('⛔ UNKNOWN NEVER OFFERS A SUB CONTROL — and never calls it absent', () => {
    // The IC-9700: AF and SQL sit in the audio stage, which no vendor statement credits to its
    // Sub. They are not offered — and they are named as NOT CONFIRMED, never "not on this
    // radio", which would be the forbidden collapse wearing per-receiver clothes.
    const s = sub(receivers(SUB_9700, true))
    expect(subChainControls(s).map((c) => c.id)).toEqual(['RF'])
    expect(subCauseFor(byId('AF'), s)).toBe('stageUnknown')
    expect(subCauseFor(byId('SQL'), s)).toBe('stageUnknown')
    expect(subUnconfirmedPlates(s)).toEqual(['AF', 'SQL'])
    // …and MAIN's pane-foot "not on this radio" line is untouched by any of it.
    const main: ControlState = { catOk: true, reported: () => true, mode: 'USB' }
    expect(absentPlates('rx', main)).toEqual([])
  })

  it('⭐ D7: the same control on two receivers of two radios of ONE class gets two answers', () => {
    // Both radios are independent dual receivers; only the per-receiver stage table separates
    // them, so an axis keyed on the radio (or on the class) cannot pass this.
    const af = byId('AF')
    expect(subCauseFor(af, sub(receivers(SUB_7610, true)))).toBeNull()
    expect(subCauseFor(af, sub(receivers(SUB_9700, true)))).toBe('stageUnknown')
    // And Main's answer for the same row does not move with either — `causeFor` never reads
    // the receivers.
    const main: ControlState = { catOk: true, reported: () => true, mode: 'USB' }
    expect(causeFor(af, main)).toBeNull()
  })

  it('a stage the Sub SHARES with Main is never offered as the Sub’s own', () => {
    // A shared front end (category 2): driving the Sub's RF gain would move Main's, because
    // there is only one. v1 offers no such radio, and the table still refuses it.
    const s = sub(receivers(rx('sub', 'sharedWithMain', 'unknown', 'own'), true))
    expect(subCauseFor(byId('RF'), s)).toBe('stageShared')
    expect(subChainControls(s).map((c) => c.id)).toEqual(['AF', 'SQL'])
    expect(subUnconfirmedPlates(s), 'shared is not unknown').toEqual([])
  })

  it('a control Nexus has not built for the Sub is omitted and never named, whatever the stage', () => {
    // An FTDX101's Sub owns its DSP by Yaesu's manual; Nexus has no Sub path for NB, NR or the
    // notches, so they get no row and no mention — blaming the radio for Nexus is the lie
    // `built: false` exists to prevent.
    const s = sub(receivers(rx('sub', 'own', 'own', 'own'), true))
    for (const id of ['BW', 'NB', 'NR', 'NRLVL', 'ANF', 'MN', 'NOTCHF', 'ATT', 'PRE', 'AGC']) {
      expect(subCauseFor(byId(id), s), id).toBe('notBuilt')
    }
    expect(subUnconfirmedPlates(sub(receivers(rx('sub', 'own', 'unknown', 'unknown'), true))),
      'an unbuilt DSP row is not named even when its stage is unknown').toEqual(['AF', 'SQL'])
  })

  it('the route: a path that cannot name the Sub, or one not yet heard from, offers nothing', () => {
    expect(subCauseFor(byId('AF'), sub(receivers(SUB_7610, false)))).toBe('noRoute')
    expect(subCauseFor(byId('AF'), sub(receivers(SUB_7610, null)))).toBe('routeUnknown')
    expect(subChainControls(sub(receivers(SUB_7610, false)))).toEqual([])
    expect(subChainControls(sub(receivers(SUB_7610, null)))).toEqual([])
  })

  it('no CAT keeps the offered rows, dead — the pane says why once', () => {
    const s = sub(receivers(SUB_7610, true), false)
    for (const id of ['RF', 'AF', 'SQL']) {
      expect(subCauseFor(byId(id), s), id).toBe('noCat')
      expect(subRendersRow(byId(id), s), id).toBe(true)
    }
    // …but no CAT does not revive a stage nobody documented.
    expect(subCauseFor(byId('AF'), sub(receivers(SUB_9700, true), false))).toBe('stageUnknown')
  })

  it('a transmit row is never a Sub control — the radio has one transmitter', () => {
    const s = sub(receivers(rx('sub', 'own', 'own', 'own'), true))
    for (const c of RIG_CONTROLS.filter((r) => r.chain === 'tx')) {
      expect(subCauseFor(c, s), c.id).toBe('notBuilt')
    }
  })
})
