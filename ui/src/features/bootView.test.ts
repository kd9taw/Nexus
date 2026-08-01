// Pins for the boot-restore clamps (bootView.ts) — the field-critical half of the
// 0.24.6 black-screen incident: whatever is persisted, the app must open on a view
// this build can render. See bootView.ts for why a miss here is a restart loop.
import { describe, expect, it } from 'vitest'
import { coerceArea, resolveBootView } from './bootView'
import { sectionFeatures } from './registry'

const SECTIONS = sectionFeatures().map((f) => f.id) as string[]
const allOn = () => true

describe('resolveBootView (nexus.view clamp-on-load)', () => {
  it('restores a valid persisted section', () => {
    // `connect` is the exact id from the 0.24.6 field report. It is VALID, so the
    // clamp must keep restoring it — the crash lived inside the view, and the fix
    // for that is the error boundary + the pane fix, never a persistence rule.
    expect(resolveBootView('', 'connect', SECTIONS, allOn, 'operate')).toBe('connect')
  })

  it('falls back to landing on an unknown persisted id', () => {
    // Ids a build update removed, pane ids that leaked, or plain garbage.
    for (const bad of ['openingsLog', 'no-such-view', 'propagation', '']) {
      expect(resolveBootView('', bad, SECTIONS, allOn, 'operate')).toBe('operate')
    }
    expect(resolveBootView('', null, SECTIONS, allOn, 'chat')).toBe('chat')
  })

  it('falls back to landing when the persisted section is disabled', () => {
    expect(
      resolveBootView('', 'connect', SECTIONS, (id) => id !== 'connect', 'operate'),
    ).toBe('operate')
  })

  it('an enabled deeplink hash outranks the persisted view', () => {
    expect(resolveBootView('logbook', 'connect', SECTIONS, allOn, 'operate')).toBe('logbook')
  })

  it('ignores an unknown or disabled deeplink hash', () => {
    expect(resolveBootView('bogus', 'connect', SECTIONS, allOn, 'operate')).toBe('connect')
    expect(
      resolveBootView('logbook', null, SECTIONS, (id) => id !== 'logbook', 'operate'),
    ).toBe('operate')
  })

  it('maps the merged-section legacy deeplinks to connect', () => {
    expect(resolveBootView('propagation', null, SECTIONS, allOn, 'operate')).toBe('connect')
    expect(resolveBootView('map', null, SECTIONS, allOn, 'operate')).toBe('connect')
  })
})

describe('coerceArea (nexus.workspace clamp-on-load)', () => {
  it('keeps the two live areas and clamps everything else to dx', () => {
    expect(coerceArea('dx')).toBe('dx')
    expect(coerceArea('msg')).toBe('msg')
    expect(coerceArea('connect')).toBe('dx') // retired area id — the migration pin
    expect(coerceArea('garbage')).toBe('dx')
    expect(coerceArea(null)).toBe('dx')
  })
})
