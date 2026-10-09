// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { CONTESTS } from './fdEvent'

// The spectator scoreboard for any contest club sync runs, executed for real — the
// companion of fdBoardPage.test.ts, which covers the Field Day board. The payload it
// renders here is the SERVER'S OWN (crates/tempo-app/tests/fixtures/scoreboard-ilqp.json,
// held to the Rust builders by `the_pages_party_fixture_is_what_this_server_sends`), the
// county map is held to the rules data, and the notices are held to their words.
// (jsdom never lays out — that the text fits and nothing overlaps on a real screen is the
// headless-Chrome pass, at 1920x1080 and 3840x2160 in both themes.)

const here = dirname(fileURLToPath(import.meta.url))
const read = (p: string) => readFileSync(resolve(here, p), 'utf8')
const html = read('../../crates/tempo-app/assets/fd_scoreboard.html')
const body = /<body>([\s\S]*)<\/body>/.exec(html)?.[1]
const script = /<script>([\s\S]*?)<\/script>/.exec(html)?.[1]
const fixture = JSON.parse(read('../../crates/tempo-app/tests/fixtures/scoreboard-ilqp.json'))
const seed = JSON.parse(read('../../crates/tempo-core/src/fd_rules.seed.json'))

type LatLon = [number, number]
type Cell = [number, number][]
type Board = {
  STRINGS: Record<string, string>
  CONTEST_NAMES: Record<string, string>
  render: (data: unknown, meta: unknown) => void
  renderMeta: (meta: unknown) => void
  setInactive: (on: boolean, info?: Record<string, string>) => void
  map: {
    IL_COUNTIES: Record<string, LatLon>
    pickLayout: (values: { code: string }[]) => Record<string, LatLon> | null
    buildMosaic: (centres: Record<string, LatLon>) => {
      codes: string[]
      sites: [number, number][]
      cells: Cell[]
    }
    inCell: (poly: Cell, x: number, y: number) => boolean
    snapshot: () => { kind: string; id: string; size: number; lit: string[] } | null
  }
}

/** The page's own globals, without a second `Window` augmentation (fdBoardPage.test.ts
 *  has one, for its own view of the seam). */
const page = window as unknown as { __FDBOARD_TEST__?: boolean; __fdboard?: Board }

function boot(search = ''): Board {
  window.history.replaceState(null, '', '/' + search)
  page.__FDBOARD_TEST__ = true
  document.body.innerHTML = body!
  new Function(script!)()
  return page.__fdboard!
}

const $ = (id: string) => document.getElementById(id)!
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v))
/** data.json as the cache splices it: rev and now_unix in front of the core. */
const data = () => ({ rev: 1, now_unix: fixture.now, ...clone(fixture.data) })
const meta = () => clone(fixture.meta)

beforeEach(() => {
  document.body.innerHTML = ''
  document.documentElement.removeAttribute('data-theme')
})

describe('the scoreboard for any contest club sync runs', () => {
  it('names every contest by the UI’s own table, and the table by it', () => {
    const b = boot()
    const ui = Object.fromEntries(CONTESTS.map((c) => [c.id, c.name]))
    expect(b.CONTEST_NAMES).toEqual(ui)
    // POSITIVE CONTROL: the same comparison catches a copy that drifted.
    expect({ ...b.CONTEST_NAMES, ilqp: 'Illinois Party' }).not.toEqual(ui)
  })

  it('has a centre for every county the rules data lists for the party, and nothing else', () => {
    const b = boot()
    const ilqp = seed.rulesets.find((r: { event: string }) => r.event === 'ilqp')
    const codes: string[] = ilqp.domains
      .find((d: { id: string }) => d.id === 'il_counties')
      .values.map((v: { code: string }) => v.code)
      .sort()
    expect(codes.length).toBe(102)
    expect(Object.keys(b.map.IL_COUNTIES).sort()).toEqual(codes)
    // Every centre is in Illinois, not a 0,0 placeholder off West Africa.
    for (const [code, [lat, lon]] of Object.entries(b.map.IL_COUNTIES)) {
      expect(lat, code).toBeGreaterThan(36.9)
      expect(lat, code).toBeLessThan(42.6)
      expect(lon, code).toBeGreaterThan(-91.6)
      expect(lon, code).toBeLessThan(-87.4)
    }
    // The map is drawn only for the WHOLE universe: one short, or one foreign, and it is
    // the chip board instead (a positive control for the match above).
    const values = codes.map((code) => ({ code }))
    expect(b.map.pickLayout(values)).not.toBeNull()
    expect(b.map.pickLayout(values.slice(1))).toBeNull()
    expect(b.map.pickLayout([...values.slice(1), { code: 'ZZZZ' }])).toBeNull()
  })

  it('tiles the counties: each cell holds its own centre, none holds another’s, no holes', () => {
    const b = boot()
    const mo = b.map.buildMosaic(b.map.IL_COUNTIES)
    expect(mo.cells.length).toBe(102)
    mo.cells.forEach((cell, i) => {
      expect(cell.length, mo.codes[i]).toBeGreaterThanOrEqual(3)
      const [x, y] = mo.sites[i]
      expect(b.map.inCell(cell, x, y), mo.codes[i]).toBe(true)
      mo.sites.forEach(([sx, sy], j) => {
        if (j !== i) expect(b.map.inCell(cell, sx, sy), `${mo.codes[i]} holds ${mo.codes[j]}`).toBe(false)
      })
    })
    // Between any two neighbouring counties, a point just on one's side of the line between
    // them belongs to EXACTLY one county: none would be a hole, two an overlap.
    let samples = 0
    for (let i = 0; i < mo.sites.length; i++) {
      for (let j = i + 1; j < mo.sites.length; j++) {
        const [ax, ay] = mo.sites[i]
        const [bx, by] = mo.sites[j]
        if (Math.hypot(ax - bx, ay - by) > 0.6) continue
        const px = ax + (bx - ax) * 0.45
        const py = ay + (by - ay) * 0.45
        const owners = mo.cells.filter((c) => b.map.inCell(c, px, py)).length
        expect(owners, `${mo.codes[i]}/${mo.codes[j]}`).toBe(1)
        samples += 1
      }
    }
    expect(samples).toBeGreaterThan(200)
  })

  it('shows the party’s claimed score and how it is made, from the server’s payload', () => {
    const b = boot()
    b.render(data(), meta())
    expect($('ev-name').textContent).toBe('Illinois QSO Party 2026')
    expect($('ev-station').textContent).toBe('W9XYZ — McLean')
    expect($('score-total').textContent).toBe('115')
    expect($('score-caption').textContent).toBe(b.STRINGS.multipliedPoints)
    const lines = $('score-lines').textContent!
    expect(lines).toContain(b.STRINGS.qsoPointsLine + '5')
    expect(lines).toContain(b.STRINGS.multsLine + '× 3')
    expect(lines).toContain(b.STRINGS.bonusLine + '+ 100')
    // No power math for a contest that has no power tier (the Field Day suite's
    // powered-board test is the positive control that this text can appear).
    expect($('col-left').textContent).not.toContain(b.STRINGS.powerLine)
    expect($('score-note').classList.contains('on')).toBe(false)
  })

  it('lights the counties the club has worked and marks the host’s own', () => {
    const b = boot()
    b.render(data(), meta())
    expect(b.map.snapshot()).toEqual({
      kind: 'mosaic',
      id: 'county',
      size: 102,
      lit: ['COOK', 'MCDN', 'WILL'],
    })
    expect($('sec-title').textContent).toBe(b.STRINGS.multCounty)
    expect($('sec-count').textContent).toBe(`3 ${b.STRINGS.ofWord} 102`)
    const labels = [...document.querySelectorAll('#mapsvg text')]
    expect(labels.length).toBe(102)
    const lit = [...document.querySelectorAll('#mapsvg path.cell.on')].map(
      (p) => labels[Number(p.getAttribute('data-i'))].textContent,
    )
    expect(lit.sort()).toEqual(['COOK', 'MCDN', 'WILL'])
    const home = [...document.querySelectorAll('#mapsvg path.cell.home')]
    expect(home.map((p) => labels[Number(p.getAttribute('data-i'))].textContent)).toEqual(['MCLN'])
    // The globe is not this board's map.
    expect($('globe-wrap').className).toBe('mosaic')
  })

  it('shows each universe the club counts, the scorer’s count, and the bonus stations', () => {
    const b = boot()
    b.render(data(), meta())
    expect($('mult-panel').classList.contains('mults')).toBe(true)
    expect($('bonus-title').textContent).toBe(`${b.STRINGS.multsTitle} · 3`)
    const rows = [...document.querySelectorAll('#mult-list .mult-row')]
    expect(rows.map((r) => r.getAttribute('data-id'))).toEqual(['county', 'mult', 'dxcc'])
    expect(rows.map((r) => r.textContent)).toEqual([
      `${b.STRINGS.multCounty}3${b.STRINGS.ofWord} 102`,
      `${b.STRINGS.multMult}0`,
      `${b.STRINGS.multDxcc}0${b.STRINGS.maxWord} 5`,
    ])
    const stations = [...document.querySelectorAll('#mult-list .mult-stations .st')].map((s) => [
      s.textContent,
      s.classList.contains('on'),
    ])
    expect(stations).toEqual([
      ['✓W9AWE', true],
      ['W9OAB', false],
    ])
  })

  it('names where the latest contact was, and lays the bands out as a line score', () => {
    const b = boot()
    b.render(data(), meta())
    const hero = $('hero').textContent!
    expect(hero).toContain('K9AAA')
    expect(hero).toContain('Cook') // COOK, named from the rules data in meta
    expect(hero).toContain('40m CW')
    expect(hero).toContain('SSB tent')
    const head = [...document.querySelectorAll('#bm-head th')].map((t) => t.textContent)
    expect(head).toEqual(['', '40m', '20m', b.STRINGS.totalCol])
    const grid = [...document.querySelectorAll('#bm-body tr')].map((tr) =>
      [...tr.children].map((c) => c.textContent),
    )
    expect(grid).toEqual([
      [b.STRINGS.phoneCol, '0', '1', '1'],
      [b.STRINGS.cwCol, '2', '0', '2'],
      [b.STRINGS.digitalCol, '0', '0', '0'],
      [b.STRINGS.totalCol, '2', '1', '3'],
    ])
    const pos = [...document.querySelectorAll('#pos-list .pos-row')]
    expect(pos[0].querySelector('.pos-label')!.textContent).toBe('CW tent')
    expect(pos[0].querySelector('.pos-band')!.textContent).toBe('40m')
  })

  it('draws a universe it has no map for as its codes, and one with no closed list as what was worked', () => {
    const b = boot()
    const m = meta()
    m.mults[0].values = m.mults[0].values.slice(1) // a county list no layout covers
    b.render(data(), m)
    expect(b.map.snapshot()!.kind).toBe('chips')
    expect(document.querySelectorAll('#chipmap .chip').length).toBe(101)
    expect(
      [...document.querySelectorAll('#chipmap .chip.on')].map((c) => c.textContent).sort(),
    ).toEqual(['COOK', 'MCDN', 'WILL'])

    // ARRL VHF's grids: no closed universe, so the board is the grids worked.
    const v = boot()
    const vm = meta()
    vm.event.kind = 'arrlvhf_sep'
    vm.map = undefined
    vm.mults = [{ id: 'grid', source: 'field', scope: 'perBand', values: [] }]
    const vd = data()
    vd.event.kind = 'arrlvhf_sep'
    vd.mults = [{ id: 'grid', count: 3, worked: ['EN50', 'EN61', 'FN31'] }]
    v.render(vd, vm)
    expect($('ev-name').textContent).toBe('ARRL September VHF Contest 2026')
    expect([...document.querySelectorAll('#chipmap .chip.on')].map((c) => c.textContent)).toEqual([
      'EN50',
      'EN61',
      'FN31',
    ])
    expect($('sec-count').textContent).toBe('3')
  })

  it('shows Winter Field Day’s claimed total and its objectives when the payload carries them', () => {
    const b = boot()
    const event = { ...fixture.data.event, kind: 'wfd', name: 'Winter Field Day', year: 2027, class: '3O', section: 'WI' }
    const m = {
      event,
      scoring_model: 'objectives',
      rules_year: 2027,
      sections: [],
      bonuses: [],
      objectives: [
        { id: 'wfd-alt-power-100', label: 'Operate 100% on alternative Power', multiplier: 2 },
        { id: 'wfd-qrp', label: 'Operate the event QRP', multiplier: 4 },
      ],
    }
    const d = {
      ...data(),
      event,
      score: { model: 'objectives', qso_points: 6, bonus_points: 0, total: 48, objective_multiplier: 7, objectives_claimed: 3 },
      sections_worked: [],
      claimed: ['wfd-qrp'],
    }
    b.render(d, m)
    expect($('score-total').textContent).toBe('48')
    expect($('score-caption').textContent).toBe(b.STRINGS.claimedPoints)
    const lines = $('score-lines').textContent!
    expect(lines).toContain(`${b.STRINGS.objectivesLine}3`)
    expect(lines).toContain(`${b.STRINGS.objectiveMultLine}7`)
    expect(lines).toContain('6 × (7 + 1) = 48')
    const rows = [...document.querySelectorAll('#bonus-list .bonus')].map((r) => r.textContent)
    expect(rows).toEqual(['☐Operate 100% on alternative Power×2', '☑Operate the event QRP×4'])
    expect($('bonus-list').querySelector('.bonus-more')!.textContent).toBe(
      b.STRINGS.moreOpen.replace('{n}', '1'),
    )
  })

  it('answers every reason there is no board in plain words, naming what it tried', () => {
    const info = {
      host: '192.168.1.10:7373',
      detail: 'its serial numbers must run in one sequence',
      contest: 'arrlss_cw',
    }
    const titles: Record<string, string> = {
      'no-club': 'noticeNoClubTitle',
      refused: 'noticeRefusedTitle',
      'club-starting': 'noticeStartingTitle',
      'club-session': 'noticeSessionTitle',
      'no-ruleset': 'noticeRulesTitle',
      'host-unreachable': 'noticeUnreachableTitle',
      'host-no-board': 'noticeNoBoardTitle',
      'host-idle': 'noticeHostIdleTitle',
      'host-bad-reply': 'noticeBadReplyTitle',
      'host-only': 'noticeGenericTitle', // an older Nexus's word
      'something-new': 'noticeGenericTitle',
    }
    for (const [reason, key] of Object.entries(titles)) {
      const b = boot()
      b.setInactive(true, { ...info, reason })
      expect($('inactive').classList.contains('on'), reason).toBe(true)
      expect($('notice-title').textContent, reason).toBe(b.STRINGS[key])
      expect($('notice-body').textContent, reason).not.toBe('')
      expect($('notice-do').textContent, reason).not.toBe('')
      expect($('notice').textContent, `${reason}: a template left unfilled`).not.toMatch(/[{}]/)
      if (reason.startsWith('host-') && reason !== 'host-only') {
        expect($('notice-body').textContent, reason).toContain('192.168.1.10:7373')
      }
    }
    const b = boot()
    b.setInactive(true, { ...info, reason: 'refused' })
    expect($('notice-body').textContent).toBe(
      'Club sync doesn’t run ARRL November Sweepstakes (CW): its serial numbers must run in one sequence.',
    )
  })

  it('keeps a live board up when only the host has gone quiet, and says so in its header', () => {
    const b = boot()
    b.render(data(), meta())
    b.setInactive(true, { reason: 'host-unreachable', host: '192.168.1.10:7373' })
    expect($('inactive').classList.contains('on')).toBe(false)
    expect($('score-total').textContent).toBe('115')
    expect($('ev-station').className).toBe('warn')
    expect($('ev-station').textContent).toBe(
      'Can’t reach the host at 192.168.1.10:7373 — this is its board from 17:10 UTC',
    )
    b.setInactive(false)
    expect($('ev-station').className).toBe('')
    expect($('ev-station').textContent).toBe('W9XYZ — McLean')
    // A reason that is not about the host never leaves an old board on the screen.
    b.setInactive(true, { reason: 'no-club' })
    expect($('inactive').classList.contains('on')).toBe(true)
  })

  it('gives the light board for ?theme=light, the dark one otherwise, and every colour both ways', () => {
    for (const [search, theme] of [
      ['?theme=light', 'light'],
      ['', 'dark'],
      ['?theme=dark', 'dark'],
      ['?theme=sepia', 'dark'],
    ]) {
      boot(search)
      expect(document.documentElement.getAttribute('data-theme'), search).toBe(theme)
    }
    const css = /<style>([\s\S]*?)<\/style>/.exec(html)![1]
    const block = (selector: string) => {
      const at = css.indexOf(selector + ' {')
      expect(at, selector).toBeGreaterThanOrEqual(0)
      return css.slice(at, css.indexOf('}', at))
    }
    const tokens = (b: string) =>
      new Map([...b.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()]))
    const dark = tokens(block(':root'))
    const light = tokens(block(':root[data-theme="light"]'))
    const colours = [...dark].filter(([, v]) => /^#|^rgba?\(/.test(v)).map(([k]) => k)
    expect(colours.length).toBeGreaterThan(12)
    expect(colours.filter((k) => !light.has(k))).toEqual([])
    // Every var() the page uses is one of its tokens or a value its script sets.
    const used = new Set([...css.matchAll(/var\((--[\w-]+)/g)].map((m) => m[1]))
    const set = ['--fit', '--band', '--cols']
    expect([...used].filter((k) => !dark.has(k) && !set.includes(k))).toEqual([])
  })
})
