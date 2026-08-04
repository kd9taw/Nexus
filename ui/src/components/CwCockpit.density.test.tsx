// @vitest-environment jsdom
//
// THE CW COCKPIT EXISTS FOR THE DECODE WINDOW, AND AT THE DEFAULT WINDOW IT HAD ~3 LINES.
//
// THE DEFECT. DECODE is declared `weight={3}` — the primary pane — but at the shipped default
// window the region is tier 2 (classifyRegionCols: 1200x750 at the fitter's 80% ⇒ region
// ~1470), which puts EVERY aux pane in DECODE's own column. Five `fit="content"` siblings are
// `flex: 0 0 auto`, i.e. unshrinkable, so the one grower in the column is the only child that
// can pay for the overflow: DECODE collapses to its `--cockpit-fill-min` floor and the column
// scrolls the rest. (Before d266d3aa it clipped instead. The scroller and the floor are that
// commit's; this file is about how much is left to scroll PAST.)
//
// WHAT THIS FILE PINS, and it is one number twice: the leading column's UNSHRINKABLE stack,
// and the transcript DECODE gets inside its floor. Everything this pass deletes is chrome that
// says what the pane frame already says once:
//   · SENT's `.cw-decode-head` — one span reading "SENT ▲" under a frame head reading "Sent".
//   · DECODE's `.cw-decode-head` — a row whose first span reads "DECODE" under a frame head
//     reading "Decode"; the rest of it (AI badge, WPM, the AI switch, status, Clear) moves to
//     the frame head's action cluster, which was rendering EMPTY.
//   · Two whole pane frames: the three rig-control strips are 3 x [head + body padding +
//     border + gap] wrapped around one row of chips each, so they merge into ONE "Rig
//     controls" frame whose body is one wrapping strip.
// NOT this pass's, but verified HERE because CW is its second host: `.pane-body > .bandstrip`
// (the card-in-card flatten landed by the Phone density pass, 8b38ff77). It is keyed on the
// FRAME, so it reaches CW's Band Activity too — and a frame-keyed rule is exactly the kind
// whose second host nobody checks. Its 34px is the Phone pass's win, not this file's: the
// baseline below is measured against 8b38ff77, with that flatten already in.
//
// HOW THIS GUARD COMPUTES. Same split as LogEntry.density.test.tsx, same engine
// (cssCascade.testkit):
//   · COMPUTED, live, from the real sheets through the real cascade on the real rendered tree:
//     every border / padding / margin / min-height / line box of every CHROME box this pass
//     touches, and the NUMBER of frames in the column. Those are the terms that move.
//   · CALIBRATED, and inert: PANE_CONTENT — the inner row each aux pane wraps around. This
//     pass changes none of them, so each is carried as one measured constant (the 2026-08-04
//     browser measurement of the pane's outer height at the default window, minus the chrome
//     computed here). Ballast: it cannot detect anything.
//
// WHAT THIS DOES NOT PROVE. No pixel here is verified against a layout engine, and the merged
// rig strip is asserted to be ONE row only in the sense that the DOM has one wrapping
// container — whether it actually wraps at the tier-2 column width is a browser fact. If it
// wraps to two rows the win is ~28px smaller than modelled and RIG_STRIP is the constant to
// re-measure. Re-measure it; do not nudge it.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { loadSheets, css, pxOf, borderY, padY, marginY, lineBox, fontSizeOf, atToken } from '../cssCascade.testkit'
import type { AppSnapshot } from '../types'

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: ['CQ CQ DE KD9TAW K'],
  keyerError: null as string | null,
  candidates: [{ call: 'W1AW', best: true }],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

vi.mock('../api', () => ({
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
  setSettings: vi.fn(async () => ({})),
  sendCw: vi.fn(async () => {}),
  setCwKeyer: vi.fn(async () => null),
  setCwWpm: vi.fn(async () => {}),
  stopCw: vi.fn(async () => {}),
  cwDecode: vi.fn(async () => decodeState),
  cwClear: vi.fn(async () => {}),
  setAiCw: vi.fn(async () => {}),
  selectPeer: vi.fn(async () => null),
  previewCw: vi.fn(async (t: string) => t),
  pointRotatorAtCall: vi.fn(async () => 0),
  setRigFunc: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  setTune: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  // RotorStrip is a real header child now that the header mock renders children.
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => {}),
  getDeclination: vi.fn(async () => 0),
  getSatTrackStatus: vi.fn(async () => null),
  stopSatTrack: vi.fn(async () => {}),
}))

// The header, the scope and the log strip are OTHER surfaces' density problems; BandStrip and
// the DSP rows are REAL here, because their own boxes are what this pass measures.
// The header renders its CHILDREN (the speed/keyer/pitch/BW cluster) but not its own chrome:
// the keyer selector's width is this pass's header item, and the rest of CockpitHeader is
// another surface's problem.
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: unknown }) => (
    <header className="cockpit-header">{children as never}</header>
  ),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

let fire: (() => void) | null = null
beforeAll(() => loadSheets())
beforeEach(() => {
  fire = null
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

function makeSnap(): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      agc: 'fast',
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
    },
    aiCw: { enabled: true, status: 'ready' },
  } as unknown as AppSnapshot
}

/** The cockpit at the SHIPPED DEFAULT WINDOW's tier: region ~1470 ⇒ data-cols 2, data-flow
 *  'fill'. Every conditional pane can render (CAT rig reporting DSP, a spots wire, something
 *  sent this session) — the loaded case, which is the one the operator complained about. */
async function renderAtDefaultWindow() {
  const r = render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  const region = document.querySelector('.cockpit-panes')!
  Object.defineProperty(region, 'clientWidth', { configurable: true, get: () => 1470 })
  act(() => fire!())
  await act(async () => {
    await new Promise((res) => requestAnimationFrame(() => res(null)))
  })
  expect(region.getAttribute('data-cols'), 'the default window is the 2-column tier').toBe('2')
  expect(region.getAttribute('data-flow')).toBe('fill')
  const cols = region.querySelectorAll(':scope > .cockpit-col')
  return { region, lead: cols[0], log: cols[cols.length - 1], ...r }
}

// ── the geometry model ──────────────────────────────────────────────────────────────────

/** The height of a ONE-LINE flex row: its own padding + border, over the tallest child. A leaf
 *  contributes a declared `height` if it has one (the 26px `.toggle` switch), else its own
 *  padding + border over one line box. */
function rowH(el: Element): number {
  const declared = css(el, 'height')
  if (declared && /^-?[\d.]+(px|em)$/.test(declared.trim())) return pxOf(el, 'height') + borderY(el)
  const kids = Array.from(el.children)
  // An EMPTY box is zero tall, not one line — the pane head's action cluster renders empty on
  // every CW pane but one, and charging it a line box would hide exactly the head this pass
  // fills. A leaf WITH text is one line box.
  const inner =
    kids.length > 0
      ? Math.max(...kids.map(rowH))
      : (el.textContent ?? '').trim().length > 0
        ? lineBox(el)
        : 0
  return padY(el) + borderY(el) + inner
}

/** A flex/grid box's row gap, through the same cascade. The sheet writes the `gap` shorthand,
 *  which jsdom will not expand while it carries a `var()`. */
function gapOf(el: Element): number {
  return pxOf(el, 'row-gap') || pxOf(el, 'gap')
}

/** Frame chrome: the border, the head row and the body's padding — everything a
 *  CockpitPaneFrame costs before any content of its own. */
function frameChrome(frame: Element): number {
  const head = frame.querySelector(':scope > .pane-head')!
  const body = frame.querySelector(':scope > .pane-body')!
  return borderY(frame) + rowH(head) + padY(body)
}

/** THE SHIPPED DEFAULT WINDOW (src-tauri/tauri.conf.json 1200x750 at the fitter's 80%): the
 *  pane region measures ~1470 x ~462 CSS px (2026-07-30 layout assessment scenario table). */
const TRACK_H = 462

/** CALIBRATED, and inert (see the header). The inner row each aux pane wraps around, measured
 *  in the browser at the default window on 2026-08-04 and reduced by the chrome computed here.
 *  Keyed by `data-pane`; a pane with no entry contributes only what this file computes. */
const PANE_CONTENT: Record<string, number> = {
  // NB / NR / Notch chip row.
  dsp: 27,
  // The NR slider + the Fast/Mid/Slow chips, one wrapping row.
  rxdsp: 31,
  // BandStrip MINUS its own card: the head row, the 34px track and the axis line.
  bandActivity: 78,
  // One row of decoded-call chips behind its "Heard" label.
  copilot: 33,
}
/** The merged rig strip, once the three control groups share ONE frame and ONE wrapping row:
 *  the tallest of them, not their sum. Calibrated from `rxdsp` above, the tallest of the three
 *  (a range input is taller than a chip). */
const RIG_STRIP = 31

/** The unshrinkable (`fit="content"`) height of one frame in the leading column, gap included
 *  — what DECODE cannot take back. */
function fixedFrameH(frame: Element, gap: number): number {
  const id = (frame as HTMLElement).dataset.pane ?? ''
  const body = frame.querySelector(':scope > .pane-body')!
  let inner = PANE_CONTENT[id] ?? 0
  // SENT is floored, not content-sized: `.cw-decode-text` is `flex: 1 1 0` inside a
  // content-height flex column, so it resolves to ZERO and the panel's own min-height is
  // the whole of the box (the `.pane-body > .cw-decode` note says exactly this). The FLOOR
  // is therefore the term, and it is computed.
  const panel = body.querySelector(':scope > .cw-decode')
  if (panel) inner = Math.max(pxOf(panel, 'min-height'), rowH(panel))
  // The merged rig frame carries the strip, not a per-group sum.
  if (id === 'rigctl') inner = RIG_STRIP + padY(body.querySelector(':scope > *')!)
  // A pane that draws its OWN card inside the frame's card pays for it twice.
  const card = body.querySelector(':scope > .bandstrip')
  if (card) inner += marginY(card) + padY(card) + borderY(card)
  return frameChrome(frame) + inner + gap
}

/** Everything in the leading column that DECODE cannot shrink, and what is left for DECODE. */
function columnBudget(lead: Element) {
  const frames = Array.from(lead.querySelectorAll(':scope > .pane-frame'))
  const decode = frames.find((f) => (f as HTMLElement).dataset.pane === 'decode')!
  // DECODE's fill floor, read off the region the frame is actually inside.
  const region = lead.closest('.cockpit-panes')!
  const gap = gapOf(region)
  expect(gap, 'the region declares no gap — every frame would be modelled flush').toBeGreaterThan(0)
  const fixed = frames
    .filter((f) => f !== decode)
    .reduce((n, f) => n + fixedFrameH(f, gap), 0)
  const floorRaw = css(region, '--cockpit-fill-min')
  expect(floorRaw, 'the region declares no --cockpit-fill-min — DECODE has no floor').toBeTruthy()
  const floor = Math.min(
    ...floorRaw!
      .replace(/^min\(|\)$/g, '')
      .split(',')
      .map((a) => a.trim())
      .filter((a) => /^[\d.]+(px|em)$/.test(a))
      .map((a) => (a.endsWith('em') ? parseFloat(a) * fontSizeOf(decode) : parseFloat(a))),
  )
  const share = Math.max(floor, TRACK_H - fixed)
  // What is actually READABLE inside DECODE: its share, less the frame's own chrome, less any
  // head row still rendering inside its body, less the panel's padding.
  const body = decode.querySelector(':scope > .pane-body')!
  const panel = body.querySelector(':scope > .cw-decode')!
  const innerHead = panel.querySelector(':scope > .cw-decode-head')
  const transcript =
    share - frameChrome(decode) - padY(panel) - (innerHead ? rowH(innerHead) + gapOf(panel) : 0)
  return { frames, fixed, floor, share, transcript, deficit: Math.max(0, fixed + share - TRACK_H) }
}

// ── the guards ──────────────────────────────────────────────────────────────────────────

describe('the CW decode window is readable at the shipped default window', () => {
  it('the leading column leaves DECODE more than four lines of transcript', async () => {
    const { lead } = await renderAtDefaultWindow()
    const b = columnBudget(lead)
    const line = lineBox(document.querySelector('.cw-decode-text')!)
    const lines = Math.floor(b.transcript / line)
    expect(
      lines,
      `DECODE shows ${lines} lines (${b.transcript}px of ${line}px line boxes). Its column ` +
        `carries ${b.fixed}px of unshrinkable siblings in a ${TRACK_H}px track, so DECODE is ` +
        `pinned at its ${b.floor}px floor and the column scrolls ${b.deficit}px. Frames in the ` +
        `column: ${b.frames.map((f) => (f as HTMLElement).dataset.pane).join(', ')}`,
    ).toBeGreaterThanOrEqual(4)
  })

  it('the unshrinkable stack fits close enough that ONE column scroll reaches the end', async () => {
    // NOT "no deficit", and that is the honest limit of this pass: with all six panes ticked,
    // chrome deletion alone cannot fit ~400px of control strips plus a readable transcript in
    // a 462px track. Closing it needs a pane to LEAVE the column, and the only column with a
    // spare track is the log column, which has none — LogEntry.density.test.tsx pins its
    // commit row at 379px inside a ~392px body, so moving Band Activity (130px) or Copilot
    // (89px) there puts the Log button back below the fold, which is the defect 4433aac2 was
    // written to fix. See the report's `deferred`.
    //
    // The bar is therefore: what is left to scroll past is under ~a quarter of the track —
    // one short drag, not two screens. 84px computed here. The 120px ceiling leaves room for
    // one --space-scale step and still fires if a whole pane frame (~56px) comes back.
    const { lead } = await renderAtDefaultWindow()
    const b = columnBudget(lead)
    expect(
      b.deficit,
      `the leading column overflows its track by ${b.deficit}px (${b.fixed}px of unshrinkable ` +
        `siblings + DECODE's ${b.share}px against a ${TRACK_H}px track)`,
    ).toBeLessThanOrEqual(120)
  })

  it('no font was made smaller, and the win survives the tightened viewports', async () => {
    // THE OPERATOR THIS IS FOR is on 1024x768 or a pinned 175%, where [data-viewport] tightens
    // --space-scale. A win that came out of token spacing would evaporate there; this one is
    // made of deleted BOXES and whole deleted FRAMES, which no spacing token can give back.
    const { lead } = await renderAtDefaultWindow()
    const text = document.querySelector('.cw-decode-text')!
    expect(pxOf(text, 'font-size'), 'the decode transcript type changed').toBe(14)
    expect(pxOf(document.querySelector('.pane-title')!, 'font-size'), 'the pane head type changed').toBe(11)
    for (const scale of ['1', '0.94', '0.86']) {
      const b = atToken('--space-scale', scale, () => columnBudget(lead))
      expect(
        Math.floor(b.transcript / lineBox(text)),
        `at --space-scale ${scale} DECODE is back under four lines — the win evaporated into ` +
          'token spacing, which is the failure this walk exists to catch',
      ).toBeGreaterThanOrEqual(4)
    }
  })
})

describe('the narrow (stacking) tier — 1024x768 and any pinned zoom', () => {
  // BELOW ~1080 the region measures narrow, flow is 'stack', and `--cockpit-pane-flex: 0 0 auto`
  // makes EVERY pane content-height: the fill floor does not apply and each transcript's box is
  // its own `.pane-body > *` min-height. That is the supported floor's layout and the pinned-175%
  // layout, so the two floors' RELATIVE size is what decides whether the echo out-reserves the
  // transcript there. It used to be a tie — both 6em — which is the wrong answer for a pane
  // that is empty at every session start beside the pane the cockpit exists for.
  it('the region stacks, and the SENT echo reserves LESS than DECODE', async () => {
    const r = render(<CwCockpit snap={makeSnap()} theme="dark" onWorkSpot={() => {}} spots={[]} />)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    const region = document.querySelector('.cockpit-panes')!
    Object.defineProperty(region, 'clientWidth', { configurable: true, get: () => 1000 })
    act(() => fire!())
    await act(async () => {
      await new Promise((res) => requestAnimationFrame(() => res(null)))
    })
    expect(region.getAttribute('data-flow'), 'a 1000px region is the narrow tier').toBe('stack')

    const decodePanel = document.querySelector('[data-pane="decode"] .cw-decode')!
    const sentPanel = document.querySelector('[data-pane="sent"] .cw-sent-panel')!
    const line = lineBox(document.querySelector('.cw-decode-text')!)
    const decodeFloor = pxOf(decodePanel, 'min-height')
    const sentFloor = pxOf(sentPanel, 'min-height')
    expect(
      Math.floor(decodeFloor / line),
      'DECODE is under four lines at the supported floor',
    ).toBeGreaterThanOrEqual(4)
    expect(
      sentFloor,
      `the echo reserves ${sentFloor}px where the transcript reserves ${decodeFloor}px — a pane ` +
        'that is empty at every session start must not stand as tall as the one being read',
    ).toBeLessThan(decodeFloor)
    r.unmount()
  })
})

describe('the deleted chrome said nothing the frame does not say', () => {
  it('SENT renders no head — the frame head names it, and the accent stripe still marks it', async () => {
    await renderAtDefaultWindow()
    const sent = document.querySelector('[data-pane="sent"]')!
    expect(sent.querySelector('.cw-decode-head'), 'the "SENT ▲" span came back').toBeNull()
    expect(sent.querySelector('.pane-title')!.textContent).toBe('Sent')
    // The stripe is what makes "these are YOUR transmissions" readable at a glance; the head
    // label was the redundant half. Computed, because `.pane-body > .panel` flattens the card
    // and a stripe rule that merely EXISTS can lose to it.
    const panel = sent.querySelector('.cw-sent-panel')!
    expect(pxOf(panel, 'border-left-width'), 'the SENT accent stripe was flattened away').toBe(3)
  })

  it('SENT stays a glanceable strip: still ~3 lines, for 28px less than it cost', async () => {
    // THE ROLE QUESTION, answered as a number. The echo keeps fit="content" (it is a strip an
    // operator glances at to confirm which call a macro expanded to, empty at every session
    // start, with its own scroller for history) — NOT a weight-1 fill, which would floor it at
    // `--cockpit-fill-min` = 140px, MORE than the 6em it had, taking from DECODE rather than
    // sharing with it. What paid for the lower floor is the deleted head row, so the visible
    // line count must not have moved.
    const { lead } = await renderAtDefaultWindow()
    const sent = lead.querySelector('[data-pane="sent"]')!
    const panel = sent.querySelector('.cw-sent-panel')!
    const line = lineBox(sent.querySelector('.cw-decode-text')!)
    const visible = Math.floor((pxOf(panel, 'min-height') - padY(panel)) / line)
    expect(visible, 'the echo was cut below a glance').toBeGreaterThanOrEqual(2)
    const gap = pxOf(lead.closest('.cockpit-panes')!, 'row-gap') || pxOf(lead.closest('.cockpit-panes')!, 'gap')
    const outer = frameChrome(sent) + pxOf(panel, 'min-height') + gap
    expect(
      outer,
      `the SENT echo costs ${outer}px of DECODE's column for ${visible} visible lines`,
    ).toBeLessThanOrEqual(120)
  })

  it('DECODE renders no head row — its controls are in the frame head that was empty', async () => {
    await renderAtDefaultWindow()
    const decode = document.querySelector('[data-pane="decode"]')!
    expect(decode.querySelector('.pane-body .cw-decode-head'), 'the DECODE head row came back').toBeNull()
    expect(decode.querySelector('.pane-title')!.textContent).toBe('Decode')
    const acts = decode.querySelector('.pane-head .cockpit-pane-acts')!
    // Every control that was in the deleted row is in the head, by the handle the operator
    // reaches for: the AI switch, the WPM readout and Clear.
    expect(acts.querySelector('[role="switch"]'), 'the AI decoder switch was lost').not.toBeNull()
    expect(acts.querySelector('.cw-decode-clear'), 'Clear was lost').not.toBeNull()
    expect(acts.textContent, 'the WPM readout was lost').toContain('WPM')
  })

  it('the three rig-control strips share ONE frame, and every control survived', async () => {
    await renderAtDefaultWindow()
    expect(document.querySelectorAll('[data-pane="dsp"]').length, 'DSP still has its own frame').toBe(0)
    expect(document.querySelectorAll('[data-pane="rxdsp"]').length).toBe(0)
    const rig = document.querySelector('[data-pane="rigctl"]')!
    expect(rig, 'no merged rig-controls frame').not.toBeNull()
    expect(rig.querySelector('.pane-title')!.textContent).toBe('Rig controls')
    // The groups keep their own labels — merged rows are only readable because each says what
    // it is — and their accessible names, which is how the ⊞ menu's three entries stay honest.
    expect(rig.querySelector('[aria-label="Rig DSP functions"]'), 'the DSP toggles vanished').not.toBeNull()
    expect(rig.querySelector('[aria-label="RX DSP levels"]'), 'the RX DSP levels vanished').not.toBeNull()
    expect(rig.querySelector('[aria-label="Noise-reduction level"]')).not.toBeNull()
    expect(rig.querySelector('[aria-label="AGC speed"]')).not.toBeNull()
  })

  it('the keyer back-end is one select, and all four back-ends keep their whole explanation', async () => {
    // THE WIDTH ITEM. Four always-visible option buttons stood ~292px wide in a header that
    // also carries the band picker, tuning strip, Tune, Stop TX, speed, pitch, macros, BW,
    // memories and the rotator — and a header that wraps at 1024 is what pushes Stop TX
    // toward the shell's scrolling valve.
    await renderAtDefaultWindow()
    expect(document.querySelectorAll('.cw-keyer-opt').length, 'the four option buttons came back').toBe(0)
    const sel = document.querySelector('.cw-keyer-select') as HTMLSelectElement
    expect(sel, 'no keyer selector in the header').not.toBeNull()
    expect(Array.from(sel.options).map((o) => o.value)).toEqual(['cat', 'serial', 'winkeyer', 'soundcard'])
    expect(sel.value, 'the selector does not show the back-end actually in use').toBe('cat')

    // RULING: A DELETED HINT'S INFORMATION MUST SURVIVE, and here it is not decoration —
    // the soundcard entry is what stops an operator keying a tone through SSB with nothing
    // routed and the drive over ALC. Every option carries its own sentence, and the SELECTED
    // back-end's sentence is on the select itself, so it is one hover away while operating.
    for (const o of Array.from(sel.options)) {
      expect(o.title.length, `the ${o.value} back-end lost its explanation`).toBeGreaterThan(40)
    }
    expect(Array.from(sel.options).find((o) => o.value === 'soundcard')!.title).toMatch(/below ALC/)
    expect(sel.parentElement!.getAttribute('title'), 'the active back-end has no explanation').toBe(
      Array.from(sel.options).find((o) => o.value === sel.value)!.title,
    )
  })

  it("the shipped BandStrip flatten reaches CW's host too, and no further", async () => {
    // NOT this pass's rule (Phone's density pass landed it); this is the PER-HOST check a
    // frame-keyed rule needs and its author's own suite cannot make.
    await renderAtDefaultWindow()
    const strip = document.querySelector('[data-pane="bandActivity"] .bandstrip')!
    // Computed, not matched: `.bandstrip` declares margin+padding+border two thousand lines
    // earlier, so a flatten that merely exists can lose the cascade.
    expect(marginY(strip), '.bandstrip keeps its own margin inside a pane body').toBe(0)
    expect(padY(strip), '.bandstrip keeps its own padding inside a pane body').toBe(0)
    expect(borderY(strip), '.bandstrip keeps its own border inside a pane body').toBe(0)
    // …and UNFRAMED it still draws the card that is then its only frame.
    cleanup()
    const bare = document.createElement('div')
    bare.className = 'bandstrip'
    document.body.appendChild(bare)
    expect(padY(bare), 'the flatten leaked outside .pane-body').toBeGreaterThan(0)
    bare.remove()
  })
})
