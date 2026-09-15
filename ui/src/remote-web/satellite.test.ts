// The satellite section's wire. It is strictly parsed at both ends, so what this file pins is
// what the relay and the station will accept: the seven gestures, their one capability, and the
// bounds that keep an unbounded or unnamed value off it.
import { expect, it } from 'vitest'
import { actionCapability, stationAction, CONTROL_CAPABILITIES, SAT_VFO_MAPS, TUNE_CAPABILITIES, TX_IDLE_CAPABILITIES, type StationAction } from './station-operation'
import { controlVersion } from './operation-version'

const every: StationAction[] = [
  { action: 'satellite.track', name: 'SO-50', aosUnix: 1785542400 },
  { action: 'satellite.stopTrack' },
  { action: 'satellite.transponder', name: 'SO-50', index: 0, auto: false },
  { action: 'satellite.transponder', name: 'SO-50', index: null, auto: true },
  { action: 'satellite.doppler', on: true },
  { action: 'satellite.uplinkMap', map: 'a-up-b-down', radioId: 1 },
  { action: 'satellite.uplinkMap', map: null, radioId: null },
  { action: 'satellite.peg', on: false },
  { action: 'satellite.elements' },
]

it('every satellite gesture parses, carries the one capability and rides operation v3', () => {
  for (const action of every) {
    expect(stationAction(structuredClone(action))).toEqual(action)
    expect(actionCapability(action)).toBe('satellite')
    // v3 is where the closed-vocabulary controls live. A v2 station never names the hint, so it is
    // never sent one of these; the client refuses below its own version anyway.
    expect(controlVersion(action)).toBe(3)
  }
})

it('the satellite hint is a post-v3 hint and is NOT a transmit-idle one', () => {
  expect(CONTROL_CAPABILITIES).toContain('satellite')
  // An older page must drop it as an unknown hint rather than gaining a control it cannot drive.
  expect(TUNE_CAPABILITIES).toContain('satellite')
  // ⛔ Deliberate, and it is safety: an operator works an FM bird by TRANSMITTING during the pass
  // these same controls armed, and the rail's Stop — the control that ends that pass — must be
  // live exactly when a transmission is armed, not dead at the moment it is wanted most.
  expect(TX_IDLE_CAPABILITIES).not.toContain('satellite')
})

it('refuses an unnamed mapping, an unbounded bird and a field the grammar does not carry', () => {
  const bad: unknown[] = [
    // A mapping outside the station's own enum is not a mapping.
    { action: 'satellite.uplinkMap', map: 'a-up-b-sideways', radioId: null },
    // Every value is named: an absent field is never read as a null.
    { action: 'satellite.uplinkMap', map: null },
    { action: 'satellite.transponder', name: 'SO-50', index: 0 },
    // A bird name is bounded and printable — it reaches the station's own resolver.
    { action: 'satellite.track', name: '', aosUnix: 1785542400 },
    { action: 'satellite.track', name: 'X'.repeat(65), aosUnix: 1785542400 },
    { action: 'satellite.track', name: 'SO\n50', aosUnix: 1785542400 },
    // An AOS is a real second, not a fraction and not a negative.
    { action: 'satellite.track', name: 'SO-50', aosUnix: 0 },
    { action: 'satellite.track', name: 'SO-50', aosUnix: 1785542400.5 },
    // A row index is a whole non-negative position in the list the page was shown.
    { action: 'satellite.transponder', name: 'SO-50', index: -1, auto: false },
    { action: 'satellite.transponder', name: 'SO-50', index: 1.5, auto: false },
    // Switches are booleans, never a truthy string.
    { action: 'satellite.doppler', on: 'yes' },
    { action: 'satellite.peg', on: 1 },
    // An unreviewed intent is refused rather than silently dropped.
    { action: 'satellite.stopTrack', name: 'SO-50' },
    { action: 'satellite.elements', source: 'celestrak' },
    { action: 'satellite.track', name: 'SO-50', aosUnix: 1785542400, rotator: true },
  ]
  for (const action of bad) expect(() => stationAction(action), JSON.stringify(action)).toThrow()
  // POSITIVE CONTROL: the nearest good shape to each of those is accepted, so the assertions above
  // are about the bound and not about a parser that refuses everything.
  for (const action of every) expect(() => stationAction(structuredClone(action))).not.toThrow()
})

it('the mapping vocabulary is the station enum, kebab for kebab', () => {
  // Rust `tempo_app::settings::SatVfoMap` is `rename_all = "kebab-case"`; a value this page can
  // send that the station cannot name would be a refusal the operator could not explain.
  expect([...SAT_VFO_MAPS]).toEqual(['off', 'downlink-only', 'uplink-only', 'a-down-b-up',
    'a-up-b-down', 'main-down-sub-up', 'main-up-sub-down'])
  for (const map of SAT_VFO_MAPS)
    expect(() => stationAction({ action: 'satellite.uplinkMap', map, radioId: null })).not.toThrow()
})
