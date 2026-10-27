// @vitest-environment jsdom
//
// #402: Tempo can come off the left-hand menu, as RTTY, SSTV and the other modes can. It is a
// Settings ▸ Features switch like theirs, ON for a new install, for an update and for every
// profile, so nobody loses it without turning it off. Off, its rail item goes, and a Tempo mode
// remembered from before opens on FT instead of a screen with no way back to it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { ModeNav } from '../components/ModeNav'
import { FEATURES, type FeatureId } from './registry'
import { coerceEnabled, defaultState, normalizeState, toggleFeature } from './state'
import { PROFILES, resolveEnabled, type ProfileId } from './profiles'
import { coerceArea } from './bootView'
import { t } from '../i18n'
import { EN } from '../i18n/en'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'

afterEach(cleanup)

function renderNav(enabled: Record<FeatureId, boolean>) {
  render(
    <ModeNav
      view="operate"
      mode="chat"
      enabled={enabled}
      onSelect={vi.fn()}
      tier="FT8"
      onDigitalMode={vi.fn()}
    />,
  )
}
const tempoItem = () => screen.queryByRole('button', { name: t('nav.digital.tempo.title') })

describe('Tempo is an optional section (#402)', () => {
  it('has a switch: it is not core, and turning it off sticks', () => {
    expect(FEATURES.find((f) => f.id === 'chat')?.core).toBe(false)
    const off = toggleFeature(defaultState(), 'chat')
    expect(off.enabled.chat).toBe(false)
    expect(normalizeState(JSON.parse(JSON.stringify(off))).enabled.chat, 'off came back on after a reload').toBe(false)
  })

  it('ships on: a new install, an update and every profile keep it', () => {
    expect(defaultState().enabled.chat).toBe(true)
    // An update: a stored state from before the switch, which always carried chat ON (core).
    expect(coerceEnabled({ chat: true }).chat).toBe(true)
    // …or none at all for it.
    expect(coerceEnabled({}).chat).toBe(true)
    for (const id of Object.keys(PROFILES) as ProfileId[]) expect(resolveEnabled(id).chat, id).toBe(true)
  })

  it('takes the Tempo item off the rail when off, and leaves FT', () => {
    const on = coerceEnabled({})
    renderNav(on)
    expect(tempoItem()).not.toBeNull()
    cleanup()
    renderNav({ ...on, chat: false })
    expect(tempoItem()).toBeNull()
    expect(screen.queryByRole('button', { name: t('nav.digital.ft.title') })).not.toBeNull()
  })

  it('a remembered Tempo mode opens on FT while Tempo is off', () => {
    expect(coerceArea('msg', false)).toBe('dx')
    expect(coerceArea('msg', true)).toBe('msg')
    expect(coerceArea('dx', false)).toBe('dx')
  })
})

describe("Tempo's Features row carries the rail's name", () => {
  it('is labelled Tempo in every catalog, the mode name the rail shows', () => {
    const labels = [EN, DE, ES, FR, JA].map((c) => c['features.chat.label'])
    expect(labels).toEqual(['Tempo', 'Tempo', 'Tempo', 'Tempo', 'Tempo'])
  })
})
