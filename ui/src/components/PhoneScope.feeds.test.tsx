// @vitest-environment jsdom
//
// WHAT EACH CW/PHONE SCOPE FEED READS, AND WHAT IT SHOWS FOR IT (2026-11-04).
//   - feed 'audio' (the audio view under, or instead of, the radio's scope) polls the audio only
//     (`getAudioScopeFrame`), never the rig scope read that a live RF row wins, and never the RF pane's.
//   - HiDef/Fast is an audio analysis: shown on CW's audio rows, never on a row of the radio's scope,
//     whatever the scope's feed.
//   - the radio scope's sweep rate is reported (onSweepRate) for the cockpit's slow-scope hint.
// Runs the component's real loop and poll on fake timers (the rfview test's harness).
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { PhoneScope } from './PhoneScope'

const ROW = Array.from({ length: 512 }, (_, i) => (i === 200 ? 0.9 : 0.1))
const src = {
  /** What the rig scope read hands back: the radio's row while it streams. */
  source: 'civ',
  seq: 0,
  /** The radio's scope has stopped: the RF reads come back empty. */
  stalled: false,
  calls: [] as string[],
}
const rfFrame = () => ({
  seq: ++src.seq,
  tMs: Date.now(),
  source: src.source,
  loHz: 14_000_000,
  hiHz: 14_100_000,
  scale: { kind: 'relative' },
  bins: ROW,
})
const audioFrame = () => ({
  seq: ++src.seq,
  tMs: Date.now(),
  source: 'audio',
  loHz: 200,
  hiHz: 3000,
  scale: { kind: 'dbfs', loDb: -120, hiDb: 0 },
  bins: ROW,
})
const empty = () => ({ seq: 0, tMs: Date.now(), source: '', loHz: 0, hiHz: 0, scale: { kind: 'relative' }, bins: [] })

vi.mock('../api', () => ({
  getScopeFrame: () => {
    src.calls.push('scope')
    return Promise.resolve(src.source === 'audio' || src.stalled ? audioFrame() : rfFrame())
  },
  getAudioScopeFrame: () => {
    src.calls.push('audio')
    return Promise.resolve(audioFrame())
  },
  getRfFrame: () => {
    src.calls.push('rf')
    return Promise.resolve(src.stalled ? empty() : rfFrame())
  },
}))

beforeEach(() => {
  Object.assign(src, { source: 'civ', seq: 0, stalled: false, calls: [] })
  localStorage.clear()
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'performance', 'Date'] })
  vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16))
  vi.stubGlobal('cancelAnimationFrame', clearTimeout)
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} })
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }))
  vi.stubGlobal('ImageData', class {
    data: Uint8ClampedArray; width: number; height: number
    constructor(a: Uint8ClampedArray | number, b: number, c?: number) {
      if (typeof a === 'number') { this.width = a; this.height = b; this.data = new Uint8ClampedArray(a * b * 4) }
      else { this.data = a; this.width = b; this.height = c ?? a.length / 4 / b }
    }
  })
  const paint = {
    fillRect() {}, clearRect() {}, putImageData() {}, beginPath() {}, closePath() {}, moveTo() {}, lineTo() {}, fillText() {},
    fill() {}, stroke() {}, setLineDash() {}, save() {}, restore() {}, rect() {}, clip() {},
    measureText: (t: string) => ({ width: t.length * 6 }), createLinearGradient: () => ({ addColorStop() {} }),
  }
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(((kind: string) =>
    kind === '2d' ? paint : null) as unknown as HTMLCanvasElement['getContext'])
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue({ x: 0, y: 0, left: 0, top: 0,
    width: 400, height: 200, right: 400, bottom: 200, toJSON() { return {} } })
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

async function run(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms)
  })
}


const hiDef = () => screen.queryByRole('button', { name: /^(HiDef|Fast)$/ })

describe("the audio view's feed", () => {
  it('reads the audio only, while the radio scope streams and after it stalls', async () => {
    render(<PhoneScope feed="audio" cockpit="cw" transmitting={false} theme="dark" viewLoHz={200} viewHiHz={3000} />)
    await run(600)
    src.stalled = true
    await run(600)
    expect(src.calls.length).toBeGreaterThan(5)
    expect(new Set(src.calls)).toEqual(new Set(['audio']))
    // And it keeps drawing: no "unavailable" plate over it.
    expect(screen.queryByText('No scope data from the radio')).toBeNull()
  })

  it("control: the radio's pane says the radio scope has stopped", async () => {
    render(<PhoneScope feed="rf" cockpit="rfpan" transmitting={false} theme="dark" />)
    await run(300)
    expect(screen.queryByText('No scope data from the radio')).toBeNull()
    src.stalled = true
    await run(300)
    expect(screen.getByText('No scope data from the radio')).toBeTruthy()
    expect(new Set(src.calls)).toEqual(new Set(['rf']))
  })
})

describe('HiDef/Fast follows the LIVE source, not the feed', () => {
  it('hidden while the CW rig scope draws a row of the radio scope', async () => {
    render(<PhoneScope cockpit="cw" transmitting={false} theme="dark" viewLoHz={-25_000} viewHiHz={25_000} />)
    await run(300)
    expect(hiDef()).toBeNull()
  })

  it('shown on the same scope once its rows are the audio again', async () => {
    src.source = 'audio'
    render(<PhoneScope cockpit="cw" transmitting={false} theme="dark" viewLoHz={200} viewHiHz={3000} />)
    await run(300)
    expect(hiDef()).not.toBeNull()
  })

  it('shown on the audio view, which never draws the radio scope', async () => {
    render(<PhoneScope feed="audio" cockpit="cw" transmitting={false} theme="dark" viewLoHz={200} viewHiHz={3000} />)
    await run(300)
    expect(hiDef()).not.toBeNull()
  })
})

describe("the radio scope's sweep rate", () => {
  it('is reported from the sweeps that arrive, and is null on the audio', async () => {
    const rates: (number | null)[] = []
    render(<PhoneScope feed="rf" cockpit="rfpan" transmitting={false} theme="dark" onSweepRate={(r) => rates.push(r)} />)
    await run(7000)
    const last = rates[rates.length - 1]
    expect(last).not.toBeNull()
    expect(last!).toBeGreaterThan(4)
    // The stream stops: within a window the rate is unknown again, never the last good one.
    src.stalled = true
    await run(7000)
    expect(rates[rates.length - 1]).toBeNull()
    cleanup()
    const audio: (number | null)[] = []
    render(<PhoneScope feed="audio" cockpit="cw" transmitting={false} theme="dark" onSweepRate={(r) => audio.push(r)} />)
    await run(7000)
    expect(audio.filter((r) => r != null)).toEqual([])
  })
})

describe('a compact scope (the stacked halves of CW Both)', () => {
  const pressed = (re: RegExp) => screen.queryAllByRole('button').filter((b) => re.test(b.title))
  const VIEW = /^(Switch to the 3D|Pause the waterfall|Scrolls (down|up))/
  it('keeps HiDef and ⚙ in its toolbar and moves the rest, G and Z included, behind ⚙', async () => {
    src.source = 'audio'
    render(<PhoneScope feed="audio" cockpit="cw" compact transmitting={false} theme="dark" smeterDb={-6} viewLoHz={200} viewHiHz={3000} />)
    await run(300)
    expect(screen.queryByRole('slider', { name: 'Scope visual gain' })).toBeNull()
    expect(hiDef()).not.toBeNull()
    expect(screen.queryByText('S9')).toBeNull()
    const viewBtns = () => pressed(VIEW)
    expect(viewBtns()).toEqual([])
    act(() => screen.getByRole('button', { name: 'Display settings' }).click())
    expect(viewBtns().length).toBe(3)
    expect(screen.getByRole('slider', { name: 'Scope visual gain' })).toBeTruthy()
  })

  it('control: the full scope keeps them all in its row', async () => {
    src.source = 'audio'
    render(<PhoneScope feed="audio" cockpit="cw" transmitting={false} theme="dark" smeterDb={0} viewLoHz={200} viewHiHz={3000} />)
    await run(300)
    expect(screen.getByText('S9')).toBeTruthy()
    expect(pressed(VIEW).length).toBe(3)
    expect(screen.getByRole('slider', { name: 'Scope visual gain' })).toBeTruthy()
  })

  it('puts its toolbar in the host the cockpit gives it, outside the scope', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    render(<PhoneScope feed="rf" cockpit="rfpan" compact toolbarHost={host} transmitting={false} theme="dark" toolbarStart={<span>chips</span>} />)
    await run(300)
    expect(host.querySelector('.ph-scope-smeter')).not.toBeNull()
    expect(host.textContent).toContain('chips')
    expect(document.querySelector('.ph-scope > .ph-scope-smeter')).toBeNull()
    host.remove()
  })
})
