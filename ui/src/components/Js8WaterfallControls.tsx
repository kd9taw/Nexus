// THE WATERFALL CONTROLS — JS8Call 3.0.3's View ▸ Show Waterfall Controls (WideGraph.ui), the
// groups of it that act on the station:
//   · the DECODE FILTER: on/off, centre and width, and "centre on my RX offset" (JS8Call's sync
//     button). On, the decoder looks only between centre ± width/2; JS8Call ships it on at
//     1500 Hz, 2000 Hz wide, and so does Nexus;
//   · QSY TO CENTRE: the passband centre, and the button that moves the dial so the TX offset sits
//     on it (narrow filters, rigs with an uneven passband);
//   · the band schedule's notice when a scheduled frequency change is waiting: Cancel or Change now.
// A plate in the waterfall's header row names the filter and opens them in a dialog: JS8Call keeps
// them with its waterfall, and a dialog keeps the cockpit's shell as it is (header, scope, TX strip,
// panes, dock). In the header row the plate costs no height (measured in Chrome at 1024-1920 px:
// the row's height is the same with and without it; in the cockpit header it wrapped that header
// to two rows at 1280 px).
//
// Receive side, except QSY to centre, which MOVES THE RIG: the engine refuses it while anything
// transmits or a message is going out, and the refusal is shown as it says it. Nothing here keys.
import { useEffect, useRef, useState } from 'react'
import type { Js8Filter, Js8State } from '../types'
import type { MenuItem } from './ui/Menu'
import { Dialog } from './ui/Dialog'
import { js8HopAnswer, js8QsyToCentre, js8SetFilter, js8SetQsyCenter } from '../api'
import { pushToast } from '../toast'
import { t } from '../i18n'
import { JS8_FILTER_DEFAULT, JS8_QSY_CENTER_DEFAULT, js8FilterWindow } from '../features/js8Frequencies'

/** Run a JS8 verb; the station's answer replaces the state, and a refusal is shown as it says it
 *  (a refusal is the engine doing its job, so it is a notice, not an error). */
async function act(call: () => Promise<Js8State>, onState: (s: Js8State) => void): Promise<boolean> {
  try {
    onState(await call())
    return true
  } catch (err) {
    const why = err instanceof Error ? err.message : typeof err === 'string' ? err : ''
    pushToast(why || t('js8.wf.failed'), 'info', 5000)
    return false
  }
}

/** The row menus' "Centre filter at …Hz": the decode filter, centred on the row's offset. JS8Call
 *  offers it only while the filter is on (UI_Constructor.cpp:981-988, :1251-1258), and so does
 *  this: null with the filter off. For the band-activity and call-activity row menus. */
export function js8CentreFilterItem(
  filter: Js8Filter | undefined,
  offsetHz: number,
  onState: (s: Js8State) => void,
): MenuItem | null {
  if (!filter?.enabled) return null
  const hz = Math.round(offsetHz)
  return {
    label: t('js8.wf.centreAt', { hz }),
    onSelect: () => {
      void act(() => js8SetFilter({ ...filter, centerHz: hz }), onState)
    },
  }
}

/** What the waterfall darkens for the decode filter (its `shade`): the passband centre ± width/2,
 *  only while the filter is on and has a width, as JS8Call draws it (CPlotter.cpp:470, :497-499).
 *  Null otherwise, and before the station has said. */
export function js8FilterShade(filter: Js8Filter | null | undefined): { loHz: number; hiHz: number } | null {
  if (!filter?.enabled || filter.widthHz <= 0) return null
  return { loHz: filter.centerHz - filter.widthHz / 2, hiHz: filter.centerHz + filter.widthHz / 2 }
}

interface Props {
  js8: Js8State | null
  rxOffsetHz: number
  txOffsetHz: number
  /** The JS8 view is the one on screen: the scheduled-change notice is raised only then. */
  active: boolean
  onState: (s: Js8State) => void
}

/** A number field that commits on blur or Enter, so a half-typed value is never sent. */
function HzInput({
  label,
  value,
  min,
  max,
  disabled,
  onCommit,
}: {
  label: string
  value: number
  min: number
  max: number
  disabled?: boolean
  onCommit: (hz: number) => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    const n = Math.round(Number(draft))
    if (draft.trim() !== '' && Number.isFinite(n) && n !== value) onCommit(Math.min(Math.max(n, min), max))
    setDraft(null)
  }
  return (
    <label className="settings-field">
      <span className="settings-label">{label}</span>
      <input
        disabled={disabled}
        className="settings-input"
        type="number"
        inputMode="numeric"
        min={min}
        max={max}
        step={10}
        value={draft ?? String(value)}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
        }}
        autoComplete="off"
      />
    </label>
  )
}

export function Js8WaterfallControls({ js8, rxOffsetHz, txOffsetHz, active, onState }: Props) {
  const [open, setOpen] = useState(false)
  // The station's own state, here and on the Remote page; absent from an older station, and
  // then there is nothing true to show (no plate rather than a default that may be wrong).
  const controls = js8?.controls ?? null
  const filter = controls?.filter ?? JS8_FILTER_DEFAULT
  const center = controls?.qsyCenterHz || JS8_QSY_CENTER_DEFAULT
  const win = js8FilterWindow(filter)
  const hop = controls?.hop ?? null
  const sending = js8?.sending === true

  // The scheduled change's notice, ONCE per change and only while JS8 is on screen: JS8Call's
  // 30-second box (mainwindow.cpp:280-296). Unanswered, the station makes the change itself.
  const noticed = useRef<string | null>(null)
  const hopKey = hop ? `${hop.mhz}|${hop.description}` : null
  useEffect(() => {
    if (!hopKey) {
      noticed.current = null
      return
    }
    if (!active || noticed.current === hopKey || !hop) return
    noticed.current = hopKey
    const secs = Math.max(0, Math.ceil((hop.firesAtMs - Date.now()) / 1000))
    const mhz = hop.mhz.toFixed(6)
    pushToast(
      hop.description
        ? t('js8.hop.toast.described', { mhz, secs, description: hop.description })
        : t('js8.hop.toast', { mhz, secs }),
      'info',
      Math.max(secs, 1) * 1000,
      {
        prominent: true,
        actionLabel: t('js8.wf.hop.cancel'),
        action: () => {
          void act(() => js8HopAnswer(false), onState)
        },
      },
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hopKey, active])

  // The dialog's countdown ticks while it is open with a change waiting.
  const [, setNow] = useState(0)
  useEffect(() => {
    if (!open || !hop) return
    const id = window.setInterval(() => setNow((n) => n + 1), 1000)
    return () => window.clearInterval(id)
  }, [open, hop])

  const setFilter = (f: Js8Filter) => void act(() => js8SetFilter(f), onState)

  if (!controls) return null
  return (
    <>
      <button
        type="button"
        className={`rtty-afc-pill js8-filter-pill${filter.enabled ? ' on' : ''}`}
        title={t('js8.wf.pill.title')}
        aria-haspopup="dialog"
        onClick={() => setOpen(true)}
      >
        {filter.enabled ? t('js8.wf.pill.on', { lo: win.lo, hi: win.hi }) : t('js8.wf.pill.off')}
      </button>
      <Dialog open={open} onOpenChange={setOpen} title={t('js8.wf.title')} className="js8-wf-dialog">
        <div className="settings-featgroup">
          <span className="settings-featgroup-title">{t('js8.wf.filter.title')}</span>
          <div className="settings-field">
            <label className="settings-toggle">
              <span className="settings-label">{t('js8.wf.filter.label')}</span>
              <button
                type="button"
                role="switch"
                aria-checked={filter.enabled}
                className={`toggle${filter.enabled ? ' on' : ''}`}
                onClick={() => setFilter({ ...filter, enabled: !filter.enabled })}
              >
                <span className="toggle-knob" />
              </button>
            </label>
            <span className="settings-hint">
              {filter.enabled ? t('js8.wf.filter.hint.on', { lo: win.lo, hi: win.hi }) : t('js8.wf.filter.hint.off')}
            </span>
          </div>
          <div className="js8-wf-pair">
            <HzInput
              label={t('js8.wf.center.label')}
              value={filter.centerHz}
              min={120}
              max={4880}
              disabled={!filter.enabled}
              onCommit={(hz) => setFilter({ ...filter, centerHz: hz })}
            />
            <HzInput
              label={t('js8.wf.width.label')}
              value={filter.widthHz}
              min={120}
              max={5000}
              disabled={!filter.enabled}
              onCommit={(hz) => setFilter({ ...filter, widthHz: hz })}
            />
          </div>
          <button
            type="button"
            className="settings-linkbtn"
            disabled={!filter.enabled}
            onClick={() => setFilter({ ...filter, centerHz: Math.round(rxOffsetHz) })}
            title={t('js8.wf.sync.title', { hz: Math.round(rxOffsetHz) })}
          >
            {t('js8.wf.sync.action')}
          </button>
        </div>
        <div className="settings-featgroup">
          <span className="settings-featgroup-title">{t('js8.wf.qsy.title')}</span>
          <div className="js8-wf-pair">
            <HzInput
              label={t('js8.wf.qsyCenter.label')}
              value={center}
              min={500}
              max={4000}
              onCommit={(hz) => void act(() => js8SetQsyCenter(hz), onState)}
            />
            <button
              type="button"
              className="settings-refresh"
              disabled={sending || Math.round(txOffsetHz) === center}
              onClick={() => void act(js8QsyToCentre, onState)}
            >
              {t('js8.wf.qsy.action')}
            </button>
          </div>
          <span className="settings-hint">{t('js8.wf.qsy.hint', { hz: center, tx: Math.round(txOffsetHz) })}</span>
        </div>
        {hop && (
          <div className="settings-featgroup" role="status">
            <span className="settings-featgroup-title">{t('js8.wf.hop.title')}</span>
            <span className="settings-hint">
              {hop.held || hop.firesAtMs === 0
                ? t('js8.wf.hop.held', { mhz: hop.mhz.toFixed(6) })
                : t('js8.wf.hop.body', {
                    mhz: hop.mhz.toFixed(6),
                    secs: Math.max(0, Math.ceil((hop.firesAtMs - Date.now()) / 1000)),
                  })}
            </span>
            {hop.description && <span className="settings-hint">{hop.description}</span>}
            <div className="rig-share-row">
              <button
                type="button"
                className="settings-refresh"
                onClick={() => void act(() => js8HopAnswer(true), onState)}
              >
                {t('js8.wf.hop.now')}
              </button>
              <button
                type="button"
                className="settings-refresh"
                onClick={() => void act(() => js8HopAnswer(false), onState)}
              >
                {t('js8.wf.hop.cancel')}
              </button>
            </div>
          </div>
        )}
      </Dialog>
    </>
  )
}
