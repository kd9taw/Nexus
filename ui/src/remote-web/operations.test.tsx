// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { RemoteLogEntry } from './operations'
import { OperationClient } from './operation-client'
import type { AppSnapshot } from '../types'
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
afterEach(cleanup)
const snap = {
  radio: { band: '20m', dialMhz: 14.25 },
  hunt: null,
  mygrid: 'FN31',
  stations: []
} as unknown as AppSnapshot
it.each(['receipt', 'checked log'])(
  'resolving by %s in another tab clears only the submitted draft',
  async (how) => {
    const sent: Record<string, any>[] = [],
      client = new OperationClient(
        (s) => sent.push(JSON.parse(s)),
        true,
        () => 1000
      )
    const reply = (value: unknown) =>
      client.receive({
        type: 'operationResponse',
        requestId: sent[sent.length - 1].request.requestId,
        value
      })
    client.open()
    reply({
      stationBootId: crypto.randomUUID(),
      allowed: true,
      phase: 'controlling',
      leaseId: crypto.randomUUID(),
      revision: 1,
      commandWindowId: crypto.randomUUID(),
      nextSequence: 1,
      leaseRemainingMs: 5000,
      actions: ['log.manual'],
      txArmed: false
    })
    try {
      render(
        <>
          <div data-testid="cw">
            <RemoteLogEntry client={client} snap={snap} mode="CW" />
          </div>
          <div data-testid="phone">
            <RemoteLogEntry client={client} snap={snap} mode="SSB" />
          </div>
        </>
      )
      const cw = within(screen.getByTestId('cw')),
        phone = within(screen.getByTestId('phone')),
        own = cw.getByPlaceholderText('Call') as HTMLInputElement,
        other = phone.getByPlaceholderText('Call') as HTMLInputElement
      fireEvent.change(own, { target: { value: 'W1AW' } })
      fireEvent.change(other, { target: { value: 'K2ABC' } })
      fireEvent.click(cw.getByRole('button', { name: 'Log' }))
      expect(cw.getByText('Saving QSO at the station…')).toBeTruthy()
      expect(cw.queryByRole('button', { name: 'Check submitted QSO result' })).toBeNull()
      const operationId = sent[sent.length - 1].request.requestId
      await act(async () =>
        reply({ outcome: 'unknown', reason: 'persistenceUnconfirmed', operationId })
      )
      expect(own.value).toBe('W1AW')
      expect(other.value).toBe('K2ABC')
      if (how === 'receipt') {
        fireEvent.click(phone.getByRole('button', { name: 'Check submitted QSO result' }))
        await act(async () =>
          reply({
            outcome: 'applied',
            evidence: 'fileSynced',
            uploads: 'stationPipeline',
            operationId
          })
        )
      } else
        fireEvent.click(
          phone.getByRole('button', { name: 'I checked the station log — finish this check' })
        )
      await waitFor(() => expect(own.value).toBe(''))
      expect(other.value).toBe('K2ABC')
      expect(sent.filter((v) => v.request.type === 'logManual')).toHaveLength(1)
    } finally {
      client.disconnected()
    }
  }
)

it('shows every retained submitted field after reload without making a new entry', () => {
  const record = {
    call: 'W1AW',
    grid: 'FN31',
    country: 'United States',
    state: 'CT',
    band: '20m',
    freqMhz: 14.25,
    mode: 'SSB',
    rstSent: '59',
    rstRcvd: '57',
    name: 'Joe',
    qth: 'Newington',
    comment: 'Shared comment',
    notes: 'Unconfirmed contact notes',
    whenUnix: null,
    confirmed: false as const,
    awardConfirmed: false as const,
    ota: { theirProgram: 'POTA' as const, theirRef: 'US-1234' }
  }
  const send = vi.fn(),
    client = new OperationClient(send, true, () => 1000, {
      read: () => crypto.randomUUID(),
      readDraft: () => record,
      write: () => {}
    })
  render(<RemoteLogEntry client={client} snap={snap} mode="CW" />)
  for (const text of [
    'W1AW',
    'FN31',
    'United States',
    'CT',
    'Joe',
    'Newington',
    'Shared comment',
    'Unconfirmed contact notes',
    'US-1234'
  ])
    expect(screen.getByText(text)).toBeTruthy()
  expect((screen.getByPlaceholderText('Call') as HTMLInputElement).value).toBe('')
  expect((screen.getByRole('button', { name: 'Log' }) as HTMLButtonElement).disabled).toBe(true)
  expect(send).not.toHaveBeenCalled()
  client.disconnected()
})
