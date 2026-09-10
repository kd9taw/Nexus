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
