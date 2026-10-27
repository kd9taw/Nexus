// THE VOICE KEYER'S FLEX DAX NOTE IN A REAL BROWSER, for `scripts/browser-probe --desktop voicenote`
// (ui/probe/voicenote-probe.mjs drives it). jsdom never lays out, so nothing else can see whether the note
// fits across the keyer's pane, or whether it pushes the slots or its Got it button out of reach. This is
// the real VoiceKeyer, told the radio is a Flex on SmartSDR CAT, in a CockpitPaneFrame in the leading column
// of a Phone-shaped `.cockpit-panes` region beside a log column, over a TX dock, on the app's stylesheets.
// The station's answers are given at the IPC bridge; a command not answered here is refused.
//
// `?cols=2` (the default) is the region as a 1024–1920 window lays it out: the keyer's leading column beside
// the log column. `?cols=3&narrow=1` is the narrowest the keyer's column gets: three columns with the
// leading one dragged down to its 16em floor.
// `&control=1` is the CONTROL: a 2400 px unbreakable line inside the note, which the probe must see.
import '../styles.css'
import '../cockpit-panes.css'
import { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { CockpitPaneFrame } from '../components/panes/CockpitPaneFrame'
import { VoiceKeyer } from '../components/VoiceKeyer'

const params = new URLSearchParams(location.search)
const control = params.get('control') === '1'
const cols = params.get('cols') === '3' ? 3 : 2
const narrow = params.get('narrow') === '1'

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()

const SLOTS = [
  [1, 'CQ'], [2, 'My Call'], [3, 'Report'], [4, 'QRZ?'], [5, '73'], [6, 'Again'], [8, 'AGN'],
].map(([slot, label]) => ({ slot, label, file: `/voice/slot${slot}.wav` }))
const answers: Record<string, () => unknown> = {
  get_voice_messages: () => SLOTS,
  play_voice_message: () => ({}),
  stop_voice: () => ({}),
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the voice note probe`))
  },
}
;(window as unknown as { __refused: () => string[] }).__refused = () => [...refused]

function Probe() {
  useViewport(1)
  useEffect(() => {
    if (!control) return
    const id = window.setInterval(() => {
      const host = document.querySelector('.vk-flex-dax-note')
      if (!host || host.querySelector('.probe-wide')) return
      const wide = document.createElement('span')
      wide.className = 'probe-wide'
      wide.textContent = 'W'.repeat(240)
      wide.style.whiteSpace = 'nowrap'
      wide.style.overflowWrap = 'normal'
      wide.style.display = 'inline-block'
      wide.style.fontSize = '16px'
      host.appendChild(wide)
    }, 100)
    return () => window.clearInterval(id)
  }, [])
  return (
    <div className="app" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div
        className="cockpit-panes"
        data-cols={cols}
        data-flow="fill"
        style={{ flex: '1 1 auto', minHeight: 0, ...(narrow ? { '--cockpit-col-a': '0.01fr' } : {}) }}
      >
        <div className="cockpit-col">
          <CockpitPaneFrame title="Voice keyer" paneId="voiceKeyer" fit="content">
            <VoiceKeyer txEnabled keyed={false} voicePlaying={false} flexDaxNote />
          </CockpitPaneFrame>
          <CockpitPaneFrame title="Band activity" paneId="activity">
            <div style={{ height: 600 }} />
          </CockpitPaneFrame>
        </div>
        {cols === 3 && (
          <div className="cockpit-col">
            <CockpitPaneFrame title="Rig" paneId="rig">
              <div style={{ height: 200 }} />
            </CockpitPaneFrame>
          </div>
        )}
        <div className="cockpit-col">
          <CockpitPaneFrame title="Log" paneId="log">
            <div style={{ height: 300 }} />
          </CockpitPaneFrame>
        </div>
      </div>
      <div className="cockpit-txdock" data-tx="TX dock" style={{ height: 90 }}>
        <button type="button">PTT</button>
      </div>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
