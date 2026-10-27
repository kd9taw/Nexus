// THE LOGBOOK CONTACT CARD IN A REAL BROWSER, for `scripts/browser-probe --desktop contactcard`
// (ui/probe/contactcard-probe.mjs drives it). jsdom never lays out, so nothing else can see whether a
// double-click on a row reaches the row, or whether the card fits the window with Save and Cancel in
// view. This is the real Logbook on the app's stylesheets, holding a log with fourteen contacts of one
// call; the station's answers are given at the IPC bridge, and a command not answered here is refused.
//
// The callbook answer for the call (with a photo) is asked once through `qrzLookup` before the card
// opens, as the log strip's own lookup would have: the card must show that photo.
//
// `?footer=static` is the CONTROL: the card's Save/Cancel bar is put back into the flow, so at 1024×768
// it starts below the fold and the probe must see it NOT on screen.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { Logbook } from '../components/Logbook'
import { qrzLookup } from '../api'
import { answerAs } from '../features/logAnswers.testkit'
import type { LogQuestion } from '../features/logAnswers'
import type { LoggedQso, QrzLookup } from '../types'

const params = new URLSearchParams(location.search)
document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
if (params.get('footer') === 'static') {
  const s = document.createElement('style')
  s.textContent = '.contact-card-actions { position: static !important; }'
  document.head.appendChild(s)
}

const PHOTO = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#3a7"/></svg>')}`
const BOOK: QrzLookup = {
  call: 'EC1DD', name: 'Pedro Antonio Gomez de la Fuente', nickname: null, qth: 'Santiago de Compostela', grid: 'IN52TK',
  state: 'Galicia', country: 'Spain', dxcc: 281, cqZone: 14, ituZone: 37, image: PHOTO, county: 'A Coruna', iota: 'EU-080',
  email: null, web: null, qslVia: null, lotw: true, eqsl: false, paperQsl: true, utcOffset: 1,
} as QrzLookup

const base = 1_790_000_000
const LOG: LoggedQso[] = Array.from({ length: 14 }, (_, n) => ({
  id: `id-${n}`, call: 'EC1DD', grid: 'IN52TK', band: n % 2 ? '40m' : '20m', freqMhz: n % 2 ? 7.074 : 14.074, mode: n % 3 ? 'FT8' : 'CW',
  rstSent: n % 3 ? '-10' : '599', rstRcvd: n % 3 ? '-12' : '579', name: 'Pedro', qth: 'Santiago de Compostela',
  comment: n === 13 ? 'A long shared comment that wraps over more than one line of the card, as an operator types one after a good ragchew on 20 metres' : null,
  notes: n === 13 ? 'Dipole at 10 m\nFT-710 at 50 W' : null, country: 'Spain', whenUnix: base + n * 3600, confirmed: n % 4 === 0,
  awardConfirmed: n % 4 === 0, qslRcvd: { card: false, lotw: n % 4 === 0, eqsl: false, qrz: false },
  extra: n === 13 ? [['CNTY', 'A CORUNA'], ['CQZ', '14'], ['ITUZ', '37'], ['APP_OTHERPROG_NOTE', 'imported']] : [],
}))
for (let n = 0; n < 30; n++) LOG.push({ ...LOG[0], id: `o-${n}`, call: `K${n}ABC`, whenUnix: base - n * 600, extra: [] })

const answers: Record<string, (args: Record<string, unknown>) => unknown> = {
  ask_log: (a) => answerAs(a.q as LogQuestion, LOG),
  lotw_sat_names: () => [],
  log_operators: () => [],
  log_activations: () => [],
  qrz_lookup: () => BOOK,
  qso_history: () => [
    { seq: 1, kind: 'create', source: 'operator', run: 'r1', atMs: (base + 13 * 3600) * 1000, undoes: null, undoneBy: null, changes: [] },
  ],
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd](args) as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the contact card probe`))
  },
}
;(window as unknown as { __refused: () => string[] }).__refused = () => [...refused]

void qrzLookup('EC1DD').then(() => document.documentElement.setAttribute('data-looked-up', 'yes'))
createRoot(document.getElementById('root')!).render(
  <div className="app" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
    <Logbook defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1} />
  </div>,
)
