// @vitest-environment jsdom
// The first visit and the empty states: what a newcomer reads before there is anything to stream.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, screen, within } from '@testing-library/react'
import { BrowserClient, RemoteError } from './client'
import type { AccountSession } from './client'
import { RemoteApp } from './RemoteApp'
import { EN } from '../i18n/en'
import { ES } from '../i18n/es'
import { DE } from '../i18n/de'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'

afterEach(() => { cleanup(); vi.restoreAllMocks() })
function account(): AccountSession {
  const accountId = crypto.randomUUID()
  return { accountId, serverNow: Date.now(), stations: [], pending: null,
    entitlement: { accountId, enabled: false, expiresAt: 0, state: 'none', startedAt: null, source: null } }
}
function client(value: AccountSession | null, post?: (path: string) => Promise<unknown>) {
  const service = { post: vi.fn(post ?? (async () => value)), authenticated: async () => !!value,
    signIn: vi.fn(async () => {}), signOut: vi.fn(async () => {}), signInRefusal: null, streamVersion: 1 }
  vi.spyOn(BrowserClient, 'load').mockResolvedValue(service as unknown as BrowserClient)
  return service
}

it('signed out, the landing has the four sections, with Sign in ahead of them', async () => {
  client(null)
  render(<RemoteApp />)
  const signIn = await screen.findByRole('button', { name: 'Sign in' })
  const headings = screen.getAllByRole('heading', { level: 2 }).map(heading => heading.textContent)
  expect(headings).toEqual(['Getting started', 'What works, and known limits', 'Safety and approval', 'Help and feedback'])
  // Sign in comes first in reading order, before the first section.
  const first = screen.getByRole('heading', { name: 'Getting started' })
  expect(signIn.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  // The steps name the shack's own tab and buttons.
  const start = first.closest('section')!
  expect(within(start).getAllByRole('listitem')).toHaveLength(4)
  expect(start.textContent).toContain('Settings → Remote')
  expect(start.textContent).toContain('Sign in to Nexus Remote')
  expect(start.textContent).toContain('Turn on Remote')
  // Free in the beta, and no price, no trial length, no date.
  expect(screen.getByText(/Free during the beta/)).toBeTruthy()
  const page = document.querySelector('main')!.textContent!
  expect(page).not.toMatch(/[$€£]|\btrial\b|fourteen|14[- ]day/i)
  // Stop TX is named as the invariant token it is on the stream page.
  expect(screen.getByRole('heading', { name: 'Safety and approval' }).closest('section')!.textContent).toContain('Stop TX stays on screen')
})

it('help and feedback gives the address, the log file and the links out', async () => {
  client(null)
  render(<RemoteApp />)
  const help = (await screen.findByRole('heading', { name: 'Help and feedback' })).closest('section')!
  expect(within(help).getByRole('link', { name: 'contact@hamradiotools.io' }).getAttribute('href')).toBe('mailto:contact@hamradiotools.io')
  expect(within(help).getByRole('link', { name: 'Discord beta channel' }).getAttribute('href')).toMatch(/^https:\/\/discord\.gg\//)
  expect(help.textContent).toContain('nexus-diag.log')
  expect(help.textContent).toContain('%LOCALAPPDATA%\\Nexus')
  expect(within(help).getByRole('link', { name: 'More about Nexus Remote' }).getAttribute('href')).toBe('https://hamradiotools.io/remote')
  expect(within(help).getByRole('link', { name: 'When Remote does not work' }).getAttribute('href')).toBe('https://hamradiotools.io/remote/help')
})

it('signed in with no station, the page says how to sign the shack in', async () => {
  client(account())
  render(<RemoteApp />)
  const card = (await screen.findByRole('heading', { name: 'Add your station' })).closest('section')!
  expect(within(card).getAllByRole('listitem').map(item => item.textContent)).toEqual([
    'In Nexus at the shack, open Settings → Remote.',
    'Press “Sign in to Nexus Remote” and sign in with this same account.',
    'Press “Turn on Remote”, and turn on “Stream this station from my browser”.',
  ])
  // The landing's four sections are for signed-out visitors only.
  expect(screen.queryByRole('heading', { name: 'Getting started' })).toBeNull()
  expect(document.querySelector('main')!.textContent).not.toMatch(/fourteen|14[- ]day/i)
})

it('the empty state goes once a station is listed', async () => {
  const value = account()
  value.stations = [{ id: 's1', name: 'Shack', device: null }]
  client(value)
  render(<RemoteApp />)
  await screen.findByRole('heading', { name: 'Shack' })
  expect(screen.queryByRole('heading', { name: 'Add your station' })).toBeNull()
})

it('an unreachable station says what to check at the shack', async () => {
  const value = account()
  value.entitlement = { ...value.entitlement, enabled: true, state: 'active', expiresAt: Date.now() + 86400000, startedAt: Date.now() }
  value.stations = [{ id: 's1', name: 'Shack', device: null }]
  client(value, async path => { if (path.startsWith('stations/')) throw new RemoteError(503, 'stationUnavailable'); return value })
  render(<RemoteApp />)
  ;(await screen.findByRole('button', { name: 'Stream' })).click()
  const alert = await screen.findByRole('alert')
  expect(alert.textContent).toContain('Settings → Remote')
  expect(alert.textContent).toMatch(/Nexus is running at the shack/)
})

// The shack's settings moved to their own Remote tab; no Remote sentence may send anyone to the old place.
it('no catalog points Remote at the old Settings → Station place', () => {
  const old = /Station → Remote access|Estación → Acceso remoto|Station → Fernzugriff|Station → Accès à distance|局 → リモートアクセス/
  for (const catalog of [EN, ES, DE, FR, JA] as Record<string, unknown>[]) {
    for (const [key, value] of Object.entries(catalog)) {
      if (key.startsWith('remote.') || key.startsWith('lanWindow.')) expect(`${key}: ${String(value)}`).not.toMatch(old)
    }
  }
})
