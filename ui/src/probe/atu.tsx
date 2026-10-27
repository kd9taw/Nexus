// THE TX STRIP'S ATU BUTTON IN A REAL BROWSER, for `scripts/browser-probe --desktop atu` (ui/probe/atu-probe.mjs
// drives it). jsdom never lays out, so nothing else can see whether "ATU ON" moves Stop TX, whether the lit
// state draws, whether the menu that switches the tuner out lands on screen without covering Stop TX, or
// whether a click on its item reaches the button behind it through React's portal bubbling. This is the real
// CockpitTxStrip on the app's stylesheets, in a cockpit-shaped shell (header, scope, the strip, a pane
// region, the TX dock). The handlers record what they were asked (`window.__calls`); nothing is sent.
//
// `?atu=on|off` the radio's tuner in line or out; `&theme=light|dark`; `&control=1` is the CONTROL: a
// 2400 px unbreakable label planted in the strip and a box laid over Stop TX, which the probe must catch.
import '../styles.css'
import '../cockpit-panes.css'
import { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { CockpitTxStrip } from '../components/CockpitTxStrip'
import type { RadioStatus } from '../types'

const params = new URLSearchParams(location.search)
const control = params.get('control') === '1'
const atuOn = params.get('atu') !== 'off'

document.documentElement.setAttribute('data-theme', params.get('theme') === 'light' ? 'light' : 'dark')
localStorage.clear()

const calls: string[] = []
;(window as unknown as { __calls: () => string[] }).__calls = () => [...calls]
const record = (name: string) => () => {
  calls.push(name)
}

const RADIO = {
  dialMhz: 14.29, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: true,
  tuning: false, txAllowed: true, rigKeyed: false, atu: atuOn, atuSwitchOut: true,
} as unknown as RadioStatus

function Probe() {
  useViewport(1)
  // The CONTROL's overflow goes INSIDE the strip's control group, where a too-wide label would.
  useEffect(() => {
    if (!control) return
    const id = window.setInterval(() => {
      const host = document.querySelector('.cockpit-txstrip .cq-txctl')
      if (!host || host.querySelector('.probe-wide')) return
      const wide = document.createElement('span')
      wide.className = 'probe-wide'
      wide.textContent = 'W'.repeat(240)
      wide.style.whiteSpace = 'nowrap'
      wide.style.display = 'inline-block'
      host.appendChild(wide)
    }, 100)
    return () => window.clearInterval(id)
  }, [])
  return (
    <div className="app" style={{ height: '100%' }}>
      <main className="layout single cw-cockpit">
        <div className="probe-header" style={{ height: 56 }} />
        <div className="probe-scope" style={{ height: 220 }} />
        <CockpitTxStrip
          radio={RADIO}
          onSetTxEnabled={record('set_tx_enabled')}
          onTune={record('set_tune')}
          onAtuTune={record('atu_tune')}
          onAtuBypass={record('atu_bypass')}
          onStopTx={record('halt_tx')}
        />
        {control && <div className="probe-cover" style={{ position: 'fixed', inset: 0, zIndex: 9999 }} />}
        <div className="cockpit-panes" data-cols={2} data-flow="fill" style={{ flex: '1 1 auto', minHeight: 0 }}>
          <div className="cockpit-col" />
          <div className="cockpit-col" />
        </div>
        <div className="cockpit-txdock" data-tx="TX dock" style={{ height: 90 }}>
          <button type="button">PTT</button>
        </div>
      </main>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
