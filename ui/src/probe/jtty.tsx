// THE JTTY SCREEN IN A REAL BROWSER, for `scripts/browser-probe --desktop jtty` (ui/probe/jtty-probe.mjs
// drives it). jsdom never lays out, so nothing else can see whether the dock's Messages box and Edit keys
// button (and, while editing, the eight key boxes and Defaults) are on screen beside the transmit
// controls, or whether the Log QSO prompt above the log strip is reachable. This is the real JttyCockpit,
// header, waterfall and log strip included, in the keep-alive host App gives it, on the app's
// stylesheets, with the engine's answers given at the IPC bridge (a command not answered here is
// refused). The engine's state carries a Log QSO prompt for W1AW.
//
// `?edit=1` presses Edit keys once the dock is drawn. `&control=1` is the CONTROL: a box laid over the
// Messages box and the prompt's Log QSO, which the probe must see as covered.
import '../styles.css'
import '../cockpit-panes.css'
import { useEffect } from 'react'
import { createRoot } from 'react-dom/client'
import { useViewport } from '../useViewport'
import { JttyCockpit } from '../components/JttyCockpit'
import { t } from '../i18n'
import settings from '../components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, JttyState } from '../types'

const params = new URLSearchParams(location.search)
const control = params.get('control') === '1'
const edit = params.get('edit') === '1'

document.documentElement.setAttribute('data-theme', 'dark')
localStorage.clear()

const NOW = Date.now()
const STATE: JttyState = {
  armed: true,
  rxHz: 1500,
  ftolHz: 20,
  lowHz: 200,
  highHz: 3000,
  heard: [],
  notice: null,
  lines: Array.from({ length: 30 }, (_, i) => ({
    id: i + 1,
    startUtcMs: NOW - (30 - i) * 4_000,
    freqHz: 1500,
    dialHz: 14_090_000,
    snr: -12,
    text: i % 3 ? `CQ W${i}AW FN31` : 'KD9TAW W1AW 599 007',
    line: i % 3 ? `1500 -12  CQ W${i}AW FN31` : '1500 -12  KD9TAW W1AW 599 007',
    state: 'complete',
    rx: i % 2 === 0,
    confirmedCalls: i % 3 ? [`W${i}AW`] : ['W1AW'],
  })),
  tx: {
    refusal: null,
    txHz: 1500,
    serial: 8,
    sending: false,
    live: false,
    left: 0,
    pending: '',
    notice: null,
    framesSent: 0,
    framesTotal: 0,
    logPrompt: { id: 1, call: 'W1AW', serial: 7, atUtcMs: NOW, replaced: 'K1ABC' },
    style: 'contest',
    fkeys: ['CQ %M CQ', '%H %E', '%H TU CQ %M CQ', '%M', '%H', 'TU NOW %Q %E', '%H AGN?', '%E'],
  },
}

const SNAP = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  hunt: null,
  b4MatchMode: false,
  radio: { dialMhz: 14.09, band: '20m', catOk: true, sideband: 'USB', transmitting: false, tuning: false, txEnabled: true, txAllowed: true },
} as unknown as AppSnapshot

const answers: Record<string, () => unknown> = {
  get_jtty_state: () => STATE,
  jtty_auto_arm: () => STATE,
  jtty_set_band: () => STATE,
  get_licensed_band_plan: () => [],
  get_settings: () => settings,
  jtty_log_prompt_answer: () => 'taken',
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the JTTY probe`))
  },
}

/** Each part, and whether it is on screen: inside the window, held by every ancestor that clips its
 *  overflow, and the topmost thing at its centre. */
function measure() {
  const holds = (o: DOMRect | { left: number; top: number; right: number; bottom: number }, i: DOMRect) =>
    i.left >= o.left - 0.5 && i.right <= o.right + 0.5 && i.top >= o.top - 0.5 && i.bottom <= o.bottom + 0.5
  const win = { left: 0, top: 0, right: innerWidth, bottom: innerHeight }
  const onScreen = (el: Element | null | undefined) => {
    if (!el) return false
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1 || !holds(win, r)) return false
    for (let a = el.parentElement; a; a = a.parentElement) {
      const s = getComputedStyle(a)
      if (/(hidden|auto|scroll|clip)/.test(s.overflowX + s.overflowY) && !holds(a.getBoundingClientRect(), r)) return false
    }
    const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    return top != null && (top === el || el.contains(top))
  }
  const dock = document.querySelector('.cockpit-txdock')
  const prompt = document.querySelector('.jtty-log-prompt')
  const byText = (root: Element | null, text: string) =>
    [...(root?.querySelectorAll('button') ?? [])].find((b) => b.textContent?.trim() === text) ?? null
  const parts: Record<string, Element | Element[] | null> = {
    messages: dock?.querySelector(`select[aria-label="${t('jtty.style.aria')}"]`) ?? null,
    editKeys: dock?.querySelector('.jtty-fkeys-edit') ?? null,
    keys: [...(dock?.querySelectorAll(edit ? '.jtty-fkey-edit input' : '.jtty-macros .cw-macro') ?? [])],
    promptText: prompt?.querySelector('[role="status"]') ?? null,
    promptNote: prompt?.querySelector('.jtty-log-prompt-note') ?? null,
    logQso: byText(prompt, t('jtty.logPrompt.log')),
    notNow: byText(prompt, t('jtty.logPrompt.dismiss')),
    ...(edit ? { defaults: byText(dock, t('jtty.fkeys.reset.label')) } : {}),
  }
  const seen: Record<string, boolean> = {}
  for (const [k, v] of Object.entries(parts)) seen[k] = Array.isArray(v) ? v.length === 8 && v.every(onScreen) : onScreen(v)
  return { viewport: document.documentElement.dataset.viewport, keys: (parts.keys as Element[]).length, seen, refused: [...refused] }
}
;(window as unknown as { __jttyProbe: () => unknown }).__jttyProbe = () => (document.querySelector('.jtty-log-prompt') ? measure() : null)

function Probe() {
  useViewport(1)
  useEffect(() => {
    let pressed = false
    const covers = [document.createElement('div'), document.createElement('div')]
    const id = window.setInterval(() => {
      const button = document.querySelector<HTMLButtonElement>('.jtty-fkeys-edit')
      const prompt = document.querySelector('.jtty-log-prompt')
      if (!button || !prompt) return
      if (edit && !pressed && button.getAttribute('aria-pressed') !== 'true') button.click()
      pressed = true
      if (!control) return
      // A box over the Messages box and one over Log QSO, kept where they stand: both must read as covered.
      const select = document.querySelector(`select[aria-label="${t('jtty.style.aria')}"]`)
      const log = [...prompt.querySelectorAll('button')].find((b) => b.textContent?.trim() === t('jtty.logPrompt.log'))
      ;[select, log].forEach((el, k) => {
        if (!el) return
        const r = el.getBoundingClientRect()
        const cover = covers[k]
        Object.assign(cover.style, { position: 'fixed', left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px`, zIndex: '99999', background: 'red' })
        if (!cover.isConnected) document.body.appendChild(cover)
      })
    }, 100)
    return () => window.clearInterval(id)
  }, [])
  return (
    <div className="app" style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div className="jtty-host">
        <JttyCockpit snap={SNAP} active onSetTxEnabled={() => {}} onSetFrequency={() => {}} theme="dark" wheelSensitivity={1} />
      </div>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Probe />)
