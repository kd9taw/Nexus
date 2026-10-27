// THE TOP BAR WITH A STATUS-LANE MESSAGE, IN A REAL BROWSER, for `scripts/browser-probe --desktop topbar`
// (ui/probe/topbar-probe.mjs drives it). jsdom never lays out, so nothing else can see the bar wrap.
// This is the real TopBar on the app's stylesheets, given the props App gives it on the FT screen
// (`?view=operate`) and on CW (`?view=cw`), inside the real `.app` with the real useViewport, so the size
// class and the zoom are the app's own.
//
// `&lane=on` raises the slot-unkey-failed lane (the longest critical message the bar carries) through
// the real status store, exactly as App does when the station reports it. `&zoom=<n>` sets the UI scale.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { TopBar } from '../components/TopBar'
import { setStatus } from '../status'
import { slotUnkeyFailedLane } from '../features/slotUnkeyFailed'
import { useViewport } from '../useViewport'
import type { LinkState, RadioStatus } from '../types'

const params = new URLSearchParams(location.search)
const view = params.get('view') ?? 'operate'
const ft = view === 'operate'

document.documentElement.setAttribute('data-theme', 'dark')
if (params.get('zoom')) document.documentElement.style.setProperty('--ui-zoom', params.get('zoom')!)
localStorage.clear()
// No station here: every command is refused, and nothing in the bar needs one to lay out.
window.__TAURI_INTERNALS__ = { invoke: (cmd: string) => Promise.reject(new Error(`${cmd} is not answered in the top bar probe`)) }

const RADIO = {
  dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true,
  nextSlotMs: 11200, txCycleAuto: true, txEven: false, txEnabled: false, txAllowed: true,
  transmitting: false, tuning: false, qsoRecording: false, catOk: true, dtSec: 0, clockOffsetMs: 0,
} as unknown as RadioStatus
const LINK = { tier: 'FT8', dtSec: 0.1 } as unknown as LinkState

if (params.get('lane') === 'on') {
  setStatus('slotUnkeyFailed', slotUnkeyFailedLane({ at: 1761570000, why: 'the radio did not answer the unkey' }))
}

function Probe() {
  useViewport(Number(params.get('zoom') ?? 1))
  const noop = () => {}
  return (
    <div className="app">
      <TopBar
        mycall="KD9TAW" mygrid="EN52xa" radio={RADIO} link={LINK} bandPlan={[]}
        onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop}
        onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} onStopRecording={noop}
        hideTxControls={ft}
        hideFrequencyControl
        hideDigitalChrome={!ft}
        ftScreen={ft}
        tier="FT8" onTierChange={noop}
        onOpenGuide={noop} field={false} onFieldChange={noop}
      />
      <main className="probe-below" style={{ flex: 1 }} />
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
