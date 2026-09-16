// @vitest-environment jsdom
// The alert controls must keep a STABLE OBJECT IDENTITY across a re-render.
//
// Why this is a test and not a comment. BrowserApplication re-renders itself every 500 ms to
// re-read the sample age, and its `status` memo feeds the `remote` object that every cockpit
// reads. If a dependency of that memo is a fresh object literal, `remote` changes identity twice
// a second and the whole workspace re-renders with it — which an operator sees as the page
// flashing, controls that will not take a click, and everything feeling slow.
//
// That contract WAS written down, in a comment directly above the memo in BrowserApplication.tsx.
// In 1.13.0 three unstable dependencies (`alerts`, `rareAlerts`, `potaAlerts`) were added
// underneath it anyway, and every gate stayed green, because a comment cannot fail. This file is
// the same contract in a form that can.
import { expect, it } from 'vitest'
import { renderHook } from '@testing-library/react'

import { useAlertOptIn } from './browser-alerts'
import { useRareDxAlerts } from './useRareDxAlerts'
import { usePotaAlerts } from './usePotaAlerts'

it('useAlertOptIn returns the same object when nothing about it changed', () => {
  const { result, rerender } = renderHook(() => useAlertOptIn('nexus.test.alerts.optin'))
  const first = result.current
  rerender()
  rerender()
  expect(result.current).toBe(first)
})

it('useRareDxAlerts returns the same object when nothing about it changed', () => {
  // No source and not ready: the poll never runs, so any identity change is the return literal.
  const { result, rerender } = renderHook(() => useRareDxAlerts(null, false, false))
  const first = result.current
  rerender()
  rerender()
  expect(result.current).toBe(first)
})

it('usePotaAlerts returns the same object when nothing about it changed', () => {
  const { result, rerender } = renderHook(() => usePotaAlerts(null, false, false))
  const first = result.current
  rerender()
  rerender()
  expect(result.current).toBe(first)
})

it('a changed input does change identity, so the check above is not vacuous', () => {
  // The positive control. Without this, all three tests above would still pass against a hook
  // that returned a frozen constant and never reported anything.
  const { result, rerender } = renderHook(({ offered }) => useRareDxAlerts(null, false, offered),
    { initialProps: { offered: false } })
  const first = result.current
  rerender({ offered: true })
  expect(result.current).not.toBe(first)
  expect(result.current.offered).toBe(true)
})
