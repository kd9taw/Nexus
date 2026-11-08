// Best times' words (features/bestTimes): each state with no answer, the basis line on every answer
// and "Rough." only for a country centre, the answer's now / next / week lines, and — in every
// language — no percent sign anywhere.
import { afterEach, describe, expect, it } from 'vitest'
import { installCatalog, setLocale, type PartialCatalog } from '../i18n'
import type { BestTimes, BestTimesView } from '../types'
import {
  basisLine,
  cellTitle,
  invitesGrid,
  invitesLookup,
  lastSpottedLine,
  modelLine,
  noWindowLine,
  nowLines,
  startMode,
  stateLine,
  weekLines,
  whereLine,
  windowRows,
} from './bestTimes'

const CATALOGS: [string, PartialCatalog][] = Object.entries(
  import.meta.glob<Record<string, unknown>>(['../i18n/[a-z][a-z].ts', '../i18n/[a-z][a-z]-[a-z][a-z][a-z][a-z].ts'], { eager: true }),
).map(([path, module]) => [path.replace(/^.*\/|\.ts$/g, ''), Object.values(module).find((v) => typeof v === 'object') as PartialCatalog])

/** 2026-05-15 12:00 UTC. */
const NOW = 1_778_846_400

function answer(over: Partial<BestTimes> = {}): BestTimes {
  return {
    engine: 'p533',
    mode: 'FT8',
    perMode: true,
    now: { kind: 'try', band: '20m', word: 'Good', untilUnix: NOW + 8 * 3600, allDay: false, runnerUp: { band: '17m', word: 'Fair' } },
    windows: [
      { band: '15m', fromUnix: NOW + 10 * 3600, toUnix: NOW + 11 * 3600, word: 'Fair', greyline: false, longShot: false },
      { band: '40m', fromUnix: NOW + 17 * 3600, toUnix: NOW + 19 * 3600, word: 'Good', route: 'long', greyline: true, longShot: false, stormKp: 5 },
    ],
    week: { forecastUntilUnix: NOW + 2 * 86_400, flags: [{ dayUnix: NOW - 12 * 3600 + 86_400, kp: 5.33 }] },
    chart: [{ band: '20m', licensed: true, cells: Array.from({ length: 24 }, () => ({ word: 'Good' as const })) }],
    hours: [],
    nowHour: 12,
    longUsed: true,
    ...over,
  }
}

function view(over: Partial<BestTimesView> = {}): BestTimesView {
  return {
    state: 'answer',
    call: 'EA8ABC',
    entity: 'Canary Islands',
    basis: { kind: 'grid', from: 'heard', grid: 'IL18' },
    km: 5000,
    bearing: 105.6,
    answer: answer(),
    engine: 'p533',
    power: { watts: 100, source: 'default', amplifier: false },
    ...over,
  }
}

/** Every line a view can put on screen. */
function allLines(v: BestTimesView): string[] {
  const n = nowLines(v)
  return [
    stateLine(v),
    basisLine(v),
    whereLine(v, 'metric'),
    n?.head,
    n?.detail,
    ...(n?.notes ?? []),
    ...windowRows(v, NOW).flatMap((r) => [r.when, r.local, r.word, ...r.tags]),
    noWindowLine(v),
    ...weekLines(v),
    modelLine(v, NOW, 0),
    lastSpottedLine(v.call, [{ call: 'EA8ABC', band: '20m', ageSecs: 840 } as never]),
    cellTitle('20m', 3, 'Fair', 'long', false),
  ].filter((x): x is string => typeof x === 'string')
}

afterEach(() => setLocale('en'))

describe('the states with no answer', () => {
  it('each says its own line, and none comes with a basis', () => {
    const say = (state: BestTimesView['state']) => stateLine(view({ state, basis: undefined, answer: undefined }))
    expect(say('noGrid')).toBe('Set your grid square in Settings to get an answer.')
    expect(say('enterCall')).toBe('Enter a callsign.')
    expect(say('unplaceable')).toMatch(/can't place this call/)
    expect(say('maritime')).toMatch(/^Maritime or aeronautical mobile: location unknown/)
    expect(say('you')).toBe("That's you.")
    expect(stateLine(view({ state: 'sameEntity', entity: 'United States', basis: undefined, answer: undefined }))).toMatch(
      /where in United States they are/,
    )
    // Negative control: an answer has no state line.
    expect(stateLine(view())).toBeNull()
  })

  it('invites a grid and a lookup only where they can help', () => {
    expect(invitesGrid(view({ state: 'maritime', basis: undefined }))).toBe(true)
    expect(invitesGrid(view({ state: 'unplaceable', basis: undefined }))).toBe(true)
    expect(invitesLookup(view({ basis: { kind: 'entity' } }))).toBe(true)
    expect(invitesLookup(view())).toBe(false)
    expect(invitesGrid(view())).toBe(false)
    expect(invitesLookup(view({ state: 'maritime', basis: undefined }))).toBe(false)
  })
})

describe('the basis line', () => {
  it('names what the location rests on, and says Rough only for a country centre', () => {
    expect(basisLine(view())).toBe('Worked out from grid IL18, sent on the air.')
    expect(basisLine(view({ basis: { kind: 'grid', from: 'logged', grid: 'IL18' } }))).toMatch(/from your log\. It may be old\.$/)
    expect(basisLine(view({ basis: { kind: 'grid', from: 'typed', grid: 'IL28' } }))).toMatch(/the grid you typed/)
    expect(basisLine(view({ basis: { kind: 'position', from: 'callbook' } }))).toMatch(/callbook's position/)
    const centre = basisLine(view({ basis: { kind: 'entity' } }))
    expect(centre).toBe('Worked out from the centre of Canary Islands. Rough.')
    for (const from of ['typed', 'logForm', 'heard', 'callbook', 'logged'] as const)
      expect(basisLine(view({ basis: { kind: 'grid', from, grid: 'IL18' } }))).not.toMatch(/Rough/)
  })
})

describe('the answer', () => {
  it('says now first, on the hour, with the runner-up', () => {
    const n = nowLines(view())!
    expect(n.head).toBe('Now: try 20m')
    expect(n.detail).toBe('Good, until about 20:00 UTC. 17m possible, weaker.')
    expect(nowLines(view({ answer: answer({ now: { kind: 'try', band: '20m', word: 'Excellent', allDay: true } }) }))!.detail).toBe(
      'Excellent, all day.',
    )
    expect(nowLines(view({ answer: answer({ now: { kind: 'longShot', band: '40m', word: 'Marginal', allDay: false } }) }))!.head).toBe(
      'Now: a long shot on 40m',
    )
    expect(nowLines(view({ answer: answer({ now: { kind: 'notNow', allDay: false } }) }))!.head).toBe('Not now.')
  })

  it('passes over a band a flare is estimated to cost 10 dB, and says so', () => {
    const n = nowLines(
      view({ answer: answer({ now: { kind: 'try', band: '15m', word: 'Fair', allDay: false, untilUnix: NOW + 3600, passedOver: { band: '20m', db: 12 } } }) }),
    )!
    expect(n.head).toBe('Now: try 15m')
    expect(n.notes[0]).toMatch(/^20m would lead, but .* about 12 dB now\.$/)
  })

  it('lists windows by day and UTC hour, with their tags', () => {
    const rows = windowRows(view(), NOW)
    expect(rows.map((r) => r.when)).toEqual(['today 22:00–23:00 UTC', 'tomorrow 05:00–07:00 UTC'])
    expect(rows[1].tags).toEqual(['greyline', 'long path', 'Kp 5 forecast'])
    expect(windowRows(view(), NOW, 1)).toHaveLength(1)
  })

  it('says when there is no workable hour, and not when now is open', () => {
    expect(noWindowLine(view({ answer: answer({ windows: [], now: { kind: 'notNow', allDay: false } }) }))).toBe(
      'The model finds no workable hour for FT8 in the next day.',
    )
    expect(noWindowLine(view({ answer: answer({ windows: [] }) }))).toBeNull()
  })

  it('says the week repeats, with a line for each forecast day that reaches the path', () => {
    const lines = weekLines(view())
    expect(lines[0]).toBe('This week: these same hours repeat each day.')
    expect(lines[1]).toMatch(/: Kp 5\.3 forecast, and this path reaches the auroral zone\.$/)
    expect(lines).toHaveLength(3)
    const none = weekLines(view({ answer: answer({ week: { flags: [] } }) }))
    expect(none).toEqual(['This week: these same hours repeat each day.', 'No Kp forecast is held, so no day can be told apart.'])
  })

  it('names the model and its month, and admits what it lacks', () => {
    expect(modelLine(view(), NOW, 0)).toBe('P.533 monthly median for May · FT8 · antenna gain not set')
    expect(modelLine(view(), NOW, 6)).toBe('P.533 monthly median for May · FT8')
    expect(modelLine(view({ answer: answer({ engine: 'heuristic', perMode: false }) }), NOW, 0)).toBe(
      'Simple model · FT8 · the same for every mode',
    )
  })

  it('takes the newest spot of the call for its one line', () => {
    const rows = [
      { call: 'ea8abc', band: '15m', ageSecs: 3000 },
      { call: 'EA8ABC', band: '20m', ageSecs: 840 },
      { call: 'EA8XYZ', band: '10m', ageSecs: 10 },
    ] as never
    expect(lastSpottedLine('EA8ABC', rows)).toBe('Last spotted 14 min ago on 20m.')
    expect(lastSpottedLine('K1ABC', rows)).toBeNull()
  })

  it('starts from the radio mode when it is one it knows, else FT8', () => {
    expect(startMode('CW')).toBe('CW')
    expect(startMode('SSB')).toBe('SSB')
    expect(startMode('FT4')).toBe('FT4')
    expect(startMode('RTTY')).toBe('FT8')
    expect(startMode(null)).toBe('FT8')
  })
})

describe('in every language', () => {
  it('no line carries a percent sign, and every answer carries its basis', () => {
    const views = [
      view(),
      view({ basis: { kind: 'entity' } }),
      view({ answer: answer({ engine: 'heuristic', perMode: false, now: { kind: 'notNow', allDay: false, passedOver: { band: '20m', db: 11 } } }) }),
      view({ state: 'sameEntity', basis: undefined, answer: undefined }),
      view({ state: 'maritime', basis: undefined, answer: undefined }),
    ]
    expect(CATALOGS.length).toBeGreaterThanOrEqual(8)
    for (const [locale, catalog] of CATALOGS) {
      installCatalog(locale, catalog)
      setLocale(locale)
      for (const v of views) {
        const lines = allLines(v)
        for (const l of lines) expect(l, `${locale}: ${l}`).not.toMatch(/%/)
        if (v.state === 'answer') expect(basisLine(v), locale).toBeTruthy()
      }
    }
  })
})
