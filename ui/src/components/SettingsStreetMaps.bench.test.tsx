// @vitest-environment jsdom
//
// THE BENCH AID IS NEVER SHOWN TO USERS. Once the street map's maps are hosted (the manifest constant
// set), operators see the Settings block, and "Install a street map file…" stays out of it unless this
// computer runs with NEXUS_STREET_MAP=1. The store's state is given directly here: the constant cannot
// be set from a test, and that the store reads the flag is pinned in features/streetMaps.test.ts.
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { StreetMapsState } from '../features/streetMaps'

const state = vi.hoisted(() => ({ bench: false }))
vi.mock('../features/streetMaps', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../features/streetMaps')>()),
  useStreetMaps: (): StreetMapsState => ({
    offered: true,
    folder: '/maps',
    bench: state.bench,
    webgl2: true,
    packs: [],
    unfinished: [],
    download: { state: 'idle' },
  }),
}))
import { SettingsStreetMaps } from './SettingsStreetMaps'
import { t } from '../i18n'

afterEach(cleanup)

describe('the bench aid', () => {
  it('is not offered to an operator, only to a bench run', () => {
    render(<SettingsStreetMaps myGrid="EM18eu" />)
    expect(screen.getByRole('button', { name: t('settings.streetMaps.download') }), 'CONTROL: the block is there').toBeTruthy()
    expect(screen.queryByRole('button', { name: t('settings.streetMaps.bench.install') })).toBeNull()
    cleanup()
    state.bench = true
    render(<SettingsStreetMaps myGrid="EM18eu" />)
    expect(screen.getByRole('button', { name: t('settings.streetMaps.bench.install') })).toBeTruthy()
  })
})
