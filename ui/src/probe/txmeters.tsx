// THE TRANSMIT METERS IN A REAL BROWSER, for `scripts/browser-probe --desktop txmeters`
// (ui/probe/txmeters-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the
// COMP row's level bar, PROC and the transmit-bandwidth chip fit beside the meter, or whether the
// temperature rows sit in the grid without pushing anything sideways.
//
// Three hosts, as the app has them: Phone's dock (`.cockpit-txdock`, the full window width, with the
// COMP row's controls; below md), the TX strip's meters cell (`&host=strip`, Phone and CW from md up,
// the same controls) and Operate's fixed-width telemetry cell (`.cq-telemetry`, meters only). The
// radio reports a PA temperature and sits behind a Celsius amplifier, so both temperature rows draw.
// The controls record what they were asked, for the probe to read back.
//
// `?units=imperial` shows °F. `&control=1` is the CONTROL: a transmit-bandwidth step named with a
// 400-character unbreakable word, which must overflow its row, and the probe must see it.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { TxMeters } from '../components/TxMeters'
import type { RadioStatus } from '../types'
import { UNITS_KEY } from '../units'

const params = new URLSearchParams(location.search)
document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
localStorage.setItem(UNITS_KEY, params.get('units') === 'imperial' ? 'imperial' : 'metric')

const wide = params.get('control') === '1' ? 'W'.repeat(400) : 'NAR'
const RADIO = {
  rigKeyed: false,
  swrScaleVerified: true,
  txSwr: null,
  compLevel: 0.35,
  comp: true,
  txProcSwitch: true,
  txBwSteps: ['WIDE', 'MID', wide],
  txBw: wide,
  paTempC: 47,
  amp: { family: 'kpa', model: '', linked: true, reason: '', temp: 52, tempCelsius: true, alarm: '' },
} as unknown as RadioStatus

const asked: { comp: number[]; proc: boolean[]; bw: string[] } = { comp: [], proc: [], bw: [] }
;(window as unknown as { __asked: typeof asked }).__asked = asked

const controls = {
  onCompLevel: (f: number) => asked.comp.push(f),
  onProc: (on: boolean) => asked.proc.push(on),
  onTxBw: (s: string) => asked.bw.push(s),
}

// `&host=strip` is the compact dock's host (the 1080p pass, md and up): the TX strip's meters cell, after
// the strip's own TX On · Tune · Stop TX and its state, as CockpitTxStrip draws them. Its wrapper carries
// `data-probe="dock"` so every check below reads it as it reads the dock, and `data-probe="strip"` marks
// the strip whose height the meters must not raise.
const strip = params.get('host') === 'strip'

createRoot(document.getElementById('root')!).render(
  <div className="app" style={{ height: '100%', position: 'relative' }}>
    <div className="cq-telemetry" data-probe="cell" style={{ position: 'absolute', top: 8, left: 8 }}>
      <TxMeters radio={RADIO} inline />
    </div>
    {strip ? (
      <div className="cockpit-txstrip" data-probe="strip" style={{ position: 'absolute', top: 'auto', left: 0, right: 0, bottom: 0 }}>
        <div className="op-controls cq-txctl" role="group">
          <button type="button" className="op-btn monitor on">TX On</button>
          <button type="button" className="op-btn tune">Tune</button>
          <button type="button" className="op-btn stop">Stop TX</button>
        </div>
        <span className="cq-statecap rx">Receiving</span>
        <div className="cockpit-txstrip-meters" data-probe="dock">
          <TxMeters radio={RADIO} strip controls={controls} />
        </div>
      </div>
    ) : (
      <div className="cockpit-txdock" data-probe="dock" style={{ position: 'absolute', left: 0, right: 0, bottom: 0 }}>
        <TxMeters radio={RADIO} pinned controls={controls} />
      </div>
    )}
  </div>,
)
