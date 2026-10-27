// THE COCKPIT HEADER'S LINES IN A REAL BROWSER, for `scripts/browser-probe --desktop header`
// (ui/probe/header-probe.mjs drives it). jsdom never lays out, so nothing else can see which line the
// header's actions (⊞ Panels, CAT) land on. This is the real CockpitHeader on the app's stylesheets, in a
// CW cockpit shell inside the real `.app`, with the mode controls a CW header carries (speed, keyer,
// pitch, bandwidth, the memory chips, split, REC) stood in by boxes of their measured widths, so the
// header wraps exactly where a CW header of that content wraps. `&zoom=<n>` sets the UI scale.
//
// `&old=on` is the CONTROL: the same boxes in the header's previous shape, the actions a header item of
// their own after the mode controls, which took a line to themselves whenever those controls wrapped.
import '../styles.css'
import '../cockpit-panes.css'
import { createRoot } from 'react-dom/client'
import { CockpitHeader } from '../components/CockpitHeader'
import { useViewport } from '../useViewport'
import type { AppSnapshot } from '../types'

const params = new URLSearchParams(location.search)
const old = params.get('old') === 'on'

document.documentElement.setAttribute('data-theme', 'light')
if (params.get('zoom')) document.documentElement.style.setProperty('--ui-zoom', params.get('zoom')!)
localStorage.clear()
// No station here: every command is refused, and nothing in the header needs one to lay out.
window.__TAURI_INTERNALS__ = { invoke: (cmd: string) => Promise.reject(new Error(`${cmd} is not answered in the header probe`)) }

const SNAP = {
  radio: { dialMhz: 14.03, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, tuning: false, txAllowed: true },
} as unknown as AppSnapshot

// The CW header's mode controls at 100 %, in px (measured in the CW cockpit: Speed with its slider and
// WPM, Keyer, Pitch, BW, the memory chips with two named memories, SPLIT with its offset, REC).
const CONTROLS = [262, 186, 168, 128, 330, 166, 66]
const box = (w: number, i: number) => (
  <span key={i} className="probe-control" style={{ display: 'inline-block', width: w, height: 32, background: 'var(--bg-elev-2)', flex: '0 0 auto' }} />
)

function Probe() {
  useViewport(Number(params.get('zoom') ?? 1))
  const header = (
    <CockpitHeader snap={SNAP} modeIndicator={<span className="cw-mode-badge">CW</span>} bandControl={<span>20m</span>}
      actions={<button type="button">⊞ Panels</button>}>
      {old ? null : CONTROLS.map(box)}
    </CockpitHeader>
  )
  return (
    <div className="app">
      <div className="shell">
        <main className="layout single cw-cockpit">{header}</main>
      </div>
    </div>
  )
}

const root = createRoot(document.getElementById('root')!)
root.render(<Probe />)
// The control's previous shape: the mode controls a header item before the actions, as they were.
if (old) {
  requestAnimationFrame(function move() {
    const h = document.querySelector('.cockpit-header')
    const actions = h?.querySelector('.ch-actions')
    if (!h || !actions) return requestAnimationFrame(move)
    const extras = document.createElement('div')
    extras.className = 'ch-mode-extras'
    for (const w of CONTROLS) {
      const s = document.createElement('span')
      s.className = 'probe-control'
      s.style.cssText = `display:inline-block;width:${w}px;height:32px;flex:0 0 auto`
      extras.appendChild(s)
    }
    h.insertBefore(extras, actions)
  })
}
