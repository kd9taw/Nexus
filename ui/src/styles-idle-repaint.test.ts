// @vitest-environment jsdom
//
// NOTHING ON A STILL SCREEN KEEPS THE COMPOSITOR BUSY. Two rules each cost a frame of work for
// as long as their state held, on a screen where nothing else moved:
//
//   1. The critical status chip pulsed its opacity forever. A missing sound card raises it and
//      keeps it up all session, so the window produced a frame every vsync for the whole session
//      (measured in Chrome: 60 frames/s against 2 paints/s on an idle FT screen; under WebKitGTK,
//      which does not hand the pulse to the compositor, ~700 px of the header repainted each frame).
//      It now pulses a few times and holds at full strength.
//   2. Every pane frame carried a backdrop blur behind an 82 %-opaque fill. No frame sits over the
//      map, so the blur showed only the plain page, but each frame became its own blurred surface,
//      re-blurred on every frame the scope or waterfall drew. The overlays that sit over the map
//      keep theirs (the control below).
//
// HOW: cascade winners over the real sheets (cssCascade.testkit, jsdom's own selector engine), on
// the class chains the app renders. A regex over the sheet would pass on a dead selector. jsdom
// lays nothing out and runs no animation, so what is proved here is what the cascade resolves; the
// frame and paint counts are measured in real Chrome (the lane's report).
import { describe, it, expect, beforeAll } from 'vitest'
import { loadSheets, css } from './cssCascade.testkit'

beforeAll(() => loadSheets())

/** The properties a @keyframes rule animates, and its frames' values for `prop` by key. */
function keyframes(name: string): { props: Set<string>; at: (key: string, prop: string) => string | null } {
  let found: CSSKeyframesRule | null = null
  for (const s of Array.from(document.styleSheets)) {
    for (const r of Array.from(s.cssRules)) if ((r as CSSKeyframesRule).name === name) found = r as CSSKeyframesRule
  }
  expect(found, `no @keyframes ${name} in the sheets`).not.toBeNull()
  const frames = Array.from(found!.cssRules) as CSSKeyframeRule[]
  const props = new Set<string>()
  for (const f of frames) for (let i = 0; i < f.style.length; i++) props.add(f.style[i])
  return {
    props,
    at: (key, prop) => {
      const f = frames.find((x) => x.keyText.split(',').map((k) => k.trim()).includes(key))
      return f ? f.style.getPropertyValue(prop) || null : null
    },
  }
}

/** An `animation` shorthand's keyframes name and iteration count (`Infinity` for infinite). */
function parseAnimation(value: string | null): { name: string; count: number } | null {
  if (!value || value.trim() === 'none') return null
  const tokens = value.trim().split(/\s+/)
  const name = tokens.find((t) => /^[a-z][\w-]*$/i.test(t) && !/^(ease|ease-in|ease-out|ease-in-out|linear|infinite|normal|reverse|alternate|alternate-reverse|forwards|backwards|both|running|paused|none)$/.test(t))
  const count = tokens.includes('infinite') ? Infinity : Number(tokens.find((t) => /^\d+(\.\d+)?$/.test(t)) ?? 1)
  expect(name, `cannot read a keyframes name from \`${value}\``).toBeTruthy()
  return { name: name!, count }
}

function mount(html: string, select: string): Element {
  document.documentElement.setAttribute('data-theme', 'dark')
  document.documentElement.setAttribute('data-viewport', 'lg')
  document.body.innerHTML = `<div class="app">${html}</div>`
  const el = document.querySelector(select)
  expect(el, `${select} did not render`).not.toBeNull()
  return el!
}

describe('the critical status chip pulses, then holds', () => {
  const chip = (tier: string) =>
    mount(
      `<header class="topbar"><div class="txrx"><div class="status-lane" role="status">` +
        `<span class="status-chip tier-${tier}"><svg></svg><span class="status-chip-msg">RADIO ENGINE STOPPED</span></span>` +
        `</div></div></header>`,
      '.status-chip',
    )

  it('pulses a few times — not forever, and not never', () => {
    const anim = parseAnimation(css(chip('critical'), 'animation'))
    expect(anim, 'the critical chip no longer pulses at all: a new alarm must still catch the eye').not.toBeNull()
    expect(anim!.count, 'the critical chip pulses forever: the window draws a frame every vsync while it is up').toBeLessThan(Infinity)
    expect(anim!.count).toBeGreaterThanOrEqual(2)
    expect(anim!.count).toBeLessThanOrEqual(5)
    // A pulse of opacity alone, so even the few it runs stay off the main thread where the
    // compositor can take them.
    expect([...keyframes(anim!.name).props]).toEqual(['opacity'])
  })

  it('ends at full strength, so the steady chip reads as loud as the pulse began', () => {
    const el = chip('critical')
    const kf = keyframes(parseAnimation(css(el, 'animation'))!.name)
    expect(kf.at('0%', 'opacity')).toBe('1')
    expect(kf.at('100%', 'opacity')).toBe('1')
    // Nothing dims the chip itself once the animation has ended.
    expect(css(el, 'opacity') ?? '1').toBe('1')
  })

  it('control: a warning chip does not pulse at all', () => {
    expect(parseAnimation(css(chip('warning'), 'animation'))).toBeNull()
  })
})

describe('a pane frame carries no backdrop blur; the map overlays keep theirs', () => {
  // The chains the frames render in: a cockpit's pane region, and Connect's rail and strip.
  const FRAMES: Array<[string, string]> = [
    ['Phone cockpit', `<main class="layout single phone-cockpit"><div class="cockpit-panes" data-cols="2" data-flow="fill"><div class="cockpit-col"><section class="pane-frame" data-pane="log"><header class="pane-head"></header><div class="pane-body"></div></section></div></div></main>`],
    ['CW cockpit', `<main class="layout single cw-cockpit"><div class="cockpit-panes" data-cols="2" data-flow="fill"><div class="cockpit-col"><section class="pane-frame" data-pane="decode"><header class="pane-head"></header><div class="pane-body"></div></section></div></div></main>`],
    ['RTTY cockpit', `<main class="layout single rtty-cockpit"><section class="pane-frame" data-pane="rx"><header class="pane-head"></header><div class="pane-body"></div></section></main>`],
    ['Connect rail', `<div class="connect-shell"><div class="connect" data-rails="both"><div class="connect-rail"><section class="pane-frame" data-slot="left-top"><header class="pane-head"></header><div class="pane-body"></div></section></div></div></div>`],
    ['Connect strip', `<div class="connect-shell"><div class="connect" data-rails="both"><div class="connect-strip"><section class="pane-frame" data-slot="bottom"><header class="pane-head"></header><div class="pane-body"></div></section></div></div></div>`],
  ]

  for (const [where, html] of FRAMES) {
    it(`${where}: no blur, and the frame still holds its positioned content`, () => {
      const frame = mount(html, '.pane-frame')
      for (const prop of ['backdrop-filter', '-webkit-backdrop-filter']) {
        expect(css(frame, prop) ?? 'none', `${prop} on a ${where} frame makes it a blurred surface of its own`).toBe('none')
      }
      // The blur was also what made each frame the containing block of what is absolutely
      // positioned inside it, and a stacking context. Both must survive its removal, or the
      // pane's own overlays (drop marks, pop-out buttons) land against some other box.
      expect(css(frame, 'position')).toBe('relative')
      expect(css(frame, 'isolation')).toBe('isolate')
    })
  }

  // The control: the boxes that do sit over the map. If the resolver could not read a
  // backdrop-filter at all, these would come back empty and the checks above would prove nothing.
  for (const [cls, html] of [
    ['.map-insights', `<div class="map-view"><div class="map-insights"></div></div>`],
    ['.map-layers', `<div class="map-view"><div class="map-layers"></div></div>`],
    ['.globe3d-layers', `<div class="globe3d"><div class="globe3d-layers"></div></div>`],
  ] as const) {
    it(`control: ${cls} over the map keeps its blur`, () => {
      expect(css(mount(html, cls), 'backdrop-filter')).toMatch(/^blur\(/)
    })
  }
})
