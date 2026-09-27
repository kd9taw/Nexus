// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MonitorApp } from './MonitorApp'
import { nativeSource } from './nativeSource'
import { ampCommand } from '../api'
import fixtures from './fixtures.v2.json'
import type { MonitorSource } from './session'
import { EN } from '../i18n'

afterEach(() => {
  cleanup()
  delete window.__TAURI_INTERNALS__
  delete window.__TAURI__
})

function source(name: keyof typeof fixtures): MonitorSource {
  let sequence = 0
  return { id: name, kind: 'fixture', read: async () => ({ ...fixtures[name], sequence: ++sequence }) }
}

it('renders native state and its actual UI interactions can only reach the observer read', async () => {
  const rejected: string[] = []
  const invoke = vi.fn(async (command: string) => {
    if (command !== 'get_remote_monitor_frame') {
      rejected.push(command)
      throw new Error('mutationBlockedByTest')
    }
    return { ...fixtures.spe, source: 'native' }
  })
  window.__TAURI_INTERNALS__ = { invoke: invoke as NonNullable<Window['__TAURI_INTERNALS__']>['invoke'] }
  render(<MonitorApp source={nativeSource} />)
  await screen.findByText('Desktop station')
  await screen.findByText('14.074000')
  fireEvent.click(screen.getByRole('button', { name: /SPE/ }))
  expect(screen.getByText('41°')).toBeTruthy()
  fireEvent.click(screen.getByRole('button', { name: 'Use light theme' }))
  fireEvent.click(screen.getByRole('button', { name: /SPE/ }))
  expect(new Set(invoke.mock.calls.map(([command]) => command))).toEqual(new Set(['get_remote_monitor_frame']))
  expect(rejected).toEqual([])
  // Positive control: the real existing mutating adapter trips this same guard.
  // A test that merely rendered no buttons would not establish this capability boundary.
  await expect(ampCommand('operate')).rejects.toThrow('mutationBlockedByTest')
  expect(rejected).toEqual(['amp_command'])
})

it('a native view with no bridge cannot fall back to example data', async () => {
  render(<MonitorApp source={nativeSource} />)
  await screen.findByText('Station updates unavailable. Current readings are hidden.')
  expect(screen.queryByText('N0CALL')).toBeNull()
  expect(screen.queryByRole('region', { name: 'Amplifier' })).toBeNull()
})

it('preserves first-miss absence, KPA units, unknown faults and radio changes in the real component', async () => {
  const view = render(<MonitorApp source={source('firstMiss')} />)
  fireEvent.click(await screen.findByRole('button', { name: /SPE/ }))
  expect(screen.getByText('The amplifier has no current reading.')).toBeTruthy()
  expect(screen.queryByText('Standby')).toBeNull()
  expect(screen.queryByText('41°')).toBeNull()
  view.rerender(<MonitorApp source={source('kpa')} />)
  fireEvent.click(await screen.findByRole('button', { name: /KPA/ }))
  expect(screen.getByText('52 °C')).toBeTruthy()
  const tx = screen.getByText('Amplifier TX flag').parentElement
  expect(tx?.textContent).toContain('—')
  view.rerender(<MonitorApp source={source('fault')} />)
  fireEvent.click(await screen.findByRole('button', { name: /SPE/ }))
  expect(screen.getByText('Reported alarm')).toBeTruthy()
  expect(document.querySelector('.amp-alarm')?.textContent).toBeTruthy()
  expect(document.querySelector('.amp-warn')?.textContent).toBeTruthy()
  view.rerender(<MonitorApp source={source('noAmp')} />)
  await waitFor(() => expect(screen.queryByRole('region', { name: 'Amplifier' })).toBeNull())
  expect(screen.queryByRole('button', { name: /SPE|KPA/ })).toBeNull()
})

it('the theme button cycles Dark → Light → System → Dark, and System follows the computer', async () => {
  // The monitor keeps its own theme (it is not the app's Settings), so the third choice lives
  // on the same one button: each press names the NEXT theme, as the Sun/Moon did.
  let dark = false
  const listeners = new Set<() => void>()
  window.matchMedia = ((q: string) => ({
    get matches() {
      return q === '(prefers-color-scheme: dark)' && dark
    },
    media: q,
    addEventListener: (_t: string, f: () => void) => listeners.add(f),
    removeEventListener: (_t: string, f: () => void) => listeners.delete(f),
  })) as unknown as typeof window.matchMedia
  const theme = () => document.documentElement.dataset.theme
  // The page's own starting theme (monitor.html is data-theme="dark"); an earlier case in this
  // file leaves it light.
  document.documentElement.dataset.theme = 'dark'
  try {
    render(<MonitorApp source={source('spe')} />)
    await screen.findByText('14.074000')
    expect(theme()).toBe('dark')
    fireEvent.click(screen.getByRole('button', { name: EN['monitor.light'] }))
    expect(theme()).toBe('light')
    fireEvent.click(screen.getByRole('button', { name: EN['monitor.system'] }))
    expect(theme(), 'System did not take the computer’s light setting').toBe('light')
    dark = true
    act(() => listeners.forEach((f) => f()))
    expect(theme(), 'System did not follow the computer going dark').toBe('dark')
    fireEvent.click(screen.getByRole('button', { name: EN['monitor.dark'] }))
    expect(theme()).toBe('dark')
    dark = false
    act(() => listeners.forEach((f) => f()))
    expect(theme(), 'a pinned Dark followed the computer').toBe('dark')
  } finally {
    delete (window as { matchMedia?: unknown }).matchMedia
    document.documentElement.dataset.theme = 'dark'
  }
})
