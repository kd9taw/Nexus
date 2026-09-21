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
  type ControlState,
  type RigControl,
  type UnavailableCause,
} from './rigControls'

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
    // MON is in the table and NOT built — the slot is declared so the sibling's field drops
    // in, and `built: false` is what keeps it off the screen meanwhile.
    expect(RIG_CONTROLS.filter((c) => c.chain === 'tx').map((c) => c.id)).toEqual([
      'MIC', 'COMP', 'COMPLVL', 'VOX', 'MON',
    ])
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

  it('the caps model, when it lands, says absent WITHOUT waiting for a silent probe', () => {
    // The stub's whole point: `lacks` is the model table's answer and does not depend on
    // whether a read ever came back. A radio reporting the field but listed as lacking it is
    // a contradiction the model table wins — it is the sounder source.
    const s = state({ caps: { lacks: ['NOTCHF'] } })
    expect(causeFor(byId('NOTCHF'), s)).toBe('absent')
    expect(causeFor(byId('RF'), s), 'caps.lacks leaked to a control it does not name').toBeNull()
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
            for (const caps of [undefined, { lacks: [c.id] }, { lacks: [] }]) {
              const cause = causeFor(c, { catOk, mode, reported, caps })
              if (cause) seen.add(cause)
            }
    expect([...seen].filter((c) => unreachable.includes(c))).toEqual([])
    // …and the sweep really did exercise the reachable ones, or it proves nothing.
    expect([...seen].sort()).toEqual(['absent', 'noCat', 'notOnMode'])
  })
})

describe('which controls get a row, and which collapse to the foot line', () => {
  it('a control Nexus has built no path for gets no row and NO mention', () => {
    // Exception 1. A row greyed on every radio in the fleet reads as "broken", and naming it
    // in a line that says "not on this radio" would blame the radio for Nexus.
    const unbuilt = RIG_CONTROLS.filter((c) => !c.built).map((c) => c.id)
    expect(unbuilt, 'the fixture has no unbuilt control, so this proves nothing').not.toEqual([])
    for (const id of unbuilt) {
      expect(rendersRow(byId(id), state()), `${id} drew a row with no path behind it`).toBe(false)
    }
    expect(absentPlates('rx', state()), 'an unbuilt control was blamed on the radio').toEqual([])
    expect(absentPlates('tx', state())).toEqual([])
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
