// STOP TX ABOVE EVERY OVERLAY, IN A REAL BROWSER, for `scripts/browser-probe --desktop stopabove`
// (ui/probe/stopabove-probe.mjs drives it). jsdom neither stacks nor hit-tests, so nothing else can see whether the
// click where Stop TX is drawn reaches it with an overlay open.
//
// The real TopBar as the Logbook gives it (its TX cluster on: TX, Tune, Stop TX, and the band pill), inside the real
// `.app` with the real useViewport, on the app's stylesheets, with the real StopLayer, and ONE overlay over it, chosen
// by `?overlay=`: `dialog` (a ui/Dialog with a field in it), `confirm` (a ConfirmHost question), `logconfirm` (the
// prompt to log), `radiopicker` (the launch radio picker), `scrim` (an aria-modal `.logconfirm-backdrop` as the
// Logbook draws its confirmations), or `bandmenu` (none drawn here: the probe opens the band pill's menu with a real
// click). `window.__probeHalts()` counts Stop TX's clicks. No station: every command is refused.
import '../styles.css'
import '../cockpit-panes.css'
import { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { TopBar } from '../components/TopBar'
import { StopLayer } from '../components/StopLayer'
import { Dialog } from '../components/ui/Dialog'
import { ConfirmHost, confirmDialog } from '../confirm'
import { LogConfirm } from '../components/LogConfirm'
import { RadioPicker } from '../components/RadioPicker'
import { useViewport } from '../useViewport'
import type { LinkState, LoggedQso, RadioStatus } from '../types'
import type { RadioLaunchInfo } from '../api'

const params = new URLSearchParams(location.search)
const overlay = params.get('overlay') ?? 'dialog'

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
window.__TAURI_INTERNALS__ = { invoke: (cmd: string) => Promise.reject(new Error(`${cmd} is not answered in the stop-above probe`)) }

let halts = 0
;(window as unknown as { __probeHalts: () => number }).__probeHalts = () => halts

const RADIO = {
  dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true, nextSlotMs: 11200, txCycleAuto: true,
  txEven: false, txEnabled: true, txAllowed: true, transmitting: true, tuning: false, qsoRecording: false, catOk: true,
  dtSec: 0, clockOffsetMs: 0,
} as unknown as RadioStatus
const LINK = { tier: 'FT8', dtSec: 0.1 } as unknown as LinkState
const QSO = { call: 'IA5X', grid: 'JN52', band: '20m', mode: 'FT8', rstSent: '-12', rstRcvd: '-09', comment: null } as unknown as LoggedQso
const PICKER = {
  showPicker: true,
  radios: [{ id: 0, name: 'HF', inUse: false }, { id: 1, name: 'VHF', inUse: false }],
  autoChoice: 'windows',
  autoSeconds: 0,
} as unknown as RadioLaunchInfo

function Overlay() {
  const [open, setOpen] = useState(true)
  useEffect(() => {
    if (overlay === 'confirm') void confirmDialog({ title: 'Remove radio?', body: 'The probe asks.', confirmLabel: 'Remove radio' })
  }, [])
  const noop = () => {}
  switch (overlay) {
    case 'dialog':
      return (
        <Dialog open={open} onOpenChange={setOpen} title="Getting started">
          <input className="probe-field" aria-label="A field in the dialog" autoFocus />
        </Dialog>
      )
    case 'confirm':
      return <ConfirmHost />
    case 'logconfirm':
      return <LogConfirm record={QSO} onConfirm={noop} onDiscard={noop} />
    case 'radiopicker':
      return <RadioPicker info={PICKER} onChoose={noop} onSingleRadio={noop} onTimeout={noop} error={null} />
    case 'scrim':
      return (
        <div className="logconfirm-backdrop" role="dialog" aria-modal="true" aria-label="Erase the log?" onClick={() => setOpen(false)}>
          {open && <div className="logconfirm">Erase the log?</div>}
        </div>
      )
    default:
      return null
  }
}

function Probe() {
  useViewport(1)
  const noop = () => {}
  return (
    <>
      <div className="app">
        <TopBar
          mycall="KD9TAW" mygrid="EN52xa" radio={RADIO} link={LINK} bandPlan={[]}
          onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={() => (halts += 1)}
          onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} onStopRecording={noop}
          tier="FT8" onTierChange={noop} onOpenGuide={noop} field={false} onFieldChange={noop}
        />
        <main className="probe-below" style={{ flex: 1 }} />
      </div>
      <Overlay />
      <StopLayer />
    </>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
