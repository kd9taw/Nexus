// @vitest-environment jsdom
//
// THE LOGBOOK GLOBE COUNTS ONLY THE SQUARES THE ENGINE HAS COUNTED: "N grid squares worked" once the engine has
// answered how many, and no count before.
//
// The globe asks the engine for its band's worked squares (`gridPoints`). Until the answer lands (at every open and
// every band picked, for a moment) and for as long as the engine refuses the question (a change made before it is
// still being saved: a slow or a full disk, features/notAnswered), the globe has no answer. It counted the empty log's
// answer in its place: "0 grid squares worked", or "0 grid squares on 20m", over a log with squares on that band. It
// now shows no count until it has one, as the Logbook's own count beside its title does. Held by value against a fake
// engine, through the real source and the real globe; answered counts, an answered 0 among them, are the controls.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { forwardRef, useEffect, useImperativeHandle, useMemo } from 'react'
import { act, cleanup, fireEvent, render } from '@testing-library/react'
import * as THREE from 'three'
import type { LoggedQso } from '../types'
import type { LogQuestion } from '../features/logAnswers'
import { answerAs } from '../features/logAnswers.testkit'
import { ASK_AGAIN_AFTER_MS, ASK_AGAIN_TIMES, NOT_ANSWERED } from '../features/notAnswered'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  askLog: vi.fn(),
}))
// jsdom loads no images, and no picture is under test here.
vi.mock('../features/globeBasemap', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/globeBasemap')>()),
  loadDayImage: () => Promise.resolve(null),
}))
// jsdom has no WebGL: `react-globe.gl` is a stub holding a real three.js scene (QsoGlobe.dots.test.tsx's).
vi.mock('react-globe.gl', () => ({
  default: forwardRef<unknown, Record<string, unknown>>(function Globe(props, ref) {
    const api = useMemo(() => {
      const canvas = document.createElement('canvas')
      const controls = { autoRotate: false, autoRotateSpeed: 0, connect() {}, disconnect() {} }
      const renderer = {
        domElement: canvas,
        capabilities: { maxTextureSize: 4096, getMaxAnisotropy: () => 1 },
        getContext: () => ({ isContextLost: () => false }),
        dispose() {},
        forceContextLoss() {},
      }
      const scene = new THREE.Scene()
      return {
        scene: () => scene,
        lights: () => [],
        renderer: () => renderer,
        controls: () => controls,
        camera: () => new THREE.PerspectiveCamera(),
        getCoords: () => ({ x: 0, y: 0, z: 0 }),
        pauseAnimation() {},
        resumeAnimation() {},
      }
    }, [])
    useImperativeHandle(ref, () => api, [api])
    useEffect(() => {
      ;(props.onGlobeReady as (() => void) | undefined)?.()
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [])
    return <div data-testid="globe" />
  }),
}))

import QsoGlobe from './QsoGlobe'
import { askLog } from '../api'

const qso = (call: string, grid: string | null, band: string) =>
  ({
    call, grid, band, freqMhz: 14.074, mode: 'FT8', rstSent: '-10', rstRcvd: '-12', name: null, qth: null, comment: null,
    notes: null, country: null, whenUnix: 1_700_000_000, confirmed: false, awardConfirmed: false, qslRcvd: null,
    qslSent: null, ota: null,
  }) as unknown as LoggedQso
/** Three squares on three bands, and a 6 m contact with no grid: 6 m is in the log and has no square on it. */
const LOG = [qso('K1ABC', 'FN31', '20m'), qso('K2ABC', 'EM12', '40m'), qso('K3ABC', 'FN20', '2m'), qso('K4ABC', null, '6m')]

/** The fake engine: its log, whether it refuses every question (a change still being saved), and the kinds of
 *  question it holds until the test answers them. */
const engine = { log: [] as LoggedQso[], refusing: false, waiting: new Set<LogQuestion['kind']>(), held: [] as (() => void)[] }

/** The globe's count, or null when it shows none. */
const count = () => document.querySelector('.qso-globe-count')?.textContent ?? null
const pick = (band: string) => fireEvent.change(document.querySelector('.qso-globe-band-pick')!, { target: { value: band } })
/** The asks of the squares. */
const squareAsks = () => vi.mocked(askLog).mock.calls.filter(([q]) => q.kind === 'gridPoints').length
const settle = () => act(async () => {})
const aSecond = () => act(() => vi.advanceTimersByTimeAsync(ASK_AGAIN_AFTER_MS))
const answerHeld = () =>
  act(async () => {
    for (const answer of engine.held.splice(0)) answer()
  })
const open = async () => {
  render(<QsoGlobe logTick={1} />)
  await settle()
}

beforeEach(() => {
  engine.log = LOG
  engine.refusing = false
  engine.waiting = new Set()
  engine.held = []
  vi.mocked(askLog).mockImplementation(async (q: LogQuestion) => {
    if (engine.waiting.has(q.kind)) await new Promise<void>((answer) => engine.held.push(answer))
    if (engine.refusing) throw `${NOT_ANSWERED}: a logbook change is still on its way (0 of 1 saved)`
    return answerAs(q, engine.log)
  })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  globalThis.IntersectionObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof IntersectionObserver
  // No 2-D context in jsdom; the globe needs a size to draw at all.
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(1200)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(320)
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.mocked(askLog).mockReset()
  vi.restoreAllMocks()
})

describe('the Logbook globe before the engine has counted its squares', () => {
  it('shows no count while they are on their way, never "0 grid squares worked"', async () => {
    engine.waiting = new Set(['gridPoints'])
    await open()
    expect(squareAsks(), 'premise: the squares were asked').toBe(1)
    expect(count(), 'a log with 3 squares is never counted 0').toBeNull()
    await answerHeld()
    expect(count()).toBe('3 grid squares worked')
  })

  it('refused while a change is being saved: no count after every ask, and a minute later', async () => {
    engine.refusing = true
    await open()
    expect(count(), 'refused once').toBeNull()
    for (let i = 0; i < ASK_AGAIN_TIMES; i++) await aSecond()
    expect(squareAsks(), 'premise: every ask refused, the last one too').toBe(1 + ASK_AGAIN_TIMES)
    expect(count(), 'refused every time').toBeNull()
    await act(() => vi.advanceTimersByTimeAsync(60_000))
    expect(count(), 'a minute later').toBeNull()
  })

  it('a band picked: no count until that band’s squares are in, then that band’s', async () => {
    await open()
    expect(count(), 'premise: the whole log, answered').toBe('3 grid squares worked')
    engine.waiting = new Set(['gridPoints'])
    pick('20m')
    await settle()
    expect(count(), '20 m before its answer').toBeNull()
    await answerHeld()
    expect(count()).toBe('1 grid square on 20m')
  })
})

describe('the Logbook globe once the engine has counted (the controls)', () => {
  it('a log with no grid in it: "0 grid squares worked"', async () => {
    engine.log = [qso('K1ABC', null, '20m'), qso('K2ABC', null, '40m')]
    await open()
    expect(count()).toBe('0 grid squares worked')
  })

  it('a band in the log with no square on it: "0 grid squares on 6m"', async () => {
    await open()
    pick('6m')
    await settle()
    expect(count()).toBe('0 grid squares on 6m')
  })
})
