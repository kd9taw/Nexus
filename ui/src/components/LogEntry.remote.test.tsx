// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { LogEntry } from './LogEntry'
import type { AppSnapshot } from '../types'
import * as api from '../api'
vi.mock('../api', () =>
  Object.fromEntries(
    [
      'fdLogManual',
      'logQso',
      'getLog',
      'lookupPark',
      'lookupParkLive',
      'qrzLookup',
      'resolveEntity',
      'searchParks',
      'setCwPeerInfo'
    ].map((n) => [n, vi.fn(async () => null)])
  )
)
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})
const snap = {
  radio: { band: '20m', dialMhz: 14.25 },
  hunt: null,
  mygrid: 'EN52',
  stations: []
} as unknown as AppSnapshot
const call = () => screen.getByPlaceholderText('Call') as HTMLInputElement
it('uses the real log form with remote submit only, and clears only after confirmed success', async () => {
  const submit = vi.fn(async (_record: unknown) => {})
  render(
    <LogEntry
      snap={snap}
      mode="SSB"
      defaultRst="59"
      exchange="terrestrial"
      remote={{
        submit,
        canSubmit: true,
        busy: false,
        resetKey: 0,
        recall: (c) => <div>Remote recall {c}</div>
      }}
    />
  )
  fireEvent.change(call(), { target: { value: 'w1aw' } })
  fireEvent.blur(call())
  fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1))
  expect(submit.mock.calls[0][0]).toMatchObject({
    call: 'W1AW',
    freqMhz: 14.25,
    band: '20m',
    mode: 'SSB',
    confirmed: false,
    awardConfirmed: false
  })
  await waitFor(() => expect(call().value).toBe(''))
  for (const f of Object.values(api)) expect(f).not.toHaveBeenCalled()
})
it('keeps the draft on an uncertain result and refuses both click and Enter without control', async () => {
  const submit = vi.fn(async () => {
    throw Error('operationUnknown')
  })
  const props = { snap, mode: 'CW', defaultRst: '599', exchange: 'terrestrial' as const },
    remote = { submit, canSubmit: true, busy: false, resetKey: 0, recall: () => null }
  const { rerender } = render(<LogEntry {...props} remote={remote} />)
  fireEvent.change(call(), { target: { value: 'W1AW' } })
  fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1))
  expect(call().value).toBe('W1AW')
  rerender(<LogEntry {...props} remote={{ ...remote, canSubmit: false }} />)
  fireEvent.keyDown(call(), { key: 'Enter' })
  fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  expect(submit).toHaveBeenCalledTimes(1)
  expect(call().value).toBe('W1AW')
  for (const f of Object.values(api)) expect(f).not.toHaveBeenCalled()
})

it('keeps a browser draft and its recall/log context on the original frequency after another QSY', async () => {
  const submit = vi.fn(async (_record: unknown) => {}), recall = vi.fn(() => null)
  const remote = { submit, canSubmit: true, busy: false, resetKey: 0, recall }
  const props = { snap, mode: 'SSB', defaultRst: '59', exchange: 'terrestrial' as const }
  const h = render(<LogEntry {...props} remote={remote} />)
  fireEvent.change(call(), { target: { value: 'W1AW' } })
  h.rerender(<LogEntry {...props} snap={{ ...snap, radio: { ...snap.radio, band: '40m', dialMhz: 7.198 } }} remote={remote} />)
  expect(document.querySelector('.le-hint')?.textContent).toContain('20m')
  expect(document.querySelector('.le-hint')?.textContent).toContain('14.250')
  expect(recall).toHaveBeenLastCalledWith('W1AW', { band: '20m', freqMhz: 14.25, mode: 'SSB' })
  fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1))
  expect(submit.mock.calls[0][0]).toMatchObject({ call: 'W1AW', band: '20m', freqMhz: 14.25, mode: 'SSB' })
  for (const f of Object.values(api)) expect(f).not.toHaveBeenCalled()
})

it('fills an empty contact only from its own Work handoff and keeps that confirmed context', () => {
  const submit = vi.fn(async () => {}), consume = vi.fn()
  const remote = { submit, canSubmit: true, busy: false, resetKey: 0, recall: () => null }
  const props = { snap, mode: 'SSB', defaultRst: '59', exchange: 'terrestrial' as const }
  const h = render(<LogEntry {...props} remote={remote} pendingWork={{ call: 'n2spot', ts: 1 }} onConsumeWork={consume} />)
  expect(call().value).toBe('N2SPOT')
  expect(consume).toHaveBeenCalledTimes(1)
  expect(screen.queryByRole('button', { name: /Clear draft and use/ })).toBeNull()
  h.rerender(<LogEntry {...props} snap={{ ...snap, radio: { ...snap.radio, band: '40m', dialMhz: 7.198 } }} remote={remote} />)
  expect(call().value).toBe('N2SPOT')
  expect(document.querySelector('.le-hint')?.textContent).toContain('14.250')
  expect(submit).not.toHaveBeenCalled()
})

it('preserves an existing draft until an explicit replacement and blocks replacement during an unresolved QSO', async () => {
  const submit = vi.fn(async (_record: unknown) => {}), consume = vi.fn()
  const remote = { submit, canSubmit: true, busy: false, resetKey: 0, recall: () => null }
  const props = { snap, mode: 'SSB', defaultRst: '59', exchange: 'terrestrial' as const }
  const h = render(<LogEntry {...props} remote={remote} />)
  fireEvent.change(call(), { target: { value: 'W1AW' } })
  fireEvent.change(document.querySelector('.le-rst')!, { target: { value: '57' } })
  const after = { ...snap, radio: { ...snap.radio, band: '40m', dialMhz: 7.198 } }
  const pendingWork = { call: 'N2SPOT', ts: 2 }
  h.rerender(<LogEntry {...props} snap={after} remote={{ ...remote, pending: true }} pendingWork={pendingWork} onConsumeWork={consume} />)
  const replace = screen.getByRole('button', { name: 'Clear draft and use N2SPOT' }) as HTMLButtonElement
  expect(replace.disabled).toBe(true)
  fireEvent.click(replace)
  expect(call().value).toBe('W1AW')
  expect((document.querySelector('.le-rst') as HTMLInputElement).value).toBe('57')
  expect(document.querySelector('.le-hint')?.textContent).toContain('14.250')
  h.rerender(<LogEntry {...props} snap={after} remote={remote} />)
  fireEvent.click(screen.getByRole('button', { name: 'Clear draft and use N2SPOT' }))
  expect(call().value).toBe('N2SPOT')
  expect((document.querySelector('.le-rst') as HTMLInputElement).value).toBe('59')
  expect(document.querySelector('.le-hint')?.textContent).toContain('7.198')
  expect(consume).toHaveBeenCalledTimes(1)
  expect(submit).not.toHaveBeenCalled()
  fireEvent.click(screen.getByRole('button', { name: 'Log' }))
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1))
  expect(submit.mock.calls[0][0]).toMatchObject({ call: 'N2SPOT', band: '40m', freqMhz: 7.198, mode: 'SSB', rstSent: '59' })
})
