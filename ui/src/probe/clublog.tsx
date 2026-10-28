// THE CLUB LOG ON A JOINED LAPTOP IN A REAL BROWSER, for `scripts/browser-probe --desktop clublog`
// (ui/probe/clublog-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the club's
// contacts, the position that logged each among them, fit under the club board on a laptop's Contest
// screen and in its club board pop-out, or whether the newest and the hundredth can be reached. This is the
// real ContestView (`?view=contest`) and the real `?panel=fdclub` window (`?view=popout`, `DetachedPanel`)
// on the app's stylesheets, for an Illinois QSO Party club this laptop joined (not hosts): five positions,
// the longest names a position can have, and the hundred newest of 4,321 contacts, as the host sent them.
// The station's answers are given at the IPC bridge; a command not answered here is refused.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { ContestView } from '../components/ContestView'
import { DetachedPanel } from '../DetachedPanel'
import { useViewport } from '../useViewport'
import settings from '../components/__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from '../types'

const T0 = 1_792_342_800
const params = new URLSearchParams(location.search)
const popout = params.get('view') === 'popout'

const NAMES = ['HQ', 'CW tent by the tower', 'Phone tent beside the parking lot', 'Digital', 'Satellite and VHF tent']
const posid = (i: number) => `${String.fromCharCode(97 + i).repeat(4)}000${i + 1}`
const board = NAMES.map((posName, i) => ({
  posid: posid(i),
  posName,
  band: ['20m', '40m', '15m', '20m', '2m'][i],
  mode: ['CW', 'CW', 'PH', 'DIG', 'PH'][i],
  operator: `KD9A${String.fromCharCode(65 + i)}A`,
  qsos: 860 + i,
  rate: 40 + i,
  lastSeenSecs: 2,
  link: i === 0 ? 'host' : 'connected',
}))
const COUNTIES = ['COOK', 'DUPG', 'LAKE', 'WILL', 'KANE', 'MCHE', 'WINN', 'PEOR', 'CHAM', 'MACO']
const rows = Array.from({ length: 100 }, (_, i) => ({
  call: `${['K9', 'W9', 'N9', 'KD9', 'WB9'][i % 5]}${String.fromCharCode(65 + (i % 26))}${String.fromCharCode(65 + ((i * 7) % 26))}Z`,
  class: '',
  section: '',
  rcvd: ['599', COUNTIES[i % COUNTIES.length]],
  band: ['20m', '40m', '15m', '20m', '2m'][i % 5],
  mode: ['CW', 'CW', 'PH', 'DIG', 'PH'][i % 5],
  submode: i % 5 === 3 ? 'RTTY' : '',
  whenUnix: T0 - 40 * i,
  posid: posid(i % 5),
  seq: 1000 - i,
  posName: NAMES[i % 5],
  ...(i % 9 === 4 ? { dupe: true } : {}),
}))
const club = {
  syncState: 'synced',
  queued: 0,
  offlineSinceUnix: 0,
  hosting: false,
  event: 'W9XYZ Illinois QSO Party',
  hostCall: 'W9XYZ',
  score: 12_345,
  qsos: 4321,
  sections: 0,
  skewSecs: 0,
  dupes: [],
  board,
}
const fieldDay = {
  running: false,
  state: 'Idle',
  qsoCount: 861,
  sections: 0,
  points: 1,
  multCount: 1,
  event: 'ilqp',
  role: 'in_state',
  log: rows.filter((r) => r.posid === posid(1)).map(({ posid: _p, seq: _s, posName: _n, ...r }) => r),
  club,
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  composing: [],
  composingText: '',
  assistanceOn: [],
  upload: { enabled: false, destinations: [], available: [], hint: '' },
  eventStartUnix: T0 - 36_000,
  eventEndUnix: T0 + 3600,
} as unknown as FieldDayStatus
const snapshot = {
  mycall: 'W9XYZ',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.03, band: '20m', sideband: 'USB', operatingMode: 'cw', txEnabled: false, txAllowed: true },
  link: { tier: 'Ft8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay,
}

document.documentElement.setAttribute('data-theme', params.get('theme') === 'light' ? 'light' : 'dark')
localStorage.clear()

const answers: Record<string, () => unknown> = {
  get_snapshot: () => snapshot,
  get_settings: () => ({ ...settings, fdActive: true, fdEvent: 'ilqp', fdJoinAddr: '192.168.1.10', fdPositionId: posid(1) }),
  fd_club_log_newest: () => ({ total: 4321, rows }),
  contest_removed: () => [],
  contest_unmerged: () => [],
  get_band_plan: () => [],
  get_need_alerts: () => [],
  get_fd_ruleset: () => null,
  get_window_on_top: () => ({ supported: true, on: false }),
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the club log probe`))
  },
}
;(window as unknown as { __refused: () => string[] }).__refused = () => [...refused]

function Contest() {
  useViewport(1)
  return (
    <div className="app" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <main className="layout single" style={{ flex: '1 1 auto', minHeight: 0 }}>
        <ContestView fieldDay={fieldDay} onSetMode={() => {}} />
      </main>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(popout ? <DetachedPanel panel="fdclub" /> : <Contest />)
