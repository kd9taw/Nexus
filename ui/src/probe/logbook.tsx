// THE LOGBOOK'S WIDTH AND ITS COLUMN MOVES IN A REAL BROWSER, for `scripts/browser-probe --desktop logbook`
// (ui/probe/logbook-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the
// Logbook spans the window it is shown in, whether a table too wide for it scrolls inside its own
// scroller rather than the page, or whether a real drag on a header reaches the column move.
//
// The real Logbook in the app's own wrapper (`.app` > `main.layout single`) on the app's stylesheets, at
// the zoom the app picks for the window (`fitScale`) with the real `useViewport` publishing the size
// class. The station's answers are given at the IPC bridge; a command not answered here is refused.
//
// `?cols=wide` shows every column the chooser offers, so the table is wider than any window here.
// `?control=cap` is the CONTROL: the old centred measure put back on the Logbook's panel, which the
// probe must see leave the panel narrower than the window's content box.
// `?zoom=1.5` pins the UI zoom instead of the window's fit (the row-button sweep runs at 100% and
// 150%), `?sat=1` gives the station satellite names so the row carries its widest cluster (the
// satellite menu too), `?n=10000` sets the log's size, and `?control=thin` is the row sweep's
// CONTROL: the old 22 px row buttons put back, which the probe must see fail the target size.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { useState } from 'react'
import { Logbook } from '../components/Logbook'
import { answerAs } from '../features/logAnswers.testkit'
import type { LogQuestion } from '../features/logAnswers'
import { LOG_COLUMNS, type LogColumnsSaved } from '../features/logColumns'
import { fitScale } from '../useScale'
import { useViewport } from '../useViewport'
import type { LoggedQso } from '../types'

const params = new URLSearchParams(location.search)
document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
if (params.get('control') === 'cap') {
  const s = document.createElement('style')
  s.textContent = '.layout.single > .panel.log-view { max-width: 1100px !important; margin: 0 auto !important; }'
  document.head.appendChild(s)
}
if (params.get('control') === 'thin') {
  const s = document.createElement('style')
  s.textContent = '.logbook-row .log-rowbtn { height: 22px !important; min-width: 22px !important; }'
  document.head.appendChild(s)
}
const zoom = Number(params.get('zoom')) || fitScale(innerWidth, innerHeight) / 100
document.documentElement.style.setProperty('--ui-zoom', String(zoom))

const base = 1_790_000_000
const LOG: LoggedQso[] = Array.from({ length: Number(params.get('n')) || 60 }, (_, n) => ({
  id: `id-${n}`, call: `K${n}ABC`, grid: 'EN52', band: n % 2 ? '40m' : '20m', freqMhz: n % 2 ? 7.074 : 14.074, mode: n % 3 ? 'FT8' : 'CW',
  rstSent: '-10', rstRcvd: '-12', name: 'Pat', qth: 'Springfield',
  comment: n % 3 ? null : 'Big signal on the long path, running 100 W into a dipole', notes: n % 5 ? null : 'Dipole at 10 m',
  country: n % 7 ? 'United States' : 'Democratic Republic of the Congo', whenUnix: base - n * 600, confirmed: n % 4 === 0, awardConfirmed: n % 4 === 0,
  qslRcvd: { card: false, lotw: n % 4 === 0, eqsl: false, qrz: false }, extra: [['CQZ', '4'], ['ITUZ', '8'], ['CONT', 'NA']],
}))

const answers: Record<string, (args: Record<string, unknown>) => unknown> = {
  ask_log: (a) => answerAs(a.q as LogQuestion, LOG),
  lotw_sat_names: () => (params.get('sat') ? ['AO-91', 'ISS', 'SO-50'] : []),
  log_operators: () => [],
  log_activations: () => [],
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd](args) as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the Logbook probe`))
  },
}
const probe = window as unknown as { __refused: () => string[]; __saved: (LogColumnsSaved | null)[] }
probe.__refused = () => [...refused]
probe.__saved = []

const WIDE: LogColumnsSaved = { shown: LOG_COLUMNS.filter((c) => !c.base).map((c) => c.id), hidden: [], widths: {} }

function Probe() {
  useViewport(zoom)
  const [columns, setColumns] = useState<LogColumnsSaved | null>(params.get('cols') === 'wide' ? WIDE : null)
  return (
    <div className="app">
      <main className="layout single">
        <Logbook
          defaultBand="20m" defaultFreqMhz={14.074} defaultMode="FT8" logTick={1}
          columns={columns}
          onColumnsChange={(c) => {
            probe.__saved.push(c)
            setColumns(c)
          }}
        />
      </main>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
