// @vitest-environment jsdom
//
// The header rotor strip never shows a raw `{{bird}}` or a `NaN°T`.
//
// The FT and RTTY headers once read "ROTOR NaN°T ⟳ {{bird}} +dial": the strip trusted whatever
// the rotor and satellite reads answered, so a bearing that was not a number went through
// Math.round into the readout and the needle, and a held transponder with no name went into the
// catalogue string with its `bird` argument undefined. The desktop backend answers null or a real
// number and always names the bird, but the strip is also mounted against other answers (the
// layout probes serve `{}` for every read they do not model), and an empty or malformed answer
// must read as "nothing there", never as a broken readout.
//
// Every case checks the whole rendered markup, attributes included, so a placeholder in a
// tooltip or a NaN in the needle's transform is caught as well as one in the visible text.
import { it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, render, cleanup, waitFor } from '@testing-library/react'
import { RotorStrip } from './RotorStrip'

const api = vi.hoisted(() => ({
  readRotator: vi.fn((): Promise<unknown> => Promise.resolve(null)),
  stopRotator: vi.fn(() => Promise.resolve()),
  getDeclination: vi.fn((): Promise<number | null> => Promise.resolve(null)),
  getSettings: vi.fn(() => Promise.resolve({ rotatorModel: 1, rotatorHost: '' } as never)),
  getSatTrackStatus: vi.fn((): Promise<unknown> => Promise.resolve(null)),
  stopSatTrack: vi.fn(() => Promise.resolve()),
  getSatTransponder: vi.fn((): Promise<unknown> => Promise.resolve(null)),
  setSatTransponder: vi.fn(() => Promise.resolve()),
}))
vi.mock('../api', () => api)
vi.mock('../toast', () => ({ pushToast: vi.fn() }))

beforeEach(() => {
  for (const f of [api.readRotator, api.getSatTrackStatus, api.getSatTransponder]) {
    f.mockReset()
    f.mockImplementation(() => Promise.resolve(null))
  }
})
afterEach(cleanup)

async function rendered(): Promise<string> {
  const { container } = render(<RotorStrip />)
  await waitFor(() => expect(api.getSatTransponder).toHaveBeenCalled())
  await act(async () => {})
  return container.innerHTML
}

function expectNoBrokenReadout(html: string) {
  expect(html).not.toContain('{{')
  expect(html).not.toContain('NaN')
  expect(html).not.toContain('undefined')
}

it('an empty answer from every read (the probe stub shape) renders no placeholder and no NaN', async () => {
  api.readRotator.mockImplementation(() => Promise.resolve({}))
  api.getSatTrackStatus.mockImplementation(() => Promise.resolve({}))
  api.getSatTransponder.mockImplementation(() => Promise.resolve({}))
  expectNoBrokenReadout(await rendered())
})

it('a bearing that is not a finite number reads as no bearing, and the held bird is still named', async () => {
  for (const bad of [Number.NaN, Number.POSITIVE_INFINITY, '123', undefined]) {
    api.readRotator.mockImplementation(() => Promise.resolve(bad))
    api.getSatTransponder.mockImplementation(() =>
      Promise.resolve({ name: 'QO-100', index: 0, description: 'NB transponder', binding: null }),
    )
    const html = await rendered()
    expectNoBrokenReadout(html)
    expect(html, String(bad)).toContain('QO-100')
    expect(html, String(bad)).not.toContain('°T')
    cleanup()
  }
})

it('CONTROL: a real bearing and a named hold still read out in full', async () => {
  api.readRotator.mockImplementation(() => Promise.resolve(123.4))
  api.getSatTransponder.mockImplementation(() =>
    Promise.resolve({ name: 'QO-100', index: 0, description: 'NB transponder', binding: null }),
  )
  const html = await rendered()
  expectNoBrokenReadout(html)
  expect(html).toContain('123°T')
  expect(html).toContain('QO-100')
})
