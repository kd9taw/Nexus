// @vitest-environment jsdom
// The account's own card: a copy of what the service holds, and deleting the account, which needs the word
// typed into the page (no browser dialog) and, with a live subscription, says first that it ends at once
// with no refund of the unused part.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { BrowserClient, RemoteError } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'
import { t } from '../i18n'

afterEach(() => { cleanup(); vi.restoreAllMocks() })

type Plan = NonNullable<AccountSession['entitlement']['plan']>
function account(plan?: Plan): AccountSession {
  const accountId = crypto.randomUUID(), now = Date.now()
  return { accountId, serverNow: now, stations: [], pending: null,
    entitlement: { accountId, enabled: true, expiresAt: now + 86400000, state: 'active', startedAt: now - 3600000,
      source: 'trial', access: 'internet', ...(plan ? { plan } : {}) } }
}
const subscription = (status: string): Plan => ({ kind: 'subscription', status, periodEnd: Date.now() + 30 * 86400000, renews: status === 'active' })

function client(value: AccountSession, answers: Record<string, () => unknown> = {}) {
  const post = vi.fn(async (path: string, _body?: object): Promise<unknown> => (answers[path] ?? (() => value))())
  const service = { post, billing: false, authenticated: async () => true, signIn: vi.fn(async () => {}), signOut: vi.fn(async () => {}), signInRefusal: null }
  vi.spyOn(BrowserClient, 'load').mockResolvedValue(service as unknown as BrowserClient)
  return service
}
const deletions = (service: ReturnType<typeof client>) => service.post.mock.calls.filter(([path]) => path === 'account/delete')
async function openDelete() {
  render(<RemoteApp />)
  fireEvent.click(await screen.findByText('Delete account'))
  return { box: screen.getByLabelText('Type DELETE to confirm'), button: screen.getByRole('button', { name: 'Delete my account' }) }
}

it('deletes only once DELETE is typed exactly, then signs out', async () => {
  const service = client(account(), { 'account/delete': () => ({ ok: true }) })
  const { box, button } = await openDelete()
  expect((button as HTMLButtonElement).disabled).toBe(true)
  for (const typed of ['delete', 'DELETE ', 'DELET', 'Delete']) {
    fireEvent.change(box, { target: { value: typed } })
    expect((button as HTMLButtonElement).disabled, typed).toBe(true)
    // Enter in the box submits the form whatever the button says: the submit is guarded too.
    fireEvent.submit(box.closest('form')!)
  }
  expect(deletions(service)).toEqual([])
  fireEvent.change(box, { target: { value: 'DELETE' } })
  expect((button as HTMLButtonElement).disabled).toBe(false)
  fireEvent.click(button)
  await waitFor(() => expect(service.signOut).toHaveBeenCalledTimes(1))
  expect(deletions(service)).toEqual([['account/delete', { confirm: 'DELETE' }]])
  expect(screen.queryByRole('heading', { name: 'Your stations' })).toBeNull()
})

it('a deletion the service refuses leaves the operator signed in, with the reason', async () => {
  const service = client(account(subscription('active')), { 'account/delete': () => { throw new RemoteError(503, 'subscriptionNotCancelled') } })
  const { box, button } = await openDelete()
  fireEvent.change(box, { target: { value: 'DELETE' } })
  fireEvent.click(button)
  expect((await screen.findByRole('alert')).textContent).toBe(t('remote.account.cancelFailed'))
  expect(service.signOut).not.toHaveBeenCalled()
  expect(screen.getByRole('heading', { name: 'Your stations' })).toBeTruthy()
})

it('warns that a live subscription ends at once with no refund, and says nothing of one there is not', async () => {
  const warning = t('remote.account.deleteSubscription')
  for (const [plan, shown] of [[subscription('active'), true], [subscription('past_due'), true], [subscription('canceled'), false],
    [{ kind: 'trial', status: null, periodEnd: null, renews: false } as Plan, false], [undefined, false]] as const) {
    client(account(plan))
    await openDelete()
    expect(!!screen.queryByText(warning), JSON.stringify(plan)).toBe(shown)
    cleanup()
  }
})

it('downloads the account\'s copy as a JSON file', async () => {
  const copy = { accountId: 'fixture', stations: [] }
  const service = client(account(), { 'account/export': () => copy })
  const made: Blob[] = []
  URL.createObjectURL = vi.fn((blob: Blob) => { made.push(blob); return 'blob:fixture' })
  URL.revokeObjectURL = vi.fn()
  const clicked = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
    expect(this.download).toBe('nexus-remote-account.json')
    expect(this.href).toBe('blob:fixture')
  })
  render(<RemoteApp />)
  fireEvent.click(await screen.findByRole('button', { name: 'Download my data' }))
  await waitFor(() => expect(clicked).toHaveBeenCalledTimes(1))
  expect(service.post).toHaveBeenCalledWith('account/export')
  expect(made[0].type).toBe('application/json')
  expect(JSON.parse(await made[0].text())).toEqual(copy)
})

// The address that gets the "device added" notice, moved by the owner: a code mailed to the new address,
// then a wait in which the old one is told. Opened from the account card; nothing is asked of the service
// until it is.
it('changes the notice address through a code, then shows the wait and can cancel it', async () => {
  let state: { address: string | null; pending: null | { address: string; verified: boolean; effectiveAt: number | null } } = { address: 'old@example.invalid', pending: null }
  const effectiveAt = Date.UTC(2026, 10, 15, 12)
  const service = client(account(), {
    'account/notice': () => state,
    'account/notice/change': () => { state = { ...state, pending: { address: 'new@example.invalid', verified: false, effectiveAt: null } }; return { ok: true } },
    'account/notice/confirm': () => { state = { ...state, pending: { address: 'new@example.invalid', verified: true, effectiveAt } }; return { ok: true, effectiveAt } },
    'account/notice/cancel': () => { state = { ...state, pending: null }; return { ok: true } },
  })
  render(<RemoteApp />)
  const summary = await screen.findByText(t('remote.notice.title'))
  expect(service.post.mock.calls.some(([path]) => String(path).startsWith('account/notice')), 'not read until opened').toBe(false)
  fireEvent.click(summary)
  expect(await screen.findByText(/old@example\.invalid/)).toBeTruthy()
  fireEvent.change(screen.getByLabelText(t('remote.notice.newAddress')), { target: { value: 'new@example.invalid' } })
  fireEvent.click(screen.getByRole('button', { name: t('remote.notice.send') }))
  await waitFor(() => expect(service.post).toHaveBeenCalledWith('account/notice/change', { address: 'new@example.invalid' }))
  const code = await screen.findByLabelText(t('remote.notice.code'))
  fireEvent.change(code, { target: { value: '12345678' } })
  fireEvent.click(screen.getByRole('button', { name: t('remote.notice.confirm') }))
  await waitFor(() => expect(service.post).toHaveBeenCalledWith('account/notice/confirm', { code: '12345678' }))
  expect(await screen.findByText(new RegExp('2026-11-15'))).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: t('remote.notice.cancel') }))
  await waitFor(() => expect(service.post).toHaveBeenCalledWith('account/notice/cancel', {}))
  expect(await screen.findByLabelText(t('remote.notice.newAddress'))).toBeTruthy()
})

it('a first notice address from a sign-in the account was not made with is refused by name', async () => {
  client(account(), { 'account/notice': () => ({ address: null, pending: null }),
    'account/notice/change': () => { throw new RemoteError(403, 'noticeAddressUnbound') } })
  render(<RemoteApp />)
  fireEvent.click(await screen.findByText(t('remote.notice.title')))
  fireEvent.change(await screen.findByLabelText(t('remote.notice.newAddress')), { target: { value: 'new@example.invalid' } })
  fireEvent.click(screen.getByRole('button', { name: t('remote.notice.send') }))
  expect((await screen.findByRole('alert')).textContent).toBe(t('remote.notice.unbound'))
})

it('a wrong notice code is named, and the code box stays for another try', async () => {
  const pending = { address: 'old@example.invalid', pending: { address: 'new@example.invalid', verified: false, effectiveAt: null } }
  client(account(), { 'account/notice': () => pending, 'account/notice/confirm': () => { throw new RemoteError(403, 'noticeCodeInvalid') } })
  render(<RemoteApp />)
  fireEvent.click(await screen.findByText(t('remote.notice.title')))
  fireEvent.change(await screen.findByLabelText(t('remote.notice.code')), { target: { value: '00000000' } })
  fireEvent.click(screen.getByRole('button', { name: t('remote.notice.confirm') }))
  expect((await screen.findByRole('alert')).textContent).toBe(t('remote.notice.codeInvalid'))
  expect(screen.getByLabelText(t('remote.notice.code'))).toBeTruthy()
})
