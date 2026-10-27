// @vitest-environment jsdom
//
// YOUR PARK ON THE MAP. With the parks layer on, the park you are activating is drawn and named,
// and the parks you have activated are drawn without a name; with the layer off, none is read or
// drawn. jsdom has no 2-D canvas, so the canvas gets a context that records its calls. MapView is
// the REAL component.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'

const feeds = vi.hoisted(() => ({
  pota: { calls: [], parks: [] } as unknown,
}))
const api = vi.hoisted(() => ({
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
  getPotaActivator: vi.fn(async () => feeds.pota),
}))
vi.mock('../api', () => api)

import { MapView } from './MapView'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}

type Op = { k: string; a: unknown[] }
const ops: Op[] = []
function recordingContext(canvas: HTMLCanvasElement) {
  const store: Record<string | symbol, unknown> = {}
  return new Proxy(store, {
    get: (t, k) => {
      if (k in t) return t[k]
      if (k === 'canvas') return canvas
      return (...a: unknown[]) => {
        ops.push({ k: String(k), a })
        if (k === 'measureText') return { width: 0 }
        return { addColorStop() {} }
      }
    },
  })
}

beforeEach(() => {
  localStorage.clear()
  ops.length = 0
  for (const f of Object.values(api)) f.mockClear()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO
  const ctxs = new WeakMap<HTMLCanvasElement, unknown>()
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    if (!ctxs.has(this)) ctxs.set(this, recordingContext(this))
    return ctxs.get(this) as RenderingContext
  })
  vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockImplementation(() => 800)
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockImplementation(() => 500)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

function parksLayer(on: boolean): void {
  localStorage.setItem('nexus.connect.intents', JSON.stringify({ dx: { layers: { ota: { visible: on } } } }))
}

async function mount() {
  await act(async () => {
    render(
      <MapView
        myGrid="EN52"
        theme={'dark' as never}
        stations={[]}
        prop={null}
        selectedCall={null}
        onSelectCall={() => {}}
        needByCall={new Map()}
        intent="dx"
        projection="world"
      />,
    )
  })
  await act(async () => {})
  await act(async () => {})
}

/** Every text the canvas filled. */
const texts = () => ops.filter((o) => o.k === 'fillText').map((o) => String(o.a[0]))

describe('MapView: your parks on the parks layer', () => {
  it('names the park you are activating and draws the ones you have activated unnamed', async () => {
    feeds.pota = {
      calls: [],
      parks: [
        { reference: 'US-0002', name: 'Here', lat: 44.35, lon: -68.21, approx: false, activating: true, activations: 3 },
        { reference: 'US-0009', name: 'There', lat: 29.25, lon: -103.25, approx: false, activating: false, activations: 21 },
      ],
    }
    parksLayer(true)
    await mount()
    expect(api.getPotaActivator).toHaveBeenCalled()
    expect(texts()).toContain('US-0002')
    expect(texts()).not.toContain('US-0009')
  })

  it('with the parks layer off, reads and draws none of them', async () => {
    feeds.pota = {
      calls: [],
      parks: [{ reference: 'US-0002', name: 'Here', lat: 44.35, lon: -68.21, approx: false, activating: true, activations: 3 }],
    }
    parksLayer(false)
    await mount()
    expect(api.getPotaActivator).not.toHaveBeenCalled()
    expect(texts()).not.toContain('US-0002')
  })
})
