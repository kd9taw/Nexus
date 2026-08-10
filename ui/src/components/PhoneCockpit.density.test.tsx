// @vitest-environment jsdom
//
// THE PHONE COCKPIT'S LEADING COLUMN IS A CONTROL COLUMN, NOT A SCROLLER.
//
// THE DEFECT (low-res sweep, 2026-08-04). At the shipped default window (1200x720) the
// leading column of Phone's pane region is FIVE fit="content" frames — Band activity, the
// voice keyer, and the rig-scope / DSP / RX-DSP-levels strips — standing ~673px inside a
// ~436px track. Nothing is trapped since `.cockpit-col` gained its scroller, but the
// operator scrolls a CONTROL column during a contact to reach the DSP buttons.
//
// WHAT THIS FILE PINS. Two of those five panes draw a bordered, padded card INSIDE the pane
// frame's card, and one of them carries a permanent paragraph:
//   1. `.bandstrip` — margin + padding + border inside `.pane-body`. Neither `.bandstrip`
//      nor `.vk` carries a `panel` class, so the shipped `.pane-body > .panel` flatten
//      misses both; the same miss `.pane-body > .rtty-stream` and `.pane-body > .log-entry`
//      already document. 34px.
//   2. `.vk` — the same card, 26px.
//   3. `.vk-note` — the "● records from your INPUT DEVICE" paragraph, two lines of the
//      pane at every window. Its warning is NOT chrome (an operator who records the rig's
//      RX audio into a slot puts the wrong audio on the air), so it may only leave the pane
//      if it lands on the controls that start a recording — which is what is asserted here
//      rather than the deletion alone.
// And in the header, three labels that repeat the control beside them: '● Record QSO' (the
// glyph is the control), 'RX' beside a `role="meter"` that is already named "RX audio
// level", and 'Colors' beside a <select> already named "Waterfall color palette". Those are
// WIDTH, in a header region that wraps — no vertical claim is made for them here.
//
// HOW THIS GUARD COMPUTES. Same machinery, and the same honesty, as
// LogEntry.density.test.tsx — read that file's header for the full argument. In short: jsdom
// lays nothing out, so the stack is SUMMED from the real sheet through the real cascade on
// the real rendered tree (selector matching is jsdom's `Element.matches`; the cascade is
// resolved here because getComputedStyle neither substitutes `var()` nor expands a
// shorthand carrying one). The machinery is duplicated rather than shared: this is its
// second use, LogEntry's shipped guard stays byte-identical, and a third use should extract
// it.
//
// WHAT THIS DOES NOT PROVE. No pixel here is verified against a layout engine. Two terms are
// measured constants and are inert — LEAD_REST (the run of boxes this pass does not touch)
// and NOTE_LINES (how many lines `.vk-note` wrapped to at the leading column's width). If
// the column's width changes, re-measure them; do not nudge them.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { BandStrip } from './BandStrip'
import { VoiceKeyer } from './VoiceKeyer'
import { PhoneCockpit } from './PhoneCockpit'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  setPtt: vi.fn(async () => {}),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
  // Slot 1 already recorded, 2–6 empty: both record entry points are on screen (the ● tool
  // renders for every slot; the slot button itself offers to record only while empty).
  getVoiceMessages: vi.fn(async () => [
    { slot: 1, label: 'CQ', file: '/tmp/f1.wav' },
    { slot: 2, label: '', file: null },
    { slot: 3, label: '', file: null },
    { slot: 4, label: '', file: null },
    { slot: 5, label: '', file: null },
    { slot: 6, label: '', file: null },
  ]),
  playVoiceMessage: vi.fn(async () => ({})),
  stopVoice: vi.fn(async () => ({})),
  startVoiceRecording: vi.fn(async () => ({})),
  stopVoiceRecording: vi.fn(async () => []),
  cancelVoiceRecording: vi.fn(async () => ({})),
  clearVoiceMessage: vi.fn(async () => []),
  importVoiceMessage: vi.fn(async () => []),
  pointRotatorAtCall: vi.fn(async () => 0),
  // The real CockpitHeader hosts RotorStrip, which polls these on mount.
  readRotator: vi.fn(async () => null),
  stopRotator: vi.fn(async () => ({})),
  getDeclination: vi.fn(async () => 0),
  getSatTrackStatus: vi.fn(async () => null),
  getSatTransponder: vi.fn(async () => null),
  setSatTransponder: vi.fn(async () => {}),
  stopSatTrack: vi.fn(async () => ({})),
  getLicensedBandPlan: vi.fn(async () => []),
  getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
  setSettings: vi.fn(async () => ({})),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas/scope children and the log strip only. CockpitHeader, BandStrip and VoiceKeyer are
// DELIBERATELY REAL — they are what this file measures.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

// ── the sheet, and the cascade over it ──────────────────────────────────────────────────

const FLAT: { rule: CSSStyleRule; order: number }[] = []
/** `:root` custom properties, resolved to px at --space-scale: 1 (the default viewport). */
const TOKENS = new Map<string, string>()

function srcFile(rel: string): string {
  return readFileSync(resolve(process.cwd(), 'src', rel), 'utf8')
}

beforeAll(() => {
  for (const sheet of ['styles.css', 'cockpit-panes.css']) {
    const style = document.createElement('style')
    style.textContent = srcFile(sheet)
    document.head.appendChild(style)
    const walk = (rules: CSSRuleList) => {
      for (let i = 0; i < rules.length; i++) {
        const r = rules[i]
        if ((r as CSSStyleRule).selectorText) FLAT.push({ rule: r as CSSStyleRule, order: FLAT.length })
        else if ((r as CSSGroupingRule).cssRules) walk((r as CSSGroupingRule).cssRules)
      }
    }
    walk(style.sheet!.cssRules)
  }
  for (const { rule } of FLAT) {
    if (rule.selectorText !== ':root') continue
    for (let i = 0; i < rule.style.length; i++) {
      const name = rule.style[i]
      if (name.startsWith('--') && !TOKENS.has(name)) TOKENS.set(name, rule.style.getPropertyValue(name).trim())
    }
  }
})

afterEach(cleanup)

/** Specificity as one comparable number: (ids, classes+attrs+pseudo-classes, elements). */
function spec(sel: string): number {
  const s = sel.replace(/:where\([^)]*\)/g, '')
  const ids = (s.match(/#[\w-]+/g) ?? []).length
  const cls = (s.match(/\.[\w-]+|\[[^\]]+\]|:[a-z-]+(?:\([^)]*\))?/g) ?? []).filter(
    (t) => !t.startsWith('::'),
  ).length
  const el = (s.match(/(?:^|[\s>+~(])([a-z][\w-]*)/g) ?? []).length
  return ids * 10000 + cls * 100 + el
}

/** Substitute `var()` and evaluate the sheet's `calc(<n>px * <n>)` form. */
function resolveVal(raw: string, depth = 0): string {
  if (depth > 8) return raw
  let v = raw.trim()
  v = v.replace(/var\(\s*(--[\w-]+)\s*(?:,\s*([^)]*))?\)/g, (_m, name, fallback) =>
    resolveVal(TOKENS.get(name) ?? fallback ?? '0', depth + 1),
  )
  v = v.replace(/calc\(\s*([\d.]+)px\s*\*\s*([\d.]+)\s*\)/g, (_m, a, b) => `${Number(a) * Number(b)}px`)
  return v.trim()
}

/** Split a resolved box shorthand into its four sides (CSS 1/2/3/4-value form). */
const SIDES = ['top', 'right', 'bottom', 'left'] as const
type Side = (typeof SIDES)[number]
function sideOf(shorthand: string, side: Side): string {
  const p = shorthand.split(/\s+/).filter(Boolean)
  if (p.length === 0) return '0'
  const four = p.length === 1 ? [p[0], p[0], p[0], p[0]]
    : p.length === 2 ? [p[0], p[1], p[0], p[1]]
      : p.length === 3 ? [p[0], p[1], p[2], p[1]]
        : p.slice(0, 4)
  return four[SIDES.indexOf(side)]
}

/** The value one RULE contributes for `prop`, falling back to the shorthand it lives in. */
function fromRule(style: CSSStyleDeclaration, prop: string): string | null {
  const direct = style.getPropertyValue(prop)
  if (direct) return direct
  const box = /^(padding|margin)-(top|right|bottom|left)$/.exec(prop)
  if (box) {
    const sh = style.getPropertyValue(box[1])
    return sh ? sideOf(resolveVal(sh), box[2] as Side) : null
  }
  const bw = /^border-(top|bottom)-width$/.exec(prop)
  if (bw) {
    for (const sh of [style.getPropertyValue(`border-${bw[1]}`), style.getPropertyValue('border')]) {
      if (!sh) continue
      const v = resolveVal(sh)
      if (/\bnone\b/.test(v)) return '0px'
      const m = /(^|\s)(-?[\d.]+)(px)?(\s|$)/.exec(v)
      return m ? `${m[2]}px` : '0px'
    }
  }
  return null
}

/** The winning declaration of `prop` for a RENDERED element. */
function css(el: Element, prop: string): string | null {
  let win: { value: string; important: boolean; spec: number; order: number } | null = null
  for (const { rule, order } of FLAT) {
    let matches = false
    try {
      matches = el.matches(rule.selectorText)
    } catch {
      continue
    }
    if (!matches) continue
    const value = fromRule(rule.style, prop)
    if (!value) continue
    const important = rule.style.getPropertyPriority(prop) === 'important'
    const s = spec(rule.selectorText)
    const better =
      win === null ||
      (important && !win.important) ||
      (important === win.important && (s > win.spec || (s === win.spec && order >= win.order)))
    if (better) win = { value, important, spec: s, order }
  }
  return win ? resolveVal(win.value) : null
}

/** `css` as a px number; 0 when the property is not declared anywhere that matches. */
function pxOf(el: Element, prop: string): number {
  const v = css(el, prop)
  if (!v) return 0
  const m = /^(-?[\d.]+)(px)?$/.exec(v.trim())
  return m ? Number(m[1]) : 0
}

function borderY(el: Element): number {
  return pxOf(el, 'border-top-width') + pxOf(el, 'border-bottom-width')
}
function paddingY(el: Element): number {
  return pxOf(el, 'padding-top') + pxOf(el, 'padding-bottom')
}
function marginY(el: Element): number {
  return pxOf(el, 'margin-top') + pxOf(el, 'margin-bottom')
}
/** The vertical box a block's own card costs its container. */
function cardY(el: Element): number {
  return marginY(el) + paddingY(el) + borderY(el)
}
/** One line box of an element, from its resolved font-size + line-height. */
function lineBox(el: Element): number {
  const fs = pxOf(el, 'font-size') || 16
  const lhRaw = css(el, 'line-height')
  const lh = lhRaw ? (/px$/.test(lhRaw) ? Number(lhRaw.replace('px', '')) : Number(lhRaw) * fs) : fs * 1.2
  return lh
}

// ── the fixtures ────────────────────────────────────────────────────────────────────────

const snap = {
  mycall: 'KD9TAW',
  radio: {
    dialMhz: 14.2,
    band: '20m',
    catOk: true,
    sideband: 'USB',
    sidebandOverride: null,
    rigMode: 'USB',
    transmitting: false,
    tuning: false,
    txEnabled: true,
    txAllowed: true,
    qsoRecording: false,
    rfPower: null,
    micGain: null,
    nrLevel: 0.3,
    agc: 'fast',
    nb: true,
    nr: true,
    notch: null,
    comp: null,
    vox: null,
    filterWidthHz: 500,
    splitTxMhz: null,
    smeterDb: null,
    phoneSegLo: null,
    phoneSegHi: null,
  },
} as unknown as AppSnapshot

/** Band activity exactly as both cockpits frame it. */
function renderFramedBandStrip() {
  const { container } = render(
    <CockpitPaneFrame title="Band activity" paneId="bandActivity" fit="content">
      <BandStrip band="20m" dialMhz={14.2} txAllowed spots={[]} onWorkSpot={() => {}} />
    </CockpitPaneFrame>,
  )
  return { container, strip: container.querySelector('.bandstrip')! }
}

/** The voice keyer exactly as Phone frames it. Async: the slot list arrives from
 *  getVoiceMessages in an effect, and the slots are half of what this file measures. */
async function renderFramedKeyer() {
  const { container } = render(
    <CockpitPaneFrame title="Voice keyer" paneId="voiceKeyer" fit="content">
      <VoiceKeyer txEnabled keyed={false} transmitting={false} />
    </CockpitPaneFrame>,
  )
  await act(async () => {})
  return { container, vk: container.querySelector('.vk')! }
}

/** The same two blocks OUTSIDE a pane frame — what the base rules still declare, and the
 *  only way to read the chrome the framed hosts stopped paying. */
function renderBareBandStrip() {
  const { container } = render(
    <div>
      <BandStrip band="20m" dialMhz={14.2} txAllowed spots={[]} onWorkSpot={() => {}} />
    </div>,
  )
  return container.querySelector('.bandstrip')!
}
async function renderBareKeyer() {
  const { container } = render(
    <div>
      <VoiceKeyer txEnabled keyed={false} transmitting={false} />
    </div>,
  )
  await act(async () => {})
  return container.querySelector('.vk')!
}

function renderCockpit() {
  return render(<PhoneCockpit snap={snap} theme="dark" onWorkSpot={() => {}} />)
}

// ── the stack ───────────────────────────────────────────────────────────────────────────

/** THE SHIPPED DEFAULT WINDOW is 1200x720 (src-tauri/tauri.conf.json). At that window Phone's
 *  region is 2-col and the LEADING column's track measured 436px while the five
 *  fit="content" frames in it stood 673px (low-res design pass, 2026-08-04). */
const LEAD_TRACK_H = 436

/** MEASURED + CALIBRATED, and inert (see the header): everything in that 673px this pass
 *  does not touch — the five pane frames' own head/body chrome, the grid gaps, the spot
 *  track, the F1–F6 grid, the DSP/NR/AGC rows. Derived by subtracting the computed terms
 *  below (100.8px, measured on the pre-pass tree) from the measured 673. It is ballast: it
 *  cannot detect anything. */
const LEAD_REST = 572

/** MEASURED + CALIBRATED, and the one computed term that depends on a wrap: `.vk-note` is
 *  ~140 characters at --fs-micro in a ~390px inner column, i.e. two lines. */
const NOTE_LINES = 2

/** The vertical chrome this pass takes out of the leading column, computed on the framed
 *  tree — 0 once the flattens win and the paragraph is gone. */
async function leadChrome(): Promise<number> {
  const { strip } = renderFramedBandStrip()
  const band = cardY(strip)
  cleanup()
  const { vk } = await renderFramedKeyer()
  const keyerCard = cardY(vk)
  const note = vk.querySelector('.vk-note')
  const noteBox = note ? NOTE_LINES * lineBox(note) + marginY(note) : 0
  cleanup()
  return band + keyerCard + noteBox
}

describe('the Phone leading column stops scrolling its own chrome', () => {
  // 673 pre-pass, 572 after. The ceiling is 580 so ONE more --space-scale step is not a
  // false alarm, while the SMALLEST box this pass removes (the keyer's 26px card) coming
  // back still fires. It is not a target to tune toward.
  it('stands no taller than 580px at the shipped default window', async () => {
    const chrome = await leadChrome()
    const stack = LEAD_REST + chrome
    expect(
      stack,
      `the leading column stands ${stack}px in a ${LEAD_TRACK_H}px track — ` +
        `${stack - LEAD_TRACK_H}px of it scrolled past the operator mid-contact. ` +
        `${chrome}px of that is card-in-card + a permanent paragraph this pass removes.`,
    ).toBeLessThanOrEqual(580)
  })

  it('the card-in-card flatten WINS for a .bandstrip inside a .pane-body', () => {
    // Computed, not matched: a flatten that merely EXISTS can lose the cascade. Assert the
    // resolved winner on the real framed element.
    const { strip } = renderFramedBandStrip()
    expect(marginY(strip), '.bandstrip doubles its margin against the pane body padding').toBe(0)
    expect(paddingY(strip), '.bandstrip keeps card padding inside a pane frame').toBe(0)
    expect(borderY(strip), '.bandstrip keeps a card border inside a pane frame').toBe(0)
  })

  it('the card-in-card flatten WINS for a .vk inside a .pane-body', async () => {
    const { vk } = await renderFramedKeyer()
    expect(paddingY(vk), '.vk keeps card padding inside a pane frame').toBe(0)
    expect(borderY(vk), '.vk keeps a card border inside a pane frame').toBe(0)
  })
})

describe('what the deleted chrome said is still said', () => {
  it('the framed band strip renders no title — the frame head names the surface', () => {
    const { container } = renderFramedBandStrip()
    expect(
      container.querySelector('.bandstrip-title'),
      'the strip says "Band activity" two lines under a frame head that says it already',
    ).toBeNull()
    expect(container.querySelector('.pane-frame')!.getAttribute('aria-label')).toBe('Band activity')
    expect(container.querySelector('.pane-title')!.textContent).toBe('Band activity')
    // The live count is NOT chrome and stays: it is the only thing that says how many spots
    // are on the band and how fresh the strip is.
    expect(container.querySelector('.bandstrip-count')!.textContent).toMatch(/20m/)
  })

  it('the input-device warning is on both controls that start a recording', async () => {
    // RULING (2026-08-04): a hint may lose its row, never its sentence. Recording the rig's
    // RX audio into a slot puts the wrong audio on the air, so the warning has to reach the
    // operator at the moment he starts a recording — and there are TWO ways to start one:
    // the ● tool, and the slot button itself while the slot is empty. Matched on titles that
    // OFFER a recording ("Record F3…"), deliberately not on every title containing the word:
    // "Clear this recording" must not be made to carry it.
    const { vk } = await renderFramedKeyer()
    expect(vk.querySelector('.vk-note'), 'the note still owns two lines of the pane').toBeNull()
    const offers = [...vk.querySelectorAll('.vk-tool, .vk-play')]
      .map((b) => ({ cls: b.className, title: b.getAttribute('title') ?? '' }))
      .filter((b) => /^Record\b/.test(b.title))
    // Both kinds, not just one: the fixture has an empty slot (a `.vk-play` that records)
    // and every slot has its ● `.vk-tool`.
    expect(offers.some((o) => o.cls.includes('vk-play')), 'no empty slot offers to record').toBe(true)
    expect(offers.some((o) => o.cls.includes('vk-tool')), 'no ● tool offers to record').toBe(true)
    for (const { title } of offers) {
      expect(title, `"${title}" lost the input-device warning`).toMatch(/input device/i)
      expect(title, `"${title}" lost the way out (Import)`).toMatch(/import/i)
    }
  })

  it('the header labels that repeat their own control are gone, and nothing lost its name', () => {
    const { container } = renderCockpit()
    // Record is the ONE control in this header that is not self-describing: a bare ● names
    // nothing, and this assertion used to require exactly that. Stripped to the glyph by this
    // density pass, it was reported as MISSING and then as "hardly visible for anyone"
    // (operator, 2026-08-08). It keeps a visible label, and the glyph is aria-hidden so the
    // accessible name stays the explicit one rather than the bullet.
    const rec = container.querySelector('.ph-rec')!
    expect(rec.textContent, 'the Record button lost its visible label and is a bare glyph again')
      .toMatch(/REC/)
    expect(
      rec.querySelector('.ph-rec-dot')!.getAttribute('aria-hidden'),
      'the record dot is read out as well as the label',
    ).toBe('true')
    expect(
      rec.getAttribute('aria-label'),
      'the Record button lost the only name it had',
    ).toMatch(/record/i)

    const meter = container.querySelector('.ph-rxmeter')!
    expect(meter.textContent, 'the RX meter still labels a control that names itself').toBe('')
    expect(
      meter.querySelector('[role="meter"]')!.getAttribute('aria-label'),
      'the RX meter lost its accessible name',
    ).toMatch(/rx/i)

    expect(
      container.querySelector('.ph-scope-head .ph-scope-head-label'),
      'the scope head still labels a <select> that names itself',
    ).toBeNull()
    expect(
      container.querySelector('.ph-scope-head select')!.getAttribute('aria-label'),
      'the palette picker lost its accessible name',
    ).toMatch(/palette/i)
  })
})

describe('the recovered chrome survives the small viewports', () => {
  // THE OPERATOR THIS IS FOR is on 1024x768 (the supported floor) or a pinned 175%, where
  // `[data-viewport]` tightens --space-scale — and a density win that came only out of token
  // spacing would shrink with it. Computed at each shipped scale on the BASE rules, i.e. the
  // chrome the framed hosts stopped paying. (useViewport.classifyViewport, effective width:
  // 1.00 md/lg/xl · 0.94 sm — 1024x768 @100% · 0.86 xs — 1024x768 @175%.)
  const SCALES: [number, string][] = [
    [1, 'md/lg/xl'],
    [0.94, 'sm'],
    [0.86, 'xs'],
  ]

  function atScale<T>(scale: number, fn: () => T): T {
    const prev = TOKENS.get('--space-scale')
    TOKENS.set('--space-scale', String(scale))
    try {
      return fn()
    } finally {
      if (prev === undefined) TOKENS.delete('--space-scale')
      else TOKENS.set('--space-scale', prev)
    }
  }

  it.each(SCALES)('at --space-scale %s (%s) the framed panes pay none of it', async (scale, cls) => {
    const framedBand = renderFramedBandStrip()
    atScale(scale, () => {
      expect(framedBand.strip.querySelector('.bandstrip-title'), `${cls}: the title came back`).toBeNull()
      expect(cardY(framedBand.strip), `${cls}: the band strip's card came back`).toBe(0)
    })
    cleanup()
    const framedKeyer = await renderFramedKeyer()
    atScale(scale, () => {
      expect(framedKeyer.vk.querySelector('.vk-note'), `${cls}: the note came back`).toBeNull()
      expect(cardY(framedKeyer.vk), `${cls}: the keyer's card came back`).toBe(0)
      // THE DISCIPLINE: no font was made smaller by this pass. Read off the ONE --fs-micro
      // element still in the keyer (`.vk-hint`), so this walks the LIVE sheet rather than the
      // retired rule — and it is why the paragraph's ~41px cannot come back as token spacing
      // instead. Its dominant term was a LINE BOX: two lines of --fs-micro type, 30.8px at
      // the note's own line-height 1.4 and 26.4px at the sheet default measured here. Either
      // way --space-scale does not touch it, exactly as it did not touch the log heading's
      // 18px line box.
      const hint = framedKeyer.vk.querySelector('.vk-hint')!
      expect(pxOf(hint, 'font-size'), `${cls}: --fs-micro changed — this pass shrank no font`).toBe(11)
      expect(
        NOTE_LINES * lineBox(hint),
        `${cls}: the paragraph's line box evaporated into token spacing`,
      ).toBeGreaterThanOrEqual(26)
    })
    cleanup()

    // …and the same two blocks UNFRAMED still declare their cards, which is what the framed
    // hosts stopped paying. Both base rules survive: the flatten is keyed on the FRAME, so a
    // future unframed host still gets the card that would be its only edge.
    const bare = renderBareBandStrip()
    atScale(scale, () => {
      // 34 / 32.1 / 29.8: two token margins, two token paddings and a 1px border per side.
      expect(cardY(bare), `${cls}: the band strip's card is not what it was`).toBeGreaterThanOrEqual(29)
    })
    cleanup()
    const bareVk = await renderBareKeyer()
    atScale(scale, () => {
      // 26 / 24.6 / 22.6: --space-3 top and bottom, plus the 1px border per side.
      expect(cardY(bareVk), `${cls}: the keyer's card is not what it was`).toBeGreaterThanOrEqual(22)
    })
    cleanup()
  })
})
