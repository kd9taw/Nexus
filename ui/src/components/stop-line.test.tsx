// @vitest-environment jsdom
//
// THE STOP LINE, COMPUTED — the wiring half of the rule in features/panelState.ts.
//
//   THE OPERATOR MUST NEVER BE UNABLE TO STOP A TRANSMISSION.
//
// Mechanically: in every cockpit, at least one control that stops a transmission renders
// OUTSIDE every ⊞-removable pane. This file is what computes that half of it.
//
// panelState.test.ts holds the NAME half: no vocabulary may contain an id named for a stop
// control. That half reads names and nothing else, so it walks straight past a stop control
// gated on an id called `dsp` — which is exactly the hole the old rule's "enforced by
// computation" claim papered over.
//
// THIS suite reads wiring and ignores names. For each cockpit it drives EVERY id in the
// REAL vocabulary through the REAL hide path — one at a time, then all at once, the state
// an operator reaches by unticking down the menu — and looks for the stop controls by their
// ACCESSIBLE NAME. If any panel id gates any of them, in any combination, this goes red and
// says which id took which control.
//
// WHAT BELONGS IN A CASE'S `stopControls`, AND WHAT MUST NOT. The list is the
// OUTSIDE-EVERY-PANE set: the controls the guarantee rests on. A stop control that lives
// INSIDE a ⊞-removable pane is deliberately absent, and there are two of them in the app —
// Phone's voice keyer hosts ■ Stop (→ stopVoice → Engine::stop_voice, which flushes the
// output ring and unkeys), and RTTY's `stream` pane hosts the "Auto on" toggle (off-click →
// seq.abort() + Engine::rtty_stop(): queue cleared, rig unkeyed). Both go away with their
// pane, and that is allowed: a pane's own stop is a CONVENIENCE built on the guarantee, never
// what holds it up. Adding either one below would make this sweep demand its pane be
// unhideable — which is exactly how the FIRST wording of the rule excluded the voice keyer,
// the pane it was written to admit. Do not "fix" a red by unhiding a pane; check first
// whether the control you added belongs on the list at all.
//
// EACH COCKPIT IS RENDERED WITH THE PROPS APP GIVES IT, and that is load-bearing rather than
// tidiness: the TX strip (CockpitTxStrip) draws the TX-enable latch as a button (TX On / TX Off)
// only when it is handed `onSetTxEnabled`, which App passes to RTTY, PSK, SSTV and JS8 — in JS8
// the latch is
// NOT a stop (slotted mode), so the JS8 case passes the prop for parity with App and lists
// Stop TX + Tune only. RTTY and SSTV have no other Enable-Tx affordance, the TopBar's being
// hidden with the digital chrome. The first version of this file omitted the prop, so for
// RTTY and SSTV the latch was never in the document and the sweep proved nothing about the
// one control the rule names BY NAME (gating it on a panel id in either cockpit was green).
// Phone and CW arm elsewhere and legitimately have no latch on screen; their `stopControls`
// say so by not listing one. The same holds for their two feeds (`spots`, `needed`): each
// renders only with the board App lends it, so both cases lend one — without it the two ids
// were in the sweep and their panes never on screen, and hiding them proved nothing.
//
// WHAT THIS FILE DOES NOT CARE ABOUT: whether a pane can START a transmission. Six can —
// Operate's Tx messages, its two decode panes and its two rosters, Phone's voice keyer — and
// all of them are hideable, correctly. The rule is about what is left ON SCREEN and nothing
// else, so the only lists here are `stopControls`.
//
// WHY THE HEADER IS NOT STUBBED HERE. Every *.structure.test.tsx mocks CockpitHeader down
// to an empty <header>, which is right for a shell census and useless for this: Stop TX and
// Tune live INSIDE that header in Phone, CW and RTTY, so a stubbed header can only prove a
// container rendered. This file pays the mock cost to render the real one, so "Stop TX is
// reachable" is an assertion about the button the operator presses.
//
// OPERATE (FT) IS SWEPT HERE TOO since 2026-10-07, when ⊞ Arrange came to it: one case per layout
// (Classic, Roster), its stop controls in the merged QSO strip rather than a CockpitHeader. Until then
// its only sweep was OperateCockpit.structure.test.tsx's presence-only one ("every protected control
// renders INSIDE .cockpit-qso with every panel id removed"), which took no baseline, compared no
// `disabled` state and hid every id at once rather than one at a time. That file still holds the
// wider dock law — the strip's whole TX and sequencer surface inside the strip — now singly and all
// at once, with a baseline, in both layouts and with the rail on either side.
//
// EACH LIST BELOW IS A SUBSET OF ITS COCKPIT'S CENSUS, NOT A COPY OF IT. panelState.ts once
// claimed "each cockpit's sweep list is the same set, which is how this is checkable in
// minutes"; that was false for four of the five swept cockpits. Two kinds of holder cannot be
// swept here, by construction rather than by oversight — this file finds BUTTONS BY ACCESSIBLE
// NAME, in one fixed fixture state:
//   · KEYBOARD-ONLY. Phone's Space bar (window keyup → setPtt(false), only while Lock is off)
//     and Esc have no accessible name and no element. Esc is a stop on every operating screen
//     and on Satellites (CW's: window keydown → the same abort() Stop TX calls; App binds
//     Tempo's, Phone's, SSTV's, APRS's and Satellites'); each is NAMED in the last block of this
//     file and pressed in stop-control-wiring.test.tsx.
//   · CONDITIONALLY RENDERED. RTTY's auto-sequencer Abort renders only inside
//     `{auto && seqState !== 'idle'}`, and the `rttyState` fixture (stop-line.api.testkit.ts) is auto:false /
//     seqState:'idle' — so there is nothing on screen to look for. Adding a second RTTY case
//     with an in-flight sequence would sweep it; not done here, and not claimed.
// SSTV is the one cockpit whose list and census match exactly, Esc (keyboard-only) apart.
//
// WHAT THIS DOES NOT COMPUTE, stated rather than guarded:
//   · A STOP CONTROL THAT IS PRESENT, ENABLED AND INERT. Every assertion here is about the
//     button being in the document and operable — never about what pressing it does.
//     Verified: `onClick={() => {}}` on CockpitHeader's Stop TX passes this file and the
//     whole suite. Nothing below would go red.
//   · The keyboard-only and conditionally rendered holders above.
//   · That a NEWLY ADDED stop control was added to the list below. Adding one is a human
//     step. Each cockpit's list is its case's `stopControls`, so the next person editing a
//     dock finds it beside the cockpit it guards.
// What IS computed is that every vocabulary in the app has a sweep at all — the last test
// in the file, driven off ALL_PANEL_VOCABULARIES, so a sixth cockpit cannot ship without one.
//
// FIELD DAY HAS NO SWEEP OF ITS OWN, AND THAT IS NOT AN OMISSION. Field Day is a MODE the app
// enters, not a screen: the contacts are made in the five cockpits swept below, whose stop
// lines are the same on the event weekend as off it. The Field Day section itself draws no
// transmit control at all — it is setup, score, sections, bonuses, the log and the club board.
//
// THIS FILE IS THE FIRST OF SEVERAL (2026-10-07). The arrangement sweeps grew long enough that this one
// file was the slowest in the UI suite by far, so they are split by cockpit and each runs on a worker of
// its own: stop-line.<view>.test.tsx sweeps that cockpit's arrangements (and Phone's file its left side;
// FT, arranged per layout, has stop-line.operate.<layout>.test.tsx).
// The cases, the fixtures and the run they share are in stop-line.testkit.tsx and stop-line.api.testkit.ts.
// What stays here is the vocabulary sweep over every case, the checks that each sweep is reading something,
// the coverage checks — every vocabulary swept, and every arranging one in the file named for its cockpit —
// the RTTY editor's stops and the Esc census.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import * as api from '../api'
import { sectionFeatures } from '../features/registry'
import { RttyCockpit } from './RttyCockpit'
import { ALL_PANEL_VOCABULARIES, BOX_IDS } from '../features/panelState'
import {
  CASES,
  CASE_BY_NAME,
  cw,
  cwDual,
  js8,
  operateClassic,
  operateRoster,
  panelsWith,
  phone,
  phoneDual,
  psk,
  rtty,
  settle,
  snap,
  sstv,
  stopsOnScreen,
  type Case,
} from './stop-line.testkit'
import { rttyState } from './stop-line.api.testkit'
import type { RttyState } from '../types'

// One api mock for every cockpit the sweeps render — the union of what they call on mount, derived from
// the real module (stop-line.api.testkit.ts says why).
vi.mock('../api', async (importOriginal) =>
  (await import('./stop-line.api.testkit')).stopLineApi(await importOriginal<Record<string, unknown>>()),
)

vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas/scope children only. CockpitHeader is DELIBERATELY REAL — see the file header.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
vi.mock('./Waterfall', () => ({ Waterfall: () => <div className="waterfall-wrap" /> }))
// A BOX's body is a Conditions box, and none of those holds a transmit control (DashRail.test.tsx
// places every box in the registry and finds none). Stubbed, so each mount measures the cockpit
// rather than twenty-nine feeds; the boxes' frames, pickers and ✕ are real, and so is where they stand.
vi.mock('./panes/BoxBody', () => ({ BoxBody: () => <div data-testid="box-body-stub" /> }))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

describe('the stop line, computed against the real cockpits', () => {
  it.each(CASES.map((c) => [c.cockpit, c] as const))(
    '%s: no ⊞ panel id gates any control that stops a transmission',
    async (_name, c) => {
      // "Still there" is not enough — a control gated `disabled={!shown('x')}` is mounted
      // and useless — so record what each stop control looks like with NOTHING hidden and
      // require the hides to leave it alone. (Several of these are legitimately disabled
      // when idle: RTTY's and SSTV's Stop are dead until `sending`. That is a property of
      // the transmitter, not of the ⊞ menu, which is exactly why the baseline is the
      // comparison rather than `disabled === false`.)
      // Explicit <string>: an empty literal would infer `never` and fail the case's own
      // PanelLayoutApi<P>.
      c.render(panelsWith<string>([]))
      await settle()
      const shown = stopsOnScreen(c.stopControls)
      const baseline = new Map(
        c.stopControls.map(([label]) => {
          const els = shown.get(label)!
          expect(
            els.length,
            `${c.cockpit}: "${label}" is not on screen with every panel SHOWN — the sweep ` +
              'below would then be asserting nothing at all',
          ).toBeGreaterThan(0)
          return [label, els.some((e) => !e.disabled)]
        }),
      )
      cleanup()

      // One id at a time, then the whole vocabulary at once. Both matter: a control might
      // survive every single hide and still vanish when two panes go, because the cockpit
      // collapses a column that happens to host it.
      const combos: Array<readonly string[]> = [...c.ids.map((id: string) => [id]), [...c.ids]]
      for (const removed of combos) {
        c.render(panelsWith(removed))
        await settle()
        const on = stopsOnScreen(c.stopControls)
        for (const [label] of c.stopControls) {
          const els = on.get(label)!
          expect(
            els.length,
            `${c.cockpit}: hiding {${removed.join(', ')}} took "${label}" with it — the ` +
              'operator can hide a way to stop a transmission',
          ).toBeGreaterThan(0)
          expect(
            els.some((e) => !e.disabled),
            `${c.cockpit}: hiding {${removed.join(', ')}} left "${label}" on screen but ` +
              'DISABLED — mounted and unusable is the same loss as gone',
          ).toBe(baseline.get(label))
        }
        cleanup()
      }
    },
    // A budget for real work (2026-10-02): eleven fresh mounts for Phone, each followed by one
    // accessible-name pass over every button, and Phone, the first case, also pays the file's
    // first render. It took 0.55 s alone, 2.6 s in the full suite, 3.0 s at a fifth of a CPU
    // and 5.9 s at a tenth, past the 5 s default. FT's cases (2026-10-07) are the heaviest:
    // seventeen mounts of the whole FT screen with its boxes and the callsign card, 1.3 s each
    // alone and 11.8 s (Classic) and 9.2 s (Roster) at a tenth of a CPU, so the budget is 30 s.
    30_000,
  )

  it('the two Sub-receiver cases really draw a SUB row — else they would sweep a copy of their twin', async () => {
    for (const c of [phoneDual, cwDual] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      expect(document.querySelector('[data-receiver="sub"]'), `${c.cockpit}: no SUB row`).not.toBeNull()
      cleanup()
    }
  })

  it('Phone’s and CW’s two feeds are on screen with nothing hidden — else hiding them would sweep nothing', async () => {
    for (const c of [phone, cw, phoneDual, cwDual] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      for (const id of ['spots', 'needed']) {
        expect(c.ids, `${c.cockpit}: "${id}" left the vocabulary`).toContain(id)
        expect(document.querySelector(`[data-pane="${id}"]`), `${c.cockpit}: the ${id} pane is not on screen`).not.toBeNull()
      }
      cleanup()
    }
  })

  it('the six boxes are on screen with nothing hidden in FT, Phone, CW and JS8 — else hiding them would sweep nothing', async () => {
    // Every box SHIPS HIDDEN (defaultRemoved); with nothing hidden each sweep above starts with all six
    // on screen, each showing an entry of the shared list, so every hide of one is a real one.
    for (const c of [phone, cw, js8, phoneDual, cwDual, operateClassic, operateRoster] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      for (const b of BOX_IDS) {
        expect(c.ids, `${c.cockpit}: "${b}" left the vocabulary`).toContain(b)
        expect(document.querySelector(`.pane-frame[data-pane="${b}"]`), `${c.cockpit}: ${b} is not on screen`).not.toBeNull()
      }
      cleanup()
      c.render(panelsWith<string>([...BOX_IDS]))
      await settle()
      expect(document.querySelector('.pane-frame[data-pane^="box"]'), `${c.cockpit}: hidden, a box is still on screen`).toBeNull()
      cleanup()
    }
    // Two mounts of each of seven cockpits (FT's two layouts among them): 4.0 s at a tenth of a CPU,
    // close to the 5 s default, so a budget of its own.
  }, 20_000)

  it('the RF scope pane is on screen with nothing hidden in FT, RTTY, PSK, JS8 and SSTV — else hiding it would sweep nothing', async () => {
    // It SHIPS HIDDEN (defaultRemoved), and every sweep above starts from "nothing hidden", so it is
    // on screen at their baseline and every hide of it — singly and with everything else — is a real
    // one. FT's stands beside its waterfall (OperateCockpit.structure.test.tsx holds where).
    for (const c of [rtty, psk, js8, sstv, operateClassic, operateRoster] as Array<Case<any>>) {
      c.render(panelsWith<string>([]))
      await settle()
      expect(c.ids, `${c.cockpit}: "rfScope" left the vocabulary`).toContain('rfScope')
      expect(document.querySelector('[data-pane="rfScope"]'), `${c.cockpit}: the RF scope pane is not on screen`).not.toBeNull()
      cleanup()
      c.render(panelsWith<string>(['rfScope']))
      await settle()
      expect(document.querySelector('[data-pane="rfScope"]'), `${c.cockpit}: hidden, the RF scope pane is still on screen`).toBeNull()
      cleanup()
    }
  })

  it('EVERY vocabulary in the app is swept — here, or in a file named here', () => {
    // A sweep is worth only what it covers, and the failure this whole batch came from was
    // a guard that looked exhaustive and silently skipped a cockpit. So the coverage is
    // computed against ALL_PANEL_VOCABULARIES rather than asserted about this file's own
    // list: add a sixth cockpit and this goes red naming it, whatever anybody remembers.
    //
    // ELSEWHERE is the honest part — a vocabulary swept in another file has to be declared here
    // to count. (`operate` was one, swept presence-only, until it gained its cases above.)
    const ELSEWHERE: Record<string, string> = {
      // Connect has a ⊞ vocabulary (its seven slots) and NO transmit control of any kind, so
      // there is no stop to lose — what is swept is the property that remains meaningful:
      // closing a pane, singly and all at once, leaves every control outside the panes on
      // screen (the map toolbar, the header).
      // ⚠️ CONNECT IS THE STOP LINE'S ONE RULED EXCEPTION. App draws no top bar there, so there is
      // no Stop TX on Connect at all — the operator, 2026-10-01: "remove all radio control from
      // connect, reclaim that space". Transmit on Connect is stopped by Esc or by leaving the
      // screen; stop-control-wiring.test.tsx holds Esc to halt_tx on Connect and every OTHER screen
      // to the bar. Nothing here is loosened for any other cockpit or screen by it.
      connect:
        'ConnectView.panes.test.tsx — "hiding every pane leaves every control outside the ' +
        'panes on screen" (Connect renders no transmit control; PRESENCE-ONLY, by name); and ' +
        'the ruled exception ("remove all radio control from connect, reclaim that space"): no top ' +
        'bar on Connect, Esc sends halt_tx there — stop-control-wiring.test.tsx',
      // The dashboard rail's four slots. The rail is a sibling of the cockpit in App's shell and
      // renders no transmit control; what is swept is that it costs no cockpit a stop control:
      // with the rail on, every control on each cockpit's list below is on screen, no more disabled
      // than with it off, and not inside the rail — in the real App, every operating cockpit.
      dashrail:
        'DashRail.stopLine.test.tsx — every operating cockpit\'s stop-line list, rail off vs on ' +
        '(PRESENCE + DISABLED, by name, not layout); DashRail.test.tsx — no transmit control in the ' +
        'rail, every box in the registry placed in it',
    }
    const here = new Set(CASES.map((c) => c.view))
    for (const vocab of ALL_PANEL_VOCABULARIES) {
      expect(
        here.has(vocab.view) || vocab.view in ELSEWHERE,
        `the "${vocab.view}" cockpit has a ⊞ vocabulary and no rendered stop-line sweep — ` +
          'add a case above, or sweep it in its own structure test and name that file in ' +
          'ELSEWHERE here',
      ).toBe(true)
    }
    // …and the cases above really do drive the real vocabularies, not copies of them.
    for (const c of CASES) {
      const vocab = ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)
      expect(vocab, `no vocabulary named "${c.view}"`).toBeDefined()
      expect([...c.ids], `${c.cockpit} sweeps a stale id list`).toEqual([...vocab!.panelIds])
    }
  })
})

// ── THE ARRANGEMENT SWEEPS, ONE FILE PER COCKPIT ────────────────────────────────────────────────
// The sweep itself (fifty seeded placements of a cockpit's panes, each with every id hidden singly and all
// at once) is stop-line.testkit.tsx's `arrangementRun`, run from stop-line.<view>.test.tsx for each
// cockpit whose vocabulary ARRANGES. It used to run here over every such case, found off the vocabularies,
// so a cockpit that gained Arrange was swept by its being exported; with the sweeps in files of their own
// that is checked instead: an arranging vocabulary with no file, or a file that sweeps another cockpit,
// is red here.
describe('THE ARRANGEMENT SWEEP: no placement of the panes gates a control that stops a transmission', () => {
  // One case per arranging vocabulary (the first; the Sub-receiver twins change no placement, and
  // each pass is 550 renders).
  // A cockpit arranged per layout (FT) has a case per layout, and each is swept.
  const arranges = (c: Case<string>) => c.arrange ?? ALL_PANEL_VOCABULARIES.find((v) => v.view === c.view)?.arrange
  const sweepKey = (c: Case<string>) => `${c.view}/${c.layout ?? ''}`
  const ARRANGING = (CASES as Array<Case<string>>).filter(
    (c, i) => arranges(c) && (CASES as Array<Case<string>>).findIndex((d) => sweepKey(d) === sweepKey(c)) === i,
  )

  it('some cockpit arranges, so this sweep is reading something', () => {
    expect(ARRANGING.map((c) => c.view)).toContain('phone')
  })

  it('every cockpit that arranges is swept over its arrangements, in the file named for it', () => {
    // Every vocabulary that arranges has a case here — one per layout where it arranges per layout —
    // so none can be missed by the file check below.
    for (const vocab of ALL_PANEL_VOCABULARIES.filter((v) => v.arrange || v.arrangeBy)) {
      const layouts = vocab.arrangeBy ? Object.keys(vocab.arrangeBy) : ['']
      for (const layout of layouts) {
        expect(ARRANGING.map(sweepKey), `the "${vocab.view}" cockpit arranges${layout ? ` its ${layout} layout` : ''} and has no stop-line case`).toContain(`${vocab.view}/${layout}`)
      }
    }
    for (const c of ARRANGING as Array<Case<string>>) {
      // A cockpit arranged per layout has a file per layout, each on a worker of its own.
      const file = `stop-line.${c.view}${c.layout ? `.${c.layout}` : ''}.test.tsx`
      const source = readFileSync(resolve(__dirname, file), 'utf8')
      const swept = [...source.matchAll(/arrangementRuns\((\w+)\)/g)].map((m) => CASE_BY_NAME[m[1]]).filter((d) => d != null)
      expect(swept.map(sweepKey), `${file} does not run the arrangement sweep for ${c.cockpit}`).toContain(sweepKey(c))
    }
    // Phone's LEFT SIDE (2026-10-03): its own sweep, in Phone's file. Any other cockpit given a left side
    // needs one too, and is red here until it has it.
    for (const vocab of ALL_PANEL_VOCABULARIES.filter((v) => v.arrange?.leftSide)) {
      const source = readFileSync(resolve(__dirname, `stop-line.${vocab.view}.test.tsx`), 'utf8')
      expect(source, `stop-line.${vocab.view}.test.tsx does not sweep the ${vocab.view} cockpit's left side`).toContain("describe('THE LEFT SIDE SWEEP")
    }
  })
})

describe('RTTY: the macro editor never stands between the operator and a stop', () => {
  // The F-key editor is the one RTTY surface that takes the caret out of the dock and puts a form
  // in front of the operator, and Esc is the key both "cancel" and "stop" live on. So, with it
  // open: Stop TX and the dock's Esc/Stop are on screen and live while an over is on the air, and
  // Esc stops exactly as it does without the editor. With nothing on the air, Esc closes the
  // editor ONLY — a stop there is haltTx, which turns TX off over a key meant as "cancel".
  //
  // Rendered with the REAL header, as every case above is, because Stop TX lives in it.
  const rttyStop = () => vi.mocked(api.rttyStop)
  const haltTx = () => vi.mocked(api.haltTx)

  /** The station this fixture stands in for, answering EVERY state question the same way. The
   *  cockpit asks twice at mount — its poll's leading read (`getRttyState`) and the decoder's
   *  auto-arm (`rttyAutoArm`) — and both answers carry the whole RTTY state, so whichever lands
   *  last is what renders. Only the poll used to be set: the auto-arm kept answering the idle
   *  file fixture, landed second and wiped `sending`/`latched`, and the check passed only because
   *  the poll's NEXT tick restored them — 500 ms later, on a wall clock, inside `waitFor`'s 1 s
   *  window. At load ~32 that tick came late and the test went red with nothing wrong. */
  const answering = (state: RttyState) => {
    vi.mocked(api.getRttyState).mockImplementation(async () => state)
    vi.mocked(api.rttyAutoArm).mockImplementation(async () => state)
  }
  const asked = () => ({ reads: vi.mocked(api.getRttyState).mock.results.length, arms: vi.mocked(api.rttyAutoArm).mock.results.length })

  /** Await every state answer the cockpit has asked for since `from`, inside act so what they
   *  deliver is committed — the event itself, never a clock, so a loaded box can make this
   *  slower but never red. The caller then asserts `ready` synchronously: a pill or a control
   *  that never comes still fails, and at once. */
  async function answered(from: { reads: number; arms: number }) {
    const reads = vi.mocked(api.getRttyState).mock
    const arms = vi.mocked(api.rttyAutoArm).mock
    expect(reads.results.length, 'the cockpit never read its RTTY state').toBeGreaterThan(from.reads)
    for (let r = from.reads, a = from.arms; r < reads.results.length || a < arms.results.length; ) {
      const pending = [...reads.results.slice(r), ...arms.results.slice(a)].map((x) => x.value)
      r = reads.results.length
      a = arms.results.length
      await act(async () => {
        await Promise.all(pending)
      })
    }
  }

  /** Mount RTTY in `state`, wait until the cockpit has it, and open the F1 editor. */
  async function openEditor(state: RttyState, ready: () => void) {
    answering(state)
    const from = asked()
    render(<RttyCockpit snap={snap} active onSetTxEnabled={() => {}} />)
    await settle()
    await answered(from)
    ready()
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Edit the F1 macro' }))
    })
    expect(document.querySelector('.rtty-macro-editor'), 'fixture: the editor is open').not.toBeNull()
    rttyStop().mockClear()
    haltTx().mockClear()
  }
  const onAir = () => expect(screen.getByText('TX ▲')).toBeTruthy()
  const latchUp = () =>
    expect(document.querySelector('.rtty-tx-latch')?.getAttribute('aria-pressed')).toBe('true')

  afterEach(() => answering(rttyState))

  it('Stop TX and the Esc/Stop macro are on screen and enabled while an over is on the air', async () => {
    await openEditor({ ...rttyState, sending: true } as RttyState, onAir)
    const stopTx = screen.getByRole('button', { name: /^stop tx$/i }) as HTMLButtonElement
    const escStop = screen.getByRole('button', { name: /^esc\s*stop$/i }) as HTMLButtonElement
    expect(stopTx.disabled).toBe(false)
    expect(escStop.disabled).toBe(false)
    // …and they still do what they are for, with the editor open.
    fireEvent.click(stopTx)
    expect(haltTx()).toHaveBeenCalled()
    fireEvent.click(escStop)
    expect(rttyStop()).toHaveBeenCalled()
  })

  it('Esc while an over is on the air STOPS — the editor does not swallow it', async () => {
    await openEditor({ ...rttyState, sending: true } as RttyState, onAir)
    fireEvent.keyDown(screen.getByLabelText(/^title$/i), { key: 'Escape' })
    expect(rttyStop()).toHaveBeenCalled()
    expect(haltTx()).toHaveBeenCalled()
  })

  it('Esc while continuous TX is latched STOPS', async () => {
    await openEditor({ ...rttyState, latched: true } as RttyState, latchUp)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(rttyStop()).toHaveBeenCalled()
    expect(haltTx()).toHaveBeenCalled()
  })

  it('Esc with nothing on the air closes the editor and stops NOTHING', async () => {
    await openEditor(rttyState, () => expect(document.querySelector('.rtty-macros')).not.toBeNull())
    fireEvent.keyDown(screen.getByLabelText(/^title$/i), { key: 'Escape' })
    expect(document.querySelector('.rtty-macro-editor'), 'Esc did not close the editor').toBeNull()
    expect(rttyStop()).not.toHaveBeenCalled()
    expect(haltTx(), 'Esc-to-cancel turned TX off').not.toHaveBeenCalled()
    // POSITIVE CONTROL: with the editor closed, the same idle Esc is a stop, as it always was.
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(rttyStop()).toHaveBeenCalled()
    expect(haltTx()).toHaveBeenCalled()
  })
})

// ── ESC, NAMED ON EVERY SCREEN ─────────────────────────────────────────────────────────────────
// Esc is the stop the keyboard holds, on every screen the operator transmits from and on
// Satellites (N71, 2026-10-01; N66 measured it missing on Tempo, SSTV and APRS, Phone's stopping
// only its voice keyer, and Satellites with no stop at all). This file cannot press it: it finds
// controls by accessible name, and five of these screens' Esc is bound by App, which no
// cockpit-level render contains. So each screen is NAMED here with what binds its Esc, and the
// list is computed against the registry: a section the operator can transmit from that is not
// named here goes red, whatever anybody remembers. Every one is PRESSED, with a real keydown, in
// stop-control-wiring.test.tsx ("Esc on every screen in the registry"), which mounts the real App
// over a fake bridge and asserts the exact commands that left the UI; that census must name each
// one too, so the two lists cannot drift apart. The hosted page's Esc is pressed in
// remote-web/remote-stop-line.test.tsx.
describe('Esc is a stop on every operating screen and on Satellites', () => {
  const ESC: Record<string, string> = {
    operate: 'OperateCockpit, on the shared capture listener (useEscStop) while on show → App handleHaltTx (halt_tx)',
    cw: 'CwCockpit, on the shared capture listener → the abort() its Stop TX calls (stop_cw, halt_tx)',
    rtty: 'RttyCockpit, on the shared capture listener → stop() (rtty_stop, halt_tx); an open F-key editor closes instead while nothing is on the air',
    psk: 'PskCockpit, as RTTY (psk_stop, halt_tx)',
    js8: 'Js8Cockpit, on the shared capture listener → stop() (halt_tx)',
    chat: 'App, while Tempo is on show → handleHaltTx, the top bar Stop TX on this screen (halt_tx)',
    phone: 'App, while Phone is on show → handleHaltTx (halt_tx, as its header Stop TX); the voice keyer also stops itself',
    sstv: 'App, while SSTV is on show → handleHaltTx (halt_tx, as its header Stop TX)',
    aprs: 'App, while APRS is on show → handleHaltTx (halt_tx); APRS draws no stop control',
    sats: 'App, while Satellites is on show → handleHaltTx (halt_tx); Satellites draws no stop control',
  }
  // Filed under Operate in the registry and never transmit: manager views that do not touch the rig.
  const NEVER_TRANSMIT = ['memories', 'program']

  it('every section the operator transmits from, and Satellites, is named with what binds its Esc', () => {
    const operating = sectionFeatures()
      .filter((f) => f.category === 'Operate' && !NEVER_TRANSMIT.includes(f.id))
      .map((f) => f.id as string)
    expect(operating.length, 'control: the registry has operating sections').toBeGreaterThan(5)
    expect(Object.keys(ESC).sort()).toEqual([...operating, 'sats'].sort())
  })

  it('…and the wire census presses Esc on each of them, expecting a stop', () => {
    const wiring = readFileSync(resolve(__dirname, '../stop-control-wiring.test.tsx'), 'utf8')
    const census = wiring.slice(wiring.indexOf('const ESC_SCREENS'))
    expect(census.length, 'control: the census is in the wiring suite').toBeLessThan(wiring.length)
    for (const view of Object.keys(ESC)) {
      expect(
        census.includes(`view: '${view}'`),
        `${view}: the wiring census does not press Esc on it`,
      ).toBe(true)
    }
  })
})
