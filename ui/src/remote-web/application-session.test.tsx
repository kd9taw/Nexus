// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, render, screen } from '@testing-library/react'
import { installApplicationTransport } from '../applicationTransport'
import { useSmeterDb } from '../components/LiveMeters'
import type { MeterReadout } from '../types'

let dispose: (() => void) | undefined
afterEach(() => { cleanup(); dispose?.(); vi.useRealTimers() })
function Meter() { const value = useSmeterDb(); return <output>{value === null ? 'unavailable' : value}</output> }
const reading = (smeterDb: number): MeterReadout => ({ rxLevel: 0, smeterDb, cwToneHz: null })
it('clears station readings on transport handover and ignores an older station response', async () => {
  vi.useFakeTimers()
  let resolveOld!: (value: MeterReadout) => void, calls = 0
  dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(): Promise<T> => {
    return (++calls === 1 ? await new Promise<MeterReadout>(resolve => { resolveOld = resolve }) : reading(-12)) as T
  } })
  render(<Meter />)
  await act(async () => { await vi.advanceTimersByTimeAsync(200) })
  expect(screen.getByRole('status').textContent).toBe('-12')
  act(() => {
    dispose?.()
    dispose = installApplicationTransport({ kind: 'remote', invoke: async <T,>(): Promise<T> => reading(-24) as T })
  })
  expect(screen.getByRole('status').textContent).toBe('unavailable')
  await act(async () => { await vi.advanceTimersByTimeAsync(100) })
  expect(screen.getByRole('status').textContent).toBe('-24')
  await act(async () => { resolveOld(reading(-70)) })
  expect(screen.getByRole('status').textContent).toBe('-24')
})
