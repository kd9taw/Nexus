// @vitest-environment jsdom
// Hysteresis on the stale DISPLAY (operator decision 2026-09-14). `stale` means the station's readings
// are older than APPLICATION_TIMEOUT_MS (3 s); controls already refuse from that moment. The fade and the
// "Station data unavailable" wording wait until the loss has lasted STALE_DISPLAY_HOLD_MS more, so a brief
// gap shows nothing, and they clear the moment readings return.
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { STALE_DISPLAY_HOLD_MS, useStaleDisplay } from './useStaleDisplay'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'

afterEach(() => { cleanup(); vi.useRealTimers() })
function Probe({ stale }: { stale: boolean }) { return <span>{String(useStaleDisplay(stale))}</span> }
async function step(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms) }) }

it('shows the stale display only after five seconds of continuous loss, and clears it on recovery', async () => {
  vi.useFakeTimers()
  // Five seconds since the last sample: three before readings count as stale, then the hold.
  expect(APPLICATION_TIMEOUT_MS + STALE_DISPLAY_HOLD_MS).toBe(5000)
  const ui = render(<Probe stale={false}/>)
  ui.rerender(<Probe stale/>)
  await step(STALE_DISPLAY_HOLD_MS - 1)
  expect(ui.container.textContent).toBe('false')
  await step(1)
  expect(ui.container.textContent).toBe('true')
  // Recovery clears it in the same render, with no delay to flash through.
  ui.rerender(<Probe stale={false}/>)
  expect(ui.container.textContent).toBe('false')
})

it('a brief or flapping loss never shows the stale display', async () => {
  vi.useFakeTimers()
  const ui = render(<Probe stale={false}/>)
  const seen: string[] = []
  for (let n = 0; n < 5; n++) {
    ui.rerender(<Probe stale/>)
    for (let elapsed = 0; elapsed < STALE_DISPLAY_HOLD_MS - 100; elapsed += 50) { await step(50); seen.push(ui.container.textContent!) }
    ui.rerender(<Probe stale={false}/>)
    await step(50); seen.push(ui.container.textContent!)
  }
  expect(seen.length).toBeGreaterThan(100)
  expect(seen.every(value => value === 'false')).toBe(true)
})
