// @vitest-environment jsdom
//
// THE SCOPE HEAD ON CW AND PHONE: the title, then the controls packed against the strip's ✕.
//
// CW's "Colors" sat against the zero-beat reading, in the same small uppercase, and read as part
// of it, while its picker stood in the middle of the row. Two causes, both pinned here:
//   1. "Colors" and the picker were two loose items in the row. They are one group now, a
//      <label> around both, after the zero beat.
//   2. The row had more than one auto margin (the title's, the picker's and the ✕'s), and the
//      free space was split between them. The title's must be the only one in front of the
//      controls; Phone's head had the same split, its picker short of the ✕.
//
// ⚠️ jsdom DOES NOT LAY OUT. The margins are the cascade's winners over the real sheets
// (cssCascade.testkit), not boxes; the gaps themselves are a browser measurement.
import type { ReactNode } from 'react'
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { PhoneCockpit } from './PhoneCockpit'
import { loadSheets, css } from '../cssCascade.testkit'
import { EN } from '../i18n'
import type { AppSnapshot } from '../types'
import type { PanelLayoutApi } from '../features/panelState'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const answers: Record<string, unknown> = {
    getCatCwUnprovenRigModels: [],
    getVoiceMessages: [],
    getLicensedBandPlan: [],
    cwDecode: { text: '', pending: '', wpm: 0, sent: [], keyerError: null, candidates: [], rst: null, name: null },
  }
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? vi.fn(async () => answers[name] ?? null) : actual[name]
  }
  return mod
})
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: ReactNode }) => <header className="cockpit-header">{children}</header>,
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => loadSheets())
beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver
})
afterEach(cleanup)

const radio = {
  dialMhz: 14.025, band: '20m', catOk: true, transmitting: false, txEnabled: true, txAllowed: true,
  cwWpm: 25, cwKeyer: 'cat', filterWidthHz: 500, splitTxMhz: null,
}
const cwSnap = { mycall: 'KD9TAW', aiCw: { enabled: true, status: '', text: '' }, radio: { ...radio, sideband: 'USB', rigMode: 'CW' } } as unknown as AppSnapshot
const phoneSnap = { mycall: 'KD9TAW', radio: { ...radio, dialMhz: 14.2, sideband: 'USB', rigMode: 'USB' } } as unknown as AppSnapshot

async function headOf(node: ReactNode): Promise<HTMLElement> {
  render(<>{node}</>)
  await act(async () => {
    for (let i = 0; i < 8; i++) await Promise.resolve()
  })
  const head = document.querySelector('.ph-scope-head') as HTMLElement | null
  expect(head, 'the scope head').not.toBeNull()
  return head!
}
/** A panel layout with every pane docked: the strip's ✕ renders only for a host that can hide it,
 *  and the ✕ is the item whose margin split the row. */
function panels<Id extends string>(): PanelLayoutApi<Id> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: () => 'docked',
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  } as unknown as PanelLayoutApi<Id>
}
const cwHead = () => headOf(<CwCockpit snap={cwSnap} theme="dark" onWorkSpot={() => {}} spots={[]} panels={panels()} />)
const phoneHead = () => headOf(<PhoneCockpit snap={phoneSnap} theme="dark" onWorkSpot={() => {}} panels={panels()} />)

/** The row's items by their first class, in order. */
const items = (head: Element) => [...head.children].filter((c) => !c.hasAttribute('hidden'))
const named = (els: Element[]) => els.map((c) => `${c.tagName.toLowerCase()}.${c.classList[0] ?? ''}`)

describe('the CW scope head', () => {
  it('"Colors" and its picker are one group after the zero-beat reading, and the picker keeps its own name', async () => {
    const head = await cwHead()
    expect(named(items(head)), 'the row: title, zero beat, the Colors group, the ✕').toEqual([
      'span.ph-scope-title',
      'span.zb',
      'label.ph-scope-palette',
      'button.cockpit-popout',
    ])
    const group = head.querySelector('.ph-scope-palette')!
    expect(named([...group.children]), 'the word and its select, siblings in the group').toEqual([
      'span.ph-scope-head-label',
      'select.wf-palette',
    ])
    expect(group.textContent?.startsWith(EN['cw.scope.colors.label'])).toBe(true)
    expect(head.querySelector('.zb')!.contains(group.querySelector('.ph-scope-head-label')), 'inside the zero beat').toBe(false)
    expect(group.querySelector('select')!.getAttribute('aria-label')).toBe(EN['waterfall.palette.aria.shared'])
    // The group is shrink-to-fit, which is what leaves the picker's own auto margin no space.
    expect(css(group, 'display')).toBe('inline-flex')
  })
})

describe('the free space of a scope head is in front of its controls, nowhere else', () => {
  for (const [name, head] of [
    ['CW', cwHead],
    ['Phone', phoneHead],
  ] as const) {
    it(`${name}: only the title, and at most the item right after it, take an auto margin`, async () => {
      const row = items(await head())
      expect(row[0].classList.contains('ph-scope-title')).toBe(true)
      expect(row[row.length - 1].classList.contains('cockpit-popout'), `${name}: the strip's ✕ ends the row`).toBe(true)
      const auto = row.flatMap((el, i) =>
        (['margin-left', 'margin-right'] as const)
          .filter((side) => css(el, side) === 'auto')
          .map((side) => `${i}:${el.classList[0]}:${side}`),
      )
      // Two auto margins in the same gap (the title's right, the next item's left) are one free
      // space; anywhere else they split it.
      const allowed = ['0:ph-scope-title:margin-right', `1:${row[1].classList[0]}:margin-left`]
      expect(auto.filter((a) => !allowed.includes(a)), `${name}: auto margins past the title's gap`).toEqual([])
      expect(auto, `${name}: the title's own`).toContain('0:ph-scope-title:margin-right')
    })
  }
})
