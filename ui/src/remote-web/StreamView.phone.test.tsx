// @vitest-environment jsdom
// The stream page on a phone (the operator's pick, 2026-10-03): on its side, a rail beside the picture with Stop TX
// first, a PTT of 96 px, Mic and its level, Listen, Keyboard and More (Keyboard on its side since 2026-10-05, with Full
// screen moved under More to make room, as it is upright); upright, a bar over the picture for Stop TX and the state and
// a bar of thumb controls under it; a typing box in both, and in the header row on a touch screen (a tablet); anywhere
// else, the header as it was. These are measured on the cascade WINNER of the page's own sheets (entry.tsx, then
// this view) against the rendered tree, never by matching text in the stylesheet. jsdom lays nothing out: the
// real-browser half is the `phone` scenario of remote/test/browser.test.mjs.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { StreamView } from './StreamView'
import type { HostedConnection } from './client'
import type { OperationState } from './operation-protocol'
import type { OperationView } from './operation-client'
import { ANSWER, LEASE, harness, last } from './stream-link.testkit'
import { parseStreamInput } from './stream-protocol'
import { streamLayout } from './stream-layout'
import { chainOf, expandWith, parseRules, reachesChain, tokensAt, winnerAt } from '../cssCascade'

const BOOT = '0f7d1c2e-5b3a-4c1d-9e8f-7a6b5c4d3e2f'
const EPOCH = '000000000000002b'
const BETA = 'Beta Remote streaming is a beta feature. Access could be revoked at any time.'
function state(phase: OperationState['phase']): OperationState {
  return { stationBootId: BOOT, allowed: true, phase, leaseId: phase === 'controlling' ? LEASE : null, revision: 1,
    commandWindowId: null, nextSequence: null, leaseRemainingMs: phase === 'controlling' ? 5000 : null, actions: [],
    txArmed: false, transmitEpoch: phase === 'controlling' ? EPOCH : null } as OperationState
}

const frames = new Map<HTMLVideoElement, (now: number, metadata: { rtpTimestamp?: number }) => void>()
beforeEach(() => {
  Object.defineProperty(HTMLVideoElement.prototype, 'requestVideoFrameCallback', { configurable: true,
    value(this: HTMLVideoElement, callback: (now: number, metadata: { rtpTimestamp?: number }) => void) { frames.set(this, callback); return 1 } })
  Object.defineProperty(HTMLVideoElement.prototype, 'cancelVideoFrameCallback', { configurable: true, value(this: HTMLVideoElement) { frames.delete(this) } })
})
afterEach(async () => {
  cleanup(); frames.clear()
  delete (HTMLVideoElement.prototype as { requestVideoFrameCallback?: unknown }).requestVideoFrameCallback
  delete (HTMLVideoElement.prototype as { cancelVideoFrameCallback?: unknown }).cancelVideoFrameCallback
  delete (window as { matchMedia?: unknown }).matchMedia
  // jsdom's own window for the next test, and nothing left published from this one.
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: 1024 })
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: 768 })
  for (const name of ['--vw-eff', '--vh-eff']) document.documentElement.style.removeProperty(name)
})

/** The window this page has, as useViewport reads it and publishes it (`--vw-eff` × `--vh-eff`) a frame later. */
async function windowSize(width: number, height: number) {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: width })
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: height })
  await act(async () => {
    window.dispatchEvent(new Event('resize'))
    await new Promise(done => requestAnimationFrame(done))
    await Promise.resolve()
  })
}
/** A phone (a coarse pointer), as the page asks the browser. */
function coarse(on: boolean) {
  Object.defineProperty(window, 'matchMedia', { configurable: true,
    value: (query: string) => ({ matches: on && query === '(pointer: coarse)', media: query, addEventListener() {}, removeEventListener() {} }) })
}

async function streaming(width: number, height: number) {
  await windowSize(width, height)
  const h = harness()
  let snapshot = { supported: true, state: state('controlling'), fresh: true, connected: true, busy: false, stopAvailable: true,
    stopSending: false, stopAccepted: false } as OperationView
  const listeners = new Set<() => void>()
  const operations = {
    subscribe: (f: () => void) => { listeners.add(f); return () => { listeners.delete(f) } }, getSnapshot: () => snapshot,
    acquire: vi.fn(async () => {}), release: vi.fn(async () => {}), stopTransmit: vi.fn(async () => ({ stop: 'accepted' })),
  }
  const disconnect = vi.fn(), signOut = vi.fn()
  const connection = { operations, stream: h.link, source: { id: 'fake', kind: 'native', read: () => Promise.reject(Error('none')) } } as unknown as HostedConnection
  const utils = render(<StreamView connection={connection} station="Home" disconnect={disconnect} signOut={signOut} />)
  // The page's own useViewport publishes the window a frame after it mounts.
  await act(async () => { await new Promise(done => requestAnimationFrame(done)); await Promise.resolve() })
  const video = utils.container.querySelector('video')!
  Object.defineProperty(video, 'videoWidth', { configurable: true, value: 3440 })
  Object.defineProperty(video, 'videoHeight', { configurable: true, value: 1440 })
  fireEvent.click(screen.getByRole('button', { name: 'Start the stream' }))
  await act(async () => { await Promise.resolve() })
  await act(async () => { h.link.receive({ type: 'streamSignal', payload: { kind: 'answer', sdp: ANSWER } }); await Promise.resolve() })
  act(() => {
    h.peer.ontrack?.({ track: 'track', streams: ['media'] })
    for (const label of ['control', 'ptt', 'audio']) h.peer.channel(label).open()
    frames.get(video)?.(0, { rtpTimestamp: 90000 })
  })
  const app = utils.container.querySelector<HTMLElement>('.remote-stream-app')!
  expect(h.link.getSnapshot().phase, 'the stream is live').toBe('live')
  return {
    h, app, video, disconnect, signOut,
    /** What the page sent on `control` that is input for Nexus's window. */
    input: () => h.peer.channel('control').sent.filter(m => m.type === 'text' || m.type === 'key' || m.type === 'pointer'),
    ptt: () => h.peer.channel('ptt').sent,
    /** The operations state moves on: `fresh: false` is the lease lapsed for a re-read. */
    set: (next: Partial<OperationView>) => act(() => { snapshot = { ...snapshot, ...next }; for (const f of listeners) f() }),
  }
}

// ── The cascade, as the page's own sheets resolve it ─────────────────────────────────────────────────────────────
const pageSheet = (path: string) =>
  readFileSync(resolve(process.cwd(), 'src', path), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const PAGE_RULES = parseRules(['styles.css', 'remote-monitor/monitor.css', 'remote-web/remote.css', 'remote-web/stream.css']
  .map(pageSheet).join('\n'))
/** The value the cascade gives `prop` on `el`, its var()s resolved where `el` sits; null when no rule sets it. */
function computed(el: Element, ...props: string[]): string | null {
  const chain = chainOf(el)
  const win = winnerAt(PAGE_RULES, 'dark', chain, ...props)
  return win ? expandWith(tokensAt(PAGE_RULES, 'dark', chain), win.value) : null
}
const px = (value: string | null) => value === null ? null : parseFloat(value)
const button = (name: string) => screen.getByRole('button', { name })
const buttonsIn = (root: Element) => within(root as HTMLElement).queryAllByRole('button').map(b => b.textContent)

describe('the layout, from the window the page has', () => {
  it('on its side and short is the rail; upright and narrow are the bars; every other window is the header as it was', () => {
    const at = (width: number, height: number) => streamLayout(width, height)
    // Phones on their sides, with and without the browser's own bar, and a short desktop window.
    expect([at(915, 412), at(844, 390), at(667, 375), at(915, 356), at(1280, 499)]).toEqual(['rail', 'rail', 'rail', 'rail', 'rail'])
    // Phones upright, a small tablet upright, a narrow window, and a small square.
    expect([at(412, 915), at(360, 640), at(744, 1133), at(700, 900), at(400, 400)]).toEqual(['bars', 'bars', 'bars', 'bars', 'bars'])
    // The desktop sizes the page is laid out for (the supported floor is 1024 x 768), a laptop's browser on that
    // screen, a tablet either way up, and the edges just past each rule.
    expect([at(1280, 800), at(1024, 768), at(1366, 657), at(768, 1024), at(1024, 700), at(1280, 500), at(768, 900)])
      .toEqual(['header', 'header', 'header', 'header', 'header', 'header', 'header'])
  })

  it('the page takes the layout of the size useViewport publishes, and follows the window as it turns', async () => {
    const v = await streaming(915, 412)
    expect(v.app.dataset.layout, 'a phone on its side').toBe('rail')
    await windowSize(412, 915)
    expect(v.app.dataset.layout, 'turned upright').toBe('bars')
    await windowSize(1280, 800)
    expect(v.app.dataset.layout, 'a desktop window').toBe('header')
  })
})

describe('on its side: the rail', () => {
  it('the picture has the whole height, beside a 120 px rail that scrolls, never the page', async () => {
    const v = await streaming(915, 412)
    expect(computed(v.app, 'grid-template-columns')).toBe('minmax(0, 1fr) 120px')
    expect(computed(v.app, 'grid-template-rows')).toBe('minmax(0, 1fr)')
    const header = v.app.querySelector('.remote-stream-header')!, stage = v.app.querySelector('.remote-stream-stage')!
    expect([computed(stage, 'grid-area'), computed(header, 'grid-area')]).toEqual(['stage', 'rail'])
    expect(computed(v.app, 'grid-template-areas')).toBe("'stage rail'")
    expect({ direction: computed(header, 'flex-direction'), wrap: computed(header, 'flex-wrap'), overflow: computed(header, 'overflow-y'), max: computed(header, 'max-height') })
      .toEqual({ direction: 'column', wrap: 'nowrap', overflow: 'auto', max: 'none' })
    expect(computed(v.app.querySelector('.remote-stream-title')!, 'display'), 'no room for the title').toBe('none')
  })

  it('Stop TX is first, held at the top of the rail on an opaque ground, and the largest: as tall as the 96 px PTT and the rail\'s full width', async () => {
    const v = await streaming(915, 412)
    const stop = button('Stop TX'), ptt = button('Hold PTT')
    expect(v.app.querySelector('button'), 'the first control').toBe(stop)
    const safety = stop.parentElement!
    expect(safety.className).toBe('remote-stream-safety')
    expect({ position: computed(safety, 'position'), top: computed(safety, 'top'), ground: computed(safety, 'background', 'background-color') })
      .toEqual({ position: 'sticky', top: '0', ground: '#0b0f17' })
    expect(px(computed(stop, 'min-height')), 'Stop TX').toBe(96)
    expect({ height: px(computed(ptt, 'min-height')), width: computed(ptt, 'width') }, 'the PTT, never wider than the rail').toEqual({ height: 96, width: 'min(96px, 100%)' })
    // Stop TX has no width of its own and is not centred, so the rail's column stretches it across its whole width;
    // the PTT is a 96 px square inside that.
    expect({ width: computed(stop, 'width'), align: computed(stop, 'align-self') }).toEqual({ width: null, align: null })
    expect(computed(safety, 'align-items')).toBe('stretch')
    expect(px(computed(ptt, 'min-height'))!, 'no taller than Stop TX').toBeLessThanOrEqual(px(computed(stop, 'min-height'))!)
  })

  it('in the operator\'s order: the PTT, then Mic, then Listen, then Keyboard, then More; End, Disconnect and Sign out under More', async () => {
    const v = await streaming(915, 412)
    const order = (selector: string) => Number(computed(v.app.querySelector(selector)!, 'order') ?? 0)
    // Listen is not offered by this station: the cascade at its place in the rail says where it would sit.
    const listen = winnerAt(PAGE_RULES, 'dark', [...chainOf(v.app.querySelector('.remote-stream-operate')!), { tag: 'span', classes: ['remote-audio'], attrs: {} }], 'order')
    expect([order('.remote-stream-ptt'), order('.remote-stream-mic'), Number(listen?.value), order('.remote-stream-keyboard')]).toEqual([1, 2, 4, 5])
    const rail = v.app.querySelector('.remote-stream-header')!
    expect(buttonsIn(rail)).toEqual(['Stop TX', 'Hold PTT', 'Mic off', 'Keyboard', 'More'])
    expect(rail.querySelector('.remote-beta'), 'the beta line is under More').toBeNull()
  })

  it('Full screen is under More, as it is upright, which leaves the rail its room for Keyboard', async () => {
    const v = await streaming(915, 412)
    Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: true })
    try {
      await windowSize(915, 411)
      expect(buttonsIn(v.app.querySelector('.remote-stream-header')!)).toEqual(['Stop TX', 'Hold PTT', 'Mic off', 'Keyboard', 'More'])
      fireEvent.click(button('More'))
      expect(buttonsIn(screen.getByRole('group', { name: 'More' }))).toEqual(['End the stream', 'Full screen', 'Disconnect and return to stations', 'Sign out'])
    } finally { delete (document as { fullscreenEnabled?: unknown }).fullscreenEnabled }
  })

  it('Keyboard: the upright\'s own control, in the rail, opening the typing box under it, focused by the press; what is typed goes as it does upright', async () => {
    const v = await streaming(915, 412)
    const rail = v.app.querySelector('.remote-stream-header')!
    const keyboard = button('Keyboard')
    expect(rail.contains(keyboard) && keyboard.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(keyboard)
    const field = screen.getByRole('textbox', { name: 'Type here for the field selected at the shack' }) as HTMLInputElement
    expect(document.activeElement, 'focused by the press, so the phone raises its keyboard').toBe(field)
    expect(keyboard.getAttribute('aria-pressed')).toBe('true')
    // In the rail's column under Keyboard, the column's width: no wider than the rail, so nothing is cut off.
    expect(rail.contains(field.form!) && field.form!.parentElement!.className).toBe('remote-stream-operate')
    expect([computed(field.form!, 'order'), computed(field.parentElement!.parentElement!, 'align-items')]).toEqual(['6', 'stretch'])
    expect([computed(field, 'min-width'), computed(field, 'flex')]).toEqual(['0', '1 1 auto'])
    field.value = 'CQ'
    fireEvent.input(field, { isComposing: false })
    fireEvent.submit(field.form!)
    expect(v.input().map(m => m.type === 'text' ? `text ${m.text}` : `${m.type} ${m.action} ${m.key}`)).toEqual(['text CQ', 'key down Enter', 'key up Enter'])
    fireEvent.click(keyboard)
    expect(screen.queryByRole('textbox'), 'Keyboard again closes it').toBeNull()
  })

  it('a chip over the picture says the state with the beta mark, and takes no press from the picture under it', async () => {
    const v = await streaming(915, 412)
    const chip = v.app.querySelector('.remote-stream-state')!
    expect(chip.textContent).toBe('BetaStreaming Nexus at the shack')
    expect({ position: computed(chip, 'position'), events: computed(chip, 'pointer-events'), left: computed(chip, 'left'), top: computed(chip, 'top') })
      .toEqual({ position: 'absolute', events: 'none', left: '8px', top: '8px' })
    expect(computed(v.app, 'position'), 'over the stage, from the app').toBe('relative')
    expect(computed(chip, 'max-width'), 'never over the rail').toBe('calc(100% - 120px - 16px)')
  })

  it('More: End the stream, Disconnect, Sign out and the beta line, over the picture; a choice closes it, and so does a press elsewhere', async () => {
    const v = await streaming(915, 412)
    const more = button('More')
    expect(more.getAttribute('aria-expanded')).toBe('false')
    fireEvent.click(more)
    const panel = screen.getByRole('group', { name: 'More' })
    expect(more.getAttribute('aria-expanded')).toBe('true')
    expect(more.getAttribute('aria-controls')).toBe(panel.id)
    expect(buttonsIn(panel)).toEqual(['End the stream', 'Disconnect and return to stations', 'Sign out'])
    expect(panel.querySelector('.remote-beta')?.textContent).toBe(BETA)
    expect(computed(panel, 'grid-area'), 'over the picture').toBe('stage')
    fireEvent.click(within(panel).getByRole('button', { name: 'Disconnect and return to stations' }))
    expect(v.disconnect).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('group', { name: 'More' }), 'a choice closes it').toBeNull()
    // A press on the picture closes it and goes no further: the cover over the picture takes it, so a press meant to
    // close a menu is never a click at the shack.
    fireEvent.click(more)
    const cover = v.app.querySelector('.remote-stream-more-cover')!
    expect([computed(cover, 'grid-area'), computed(cover, 'z-index')], 'over the picture').toEqual(['stage', '3'])
    const before = v.input().length
    fireEvent.pointerDown(cover, { pointerId: 5, pointerType: 'touch', button: 0 })
    fireEvent.pointerUp(cover, { pointerId: 5, pointerType: 'touch', button: 0 })
    expect(screen.queryByRole('group', { name: 'More' }), 'a press on the picture closes it').toBeNull()
    expect(v.app.querySelector('.remote-stream-more-cover'), 'and the cover goes with it').toBeNull()
    expect(v.input().slice(before), 'nothing reached the shack').toEqual([])
    // A press on another control closes it as well, and is that control's own.
    fireEvent.click(more)
    fireEvent.pointerDown(button('Mic off'))
    expect(screen.queryByRole('group', { name: 'More' }), 'a press elsewhere closes it').toBeNull()
    fireEvent.click(more)
    fireEvent.click(more)
    expect(screen.queryByRole('group', { name: 'More' }), 'More again closes it').toBeNull()
  })
})

describe('upright: the bars', () => {
  it('a bar over the picture for Stop TX, the state and More, the beta line, the picture, and the thumbs\' bar under it', async () => {
    const v = await streaming(412, 915)
    expect(computed(v.app, 'grid-template-areas')).toBe("'safety state session' 'beta beta beta' 'stage stage stage' 'operate operate operate'")
    expect(computed(v.app, 'grid-template-rows')).toBe('auto auto minmax(0, 1fr) auto')
    const at = (selector: string) => computed(v.app.querySelector(selector)!, 'grid-area')
    expect([at('.remote-stream-safety'), at('.remote-stream-state'), at('.remote-stream-session'), at('.remote-stream-beta'), at('.remote-stream-stage'), at('.remote-stream-operate')])
      .toEqual(['safety', 'state', 'session', 'beta', 'stage', 'operate'])
    // The header and its controls draw no box, so each group is a cell of the page's grid.
    expect([computed(v.app.querySelector('.remote-stream-header')!, 'display'), computed(v.app.querySelector('.remote-stream-controls')!, 'display')])
      .toEqual(['contents', 'contents'])
    expect(buttonsIn(v.app.querySelector('.remote-stream-operate')!)).toEqual(['Hold PTT', 'Mic off', 'Keyboard'])
    expect(buttonsIn(v.app.querySelector('.remote-stream-session')!)).toEqual(['More'])
    expect(v.app.querySelector('.remote-stream-beta')?.textContent, 'the beta line stays on screen upright').toBe(BETA)
  })

  it('Stop TX is first and the largest: at least as wide as the PTT may grow, and as tall', async () => {
    const v = await streaming(412, 915)
    const stop = button('Stop TX'), ptt = button('Hold PTT')
    expect(v.app.querySelector('button')).toBe(stop)
    expect({ width: px(computed(stop, 'min-width')), height: px(computed(stop, 'min-height')) }).toEqual({ width: 128, height: 96 })
    expect({ width: px(computed(ptt, 'max-width')), height: px(computed(ptt, 'min-height')), grow: computed(ptt, 'flex-grow') })
      .toEqual({ width: 128, height: 96, grow: '1.4' })
  })

  it('More holds End, Full screen, Disconnect and Sign out, under its bar', async () => {
    const v = await streaming(412, 915)
    Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: true })
    try {
      await windowSize(412, 914)
      fireEvent.click(button('More'))
      const panel = screen.getByRole('group', { name: 'More' })
      expect(buttonsIn(panel)).toEqual(['End the stream', 'Full screen', 'Disconnect and return to stations', 'Sign out'])
      expect([computed(panel, 'align-self'), computed(panel, 'justify-self')]).toEqual(['start', 'end'])
      expect(buttonsIn(v.app.querySelector('.remote-stream-header')!), 'and not in the bars').toEqual(['Stop TX', 'Hold PTT', 'Mic off', 'Keyboard', 'More'])
    } finally { delete (document as { fullscreenEnabled?: unknown }).fullscreenEnabled }
  })

  it('on a phone, says to turn it for a bigger picture; CONTROL: not to a mouse, which has no phone to turn', async () => {
    coarse(true)
    const v = await streaming(412, 915)
    expect(v.app.querySelector('.remote-stream-turn')?.textContent).toBe('Turn the phone on its side for a bigger picture.')
    cleanup()
    coarse(false)
    const w = await streaming(412, 915)
    expect(w.app.querySelector('.remote-stream-turn')).toBeNull()
  })
})

describe('a touch screen in the header layout, a tablet (the operator\'s pick, 2026-10-05)', () => {
  it('Keyboard follows Mic in the header row and opens the typing box on a line of its own after the buttons; CONTROL: a mouse has neither, and the row is as it was', async () => {
    coarse(true)
    const v = await streaming(1024, 768)
    expect(v.app.dataset.layout, 'an iPad on its side').toBe('header')
    const header = v.app.querySelector('.remote-stream-header')!
    expect(buttonsIn(header)).toEqual(['Stop TX', 'Hold PTT', 'Mic off', 'Keyboard', 'End the stream', 'Disconnect and return to stations', 'Sign out'])
    const half = computed(header, 'max-height')
    fireEvent.click(button('Keyboard'))
    const field = screen.getByRole('textbox', { name: 'Type here for the field selected at the shack' }) as HTMLInputElement
    expect(document.activeElement, 'focused by the press, so the tablet raises its keyboard').toBe(field)
    // Open, the row may take the whole height and not half of it, so the box the browser scrolls into view above the
    // tablet's own keyboard never scrolls Stop TX out of the row. (`--vh-eff` is set on <html> as the page runs, so the
    // sheets alone resolve it to its fallback, the whole height.)
    expect([half, computed(header, 'max-height')]).toEqual(['calc(100% * 0.5)', '100%'])
    // A flex item of the controls' wrapping row (its group draws no box), after every button, on a line of its own.
    const controls = header.querySelector('.remote-stream-controls')!
    expect([computed(field.form!.parentElement!, 'display'), computed(controls, 'flex-wrap')]).toEqual(['contents', 'wrap'])
    expect([computed(field.form!, 'order'), computed(field.form!, 'flex')]).toEqual(['1', '1 1 100%'])
    field.value = 'CQ'
    fireEvent.input(field, { isComposing: false })
    fireEvent.submit(field.form!)
    expect(v.input().map(m => m.type === 'text' ? `text ${m.text}` : `${m.type} ${m.action} ${m.key}`)).toEqual(['text CQ', 'key down Enter', 'key up Enter'])
    cleanup()
    coarse(false)
    const w = await streaming(1024, 768)
    expect(w.app.dataset.layout).toBe('header')
    expect(buttonsIn(w.app.querySelector('.remote-stream-header')!), 'a mouse: no Keyboard').toEqual(['Stop TX', 'Hold PTT', 'Mic off', 'End the stream', 'Disconnect and return to stations', 'Sign out'])
  })

  it('the header row holds while the operator types: the tablet\'s keyboard takes height and never width; CONTROL: once the box lets go, the window decides', async () => {
    coarse(true)
    const v = await streaming(1024, 768)
    fireEvent.click(button('Keyboard'))
    const field = screen.getByRole('textbox') as HTMLInputElement
    // The tablet's own keyboard up: wider than tall and under 500 px, the rail's shape.
    await windowSize(1024, 340)
    expect(v.app.dataset.layout, 'held while typing').toBe('header')
    expect(document.activeElement, 'the field, and the keyboard with it, stay').toBe(field)
    act(() => { field.blur() })
    expect(v.app.dataset.layout, 'not typing: the window decides').toBe('rail')
  })
})

describe('the header layout is the page as it was', () => {
  it('no phone rule reaches anything in it, the four groups draw no box, and its controls are in their old order', async () => {
    const v = await streaming(1280, 800)
    expect(v.app.dataset.layout).toBe('header')
    const phoneRulesReaching = () => [v.app, ...v.app.querySelectorAll('*')].flatMap(el => PAGE_RULES
      .filter(rule => rule.selector.includes('data-layout') && reachesChain(rule.selector, chainOf(el), 'dark'))
      .map(rule => `${rule.selector} → <${el.tagName.toLowerCase()} class="${el.className}">`))
    expect(phoneRulesReaching()).toEqual([])
    // CONTROL: the same scan over the same tree, stamped as the rail, finds the rail's rules.
    v.app.dataset.layout = 'rail'
    expect(phoneRulesReaching().length).toBeGreaterThan(10)
    v.app.dataset.layout = 'header'
    for (const group of ['state', 'safety', 'operate', 'session']) expect(computed(v.app.querySelector(`.remote-stream-${group}`)!, 'display'), group).toBe('contents')
    expect(buttonsIn(v.app.querySelector('.remote-stream-header')!)).toEqual(['Stop TX', 'Hold PTT', 'Mic off', 'End the stream', 'Disconnect and return to stations', 'Sign out'])
    expect(v.app.querySelector('.remote-stream-header .remote-beta')?.textContent).toBe(BETA)
  })
})

describe('turning the phone moves controls and remounts none', () => {
  it('a PTT held while the phone turns stays held, the same button, with no release sent; the picture is the same element', async () => {
    const v = await streaming(412, 915)
    const ptt = button('Hold PTT')
    fireEvent.pointerDown(ptt, { button: 0, pointerId: 1 })
    expect(ptt.getAttribute('aria-pressed')).toBe('true')
    const before = v.ptt().length
    await windowSize(915, 412)
    expect(v.app.dataset.layout).toBe('rail')
    expect(button('Hold PTT'), 'the same element').toBe(ptt)
    expect(ptt.isConnected && ptt.getAttribute('aria-pressed')).toBe('true')
    expect(v.ptt().slice(before).filter(m => m.type === 'pttRelease'), 'nothing let go of it').toEqual([])
    expect(v.app.querySelector('video'), 'the same picture').toBe(v.video)
    fireEvent.pointerUp(ptt, { pointerId: 1 })
    expect(last(v.ptt())?.type).toBe('pttRelease')
  })
})

// A press on a greyed-out PTT sends nothing (the operator's pick "Refuse it on the page", 2026-10-04): the rail's PTT
// and the thumbs' bar's are the one button, and a finger is a pointer press on it.
describe('a greyed-out PTT on a phone', () => {
  it.each([[915, 412, 'rail'], [412, 915, 'bars']] as const)('a finger on the PTT greyed out by a lapse of the lease starts nothing (%i x %i, %s); CONTROL: lit again, it holds', async (width, height, layout) => {
    const v = await streaming(width, height)
    expect(v.app.dataset.layout).toBe(layout)
    const ptt = button('Hold PTT') as HTMLButtonElement
    const holds = () => v.ptt().filter(m => m.type === 'pttHold')
    v.set({ fresh: false })
    expect(ptt.getAttribute('aria-disabled'), 'greyed out by the lapse').toBe('true')
    fireEvent.pointerDown(ptt, { button: 0, pointerId: 1, pointerType: 'touch' })
    expect(holds(), 'a finger on the greyed-out PTT').toEqual([])
    fireEvent.pointerUp(ptt, { button: 0, pointerId: 1, pointerType: 'touch' })
    v.set({ fresh: true })
    expect(ptt.getAttribute('aria-disabled')).toBe(null)
    fireEvent.pointerDown(ptt, { button: 0, pointerId: 2, pointerType: 'touch' })
    expect(holds(), 'lit, the finger holds').toHaveLength(1)
    fireEvent.pointerUp(ptt, { button: 0, pointerId: 2, pointerType: 'touch' })
    expect(last(v.ptt())?.type).toBe('pttRelease')
  })
})

describe('the typing box', () => {
  async function typing() {
    const v = await streaming(412, 915)
    const keyboard = button('Keyboard')
    expect(keyboard.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(keyboard)
    const field = screen.getByRole('textbox', { name: 'Type here for the field selected at the shack' }) as HTMLInputElement
    expect(document.activeElement, 'focused by the press, so the phone raises its keyboard').toBe(field)
    expect(keyboard.getAttribute('aria-pressed')).toBe('true')
    /** The keyboard putting `value` in the field, as committed text or as a composition still under way. */
    const type = (value: string, composing = false) => { field.value = value; fireEvent.input(field, { isComposing: composing }) }
    const sent = () => v.input().map(m => m.type === 'text' ? `text ${m.text}` : `${m.type} ${m.action} ${m.key}`)
    return { ...v, field, type, sent }
  }

  it('sends exactly the committed text, character by character as it is typed, spaces included', async () => {
    const v = await typing()
    for (const value of ['W', 'W1', 'W1A', 'W1AW', 'W1AW ', 'W1AW D', 'W1AW DE']) v.type(value)
    expect(v.sent()).toEqual(['text W', 'text 1', 'text A', 'text W', 'text  ', 'text D', 'text E'])
    expect(v.input().filter(m => m.type === 'text').map(m => m.text).join(''), 'the committed text, exactly').toBe('W1AW DE')
    // Every one is the contract's own message: only `text`, as the station reads it.
    for (const message of v.input()) expect(() => parseStreamInput(message)).not.toThrow()
  })

  it('a word being composed goes once, when the keyboard commits it, and never its stages', async () => {
    const v = await typing()
    fireEvent.compositionStart(v.field)
    for (const stage of ['k', 'ka', 'かな']) v.type(stage, true)
    expect(v.sent(), 'nothing while it is composed').toEqual([])
    v.field.value = '仮名'
    fireEvent.compositionEnd(v.field, { data: '仮名' })
    expect(v.sent()).toEqual(['text 仮名'])
    // A browser that sends the input after the composition ends as well (Firefox) adds nothing.
    fireEvent.input(v.field, { isComposing: false })
    expect(v.sent()).toEqual(['text 仮名'])
  })

  it('a word the keyboard corrects, or one deleted here, is corrected at the shack: Backspaces, then the new text', async () => {
    const v = await typing()
    v.type('teh ')
    v.type('the ')
    expect(v.sent()).toEqual(['text teh ', 'key down Backspace', 'key up Backspace', 'key down Backspace', 'key up Backspace', 'key down Backspace', 'key up Backspace', 'text he '])
    v.type('the')
    expect(v.sent().slice(-2)).toEqual(['key down Backspace', 'key up Backspace'])
  })

  it('Enter goes as Enter and empties the box; Backspace with the box empty goes as Backspace; CONTROL: with text in it, Backspace is the field\'s own', async () => {
    const v = await typing()
    v.type('K1ABC')
    fireEvent.submit(v.field.form!)
    expect(v.sent().slice(-2)).toEqual(['key down Enter', 'key up Enter'])
    expect(v.field.value).toBe('')
    const count = v.sent().length
    fireEvent.keyDown(v.field, { key: 'Backspace' })
    expect(v.sent().slice(count)).toEqual(['key down Backspace', 'key up Backspace'])
    v.type('ab')
    const typed = v.sent().length
    fireEvent.keyDown(v.field, { key: 'Backspace' })
    expect(v.sent().length, 'the key itself sends nothing: the field changes, and that change goes').toBe(typed)
    v.type('a')
    expect(v.sent().slice(typed)).toEqual(['key down Backspace', 'key up Backspace'])
    expect(new Set(v.input().map(m => m.type)), 'only the contract\'s text and key').toEqual(new Set(['text', 'key']))
  })

  it('nothing goes while the picture is not live: what was typed then is taken back out of the box', async () => {
    const v = await typing()
    v.type('CQ')
    v.h.tick(2500)
    await act(async () => { await new Promise(resolve => setTimeout(resolve, 300)) })
    expect(v.h.link.getSnapshot().phase, 'the picture froze').toBe('stalled')
    v.type('CQ DX')
    fireEvent.submit(v.field.form!)
    fireEvent.keyDown(v.field, { key: 'Backspace' })
    expect(v.sent()).toEqual(['text CQ'])
    expect(v.field.value).toBe('CQ')
    // CONTROL: a new frame, and what is typed goes again.
    act(() => frames.get(v.video)?.(0, { rtpTimestamp: 180000 }))
    expect(v.h.link.getSnapshot().phase).toBe('live')
    v.type('CQ DX')
    expect(v.sent()).toEqual(['text CQ', 'text  DX'])
  })

  it('Keyboard again closes the box; turning the phone keeps it, the same field, focused, with its text; a window in the header layout with a mouse has neither', async () => {
    const v = await typing()
    const keyboard = button('Keyboard')
    fireEvent.click(keyboard)
    expect(screen.queryByRole('textbox')).toBeNull()
    fireEvent.click(keyboard)
    const field = screen.getByRole('textbox') as HTMLInputElement
    field.value = 'CQ'
    fireEvent.input(field, { isComposing: false })
    await windowSize(915, 412)
    expect(v.app.dataset.layout).toBe('rail')
    expect(screen.getByRole('textbox'), 'the same field').toBe(field)
    expect([document.activeElement === field, field.value]).toEqual([true, 'CQ'])
    expect([button('Keyboard'), keyboard.getAttribute('aria-pressed')], 'the same Keyboard, still pressed, now in the rail').toEqual([keyboard, 'true'])
    await windowSize(1280, 800)
    expect(v.app.dataset.layout).toBe('header')
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Keyboard' }), 'no Keyboard in the header').toBeNull()
  })

  it('the layout holds while the operator types: the keyboard takes height and never width; CONTROL: once the box lets go, the layout follows the window', async () => {
    const v = await typing()
    // A small phone's keyboard leaves it wider than tall, which is the rail's shape.
    await windowSize(412, 380)
    expect(v.app.dataset.layout, 'held while typing').toBe('bars')
    expect(document.activeElement, 'the field, and the keyboard with it, stay').toBe(v.field)
    act(() => { v.field.blur() })
    expect(v.app.dataset.layout, 'not typing: the window decides').toBe('rail')
  })
})
