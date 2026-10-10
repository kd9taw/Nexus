// @vitest-environment jsdom
//
// THE TORN-OFF SATELLITES WINDOW'S LOG LINE: its hunt tag's ✕ ends the hunt, and the window applies
// the snapshot the clear answered, as the main window does (App.huntChipClear.test.tsx). The window's
// subscription pushes one hunted snapshot and nothing after it, so only the handed one can take the
// tag away.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, waitFor, within } from '@testing-library/react'
import type { AppSnapshot } from './types'

const push = vi.hoisted(() => ({ fn: null as ((s: AppSnapshot) => void) | null }))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers, APP_SNAPSHOT } = await import('./appCockpits.testkit')
  return {
    ...auto,
    ...appApiAnswers(),
    subscribeSnapshot: vi.fn((fn: (s: AppSnapshot) => void) => {
      push.fn = fn
      return () => {}
    }),
    clearHuntTarget: vi.fn(async () => ({ ...APP_SNAPSHOT, hunt: null })),
    // No catalog (App.connectBoards.test.tsx's answer); the log line needs none.
    getSatellites: vi.fn(async () => null),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    qrzLookup: vi.fn(async () => null),
  }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))

import { DetachedPanel } from './DetachedPanel'
import { clearHuntTarget } from './api'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import { t } from './i18n'

vi.setConfig({ testTimeout: 15_000 })

afterEach(cleanup)

describe('the torn-off Satellites window', () => {
  it('ends the hunt from its log line’s tag, and the tag goes with the snapshot it answered', async () => {
    render(<DetachedPanel panel="sats" />)
    await waitFor(() => expect(push.fn).toBeTruthy())
    act(() => push.fn!({ ...APP_SNAPSHOT, hunt: { program: 'POTA', reference: 'US-1000', call: 'K9ABC' } } as unknown as AppSnapshot))
    const tag = await waitFor(() => {
      const el = document.querySelector<HTMLElement>('.sats-log .le-hunt-chip')
      expect(el, 'no hunt tag on the window’s log line').toBeTruthy()
      return el!
    })
    fireEvent.click(within(tag).getByRole('button', { name: t('ota.hunt.clear') }))
    await waitFor(() => expect(document.querySelector('.sats-log .le-hunt-chip'), 'the tag outlived the hunt').toBeNull())
    expect(vi.mocked(clearHuntTarget).mock.calls).toEqual([[]])
  })
})
