// @vitest-environment jsdom
//
// ONE STATION, ONE COLOUR, ON EVERY VIEW THAT SHOWS IT (operator, 2026-10-04, "New park wins": "The
// needed-park color shows on the strip, the band map and the decode list alike. Today the strip and
// map show confirm grey while the decode list shows the park.").
//
// The rows are the station's own. src-tauri/tests/fixtures/needed-park-and-confirm.json is the Needed
// board `read_need_alerts` builds for K1ABC, an activator at a park the operator still needs who is
// also a confirmation opportunity (the USA worked on 20 m and never confirmed), on both of the board's
// paths: a cluster spot in CW and the hunter feed in FT8. The station's own test holds the file to the
// board it builds, so this reads what the station sends, not what it was meant to send.
//
// The band strip and the band map colour a tick from the call's strongest row's FIRST need
// (`topNeedByCall`). The decode list ranks every need on the rows by NEED_PRECEDENCE, where a park
// outranks a confirmation, so it showed the park whatever the order; the strip and the map agree with
// it only when the station sends the park first.
import { describe, it, expect, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import { cleanup, render } from '@testing-library/react'
import { BandMap } from './components/BandMap'
import { BandStrip } from './components/BandStrip'
import { OperateDecodes } from './components/OperateDecodes'
import { resolveDecodeNeeds } from './features/decodeNeeds'
import { activityTypeByCall, alertsByCall, topNeedByCall, visibleNeeds } from './features/needs'
import type { DecodeRow, NeedAlert, SpotRow } from './types'

const BOARD = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../../src-tauri/tests/fixtures/needed-park-and-confirm.json'),
    'utf8',
  ),
) as NeedAlert[]

afterEach(() => {
  cleanup()
  localStorage.clear()
})

/** The maps exactly as App derives them from the station's rows. */
function hostMaps(rows: NeedAlert[]) {
  const gated = visibleNeeds(rows, { cw: true, phone: true })
  const needAlertsByCall = alertsByCall(gated)
  return { needAlertsByCall, needByCall: topNeedByCall(needAlertsByCall), typeByCall: activityTypeByCall(gated) }
}

/** K1ABC's cluster spot, as the spot list carries it to Band activity and the band map. */
const SPOT: SpotRow = {
  call: 'K1ABC',
  entity: 'United States',
  zone: 5,
  band: '20m',
  freqMhz: 14.025,
  mode: 'CW',
  spotter: 'W3LPL',
  corroborators: [],
  ageSecs: 10,
  comment: '',
  licensed: true,
}

/** K1ABC calling CQ in FT8, as the decode list shows it. */
const DECODE = {
  from: 'K1ABC',
  snr: -10,
  dtSec: 0.1,
  freqHz: 1200,
  message: 'CQ POTA K1ABC FN42',
  isCq: true,
  directedToMe: false,
  worked: false,
  tier: 'FT8',
  rv: 0,
} as DecodeRow

/** The colour classes on a mark: a need's (`is-need`, `need-*`) or the dim POTA one. */
const colours = (el: Element | null | undefined) =>
  [...(el?.classList ?? [])].filter((c) => c === 'is-need' || c === 'pota-dim' || c.startsWith('need-')).sort()

/** What each view paints K1ABC with, rendered from these rows. */
function painted(rows: NeedAlert[]) {
  const maps = hostMaps(rows)
  const strip = render(
    <BandStrip
      band="20m"
      dialMhz={14.03}
      txAllowed
      spots={[SPOT]}
      spotMode="CW"
      needByCall={maps.needByCall}
      typeByCall={maps.typeByCall}
      onWorkSpot={() => {}}
    />,
  ).container
  const stripTick = strip.querySelector('.bandstrip-spot .bandstrip-tick')
  const out = { strip: colours(stripTick), mapTick: [] as string[], mapLabel: [] as string[], decodeRow: '' }
  cleanup()
  const map = render(
    <BandMap
      band="20m"
      dialMhz={14.03}
      txAllowed
      spots={[SPOT]}
      spotMode="CW"
      needByCall={maps.needByCall}
      typeByCall={maps.typeByCall}
      onWorkSpot={() => {}}
    />,
  ).container
  const label = map.querySelector('.bandmap-spot')
  expect(label?.previousElementSibling?.classList.contains('bandmap-tick'), 'the tick sits beside its label').toBe(true)
  out.mapTick = colours(label?.previousElementSibling)
  out.mapLabel = colours(label)
  cleanup()
  const feed = render(
    <OperateDecodes
      decodes={[DECODE]}
      slot={100}
      rxOffsetHz={1200}
      band="20m"
      tier="FT8"
      harqRescues={0}
      onCall={() => {}}
      needAlertsByCall={maps.needAlertsByCall}
    />,
  ).container
  // The row's colour is the class after `decode-row` (OperateDecodes' rowClass).
  out.decodeRow = feed.querySelector('.decode-row')?.classList[1] ?? ''
  cleanup()
  return out
}

describe('the station’s rows for an activator at a needed park who is also a Confirm need', () => {
  it('are what this file is about: the park, the confirmation and the programme on both paths', () => {
    expect(BOARD.map((r) => [r.call, r.mode])).toEqual([
      ['K1ABC', 'CW'],
      ['K1ABC', 'Digital'],
    ])
    for (const r of BOARD) expect([...r.tags].sort(), `${r.mode} row`).toEqual(['Confirm', 'NewPark', 'Pota'])
    // …and the decode list's own surface (20 m, FT8) holds both needs, so its choice is a real one.
    expect(resolveDecodeNeeds(DECODE, '20m', BOARD).cats).toEqual(['park', 'confirm', 'pota'])
  })

  it('wear the new-park colour on Band activity, the band map and the decode list alike', () => {
    expect(painted(BOARD)).toEqual({
      strip: ['is-need', 'need-pota'],
      mapTick: ['need-pota'],
      mapLabel: ['need-pota'],
      decodeRow: 'need-pota',
    })
  })

  it('FIRES: the same rows without the park need show the confirmation on every view', () => {
    // So the colour above is the park winning, never the confirmation going missing. The decode
    // list ranks a confirmation below a CQ, so a CQ with only a confirmation reads as a CQ.
    const noPark = BOARD.map((r) => ({ ...r, tags: r.tags.filter((t) => t !== 'NewPark') }))
    expect(painted(noPark)).toEqual({
      strip: ['is-need', 'need-confirm'],
      mapTick: ['need-confirm'],
      mapLabel: ['need-confirm'],
      decodeRow: 'cq',
    })
  })
})
