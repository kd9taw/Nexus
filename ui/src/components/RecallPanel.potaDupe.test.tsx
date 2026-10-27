// @vitest-environment jsdom
//
// While activating, the card's dupe is POTA's: "Dupe today @ US-1234", naming the earlier
// contact's band, mode and time, and never a refusal. The lifetime "Dupe 20m" — worked ever on
// this band — is hidden meanwhile, because a hunter worked last week is no dupe for today's
// activation; outside an activation the card is as it always was.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { RecallPanel } from './RecallPanel'
import type { CallHistory } from '../features/callHistory'
import type { LoggedQso } from '../types'
import { t } from '../i18n'

vi.mock('../api', () => ({
  openQrzPage: vi.fn(async () => {}),
  rotatorBearingToCall: vi.fn(() => Promise.reject(new Error('unset'))),
}))

afterEach(cleanup)

// 2026-10-25 14:02Z.
const earlier = {
  call: 'W1ABC',
  band: '20m',
  mode: 'SSB',
  whenUnix: Date.UTC(2026, 9, 25, 14, 2) / 1000,
  ota: { myProgram: 'POTA', myRef: 'US-1234' },
} as LoggedQso
const hist: CallHistory = {
  qsos: [earlier],
  count: 1,
  workedBefore: true,
  dupeThisBand: true,
  lastUnix: earlier.whenUnix,
  confirmedCount: 0,
  bands: ['20m'],
  modes: ['SSB'],
}

it('shows the POTA dupe with the earlier contact, and hides the lifetime badge', () => {
  render(<RecallPanel call="W1ABC" band="20m" hist={hist} activating potaDupe={{ park: 'US-1234', qso: earlier }} />)
  const badge = screen.getByText(t('recall.potaDupe.label', { park: 'US-1234' }))
  expect(badge.getAttribute('title')).toBe(
    t('recall.potaDupe.title', { park: 'US-1234', band: '20m', mode: 'SSB', time: '1402' }),
  )
  expect(screen.queryByText(t('recall.dupe.label', { band: '20m' }))).toBeNull()
})

it('activating with no POTA dupe shows neither', () => {
  render(<RecallPanel call="W1ABC" band="20m" hist={hist} activating potaDupe={null} />)
  expect(screen.queryByText(t('recall.potaDupe.label', { park: 'US-1234' }))).toBeNull()
  expect(screen.queryByText(t('recall.dupe.label', { band: '20m' }))).toBeNull()
})

it('outside an activation the lifetime badge is as it always was', () => {
  render(<RecallPanel call="W1ABC" band="20m" hist={hist} />)
  expect(screen.getByText(t('recall.dupe.label', { band: '20m' }))).toBeTruthy()
})
