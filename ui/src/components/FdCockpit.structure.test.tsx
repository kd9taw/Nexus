// @vitest-environment jsdom
//
// THE FIELD DAY COCKPIT SHELL — the guard that stands in for the ones this cockpit refuses.
//
// Every other cockpit gets two mechanical checks for free: `panelState.test.ts` (no stop
// control has a ⊞ id in any vocabulary) and the `stop-line.test.tsx` sweep (with every id
// removed, the stop controls are still on screen and no more disabled). Neither can run here,
// because this cockpit HAS no panel vocabulary — mis-hiding a pane is unrepresentable rather
// than guarded, which is the whole design decision. The cost of that decision is this file:
// what the sweeps would have proved is asserted here by construction instead.
//
// What it pins, and why each one is a real failure rather than a shape:
//
//   THE SHELL CENSUS       — four child kinds and no more; exactly one pane region and one TX
//                            dock, dock after region (pinned at the bottom).
//   STOP TX BY NAME        — found the way every sweep finds it (`/^stop tx$/i`), rendering
//                            OUTSIDE the pane region and outside every pane frame, and not
//                            disabled. This is the control the operator's safety rests on.
//   ZERO REMOVABLE PANES   — no pane carries a hide button, and no ⊞ menu exists. The claim in
//                            the module header is checked, not asserted.
//   ESC ORDERING           — Esc stops the rig FROM INSIDE THE CALLSIGN FIELD. A stop that only
//                            works when nothing is focused is not a stop, and "mid-callsign" is
//                            exactly when an operator reaches for it.
//   PTT UNKEY ON UNMOUNT   — unmounting the cockpit unkeys the rig. Leaving the screen with a
//                            held mic key is the 2 AM failure the verbatim lift exists for.
//   FOCUS RETURN           — after a logged contact the caret is back in Call, and nothing in
//                            the cockpit took it away.
//   THE BARE-KEY ROUTER    — a printable key with nothing focused lands the caret in Call; a
//                            printable key with a field focused does NOT (both directions, or
//                            it is half a test).
//   INERT BOARDS           — clicking a grid cell or a section chip does not move focus off the
//                            entry field.
//   STABLE COLUMN KEYS     — a tier flip does not remount the boards or the entry strip.
//
// jsdom has no layout, so widths are stubbed the way useRegionCols.test.ts does. The two heavy
// mode panes are stubbed; CockpitHeader and LogEntry are DELIBERATELY REAL — the first draws
// the stop control this file is about, and the second owns the focus behaviour.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act, fireEvent, waitFor } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { FdCockpit } from './FdCockpit'
import type { AppSnapshot, FieldDayStatus } from '../types'

/** Read back by two source guards below. Named constants, because Vite rewrites a LITERAL
 *  `new URL('./x', import.meta.url)` into a served asset URL and `fileURLToPath` then throws
 *  "The URL must be of scheme file". */
const SELF = './FdCockpit.structure.test.tsx'
const SELF_COMPONENT = './FdCockpit.tsx'
const SHEET = '../styles.css'

// ⭐ DERIVED FROM THE REAL MODULE, not a hand-kept list (the stop-line sweep's lesson): a mock
// that omits an export added later makes the component THROW ON MOUNT, which reads as a
// behaviour regression rather than the stale mock it is. Only the calls this file asserts on
// are overridden, so their shapes are the real ones.
// `vi.hoisted` because `vi.mock`'s factory is hoisted above every `const` in this file: a
// plain top-level spy is in its temporal dead zone when the factory runs.
const { setPtt, haltTx, stopCw, sendCw, fdLogManual } = vi.hoisted(() => ({
  setPtt: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  stopCw: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => ({})),
  fdLogManual: vi.fn(async () => ({})),
}))
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    setPtt,
    haltTx,
    stopCw,
    sendCw,
    fdLogManual,
    getLog: vi.fn(async () => []),
    // The auto-stub resolves `{}`, which LogEntry stores as the contact's COUNTRY and then
    // calls `.trim()` on. `null` is the real "no callbook answer" shape.
    resolveEntity: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    cwDecode: vi.fn(async () => ({ text: 'CQ FD DE W1AW', sent: ['CQ FD DE KD9TAW K'] })),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

// The two heavy mode panes. The DIG stub keeps ONE thing real — the `onCall` handoff — because
// the read-only-monitor contract (a double-click prefills the log and never transmits) is what
// this cockpit's relationship with the FT path rests on.
vi.mock('./OperateDecodes', () => ({
  OperateDecodes: (p: { onCall: (c: string) => void }) => (
    <button type="button" data-testid="decode-stub" onClick={() => p.onCall('W1AW')}>
      decode
    </button>
  ),
}))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="keyer-stub" /> }))

/** The observed element's callback, so a test can fire a resize the way the browser does. */
let fire: (() => void) | null = null
beforeEach(() => {
  fire = null
  vi.clearAllMocks()
  localStorage.clear()
  globalThis.ResizeObserver = class {
    constructor(cb: () => void) {
      fire = cb
    }
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

/** jsdom has no layout: clientWidth is 0 unless stubbed. */
function stubWidth(el: Element, w: number) {
  Object.defineProperty(el, 'clientWidth', { configurable: true, get: () => w })
}

/** Flush the region hook's rAF debounce. */
async function frame() {
  await act(async () => {
    await new Promise((r) => requestAnimationFrame(() => r(null)))
  })
}

const radio = {
  dialMhz: 14.25,
  band: '20m',
  catOk: true,
  sideband: 'USB',
  rigMode: 'USB',
  operatingMode: 'phone',
  transmitting: false,
  tuning: false,
  txEnabled: true,
  txAllowed: true,
  slot: 0,
  rxOffsetHz: 1500,
  cwWpm: 22,
  rfPower: null,
  smeterDb: null,
  splitTxMhz: null,
}

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    harqRescues: 0,
    recentDecodes: [],
    radio: { ...radio, ...over },
  } as unknown as AppSnapshot
}

const fieldDay = {
  myClass: '3A',
  mySection: 'WI',
  running: true,
  state: 'running',
  qsoCount: 12,
  sections: 4,
  points: 24,
  workedSections: ['WI', 'EMA'],
  log: [
    { call: 'W1AW', band: '20m', mode: 'PH', class: '3A', section: 'EMA', whenUnix: 100 },
    { call: 'K1ABC', band: '40m', mode: 'CW', class: '2A', section: 'WI', whenUnix: 200 },
  ],
} as unknown as FieldDayStatus

function renderCockpit(props: Partial<Parameters<typeof FdCockpit>[0]> = {}) {
  return render(
    <FdCockpit snap={makeSnap()} fieldDay={fieldDay} onSetMode={() => {}} {...props} />,
  )
}

const shell = () => document.querySelector('main.layout.single.fd-cockpit')!
const callBox = () => screen.getByPlaceholderText('W1AW') as HTMLInputElement
const stopTx = () => screen.getByRole('button', { name: /^stop tx$/i })

/** Pick the position's class through the header's override select. */
function setClass(cls: 'CW' | 'PH' | 'DIG') {
  fireEvent.change(screen.getByLabelText('Position mode class'), { target: { value: cls } })
}

describe('the shell holds the contract and nothing else', () => {
  it('renders the shell class the SHELLS census in cockpit-shells.test.ts reasons about', () => {
    // The deficit-valve guard models this chain by hand. A class list that drifts in the
    // component would leave that guard computing the overflow of a shell nobody renders —
    // the APRS lesson, pinned the way APRS's own entry is.
    renderCockpit()
    expect(shell()).not.toBeNull()
    const src = readFileSync(fileURLToPath(new URL(SELF, import.meta.url)), 'utf8')
    expect(src, 'this file no longer names the shell it pins').toContain('layout single fd-cockpit')
  })

  it('holds no child kinds beyond the four-child contract', () => {
    renderCockpit()
    const ALLOWED = ['.cockpit-header', '.cockpit-panes', '.cockpit-txdock']
    for (const el of Array.from(shell().children)) {
      expect(
        ALLOWED.some((s) => el.matches(s)),
        `unexpected shell-level child <${el.tagName.toLowerCase()} class="${el.className}">`,
      ).toBe(true)
    }
    expect(shell().querySelectorAll(':scope > .cockpit-panes').length).toBe(1)
    expect(shell().querySelectorAll(':scope > .cockpit-txdock').length).toBe(1)
  })

  it('the dock comes after the region, and holds no pane frame', () => {
    renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    const dock = document.querySelector('.cockpit-txdock')!
    expect(region.compareDocumentPosition(dock) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(
      dock.querySelector('.pane-frame'),
      'a pane frame in the dock — a pane scrolls, and the entry row must not',
    ).toBeNull()
  })

  it('the region never opens more than the two columns it can fill', async () => {
    renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 2400) // wide enough for the 3-column tier
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols'), 'a third track with nothing to put in it').toBe('2')
    expect(region.querySelectorAll(':scope > .cockpit-col').length).toBe(2)
  })

  it('every board renders through a CockpitPaneFrame inside the region', () => {
    renderCockpit()
    for (const id of ['fdGrid', 'fdSections']) {
      const pane = document.querySelector(`[data-pane="${id}"]`)
      expect(pane, `pane "${id}" missing`).not.toBeNull()
      expect(pane!.classList.contains('pane-frame')).toBe(true)
      expect(pane!.closest('.cockpit-panes'), `"${id}" renders outside the region`).not.toBeNull()
    }
  })
})

describe('THE STOP LINE', () => {
  it('Stop TX is present by accessible name, enabled, and outside every pane', () => {
    renderCockpit()
    // No jest-dom in this project's setup: presence is `getByRole` not throwing, and the
    // disabled state is read off the element.
    const stop = stopTx() as HTMLButtonElement
    expect(stop.isConnected).toBe(true)
    expect(stop.disabled, 'Stop TX is disabled — a stop that cannot be pressed is not one').toBe(false)
    expect(stop.closest('.cockpit-panes'), 'Stop TX renders inside the pane region').toBeNull()
    expect(stop.closest('.pane-frame'), 'Stop TX renders inside a pane frame').toBeNull()
    expect(stop.closest('.cockpit-header'), 'Stop TX is not header chrome').not.toBeNull()
  })

  it('Stop TX halts the transmitter, and stops the keyer too on a CW position', async () => {
    renderCockpit()
    fireEvent.click(stopTx())
    expect(haltTx).toHaveBeenCalled()
    expect(stopCw, 'a phone position has no keyer to stop').not.toHaveBeenCalled()

    setClass('CW')
    fireEvent.click(stopTx())
    expect(stopCw).toHaveBeenCalled()
  })

  it('Esc stops the transmitter FROM INSIDE the callsign field (ordering)', () => {
    renderCockpit()
    const box = callBox()
    box.focus()
    fireEvent.change(box, { target: { value: 'K1AB' } })
    expect(document.activeElement).toBe(box)
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(haltTx, 'Esc was swallowed by the typing guard — the stop is dead mid-callsign').toHaveBeenCalled()
    // …and it stopped the RIG, not the entry: the half-typed call is still there.
    expect(box.value).toBe('K1AB')
  })

  // ── THE LIFT STAYS A LIFT ────────────────────────────────────────────────────────────
  // The PTT row is the app's most safety-critical markup and this cockpit is its SECOND site.
  // The only defensible way to have two is to keep them identical, so "identical" is computed
  // rather than promised: both rows are read out of their sources, the four prop substitutions
  // are undone, comments and whitespace are normalised away, and what is left must match. It
  // fires when Phone rewords a label (#81's four states are what the sweeps match by name) or
  // when either side gains a handler the other lacks — a pointer-leave dropped here would be a
  // rig left keyed by a bumped tent table, and nothing else in this suite would see it.
  it('the PTT row is still character-for-character Phone\'s, modulo its props', () => {
    // Comments and FORMATTING are normalised away — the two files wrap the same template
    // literal differently because our prop names are shorter than Phone's `snap.radio.*`
    // paths, and a line break inside `${…}` is not a difference in the control. What survives
    // is every class, handler, state branch and label, in order.
    const strip = (s: string) =>
      s
        .replace(/\{?\s*\/\*[\s\S]*?\*\/\s*\}?/g, '')
        .replace(/^\s*\/\/.*$/gm, '')
        .replace(/\s+/g, ' ')
        .replace(/\s*([{}()$?:&|`=<>,])\s*/g, '$1')
        .trim()
    /** The `.ph-ptt-row` block, from its opening tag to the `</div>` after the Lock toggle. */
    const row = (file: string) => {
      const src = readFileSync(fileURLToPath(new URL(file, import.meta.url)), 'utf8')
      const a = src.indexOf('<div className="ph-ptt-row">')
      const lock = src.indexOf('<span>Lock</span>', a)
      const end = src.indexOf('</div>', lock)
      if (a < 0 || lock < 0 || end < 0) throw new Error(`no .ph-ptt-row in ${file}`)
      return src.slice(a, end + '</div>'.length)
    }
    const phone = row('./PhoneCockpit.tsx')
    const ours = row('./FdPttRow.tsx')
    // Positive control: the extraction found real rows, not two empty strings.
    expect(phone, 'the Phone row was not extracted').toContain('PUSH TO TALK')
    expect(ours, 'our row was not extracted').toContain('PUSH TO TALK')

    // The ONLY sanctioned differences: Phone reads its state off `snap` and closes over its own
    // setters; ours takes both as props, because the shell owns the state (the voice keyer pane
    // must see `keyed`, and the unmount force-unkey belongs to the shell's lifetime).
    const asProps = phone
      .replace(/snap\.radio\.txAllowed/g, 'txAllowed')
      .replace(/snap\.radio\.txEnabled/g, 'txEnabled')
      .replace(/key\(!keyed\)/g, 'onToggleKey()')
      .replace(/setLock\(e\.target\.checked\)/g, 'onLockChange(e.target.checked)')
    expect(
      strip(ours),
      'the Field Day PTT row and Phone\'s have drifted apart. They are one control with two ' +
        'sites: reword or rewire BOTH, and re-run the stop-line sweeps.',
    ).toBe(strip(asProps))

    // …and the HANDLERS behind it, which is the half that actually keys the rig: the two
    // separate refusals (#81 — licence lock, then TX-switched-off, each with its own remedy)
    // and the hold/toggle split. `key(false)` must reach `setPtt` through BOTH guards, because
    // an unkey that a guard can swallow is a stuck transmitter.
    const handlers = (file: string) => {
      const src = readFileSync(fileURLToPath(new URL(file, import.meta.url)), 'utf8')
      const a = src.indexOf('  const key = (on: boolean) => {')
      const tail = '  const onPttUp = () => {\n    if (!lock) key(false)\n  }'
      const b = src.indexOf(tail, a)
      if (a < 0 || b < 0) throw new Error(`no PTT handler block in ${file}`)
      return src.slice(a, b + tail.length)
    }
    const phoneKey = handlers('./PhoneCockpit.tsx')
    expect(phoneKey, 'the Phone handler block was not extracted').toContain('setPtt(on)')
    expect(
      strip(handlers('./FdCockpit.tsx')),
      'the PTT HANDLERS have drifted from Phone\'s. This is the block that keys and unkeys ' +
        'the rig; a guard added on one side only is a refusal (or a stuck key) in one cockpit ' +
        'and not the other.',
    ).toBe(strip(phoneKey))
  })

  // ── THE SPACE BAR ────────────────────────────────────────────────────────────────────
  // The first Space-key tests in this repository (`grep -rn "code: 'Space'" --include=*.test.tsx`
  // found nothing before these), and they exist because the two stuck-PTT defects this cockpit
  // shipped with both lived on this listener rather than on the row above it.
  it('a held Space RELEASES wherever the caret went — the router may move it mid-hold', () => {
    // ⚠️ THE STUCK TRANSMITTER. Phone guards its keyup on the target ("not while typing in a
    // field"), which is sound there because nothing in that cockpit moves focus by itself.
    // Here the bare-key router does: key with Space, type the callsign you are hearing — the
    // single most common Field Day action — and the release lands on an INPUT. A target guard
    // on the UP half swallows it, and the rig stays keyed under a button still reading
    // "release to stop", which the operator did.
    renderCockpit()
    ;(document.activeElement as HTMLElement | null)?.blur()
    fireEvent.keyDown(window, { code: 'Space' })
    expect(setPtt, 'Space did not key the rig').toHaveBeenCalledWith(true)
    expect(screen.getByRole('button', { name: /on air/i }).isConnected).toBe(true)

    // Still holding, the operator types. The router lands the caret in Call.
    fireEvent.keyDown(window, { key: 'k' })
    expect(document.activeElement).toBe(callBox())

    setPtt.mockClear()
    fireEvent.keyUp(callBox(), { code: 'Space' })
    expect(
      setPtt,
      'the release was swallowed by a guard — the rig is keyed with nothing left to unkey it',
    ).toHaveBeenCalledWith(false)
    expect(screen.getByRole('button', { name: /push to talk/i }).isConnected).toBe(true)
  })

  it('a space typed into the callsign field does not key the rig', () => {
    // The other direction, and it is the one the DOWN guard holds: the caret is in Call for
    // essentially the whole event, and a transmission must never start from a keystroke the
    // operator meant as text. (The character does not survive either — LogEntry drops spaces
    // from the FD call field; that half is pinned in LogEntry.fddraft.test.tsx.)
    renderCockpit()
    callBox().focus()
    fireEvent.keyDown(callBox(), { code: 'Space' })
    expect(setPtt, 'typing a space keyed the transmitter').not.toHaveBeenCalledWith(true)
  })

  it('the PTT button never claims ON AIR over a rig this cockpit has unkeyed', () => {
    // A mode-class flip force-unkeys (the effect cleanup). If `keyed` survived it, flipping
    // back — which one CAT poll reading CW and the next reading USB does, with no operator
    // action at all — would redraw a red "ON AIR — release to stop" over an idle rig, and in
    // Lock mode the press that reads as a release would key the transmitter.
    renderCockpit()
    ;(document.activeElement as HTMLElement | null)?.blur()
    fireEvent.keyDown(window, { code: 'Space' })
    expect(screen.getByRole('button', { name: /on air/i }).isConnected).toBe(true)
    setClass('CW')
    expect(setPtt).toHaveBeenCalledWith(false)
    setClass('PH')
    expect(
      screen.queryByRole('button', { name: /on air/i }),
      'the button reads ON AIR over a rig that was unkeyed on the way out of the phone class',
    ).toBeNull()
    expect(screen.getByRole('button', { name: /push to talk/i }).isConnected).toBe(true)
  })

  it('the Space listener is still Phone\'s, and every difference is named here', () => {
    // The row markup and the key/onPttDown/onPttUp block are compared above; this is the third
    // piece of the same control and the one where BOTH stuck-PTT defects lived. It is allowed
    // to differ from Phone's — but only in the four ways written out below, so a fifth
    // difference (a second early return in `up`, a target guard put back, BUTTON added to
    // `isField`) fails here instead of on the air.
    const strip = (s: string) =>
      s
        .replace(/\{?\s*\/\*[\s\S]*?\*\/\s*\}?/g, '')
        .replace(/^\s*\/\/.*$/gm, '')
        .replace(/\s+/g, ' ')
        .replace(/\s*([{}()$?:&|`=<>,])\s*/g, '$1')
        .trim()
    const listener = (file: string, deps: string) => {
      const src = readFileSync(fileURLToPath(new URL(file, import.meta.url)), 'utf8')
      const a = src.indexOf('    const isField = ')
      const b = src.indexOf(deps, a)
      if (a < 0 || b < 0) throw new Error(`no Space listener in ${file}`)
      return src.slice(a, b + deps.length)
    }
    const phone = listener('./PhoneCockpit.tsx', '}, [lock])')
    const ours = listener('./FdCockpit.tsx', '}, [lock, phone])')
    // Positive control: two real listeners, not two empty strings.
    expect(phone, 'the Phone listener was not extracted').toContain("e.code === 'Space'")
    expect(ours, 'our listener was not extracted').toContain("e.code === 'Space'")

    const asOurs = phone
      // 1. `t` is the i18n function in this cockpit, so the parameter is `el`.
      .replace(/\bt: EventTarget \| null\b/, 'el: EventTarget | null')
      .replace(/\bt instanceof HTMLElement\b/, 'el instanceof HTMLElement')
      .replace(/\bt\.tagName\b/g, 'el.tagName')
      // 2. THE RELEASE IS UNCONDITIONAL ON THE TARGET. See the test above: the router can move
      //    focus mid-hold here, and an unkey a guard can swallow is a stuck transmitter.
      .replace("if (e.code === 'Space' && !isField(e.target) && !lock) {", "if (e.code === 'Space' && keyedRef.current && !lock) {")
      // 3. The force-unkey clears this cockpit's own `keyed` with the rig's.
      .replace('void setPtt(false) // safety: never leave the rig keyed on unmount', 'setKeyed(false)\n      void setPtt(false)')
      // 4. Bound on the PHONE class only (a CW position has no PTT row to release the key).
      .replace('}, [lock])', '}, [lock, phone])')
    expect(
      strip(ours),
      'the Field Day Space listener has drifted from Phone\'s in a way this guard does not ' +
        'name. It is the block that keys and releases the rig from the keyboard: change both ' +
        'sides, or add the difference here with the reason it is safe.',
    ).toBe(strip(asOurs))
  })

  it('leaving the phone class unkeys the rig, not just leaving the cockpit', () => {
    // The PTT row is gone the moment the class changes, so the mic key it was holding has no
    // button left to release it. The Space/PTT effect is bound on the phone class for exactly
    // this reason: its cleanup is the release.
    renderCockpit()
    setClass('CW')
    expect(setPtt, 'the class change left the rig keyed with no PTT row on screen').toHaveBeenCalledWith(false)
    expect(screen.queryByRole('button', { name: /push to talk/i })).toBeNull()
  })

  it('leaving the cockpit unkeys the rig', () => {
    // The verbatim-lifted Space/PTT effect's cleanup. A nav away, or a mode-class change that
    // takes the PH strip off the dock, must never leave a held mic key on the air.
    const r = renderCockpit()
    expect(screen.getByRole('button', { name: /push to talk/i }).isConnected).toBe(true)
    r.unmount()
    expect(setPtt, 'unmount did not force-unkey').toHaveBeenCalledWith(false)
  })
  it('leaving the cockpit stops the CW keyer, not just the mic key', () => {
    // "Leaving this screen is a stop" held for the PH class only: the force-unkey lives in the
    // Space/PTT effect, which is bound on the phone class. A CW macro is a QUEUE the engine
    // keeps sending — F1, then the Dashboard button in this cockpit's own header, and the
    // keyer runs on with the only Stop TX beside it gone.
    const r = renderCockpit()
    setClass('CW')
    fireEvent.keyDown(window, { key: 'F1' })
    expect(sendCw, 'no macro was sent — this test would prove nothing').toHaveBeenCalled()
    expect(stopCw).not.toHaveBeenCalled()
    r.unmount()
    expect(stopCw, 'the cockpit left a CW macro sending on the way out').toHaveBeenCalled()
  })
})

describe('zero removable panes — the vocabulary the sweeps would have checked does not exist', () => {
  it('no pane offers a hide control, and there is no ⊞ menu', () => {
    renderCockpit()
    expect(document.querySelectorAll('[data-pane]').length).toBeGreaterThan(0)
    for (const b of Array.from(document.querySelectorAll('.cockpit-popout'))) {
      expect(
        b.getAttribute('aria-label') ?? '',
        'a pane in this cockpit can be hidden — the design is zero vocabulary',
      ).not.toMatch(/hide/i)
    }
    expect(document.querySelector('.panels-menu')).toBeNull()
  })

  it('the source declares no panel vocabulary at all', () => {
    // Cheap and exact: the guarantee here is the ABSENCE of ids, so the absence is what is
    // checked. A cockpit that grows a vocabulary must grow the sweeps with it.
    // Comments stripped first: this file's own module header NAMES the things it refuses,
    // and a guard that reads prose as code is a guard that can only ever be green by luck.
    const src = readFileSync(fileURLToPath(new URL(SELF_COMPONENT, import.meta.url)), 'utf8')
      .replace(/\/\*[\s\S]*?\*\//g, '')
      .replace(/^\s*\/\/.*$/gm, '')
    for (const banned of ['usePanelLayout', 'PanelsMenu', 'panelHost', 'onRemove=']) {
      expect(src, `FdCockpit now uses ${banned} — add it to the stop-line sweeps`).not.toContain(
        banned,
      )
    }
  })
})

/**
 * THE ENTRY ROW UNDER THE OPERATOR'S HANDS — the two facts about the dock that jsdom cannot
 * see, computed off the sheet instead of grepped for (a regex-presence CSS test is how two dead
 * fixes shipped here before the overhaul: the rule was present and the cascade ignored it).
 *
 * Measured in headless Chrome at 1024×768 while this was written: the FD strip is 117px of
 * content, a verdict line is 13px, and the floor this shipped with — `min-height: 8.5em` on
 * `.log-entry-fd`, 119px — reserved TWO of those thirteen. The row still jumped, mid-callsign,
 * which is the exact failure the rule's own comment said it prevented.
 */
describe('the dock reserves what it says it reserves', () => {
  /** Top-level rules only (depth 0): a rule inside `@media`/`@supports` answers a different
   *  question and must not be mistaken for the unconditional winner. */
  function topLevelRules(css: string): Array<{ sel: string; body: string; order: number }> {
    const out: Array<{ sel: string; body: string; order: number }> = []
    const src = css.replace(/\/\*[\s\S]*?\*\//g, '')
    let depth = 0
    let start = 0
    let order = 0
    for (let i = 0; i < src.length; i++) {
      const c = src[i]
      if (c === '{') {
        if (depth === 0) {
          const sel = src.slice(start, i).trim()
          if (sel.startsWith('@')) {
            depth++
            start = i + 1
            continue
          }
          // a plain rule: find its closing brace
          const end = src.indexOf('}', i)
          for (const one of sel.split(','))
            out.push({ sel: one.trim(), body: src.slice(i + 1, end), order: order++ })
          i = end
          start = i + 1
        } else {
          depth++
        }
      } else if (c === '}') {
        if (depth > 0) depth--
        start = i + 1
      }
    }
    return out
  }

  /** A selector part split into TOKENS — `.class` and `[attr='v']`, which is all the rules
   *  this reasons about use. Anything else (a tag, an id, a pseudo, a combinator) makes the
   *  rule unmatchable here rather than silently half-matched. */
  const tokens = (part: string): string[] | null => {
    const found = part.match(/\.[a-z][a-z0-9-]*|\[[^\]]+\]/g) ?? []
    return found.join('') === part && found.length > 0 ? found : null
  }

  /** Does `sel` match an element carrying `self` tokens, nested under `ancestors`?
   *  Descendant combinators only — which is all this file uses. */
  function matches(sel: string, self: string[], ancestors: string[]): boolean {
    if (/[>+~]/.test(sel)) return false
    const parts = sel.split(/\s+/).map(tokens)
    if (parts.some((p) => p === null)) return false
    const list = parts as string[][]
    if (!list[list.length - 1].every((tk) => self.includes(tk))) return false
    return list.slice(0, -1).every((p) => p.every((tk) => ancestors.includes(tk)))
  }

  const spec = (sel: string) => (sel.match(/\.[a-z][a-z0-9-]*|\[[^\]]+\]/g) ?? []).length

  /** Cascade winner for one property on one hypothetical element. */
  function winner(prop: string, self: string[], ancestors: string[]) {
    const css = readFileSync(fileURLToPath(new URL(SHEET, import.meta.url)), 'utf8')
    let win: { value: string; sel: string; spec: number; order: number } | null = null
    for (const r of topLevelRules(css)) {
      if (!matches(r.sel, self, ancestors)) continue
      const m = new RegExp(`(?:^|;)\\s*${prop}\\s*:\\s*([^;]+)`).exec(r.body)
      if (!m) continue
      const sp = spec(r.sel)
      if (!win || sp > win.spec || (sp === win.spec && r.order >= win.order)) {
        win = { value: m[1].trim(), sel: r.sel, spec: sp, order: r.order }
      }
    }
    return win
  }

  const FD_ANCESTORS = [
    '.app',
    '.layout',
    '.single',
    '.fd-cockpit',
    '.cockpit-txdock',
    '.fd-entry',
    '.log-entry',
    '.log-entry-fd',
  ]

  it('the verdict slot is always in the DOM, empty or not', () => {
    // The reservation is a property of a box that is ALWAYS there. A slot that mounts with its
    // first verdict reserves nothing.
    renderCockpit()
    const strip = document.querySelector('.log-entry-fd')!
    const slot = strip.querySelector('.le-fd-verdicts')
    expect(slot, 'no verdict slot — nothing holds the row still').not.toBeNull()
    expect(slot!.children.length, 'this fixture should paint no verdict yet').toBe(0)
    // …and it fills rather than grows: type a call with no section and the verdict lands INSIDE
    // the slot that was already there.
    fireEvent.change(callBox(), { target: { value: 'K1ABC' } })
    fireEvent.change(screen.getByPlaceholderText('WI'), { target: { value: '' } })
    expect(document.querySelector('.le-fd-verdicts')!.children.length).toBeGreaterThan(0)
    expect(document.querySelector('.le-fd-hint[role="alert"]')!.closest('.le-fd-verdicts')).not.toBeNull()
  })

  it('the slot carries a real floor, and the strip does not pretend to', () => {
    const slot = winner('min-height', ['.le-fd-verdicts'], FD_ANCESTORS)
    expect(slot, 'nothing floors the verdict slot').not.toBeNull()
    const px = Number(/^([\d.]+)px$/.exec(slot!.value)?.[1] ?? NaN)
    expect(px, `the slot's floor computes to "${slot!.value}" — one 13px verdict line is the point`).toBeGreaterThanOrEqual(13)
    // The strip-level floor was the dead one: 8.5em against a 117px natural strip. A floor
    // there has to be re-derived every time the header or the field row changes height, so it
    // is not where this reservation lives.
    const strip = winner('min-height', ['.log-entry', '.log-entry-fd'], FD_ANCESTORS)
    expect(
      strip,
      `the strip carries "min-height: ${strip?.value}" again — reserve on the SLOT, or the ` +
        'reservation silently stops covering a line the day either row above it changes height',
    ).toBeNull()
  })

  it('the callsign field really is the biggest thing in the entry row', () => {
    // ⚠️ READ THE WINNER, NOT THE RULE YOU MEANT TO WRITE. `.le-fd-input` declares 30px, and
    // it never renders: `.settings-input` is the same specificity and lands later, so the FD
    // callsign field is 14px in the Phone and CW cockpits. The only reason it is big here is
    // the (0,2,0) rule this asserts.
    const call = winner(
      'font-size',
      ['.settings-input', '.mono', '.le-fd-input', '.le-fd-input-call'],
      FD_ANCESTORS,
    )
    const base = winner('font-size', ['.settings-input', '.mono', '.le-fd-input'], [
      '.app',
      '.log-entry',
      '.log-entry-fd',
    ])
    const px = (v: string | undefined) => Number(/^([\d.]+)px$/.exec(v ?? '')?.[1] ?? NaN)
    expect(px(call?.value), 'the callsign field has no computed size of its own').toBeGreaterThan(0)
    expect(
      px(call?.value),
      `the run field computes to ${call?.value} against a ${base?.value} base — the one field a ` +
        'run is typed into is no bigger than the rest of the form',
    ).toBeGreaterThan(px(base?.value))
  })

  it('the header chips give the boards their height back at the supported floor', () => {
    // MEASURED (headless Chrome, 1024×768 and an effective 1093 — a 1366 laptop at 125% zoom):
    // the chip block wrapped to three rows and took the header from 44px to 196.3px, and all of
    // it came out of the pane region (272px with the chips, 425px with the block emptied). The
    // advisory is the widest item by far — a whole sentence at `max-width: 32em` — and it
    // already ellipsizes into its own `title`, so capping it at the two narrow viewport classes
    // put the header back to its one-row height (141px, the same as at 1366) and gave the
    // boards 55px back. `data-viewport`, never a px `@media`: the width that matters is
    // zoom-adjusted, and a pinned-zoom operator is exactly who is at the floor.
    const ADV = ['.fd-advisory', '.banned']
    const base = winner('max-width', ADV, ['.app', '.fd-cockpit', '.cockpit-header', '.ch-mode-extras'])
    const narrow = winner('max-width', ADV, [
      "[data-viewport='sm']",
      '.app',
      '.fd-cockpit',
      '.cockpit-header',
      '.ch-mode-extras',
    ])
    const em = (v: string | undefined) => Number(/^([\d.]+)em$/.exec(v ?? '')?.[1] ?? NaN)
    expect(em(base?.value), 'the advisory lost its unconditional width').toBeGreaterThan(0)
    expect(
      em(narrow?.value),
      `the advisory computes to ${narrow?.value} at the 1024 floor against ${base?.value} wide — ` +
        'the chip block wraps to three rows again and the boards pay for it',
    ).toBeLessThan(em(base?.value))
  })
})

describe('focus is the product', () => {
  it('the entry row starts focused, and comes back after a logged contact', async () => {
    renderCockpit()
    await waitFor(() => expect(document.activeElement).toBe(callBox()))
    fireEvent.change(callBox(), { target: { value: 'W2XYZ' } })
    fireEvent.change(screen.getByPlaceholderText('1D'), { target: { value: '3A' } })
    fireEvent.change(screen.getByPlaceholderText('WI'), { target: { value: 'WI' } })
    // Move focus away exactly as a mouse click on the button does.
    const log = screen.getByRole('button', { name: /log fd/i })
    log.focus()
    await act(async () => {
      fireEvent.click(log)
      await Promise.resolve()
    })
    expect(fdLogManual).toHaveBeenCalledWith('W2XYZ', '3A', 'WI', 'PH', undefined)
    await waitFor(() => expect(document.activeElement).toBe(callBox()))
    expect(callBox().value, 'the call did not clear for the next contact').toBe('')
  })

  it('a bare printable key with nothing focused lands the caret in Call', () => {
    renderCockpit()
    ;(document.activeElement as HTMLElement | null)?.blur()
    expect(document.activeElement).toBe(document.body)
    fireEvent.keyDown(window, { key: 'k' })
    expect(document.activeElement, 'the router did not catch the first keystroke').toBe(callBox())
  })

  it('the router never fires while a field is focused (the other direction)', () => {
    renderCockpit()
    // The rate-goal editor is the one other text input on this screen; a router that fired
    // here would yank the caret out of it mid-number.
    fireEvent.click(screen.getByRole('button', { name: 'Contacts-per-hour goal' }))
    const goal = screen.getByLabelText('Contacts-per-hour goal') as HTMLInputElement
    goal.focus()
    fireEvent.keyDown(window, { key: '6' })
    expect(document.activeElement, 'the router stole focus out of a live field').toBe(goal)
  })

  it('the router leaves a focused <select> alone — type-ahead is a selection, not typing', () => {
    // A native select answers a printable key with type-ahead: "4" picks 40m in the band
    // picker, "C" picks CW in the mode-class override. A router that fired there would eat the
    // selection AND yank the caret out of a control the operator is mid-way through using.
    renderCockpit()
    const sel = screen.getByLabelText('Position mode class') as HTMLSelectElement
    sel.focus()
    fireEvent.keyDown(window, { key: 'C' })
    expect(document.activeElement, 'the router stole focus out of a select mid-selection').toBe(sel)
  })

  it('a modifier combination passes straight through', () => {
    // Ctrl/⌘+1–9 is App's global memory recall; the router must never eat one.
    renderCockpit()
    ;(document.activeElement as HTMLElement | null)?.blur()
    fireEvent.keyDown(window, { key: '3', ctrlKey: true })
    expect(document.activeElement).toBe(document.body)
    fireEvent.keyDown(window, { key: 'k', metaKey: true })
    expect(document.activeElement).toBe(document.body)
  })

  it('the boards are inert — clicking one does not take focus off the entry', () => {
    renderCockpit()
    const box = callBox()
    box.focus()
    fireEvent.change(box, { target: { value: 'W1AW' } })
    const cell = document.querySelector('[data-cell="20m|PH"]')!
    fireEvent.click(cell)
    expect(document.activeElement, 'a grid cell took focus off the callsign').toBe(box)
    // …and a real section cell on the board below it (they carry an aria-label each).
    const section = document.querySelectorAll('.fd-sections-panel span[aria-label]')[0]
    expect(section, 'no section cell rendered — this half of the test proves nothing').toBeTruthy()
    fireEvent.click(section)
    expect(document.activeElement, 'the sections board took focus off the callsign').toBe(box)
    expect(box.value).toBe('W1AW')
  })

  it('a decode double-click prefills the call and puts the caret in it — and transmits nothing', async () => {
    renderCockpit()
    setClass('DIG')
    fireEvent.click(screen.getByTestId('decode-stub'))
    await waitFor(() => expect(callBox().value).toBe('W1AW'))
    expect(document.activeElement).toBe(callBox())
    // Nothing went on the air. (`setPtt(false)` DOES fire on the way out of the phone class —
    // that is the force-unkey, and it is asserted as such below.)
    expect(sendCw).not.toHaveBeenCalled()
    expect(setPtt, 'the digital monitor keyed the rig').not.toHaveBeenCalledWith(true)
  })
})

describe('the mode class picks the pane and the dock strip', () => {
  it('PH: the voice keyer and the PTT row', () => {
    renderCockpit()
    expect(screen.getByTestId('keyer-stub').isConnected).toBe(true)
    const ptt = screen.getByRole('button', { name: /push to talk/i })
    expect(ptt.closest('.cockpit-txdock'), 'the PTT row left the dock').not.toBeNull()
    expect(ptt.closest('.pane-frame'), 'the PTT row is inside a pane').toBeNull()
  })

  it('CW: the Field Day macro strip in the dock, and F3 sends its text', () => {
    renderCockpit()
    setClass('CW')
    const macros = document.querySelector('.cw-macros')!
    expect(macros.closest('.cockpit-txdock'), 'the macro strip left the dock').not.toBeNull()
    expect(macros.querySelectorAll('.cw-macro').length).toBe(8)
    // Bound regardless of focus — the whole point of a run is to send the exchange without
    // leaving the Call field.
    callBox().focus()
    fireEvent.keyDown(window, { key: 'F3' })
    expect(sendCw).toHaveBeenCalledWith('! DE {MYCALL} {EXCH} {EXCH} K')
  })

  it('CW: the copy and the sent echo are two panes, not two full-height blocks in one', () => {
    // ⚠️ STRUCTURAL, not cosmetic. `.pane-body > .cw-decode` is `height: 100%` (styles.css), so
    // two of them inside ONE pane body ask for 200% of it and the second renders entirely below
    // the fold at every window size — measured 332 visible of 648 at 1366×768. The sent echo is
    // the only confirmation that a macro expanded to the right callsign, and `usePinnedScroll`
    // does not follow a pane body, so it would never come back on screen. CwCockpit puts each
    // in its own frame — fill for the copy, `fit="content"` for the echo — and so does this.
    renderCockpit()
    setClass('CW')
    for (const body of Array.from(document.querySelectorAll('.cockpit-panes .pane-body'))) {
      expect(
        body.querySelectorAll(':scope > .cw-decode').length,
        'two height:100% blocks share one pane body — the second is below the fold',
      ).toBeLessThan(2)
    }
    const sent = document.querySelector('.cw-sent-panel')!.closest('.pane-frame')!
    const copy = document.querySelector('[data-pane="fdCw"]')!
    expect(sent.isSameNode(copy), 'the sent echo is still inside the copy pane').toBe(false)
    expect(sent.getAttribute('data-fit'), 'a bounded echo must not fight the copy for height').toBe(
      'content',
    )
    expect(copy.getAttribute('data-fit')).toBe('fill')
  })

  it('the FD cockpit names the on-air mode when the rig is in one', async () => {
    // ⚠️ THE ONE THE FIRST CUT MISSED. The RTTY and PSK cockpits were taught to name their
    // on-air mode, but THIS cockpit — the surface a Field Day operator actually sits on —
    // was not, and the engine fills a missing submode from the TIER. So a rig in RTTY,
    // logged from here, wrote MODE=FT8 into the Field Day ADIF and DG into the Cabrillo
    // where RY belonged: the exact defect the batch was written to close, on the most
    // likely screen. The rig's own mode is the only honest source for it.
    cleanup()
    vi.clearAllMocks()
    renderCockpit({ snap: makeSnap({ rigMode: 'RTTY' }) })
    setClass('DIG')
    fireEvent.change(callBox(), { target: { value: 'W2XYZ' } })
    fireEvent.change(screen.getByPlaceholderText('1D'), { target: { value: '3A' } })
    fireEvent.change(screen.getByPlaceholderText('WI'), { target: { value: 'WI' } })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: /log fd/i }))
      await Promise.resolve()
    })
    expect(fdLogManual).toHaveBeenCalledWith('W2XYZ', '3A', 'WI', 'DIG', 'RTTY')
  })

  it('a contact is committed under the class the header shows — all three of them', async () => {
    // ⚠️ THE SCORING BUG THIS LANDING FOUND. `fdLogManual`'s TS signature and LogEntry's
    // `fdMode` prop both listed 'CW' | 'PH' only, because their first two callers were the CW
    // and Phone cockpits — while the engine's `log_mode_at` has always taken 'DIG' and stamped
    // the real submode behind it. A digital position logging a picked-up station by hand had
    // no way to say so, and a contact credited to phone is a wrong class in the Cabrillo AND a
    // dupe check against the wrong cell. Driven for every class, not just the new one.
    for (const cls of ['CW', 'PH', 'DIG'] as const) {
      cleanup()
      vi.clearAllMocks()
      renderCockpit()
      setClass(cls)
      fireEvent.change(callBox(), { target: { value: 'W2XYZ' } })
      fireEvent.change(screen.getByPlaceholderText('1D'), { target: { value: '3A' } })
      fireEvent.change(screen.getByPlaceholderText('WI'), { target: { value: 'WI' } })
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: /log fd/i }))
        await Promise.resolve()
      })
      // The fifth argument is the ON-AIR mode behind the class. CW and PH ARE their on-air
      // mode and carry none. DIG carries one only when the RIG NAMES a mode the FT tiers do
      // not cover — see `fdSubmodeFromRig`. These fixtures report no rig mode, so DIG passes
      // none here and the engine fills it from the tier, which is correct for an FT contact.
      // A rig actually in RTTY is the case that used to log FT8; it is pinned separately in
      // `the FD cockpit names the on-air mode when the rig is in one` below.
      expect(fdLogManual, `a ${cls} position logged under the wrong class`).toHaveBeenCalledWith(
        'W2XYZ',
        '3A',
        'WI',
        cls,
        undefined,
      )
    }
  })

  it('DIG: the read-only note, and no transmit control of its own', () => {
    renderCockpit()
    setClass('DIG')
    expect(document.querySelector('.fd-dig-note')).not.toBeNull()
    expect(screen.queryByRole('button', { name: /push to talk/i })).toBeNull()
    expect(document.querySelector('.cw-macros')).toBeNull()
  })

  it('the override wins over the radio, and Auto follows it back', () => {
    // The rig says USB; the operator says this position is running CW.
    const { rerender } = renderCockpit()
    expect(document.querySelector('.fd-modeclass-val')!.textContent).toBe('PH')
    setClass('CW')
    expect(document.querySelector('.fd-modeclass-val')!.textContent).toBe('CW')
    fireEvent.change(screen.getByLabelText('Position mode class'), { target: { value: '' } })
    rerender(
      <FdCockpit snap={makeSnap({ rigMode: 'PKTUSB' })} fieldDay={fieldDay} onSetMode={() => {}} />,
    )
    expect(document.querySelector('.fd-modeclass-val')!.textContent).toBe('DIG')
  })
})

describe('a tier flip is not a remount', () => {
  // HONEST SCOPE, measured rather than assumed (the CW twin's lesson): both columns are
  // ALWAYS rendered as static slots of one fragment, so dropping `key="lead"` / `key="boards"`
  // does NOT fail this test — position alone holds their identity today. What this DOES catch
  // is the region being rewritten as a tier ternary (CW and Phone both have one) without the
  // keys, which is the shape that wiped a half-typed QSO in fix-round D1.
  it('a 1↔2 flip keeps the entry strip and the boards mounted', async () => {
    renderCockpit()
    const region = document.querySelector('.cockpit-panes')!
    stubWidth(region, 1400)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('2')
    const entry0 = document.querySelector('.log-entry-fd')!
    const grid0 = document.querySelector('[data-pane="fdGrid"]')!
    const keyer0 = screen.getByTestId('keyer-stub')

    stubWidth(region, 900)
    act(() => fire!())
    await frame()
    expect(region.getAttribute('data-cols')).toBe('1')
    expect(document.querySelector('.log-entry-fd')!.isSameNode(entry0), 'entry strip remounted').toBe(true)
    expect(document.querySelector('[data-pane="fdGrid"]')!.isSameNode(grid0), 'grid remounted').toBe(true)
    expect(screen.getByTestId('keyer-stub').isSameNode(keyer0), 'mode pane remounted').toBe(true)

    stubWidth(region, 1400)
    act(() => fire!())
    await frame()
    expect(document.querySelector('.log-entry-fd')!.isSameNode(entry0), 'entry strip remounted on the way back').toBe(true)
    expect(document.querySelector('[data-pane="fdGrid"]')!.isSameNode(grid0), 'grid remounted on the way back').toBe(true)
  })
})
