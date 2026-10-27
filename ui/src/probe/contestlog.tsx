// THE CONTEST LOGGER WINDOW IN A REAL BROWSER, for `scripts/browser-probe --desktop contestlog`
// (ui/probe/desktop-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the
// window's entry — the call, the exchange boxes, Log, the dupe warning — and its last contacts are on
// screen at the window's smallest size, in the whole window and in its entry-only layout. This is the real `?panel=contestlog` window (`DetachedPanel`) on
// the app's stylesheets; the station's answers are given at the IPC bridge, and a command not answered
// here is refused.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { DetachedPanel } from '../DetachedPanel'
import settings from '../components/__fixtures__/defaultSettings.json'
import { LOGGER_LAYOUT_KEY } from '../features/contestLoggerLayout'

const T0 = 1_792_342_800
const params = new URLSearchParams(location.search)
/** `?rstsent=1`: the entry's optional "RST sent" box turned on (Settings `contestRstSentBox`), which the
 *  engine reports as the session's own report. */
const rstSent = params.get('rstsent') === '1'
/** `?edit=club`: twelve contacts, more than the last contacts show, at a club, where a last contact's
 *  Edit is refused with the line the list leaves under it. */
const editClub = params.get('edit') === 'club'
const more = editClub
  ? Array.from({ length: 11 }, (_, i) => ({
      call: `N9A${String.fromCharCode(65 + i)}A`,
      class: '',
      section: '',
      band: '20m',
      mode: 'PH',
      submode: 'SSB',
      whenUnix: T0 + 60 * (i + 1),
      rcvd: ['59', 'KANE'],
    }))
  : []
/** An Illinois QSO Party with one contact, K9AAA on 20 m phone, and the same call in the entry: a dupe. */
const fieldDay = {
  running: false,
  state: 'Idle',
  qsoCount: 1 + more.length,
  sections: 0,
  points: 1,
  multCount: 1,
  event: 'ilqp',
  role: 'in_state',
  log: [
    { call: 'K9AAA', class: '', section: '', band: '20m', mode: 'PH', submode: 'SSB', whenUnix: T0, rcvd: ['59', 'COOK'] },
    ...more,
  ],
  ...(editClub ? { club: { syncState: 'synced', queued: 0, hosting: false, dupes: [], board: [] } } : {}),
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  composing: [],
  composingText: '',
  assistanceOn: [],
  ...(rstSent ? { rstSentBox: '599' } : {}),
  upload: { enabled: false, destinations: [], available: ['clublog'], hint: 'hint' },
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
}
const snapshot = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: false, txAllowed: true },
  link: { tier: 'Ft8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay,
  contestEntry: { rev: 1, call: 'K9AAA', fields: {}, marks: null },
}

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
// `?layout=entry`: the window opened for the entry alone (features/contestLoggerLayout).
if (params.get('layout') === 'entry') localStorage.setItem(LOGGER_LAYOUT_KEY, 'entry')

const answers: Record<string, () => unknown> = {
  get_snapshot: () => snapshot,
  get_settings: () => ({ ...settings, fdActive: true, fdEvent: 'ilqp', fdOperator: 'KD9TAW', contestRstSentBox: rstSent }),
  get_window_on_top: () => ({ supported: true, on: false }),
  contest_entry_put: () => 2,
  contest_removed: () => [],
  contest_unmerged: () => [],
  get_band_plan: () => [],
  get_need_alerts: () => [],
  get_scp_calls: () => [],
  get_propagation: () => null,
  get_fd_ruleset: () => null,
  resolve_entity: () => null,
  contest_place: () => null,
  get_call_history: () => null,
  get_sat_transponder: () => null,
}
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    return Promise.reject(new Error(`${cmd} is not answered in the contest logger probe`))
  },
}

createRoot(document.getElementById('root')!).render(<DetachedPanel panel="contestlog" />)
