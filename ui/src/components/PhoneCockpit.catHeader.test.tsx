// @vitest-environment jsdom
//
// STOP TX STAYS PUT WHEN THE RADIO'S STATE CHANGES — the Phone header (OperateCockpit.structure.test.tsx
// has the FT twin). The header is in flow above the TX strip, so a box it adds or drops re-wraps it and
// moves Stop TX: "⚠ no rig control" and the rig-mode chip appearing on a one-row header moved Stop TX
// 32 px at 1920x1080 (real Chrome). The state words now live in `.ch-statecell`, which is always drawn
// and zero wide to the line breaker (cockpit-panes.css, computed in cockpit-panes.test.ts); outside it,
// the radio's state may repaint a header box but not add or remove one.
// ⚠️ jsdom never lays out: this pins the boxes; their sizes are the browser sweep's job.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

const { setPtt, armStreamMic, releaseStreamMic } = vi.hoisted(() => ({
  setPtt: vi.fn(async (_on: boolean) => ({})),
  armStreamMic: vi.fn(async () => true),
  releaseStreamMic: vi.fn(async () => {}),
}))

vi.mock('../api', async (importOriginal) => {
  // Derived from the real module (stop-line.test.tsx's rule): the real header mounts more of it
  // than this file names, and a hand-kept list throws on mount the day one is added.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    setPtt,
    armStreamMic,
    releaseStreamMic,
    getSettings: vi.fn(async () => ({})),
    readRotator: vi.fn(async () => null),
    getDeclination: vi.fn(async () => 0),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    getLicensedBandPlan: vi.fn(async () => []),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// Canvas and feed children only. CockpitHeader and CockpitTxStrip are DELIBERATELY REAL.
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

function makeSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.2, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
      transmitting: false, tuning: false, txEnabled: true, txAllowed: true, txBusyReason: null, rigKeyed: false,
      streamMic: null, qsoRecording: false, rfPower: null, micGain: null, nrLevel: 0.3, agc: 'fast', nb: true,
      nr: true, notch: null, comp: null, vox: null, filterWidthHz: null, splitTxMhz: null, smeterDb: null,
      rxLevel: 0, phoneSegLo: null, phoneSegHi: null, ...radio,
    },
  } as unknown as AppSnapshot
}
const phone = (radio: Record<string, unknown> = {}) =>
  <PhoneCockpit snap={makeSnap(radio)} theme="dark" onWorkSpot={() => {}} spots={[]} />
const settle = () => act(async () => { for (let i = 0; i < 5; i++) await Promise.resolve() })
/** Every header box outside the state cell — and outside Split's own "⊘ not reported" mark, which the
 *  2026-09-20 ruling keeps on Phone's Split while CAT is down (inline at sm and xl only; at md and lg
 *  Split is behind the header's ⋯, out of flow). */
const outsideCell = () =>
  [...document.querySelectorAll('.cockpit-header *')]
    .filter((e) => !e.closest('.ch-statecell') && !e.closest('.ph-split .ph-unavail'))
    .map((e) => `${e.tagName.toLowerCase()}.${e.classList[0] ?? ''}`)
const cellWords = () => [...document.querySelectorAll('.cockpit-header .ch-statecell > *')].map((e) => e.className)

describe('the Phone header as the radio changes', () => {
  it.each([
    ['the rig on a mode it was not told', { rigMode: 'FM' }],
    ['no rig control', { catOk: false }],
  ])('%s: its words go in the state cell, and no box outside it comes or goes', async (_name, radio) => {
    const view = render(phone())
    await settle()
    const before = outsideCell()
    expect(cellWords()).toEqual([])
    view.rerender(phone(radio))
    await settle()
    expect(cellWords().length).toBe(1)
    for (const c of cellWords()) expect(c).toContain('ch-stateword')
    expect(outsideCell()).toEqual(before)
  })
})
