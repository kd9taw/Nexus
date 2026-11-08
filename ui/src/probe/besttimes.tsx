// THE BEST TIMES BOX BESIDE THE TRANSMIT CONTROLS IN A REAL BROWSER, for `scripts/browser-probe --desktop
// besttimes` (ui/probe/besttimes-probe.mjs drives it). jsdom never lays out, so nothing else can see
// whether the box growing when its answer lands moves a transmit control.
//
// The real dashboard rail with the Best times box at its top, beside a cockpit column ending in a TX
// strip (Stop TX, Tune) and a TX dock (PTT), laid out in the flow as a cockpit lays them out. The box
// opens on a call already asked (the per-window "last call"), and its answer is held back 1.5 s, so the
// probe sees the box before and after it grows (`data-answered` on <html> once it has).
//
// `&control=1` is the CONTROL: the same box drawn bare in the cockpit column ABOVE the TX strip, outside
// any frame, where its growing must push Stop TX down; the probe must see that move.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { OwnedDashRail } from '../components/DashRail.testkit'
import { saveRailWidth } from '../features/dashRail'
import { surfaceSet } from '../features/windowScope'
import { BestTimesPane } from '../components/prop/BestTimesPane'
import settings from '../components/__fixtures__/defaultSettings.json'

const params = new URLSearchParams(location.search)
const control = params.get('control') === '1'

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
saveRailWidth(340)
surfaceSet('nexus.dashrail.config', JSON.stringify({ slots: { rail1: 'bestTimes', rail2: 'clock', rail3: 'bandTiles', rail4: 'spacewx' } }))
surfaceSet('nexus.bestTimes.call', 'EC1DD')

const now = Math.floor(Date.now() / 3_600_000) * 3600
const ANSWER = {
  state: 'answer', call: 'EC1DD', entity: 'Spain', basis: { kind: 'grid', from: 'callbook', grid: 'IN52TK' },
  km: 1530.6, bearing: 227.4, engine: 'p533', power: { watts: 100, source: 'default', amplifier: false },
  answer: {
    engine: 'p533', mode: 'FT8', perMode: true,
    now: { kind: 'try', band: '20m', word: 'Good', untilUnix: now + 4 * 3600, allDay: false, runnerUp: { band: '17m', word: 'Fair' } },
    windows: [5, 8, 11, 15, 19].map((o, i) => ({ band: ['15m', '40m', '20m', '80m', '17m'][i], fromUnix: now + o * 3600, toUnix: now + (o + 2) * 3600, word: 'Fair', greyline: i === 1, longShot: false })),
    week: { forecastUntilUnix: now + 2 * 86_400, flags: [{ dayUnix: now - (now % 86_400) + 86_400, kp: 5.3 }] },
    chart: ['10m', '12m', '15m', '17m', '20m', '30m', '40m', '60m', '80m', '160m'].map((band, b) => ({
      band, licensed: b !== 7, cells: Array.from({ length: 24 }, (_, h) => ({ word: (['Closed', 'Marginal', 'Fair', 'Good', 'Excellent'] as const)[(h + b) % 5] })),
    })),
    hours: [], nowHour: new Date().getUTCHours(), longUsed: false,
  },
}
const answers: Record<string, () => unknown> = {
  get_settings: () => settings,
  get_best_times: () =>
    new Promise((resolve) =>
      setTimeout(() => {
        resolve(ANSWER)
        setTimeout(() => document.documentElement.setAttribute('data-answered', 'yes'), 300)
      }, 1500),
    ),
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the Best times probe`))
  },
}
;(window as unknown as { __refused: () => string[] }).__refused = () => [...refused]

function Probe() {
  useViewport(1)
  return (
    <div className="app" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', flex: '1 1 auto', minHeight: 0 }}>
        <div style={{ flex: '1 1 auto', display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          {control ? <BestTimesPane units="metric" /> : <div style={{ flex: '1 1 auto' }} />}
          <div className="cockpit-txstrip" data-tx="TX strip" style={{ display: 'flex', gap: 8, padding: 6 }}>
            <button type="button" className="op-btn stop" data-tx="Stop TX">Stop TX</button>
            <button type="button" data-tx="Tune">Tune</button>
          </div>
          <div className="cockpit-txdock" data-tx="TX dock" style={{ height: 90 }}>
            <button type="button" data-tx="PTT">PTT</button>
          </div>
        </div>
        {!control && (
          <OwnedDashRail section="phone" myGrid="EN52" theme="dark" stations={[]} prop={null} needByCall={new Map()} onHide={() => {}} />
        )}
      </div>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
