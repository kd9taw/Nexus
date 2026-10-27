// THE ACTIVATION PANE IN A REAL BROWSER, for `scripts/browser-probe --desktop activation`
// (ui/probe/activation-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the
// pane's lines fit across its box, whether it spills onto the transmit controls beside it, or whether the
// Stop checklist fits the window. Three cases, on the app's stylesheets, with the station's answers given
// at the IPC bridge (a command not answered here is refused):
//
//   `?case=rail`   the real dashboard rail with the Activation box at its top, beside a cockpit column
//                  that ends in a TX strip (Stop TX, Tune) and a TX dock (PTT), laid out as a cockpit
//                  lays them out — in the flow, not fixed;
//   `?case=window` the pane in its own window, the shell DetachedPanel gives it (`.app.detached
//                  .detached-activation`);
//   `?case=over`   the "Activation over?" checklist, open, with four files, QRT offered and every
//                  reminder.
//
// `&control=1` is the CONTROL: a 2400 px unbreakable line inside the pane (or the checklist), and the TX
// strip and dock moved under the rail where the pane stands, so the pane covers them. The probe must see the overflow and the
// cover there.
import '../styles.css'
import '../cockpit-panes.css'
import { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { OwnedDashRail } from '../components/DashRail.testkit'
import { saveRailWidth } from '../features/dashRail'
import { surfaceSet } from '../features/windowScope'
import { ActivationPane } from '../components/ActivationPane'
import { ActivationOverDialog } from '../components/ActivationOverDialog'
import settings from '../components/__fixtures__/defaultSettings.json'
import type { Activation } from '../types'

const params = new URLSearchParams(location.search)
const which = params.get('case') ?? 'rail'
const control = params.get('control') === '1'

const DAY = 86_400
const NOW = Math.floor(Date.now() / 1000)
const START = NOW - 2 * 3600 - 300
/** A two-fer, one park valid and one short, park to park on both, on a state line with no state picked
 *  and no grid: every line the pane and the checklist can draw. */
const ACT: Activation = {
  program: 'POTA',
  reference: 'US-10234,US-10567',
  qsoCount: 7,
  states: ['US-ND', 'US-MN'],
  myState: null,
  parks: [
    { reference: 'US-10234', count: 7, p2p: 2 },
    { reference: 'US-10567', count: 12, p2p: 1 },
  ],
  grid: null,
  startedAt: START,
  utcDay: Math.floor(NOW / DAY),
}
const ROWS = Array.from({ length: 40 }, (_, i) => ({
  id: `q${i}`,
  call: i % 5 === 0 ? `VE3/KD${i}ABC/P` : `W${i % 10}ABC`,
  band: ['20m', '40m', '17m'][i % 3],
  mode: i % 2 ? 'SSB' : 'CW',
  freqMhz: 14.285,
  whenUnix: NOW - i * 180,
  ota: { myProgram: 'POTA', myRef: ACT.reference, theirProgram: i % 4 === 0 ? 'POTA' : null, theirRef: i % 4 === 0 ? 'US-0001,US-0002' : null },
}))
const day = (unix: number) => Math.floor(unix / DAY) * DAY
const files = [ACT.parks![0].reference, ACT.parks![1].reference].flatMap((reference) =>
  [day(START), day(START) + DAY].map((dayStartUnix) => ({
    program: 'POTA',
    reference,
    dayStartUnix,
    date: new Date(dayStartUnix * 1000).toISOString().slice(0, 10),
    callsign: 'KD9TAW/P',
    qsos: 9,
  })),
)

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
saveRailWidth(300)
surfaceSet('nexus.dashrail.config', JSON.stringify({ slots: { rail1: 'activation', rail2: 'clock', rail3: 'bandTiles', rail4: 'spacewx' } }))

const answers: Record<string, (args?: Record<string, unknown>) => unknown> = {
  get_settings: () => settings,
  ask_log: (args) => {
    const q = (args?.q ?? {}) as { kind?: string; query?: unknown }
    if (q.kind !== 'page') return null
    return { query: q.query, revision: 1, orderRev: 1, contentRev: 1, total: ROWS.length, rows: ROWS, keys: ROWS.map((r) => r.id), editKeys: ROWS.map((r) => `ek:${r.id}`) }
  },
  log_activations: () => files,
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd](args) as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the activation probe`))
  },
}
;(window as unknown as { __refused: () => string[] }).__refused = () => [...refused]

/** The control's line: one word 2400 px wide, which nothing can wrap. */
function useControlLine(selector: string) {
  useEffect(() => {
    if (!control) return
    const id = window.setInterval(() => {
      const host = document.querySelector(selector)
      if (!host || host.querySelector('.probe-wide')) return
      const wide = document.createElement('div')
      wide.className = 'probe-wide'
      wide.textContent = 'W'.repeat(240)
      wide.style.whiteSpace = 'nowrap'
      wide.style.fontSize = '16px'
      host.appendChild(wide)
    }, 100)
    return () => window.clearInterval(id)
  }, [selector])
}

function Probe() {
  useViewport(1)
  useControlLine(which === 'over' ? '.activation-over' : '.act-pane')
  if (which === 'window') {
    return (
      <div className="app detached detached-activation">
        <h2 className="act-window-title">Activation</h2>
        <ActivationPane act={ACT} logTick={1} />
      </div>
    )
  }
  if (which === 'over') {
    return (
      <div className="app">
        <ActivationOverDialog act={ACT} canQrt onKeep={() => {}} onEnd={async () => {}} />
      </div>
    )
  }
  // The control lays the TX strip and dock over the rail, where the pane stands.
  // Under the rail (a negative z-index paints beneath the rail's boxes), so the pane is what covers them.
  const over = control ? ({ position: 'fixed', right: 0, width: 300, zIndex: -1 } as const) : undefined
  return (
    <div className="app" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', flex: '1 1 auto', minHeight: 0 }}>
        <div style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          <div style={{ flex: '1 1 auto' }} />
          <div className="cockpit-txstrip" data-tx="TX strip" style={{ display: 'flex', gap: 8, padding: 6, ...(over && { ...over, top: 60, height: 44 }) }}>
            <button type="button" className="op-btn stop" data-tx="Stop TX">Stop TX</button>
            <button type="button" data-tx="Tune">Tune</button>
          </div>
          <div className="cockpit-txdock" data-tx="TX dock" style={{ height: 90, ...(over && { ...over, top: 110 }) }}>
            <button type="button" data-tx="PTT">PTT</button>
          </div>
        </div>
        <OwnedDashRail
          section="phone"
          myGrid="EN52"
          theme="dark"
          stations={[]}
          prop={null}
          needByCall={new Map()}
          activation={{ act: ACT, logTick: 1 }}
          onHide={() => {}}
        />
      </div>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
