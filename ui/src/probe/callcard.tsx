// THE STATION CARD BESIDE THE TRANSMIT CONTROLS IN A REAL BROWSER, for `scripts/browser-probe --desktop
// callcard` (ui/probe/callcard-probe.mjs drives it). jsdom never lays out, so nothing else can see where
// the card lands. This is the real CallCardHost and a real trigger on the app's stylesheets, with the
// transmit surfaces the card keeps off (`TX_AVOID`: the TX strip with Stop TX, the TX dock, FT's QSO
// strip with its Stop) laid out where a cockpit puts them, around the call. The station's answers are
// given at the IPC bridge, and a command not answered here is refused.
//
// `?case=strip`: the call just above the TX strip, the dock under that: the room under the call is too
// short, so the card must go elsewhere. `?case=qso`: FT's QSO strip with its Stop directly under a call
// near the top. Each Stop TX stands under the call, where the card would go if nothing kept it off.
// `&avoid=off` is the CONTROL: the same boxes without their transmit classes, so the card
// is free to land on them, and the probe must see it there.
//
// `?case=phone`: Phone's log strip Card button, for a call the strip has ALREADY looked up (the
// callbook answer, with a photo and a long name and address, is asked once through `qrzLookup` before
// the card opens, as the strip's own lookup does). The card must show that answer and its photo
// without a Look up of its own, and its text must use the card's width, not the callsign's.
// `&zoom=<n>` sets the app's UI scale (`--ui-zoom`).
//
// `&card=best`: the same cases with the BEST TIMES card (BestTimesCard), opened as a call menu's Best
// times item opens it, beside the call: it is placed by the same rule and must keep off the same boxes.
import '../styles.css'
import '../cockpit-panes.css'
import type { CSSProperties, MouseEvent as ReactMouseEvent } from 'react'
import { createRoot } from 'react-dom/client'
import { CallCardHost, callCardTrigger } from '../components/CallCard'
import { BestTimesCardHost, openBestTimesCard } from '../components/BestTimesCard'
import { qrzLookup } from '../api'
import type { AppSnapshot, QrzLookup } from '../types'

const params = new URLSearchParams(location.search)
const which = params.get('case') ?? 'strip'
const avoid = params.get('avoid') !== 'off'
const best = params.get('card') === 'best'
/** The class a box has in a cockpit, or none in the control. */
const tx = (cls: string) => (avoid ? cls : 'probe-not-tx')

document.documentElement.setAttribute('data-theme', 'dark')
if (params.get('zoom')) document.documentElement.style.setProperty('--ui-zoom', params.get('zoom')!)
localStorage.clear()

/** A photo the page draws without the network: a 64×64 square. */
const PHOTO = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" fill="#3a7"/></svg>')}`
const BOOK: QrzLookup = {
  call: 'EC1DD', name: 'Pedro Antonio Gomez de la Fuente', nickname: null, qth: 'Santiago de Compostela', grid: 'IN52TK',
  state: 'Galicia', country: 'Spain', dxcc: 281, cqZone: 14, ituZone: 37, image: PHOTO, county: 'A Coruna', iota: 'EU-080',
  email: 'pedro.gomez.delafuente@example.invalid', web: 'https://www.example.invalid/ec1dd/station', qslVia: 'EA1RCI',
  lotw: true, eqsl: false, paperQsl: true, utcOffset: 1,
}

const answers: Record<string, () => unknown> = {
  get_call_stats: () => ({
    call: 'EC1DD', bands: [], modes: [], newOne: false, newBand: true, newMode: true,
    dxcc: null, wasFamily: false, was: null, waz: null, countryQsl: null, callQsl: {},
  }),
  rotator_bearing_to_call: () => ({ pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 }),
  ...(which === 'phone' ? { qrz_lookup: () => BOOK } : {}),
  // A full Best times answer: five windows, the week's lines and every footnote, so the card is as tall
  // as it gets.
  get_best_times: () => {
    const now = Math.floor(Date.now() / 3_600_000) * 3600
    return {
      state: 'answer', call: 'EC1DD', entity: 'Spain', basis: { kind: 'grid', from: 'callbook', grid: 'IN52TK' },
      km: 1530.6, bearing: 227.4, engine: 'p533', power: { watts: 100, source: 'default', amplifier: false },
      answer: {
        engine: 'p533', mode: 'FT8', perMode: true,
        now: { kind: 'try', band: '20m', word: 'Good', untilUnix: now + 4 * 3600, allDay: false, runnerUp: { band: '17m', word: 'Fair' } },
        windows: [5, 8, 11, 15, 19].map((o, i) => ({ band: ['15m', '40m', '20m', '80m', '17m'][i], fromUnix: now + o * 3600, toUnix: now + (o + 2) * 3600, word: 'Fair', greyline: i === 1, longShot: false, route: i === 2 ? 'long' : undefined })),
        week: { forecastUntilUnix: now + 2 * 86_400, flags: [{ dayUnix: now - (now % 86_400) + 86_400, kp: 5.3 }] },
        chart: [], hours: [], nowHour: new Date().getUTCHours(), longUsed: true,
      },
    }
  },
}
const refused = new Set<string>()
window.__TAURI_INTERNALS__ = {
  invoke: <T,>(cmd: string): Promise<T> => {
    if (cmd in answers) return Promise.resolve(answers[cmd]() as T)
    refused.add(cmd)
    return Promise.reject(new Error(`${cmd} is not answered in the station card probe`))
  },
}
;(window as unknown as { __refused: () => string[] }).__refused = () => [...refused]

const SNAP = {
  radio: { band: '20m', operatingMode: 'digital', sideband: 'USB' },
  link: { tier: 'FT8' },
  logTick: 1,
} as unknown as AppSnapshot

const at = (style: CSSProperties): CSSProperties => ({ position: 'fixed', boxSizing: 'border-box', ...style })

/** What opens the card on the call: the station card's trigger, or (`&card=best`) the Best times item's
 *  call, on the same element. */
const trigger = (call: string) =>
  best ? { onClick: (e: ReactMouseEvent<HTMLElement>) => openBestTimesCard(call, e.currentTarget) } : callCardTrigger(call)

function Probe() {
  return (
    <div className="app">
      <CallCardHost snap={SNAP} spots={[]} needAlertsByCall={new Map()} />
      <BestTimesCardHost />
      {which === 'phone' ? (
        <>
          <button type="button" className="probe-call le-qrz le-lookup le-card" style={at({ left: 420, top: 300, height: 24 })} {...trigger('EC1DD')}>
            Card
          </button>
          <div className={tx('cockpit-txdock')} data-tx="TX dock" style={at({ left: 0, right: 0, bottom: 0, height: 90 })}>
            <button type="button">PTT</button>
          </div>
        </>
      ) : which === 'strip' ? (
        <>
          <button type="button" className="probe-call" style={at({ left: 120, top: 560, height: 24 })} {...trigger('EC1DD')}>
            EC1DD
          </button>
          <div className={tx('cockpit-txstrip')} data-tx="TX strip" style={at({ left: 0, right: 0, top: 600, height: 44, display: 'flex', gap: 8, padding: '6px 6px 6px 130px' })}>
            <button type="button" className={tx('op-btn stop')} data-tx="Stop TX">Stop TX</button>
            <button type="button">Tune</button>
          </div>
          <div className={tx('cockpit-txdock')} data-tx="TX dock" style={at({ left: 0, right: 0, bottom: 0, height: 90 })}>
            <button type="button">PTT</button>
          </div>
        </>
      ) : (
        <>
          <button type="button" className="probe-call" style={at({ left: 120, top: 100, height: 24 })} {...trigger('EC1DD')}>
            EC1DD
          </button>
          <div className={tx('cockpit-qso')} data-tx="QSO strip" style={at({ left: 0, width: 700, top: 132, height: 40, display: 'flex', gap: 8, padding: '4px 4px 4px 130px' })}>
            <button type="button" className={tx('op-btn stop')} data-tx="Stop TX">Stop TX</button>
          </div>
        </>
      )}
    </div>
  )
}

// The log strip's lookup, before the card is opened.
if (which === 'phone') void qrzLookup('EC1DD').then(() => document.documentElement.setAttribute('data-looked-up', 'yes'))
createRoot(document.getElementById('root')!).render(<Probe />)
