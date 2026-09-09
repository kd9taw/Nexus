// @vitest-environment jsdom
//
// FULL SCREEN ON THE CONNECT SURFACE — the map's button has to clear CONNECT's frame too.
//
// This is the seam, and it is the whole feature: MapView can only hide its own toolbar and
// Layers panel, but on Connect the map is the centre cell of a wrap-the-globe grid — a
// header, four rail panes and a three-up bottom strip around it. Hiding the 200 px Layers
// column and leaving 600 px of rails is not "the whole window is map".
//
// ⚠️ THE REAL MapView IS MOUNTED HERE, deliberately. The sibling Connect tests stub it
// (ConnectView.bandoutlook.test.tsx:26) because they are about data cadence; a stub here
// would prove that a prop is passed and nothing about the button actually reaching the
// frame — the stubbed-component seam bug this codebase has shipped before.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, fireEvent, screen, act } from '@testing-library/react'

// Derived from the real module — a partial mock leaves every other export undefined and the
// component throws on mount (the AmpStrip.test.tsx trap). Both the Connect polls and the
// map's own reads are stubbed; nothing here touches a backend.
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLog: vi.fn(async () => []),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))
import { ConnectView } from './ConnectView'

const props = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  needAlerts: [],
  amp: null,
}

async function mount() {
  let r!: ReturnType<typeof render>
  await act(async () => {
    r = render(<ConnectView {...props} />)
  })
  return r
}

/** What "full screen" has to remove: Connect's own frame around the map cell. */
function frame(c: HTMLElement) {
  return {
    header: c.querySelector('.connect-header'),
    panes: c.querySelectorAll('.pane-frame').length,
    strip: c.querySelector('.connect-strip'),
    shell: c.querySelector('.connect-shell')?.className ?? '',
    map: c.querySelector('.map-view'),
  }
}

describe('the map button clears the whole Connect frame, not just the Layers panel', () => {
  beforeEach(() => {
    localStorage.clear()
    // jsdom has no ResizeObserver; the house stub (stop-line.test.tsx:299).
    globalThis.ResizeObserver = class {
      observe() {}
      disconnect() {}
      unobserve() {}
    } as unknown as typeof ResizeObserver
  })
  afterEach(() => cleanup())

  it('hides the header, the rail panes and the bottom strip — and puts them all back', async () => {
    const { container } = await mount()
    const before = frame(container)
    expect(before.header, 'control: the frame is there to start with').not.toBeNull()
    expect(before.panes, 'control: rail + strip panes are mounted').toBeGreaterThan(0)
    expect(before.strip).not.toBeNull()

    fireEvent.click(screen.getByRole('button', { name: /full screen/i }))

    const on = frame(container)
    expect(on.header).toBeNull()
    expect(on.panes).toBe(0)
    expect(on.strip).toBeNull()
    expect(on.shell).toContain('map-full')
    expect(on.map, 'the map itself is the one thing that stays').not.toBeNull()

    // The same control, still on screen inside the map's toolbar, brings the frame back.
    fireEvent.click(screen.getByRole('button', { name: /exit full screen/i }))
    const off = frame(container)
    expect(off.header).not.toBeNull()
    expect(off.panes).toBe(before.panes)
    expect(off.strip).not.toBeNull()
    expect(off.shell).not.toContain('map-full')
  })

  it('Escape brings the frame back too', async () => {
    const { container } = await mount()
    fireEvent.click(screen.getByRole('button', { name: /full screen/i }))
    expect(frame(container).header).toBeNull()

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(frame(container).header, 'Escape must restore Connect’s frame, not only the map’s').not.toBeNull()
    expect(frame(container).shell).not.toContain('map-full')
  })
})
