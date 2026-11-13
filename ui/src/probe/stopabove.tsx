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
//
// THE FILLED IOTA BOX (`&iota=1`): the station names EU-901 for any call (`get_iota_offer`, from a spot) and the island
// list holds it under a very long group name, so the prompt to log (`overlay=logconfirm`) opens with its IOTA box
// filled, and `overlay=strip` (no overlay: the real log strip, LogEntry, below the top bar) fills its box once a call
// is typed. `&zoom=<n>` sets the UI scale (`--ui-zoom`); `&iotacontrol=1` keeps the group name on one line, the
// control that must come back NOT drawn whole.
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
import { LogEntry } from '../components/LogEntry'
import type { AppSnapshot } from '../types'
import { useViewport } from '../useViewport'
import type { LinkState, LoggedQso, RadioStatus } from '../types'
import type { RadioLaunchInfo } from '../api'

const params = new URLSearchParams(location.search)
const overlay = params.get('overlay') ?? 'dialog'

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()
if (params.get('zoom')) document.documentElement.style.setProperty('--ui-zoom', params.get('zoom')!)
if (params.get('iotacontrol') === '1') {
  const s = document.createElement('style')
  s.textContent = '.iota-name, .iota-name-text { white-space: nowrap !important; overflow-wrap: normal !important; }'
  document.head.appendChild(s)
}
// The island picker beside the filled box: the group's islands are answered too (`get_iota_islands`), one
// with a name at the list's 200-character limit, unless `&islands=0` (the layout as it is with no picker:
// the baseline the probe holds the strip's controls to). `&islandcontrol=1` is the picker's CONTROL: its
// containment taken away, so a select as wide as its longest option sizes its host again.
if (params.get('islandcontrol') === '1') {
  const s = document.createElement('style')
  s.textContent = '.iota-island { contain: none !important; width: auto !important; } .iota-island-pick { width: auto !important; flex: none !important; }'
  document.head.appendChild(s)
}
/** An island name at the list's 200-character limit (`propagation::iota::NAME_MAX_CHARS`). */
const LONG_ISLAND = `Isola grande di prova con un nome lunghissimo ${'e i suoi scogli '.repeat(10)}`.slice(0, 200)
/** A group name as long as IOTA's longest run: a list of islands with their other names. */
const LONG_GROUP =
  'Eolie Islands (Lipari, Salina, Vulcano, Stromboli, Panarea, Filicudi, Alicudi and the rocks of Basiluzzo, Dattilo and Strombolicchio) - aka Aeolian Islands'
const IOTA_ANSWERS: Record<string, unknown> =
  params.get('iota') === '1'
    ? {
        get_iota_offer: { reference: 'EU-901', source: 'spot' },
        get_iota_list_status: { enabled: true, count: 1, fetchedAt: 1_790_000_000, bundled: true, lastError: null },
        get_iota_groups: [{ reference: 'EU-901', name: LONG_GROUP }],
        ...(params.get('islands') === '0'
          ? {}
          : { get_iota_islands: [{ id: 9001, name: LONG_ISLAND, comment: 'in Example Land' }, { id: 9002, name: 'Short Isle' }] }),
      }
    : {}
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> =>
    cmd in IOTA_ANSWERS
      ? Promise.resolve(IOTA_ANSWERS[cmd] as T)
      : Promise.reject(new Error(`${cmd} is not answered in the stop-above probe`)),
}

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

const SNAP = { mycall: 'KD9TAW', mygrid: 'EN52', radio: { band: '20m', dialMhz: 14.025 }, hunt: null } as unknown as AppSnapshot

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
        <main className="probe-below" style={{ flex: 1 }}>
          {overlay === 'strip' && <LogEntry snap={SNAP} mode="CW" defaultRst="599" exchange="terrestrial" titled={false} />}
        </main>
      </div>
      <Overlay />
      <StopLayer />
    </>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
