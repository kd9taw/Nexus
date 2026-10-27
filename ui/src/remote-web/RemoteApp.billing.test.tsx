// @vitest-environment jsdom
// Subscribe and Manage on the account page: shown only where the service says its billing is on, with the
// renewal terms beside Subscribe and no price anywhere on the page (the provider's checkout shows it).
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient, RemoteError } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'
import { t } from '../i18n'

afterEach(() => { cleanup(); vi.restoreAllMocks() })

const HOUR = 3600000
const TERMS = 'The plan renews every month or every year, as you choose, at the price shown at checkout, until you cancel. Cancel any time with Manage subscription: the plan ends at the end of the period paid for, and nothing more is charged.'
type Plan = NonNullable<AccountSession['entitlement']['plan']>

function account(state: AccountSession['entitlement']['state'], plan: Plan | undefined, left = 10 * 24 * HOUR): AccountSession {
  const accountId = crypto.randomUUID(), now = Date.now()
  return { accountId, serverNow: now, stations: [], pending: null,
    entitlement: { accountId, enabled: state === 'active', expiresAt: now + left, state, startedAt: now - HOUR,
      source: plan?.kind === 'trial' ? 'trial' : null, access: state === 'active' ? 'internet' : 'lan', ...(plan ? { plan } : {}) } }
}
const trialPlan: Plan = { kind: 'trial', status: null, periodEnd: null, renews: false }
const subscription = (status: string): Plan => ({ kind: 'subscription', status, periodEnd: Date.now() + 30 * 24 * HOUR, renews: status === 'active' })

function client(value: AccountSession, billing: boolean) {
  const post = vi.fn(async (_path: string, _body?: object): Promise<unknown> => value)
  const billingPage = vi.fn(async (_kind: string, _body?: object): Promise<string> => { throw new RemoteError(503, 'serviceUnavailable') })
  const service = { post, billingPage, billing, authenticated: async () => true, signIn: vi.fn(async () => {}), signOut: vi.fn(async () => {}), signInRefusal: null }
  vi.spyOn(BrowserClient, 'load').mockResolvedValue(service as unknown as BrowserClient)
  return service
}
const noPrice = () => {
  // Any currency sign, or a number with a price's decimals, anywhere on the page.
  expect(document.body.textContent).not.toMatch(/[$€£¥]|\d+[.,]\d{2}\b/)
}

it('shows no Subscribe or Manage while the service has billing off, whatever the account holds', async () => {
  for (const value of [account('ended', trialPlan), account('active', subscription('active'))]) {
    client(value, false)
    render(<RemoteApp />)
    await screen.findByRole('heading', { name: 'Your stations' })
    expect(screen.queryByRole('heading', { name: 'Paid plan' })).toBeNull()
    expect(screen.queryByRole('button', { name: /Subscribe/ })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Manage subscription' })).toBeNull()
    expect(screen.queryByText(TERMS)).toBeNull()
    cleanup()
  }
})

it('offers Subscribe with the renewal terms beside it, and no price, once billing is on', async () => {
  const service = client(account('ended', trialPlan, -HOUR), true)
  render(<RemoteApp />)
  const monthly = await screen.findByRole('button', { name: 'Subscribe monthly' })
  const yearly = screen.getByRole('button', { name: 'Subscribe yearly' })
  // The terms are in the same card as the buttons.
  const card = monthly.closest('section')!
  expect(card.contains(yearly)).toBe(true)
  expect(card.textContent).toContain(TERMS)
  // Nothing to manage yet, and no trial to keep.
  expect(screen.queryByRole('button', { name: 'Manage subscription' })).toBeNull()
  expect(screen.queryByText(/first payment/)).toBeNull()
  noPrice()

  fireEvent.click(monthly)
  await waitFor(() => expect(service.billingPage).toHaveBeenCalledWith('checkout', { plan: 'monthly' }))
  fireEvent.click(await screen.findByRole('button', { name: 'Subscribe yearly' }))
  await waitFor(() => expect(service.billingPage).toHaveBeenCalledWith('checkout', { plan: 'annual' }))
  // A failure shows as the page's own sentence, and the page stays where it is.
  expect((await screen.findByRole('alert')).textContent).toBe(t('remote.requestFailed'))
  expect(window.location.pathname).toBe('/')
})

it('says before the press whether the trial is kept or the first payment is taken at once', async () => {
  client(account('active', trialPlan, 49 * HOUR), true)
  render(<RemoteApp />)
  expect(await screen.findByText('Subscribing now keeps the rest of your trial: the first payment is taken when the trial ends.')).toBeTruthy()
  expect(screen.queryByText(/less than 48 hours/)).toBeNull()
  cleanup()

  client(account('active', trialPlan, 47 * HOUR), true)
  render(<RemoteApp />)
  expect(await screen.findByText('Your trial has less than 48 hours left, so subscribing now takes the first payment at once and the plan starts now.')).toBeTruthy()
  expect(screen.queryByText(/keeps the rest of your trial/)).toBeNull()
  cleanup()

  // A grant by hand is no trial the plan can keep: neither sentence.
  client(account('active', { kind: 'manual', status: null, periodEnd: null, renews: false }, 10 * 24 * HOUR), true)
  render(<RemoteApp />)
  await screen.findByRole('button', { name: 'Subscribe monthly' })
  expect(screen.queryByText(/first payment/)).toBeNull()
})

it('a live subscription gets Manage and no Subscribe; an ended one gets both; a switched-off account neither', async () => {
  const service = client(account('active', subscription('active')), true)
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: 'Manage subscription' }))
  await waitFor(() => expect(service.billingPage).toHaveBeenCalledWith('portal', {}))
  expect(screen.queryByRole('button', { name: /Subscribe/ })).toBeNull()
  cleanup()

  client(account('ended', subscription('canceled'), -HOUR), true)
  render(<RemoteApp />)
  await screen.findByRole('button', { name: 'Subscribe monthly' })
  expect(screen.getByRole('button', { name: 'Manage subscription' })).toBeTruthy()
  cleanup()

  client({ ...account('disabled', trialPlan), entitlement: { ...account('disabled', trialPlan).entitlement, enabled: false } }, true)
  render(<RemoteApp />)
  await screen.findByRole('heading', { name: 'Your stations' })
  expect(screen.queryByRole('heading', { name: 'Paid plan' })).toBeNull()
})

it('names the billing refusals the page can meet', async () => {
  const service = client(account('ended', trialPlan, -HOUR), true)
  render(<RemoteApp />)
  const cases: [string, string][] = [
    ['alreadySubscribed', 'This account already has a subscription. Use Manage subscription to change it.'],
    ['billingNotAvailable', 'Subscriptions are not available yet.'],
  ]
  for (const [code, sentence] of cases) {
    service.billingPage.mockRejectedValueOnce(new RemoteError(409, code))
    fireEvent.click(await screen.findByRole('button', { name: 'Subscribe monthly' }))
    expect((await screen.findByRole('alert')).textContent).toBe(sentence)
  }
})
