// @vitest-environment jsdom
//
// The activation said where the operator is: one taken up at launch asks "Still activating …?"
// once, with "Still activating" on the toast, which resumes it (it is paused until then); the
// 0000Z rollover says the count starts again; a Start or the first sight of an activation says
// neither. The real toast bus, so what is asserted is
// what the operator would see.
import { afterEach, expect, it, vi } from 'vitest'
import { renderHook } from '@testing-library/react'
import type { Activation } from './types'
import { t } from './i18n'

const api = vi.hoisted(() => ({ resumeActivation: vi.fn(async () => ({})) }))
vi.mock('./api', () => api)

import { useActivationNotices } from './useActivationNotices'
import { dismissToast, subscribeToasts } from './toast'

function toasts(): { message: string; action?: () => void; actionLabel?: string }[] {
  let live: { message: string; action?: () => void; actionLabel?: string }[] = []
  subscribeToasts((all) => {
    live = all
  })()
  return live
}

afterEach(() => {
  for (const toast of toasts() as unknown as { id: number }[]) dismissToast(toast.id)
  api.resumeActivation.mockClear()
})

const DAY = 20_694
const act = (over: Partial<Activation> = {}): Activation => ({
  program: 'POTA',
  reference: 'US-1234,US-5678',
  qsoCount: 0,
  parks: [
    { reference: 'US-1234', count: 0, p2p: 0 },
    { reference: 'US-5678', count: 0, p2p: 0 },
  ],
  utcDay: DAY,
  ...over,
})

it('asks once about an activation taken up at launch, and its Still activating resumes it', () => {
  const { rerender } = renderHook(({ a }) => useActivationNotices(a), { initialProps: { a: act({ restored: true }) } })
  rerender({ a: act({ restored: true, qsoCount: 1 }) })
  const said = toasts().filter((x) => x.message === t('ota.activation.restored', { reference: 'US-1234, US-5678' }))
  expect(said).toHaveLength(1)
  expect(said[0].actionLabel).toBe(t('ota.activation.resume.label'))
  said[0].action?.()
  expect(api.resumeActivation).toHaveBeenCalledTimes(1)
})

it('says the new UTC day once when 0000Z passes, and nothing at a Start', () => {
  const { rerender } = renderHook(({ a }) => useActivationNotices(a), {
    initialProps: { a: null as Activation | null },
  })
  rerender({ a: act() })
  expect(toasts()).toHaveLength(0)
  rerender({ a: act({ utcDay: DAY + 1 }) })
  rerender({ a: act({ utcDay: DAY + 1, qsoCount: 1 }) })
  expect(toasts().map((x) => x.message)).toEqual([t('ota.activation.newDay', { reference: 'US-1234, US-5678' })])
})
