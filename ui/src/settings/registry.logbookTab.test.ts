// Settings ▸ Logbook (operator, 2026-10-25): the logbook's own settings — its display, how long it
// keeps a deleted contact, the callbook lookups and the reference data a logged contact is checked
// against — left Logging & Connectors, Appearance and Config for one tab. Logging & Connectors kept
// the service sign-ins, the uploads and the programs that log through Nexus.
//
// An operator who learned the old place still searches with the old words, and a pointer written
// against the old place must still land on the control. Both are checked here.
//
// POSITIVE CONTROL: words that never moved still reach the sections that kept them, so a green run
// cannot mean the search itself stopped matching.
import { describe, it, expect } from 'vitest'
import { resolveTarget, searchSettings, sectionsForTab } from './registry'

const ids = (q: string) => searchSettings(q).map((h) => h.section.id)

describe('Settings ▸ Logbook', () => {
  it('holds the logbook sections, in the order the panel renders them', () => {
    expect(sectionsForTab('logbook').map((s) => s.id)).toEqual([
      'logbook-display',
      'logbook-history',
      'callbook',
      'connections-b4',
      'lotw-users',
      'callsign-state',
      'country-file',
    ])
  })

  it('leaves the sign-ins, uploads and logging programs on Logging & Connectors', () => {
    expect(sectionsForTab('logging').map((s) => s.id)).toEqual([
      'connections',
      'integrations-feeds',
      'antenna-gain',
      'dxkeeper',
      'n3fjp',
      'n1mm',
      'confirmations',
    ])
  })
})

describe('the old search words find the new place', () => {
  it.each([
    ['logbook columns', 'logbook-display'],
    ['column width', 'logbook-display'],
    ['station card', 'logbook-display'],
    ['hover', 'logbook-display'],
    ['recently deleted', 'logbook-history'],
    ['keep deleted', 'logbook-history'],
    ['retention', 'logbook-history'],
    ['callbook cache', 'callbook'],
    ['callook', 'callbook'],
    ['hamqth', 'callbook'],
    ['offline lookup', 'callbook'],
    ['clear cache', 'callbook'],
  ])('"%s" reaches %s first', (q, id) => {
    expect(ids(q)[0]).toBe(id)
  })

  it('no longer sends those words to the sections they left', () => {
    expect(ids('logbook columns')).not.toContain('map-globe')
    expect(ids('station card')).not.toContain('workspace')
    expect(ids('recently deleted')).not.toContain('data-folder')
    expect(ids('callbook cache')).not.toContain('confirmations')
  })

  it('positive control — words that did not move still land where they were', () => {
    expect(ids('logbook globe')[0]).toBe('map-globe')
    expect(ids('text size')[0]).toBe('workspace')
    expect(ids('nas')[0]).toBe('data-folder')
    expect(ids('tqsl')[0]).toBe('confirmations')
    expect(ids('qrz')).toContain('confirmations')
  })
})

describe('a pointer written against the old place lands on the control', () => {
  it.each([
    ['Settings ▸ Logging & Connectors ▸ Confirmations ▸ Callbook cache', 'callbook'],
    ['Settings ▸ Appearance ▸ Map & globe ▸ Logbook columns', 'logbook-display'],
    ['Settings ▸ Logbook & QSL ▸ HamQTH', 'callbook'],
    ['Settings ▸ Logging & Connectors ▸ Country file (DXCC)', 'country-file'],
    ['Settings ▸ Logging & Connectors ▸ Worked-before (B4) & dupes', 'connections-b4'],
  ])('%s', (raw, section) => {
    expect(resolveTarget(raw)).toEqual({ tab: 'logbook', section })
  })

  it('a bare "Logbook" is the new tab', () => {
    expect(resolveTarget('Settings ▸ Logbook')).toEqual({ tab: 'logbook' })
  })

  it('positive control — the QRZ Logbook key still lands on Logging & Connectors', () => {
    expect(resolveTarget('Settings ▸ Logging & Connectors ▸ Confirmations')).toEqual({
      tab: 'logging',
      section: 'confirmations',
    })
  })
})
