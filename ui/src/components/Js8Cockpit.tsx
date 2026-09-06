// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), for ONE reason:
// the TX-on-air pill's tooltip, which states what Stop TX does to a frame in flight. That is
// transmit-path wording and moves with the JS8 TX batch (B7), with the stop-line sweeps re-run.
// Everything else operator-visible is in the catalog under `js8.*`. What is NOT prose and stays
// in the code is the mode's own vocabulary (js8Vocab.ts): JS8, HB, CQ, @ALLCALL, the speed
// names and their ALL.TXT letters, the 32 directed-command texts, callsigns, grids, offsets in
// Hz, SNR in dB, UTC stamps and the s/m/h age units.
import { useEffect, useRef, useState } from 'react'
import type { AppSnapshot, BandChannel, Js8InboxState, Js8State } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PanelsMenu } from './PanelsMenu'
import { panelHost } from '../features/panelHost'
import { JS8_PANEL_IDS, type Js8PanelId, type PanelLayoutApi } from '../features/panelState'
import { FrequencyControl } from './FrequencyControl'
import { LogEntry } from './LogEntry'
import { Waterfall } from './Waterfall'
import { useRegionCols, type RegionCols } from '../useRegionCols'
import {
  atuTune,
  getJs8State,
  getLicensedBandPlan,
  haltTx,
  js8Arm,
  js8CallCq,
  js8Enter,
  js8InboxDelete,
  js8InboxMark,
  js8Send,
  js8SendCommand,
  js8SetSpeed,
  setRxOffset,
  setTune,
  setTxLevel,
  setTxOffset,
} from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast, withErrorToast } from '../toast'
import { usePinnedScroll } from '../usePinnedScroll'
import { t } from '../i18n'
import {
  ALLCALL,
  CQ,
  HB,
  HZ,
  JS8,
  JS8_CQS,
  JS8_QUICK_QUERIES,
  JS8_SPEEDS,
  JS8_SPEED_LIST,
  RX_PLATE,
  TX_PLATE,
  ageLabel,
  countBits,
  fmtSnr,
  utcClock,
} from '../js8Vocab'

interface Props {
  /** Open the Logbook filtered to a callsign — handed to the log strip's recall card. */
  onOpenLogbook?: (call: string) => void
  /** Live snapshot — may be absent while the app is still connecting. */
  snap?: AppSnapshot | null
  /** Apply a snapshot returned by a command without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** True when JS8 is the visible view. The cockpit stays MOUNTED in its keep-alive host;
   * this pauses the display poll while hidden and drives the view-entry `js8_enter`
   * (rising edge). */
  active?: boolean
  /** QSY to a band-plan channel / a typed dial — a QSY, never TX. */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
  /** Arm/disarm TX (WSJT-X "Enable Tx") — the header pill becomes the arm control, since the
   * TopBar's cluster is hidden with the digital chrome in this view. NOT a stop control in a
   * slotted mode (the Operate ruling): a frame in flight completes. */
  onSetTxEnabled?: (on: boolean) => void
  theme?: string
  wheelSensitivity?: number
  /** Panel visibility record — host-owned (App) so it survives remounts. */
  panels?: PanelLayoutApi<Js8PanelId>
}

/** Display labels for the JS8 removable panels — resolved when the menu is BUILT. */
const js8PanelLabels = (): Record<Js8PanelId, string> => ({
  scope: t('js8.panel.scope'),
  activity: t('js8.panel.activity'),
  stations: t('js8.panel.stations'),
  inbox: t('js8.panel.inbox'),
  log: t('js8.panel.log'),
})

/** Literal keys per state, so the orphan guard sees each of them referenced. */
function inboxStateLabel(s: Js8InboxState): string {
  switch (s) {
    case 'unread':
      return t('js8.inbox.state.unread')
    case 'read':
      return t('js8.inbox.state.read')
    case 'store':
      return t('js8.inbox.state.store')
    case 'delivered':
      return t('js8.inbox.state.delivered')
  }
}

/**
 * JS8 operating cockpit (Digital rail: FT · Tempo · RTTY · PSK · SSTV · APRS · JS8) — the
 * JS8Call-compatible keyboard mode on FT8's physical layer. RX is engine-owned and starts on
 * view entry (`js8_enter` = set_tier(JS8) + the JS8 watering hole for the band, decoding every
 * speed the operator has enabled); nothing here keys. Every send asks the engine, which
 * re-checks the session TX latch, privileges, identity and — in B7 — the two-act arm for
 * automatic origins, and answers with a reason when it refuses.
 *
 * THE STOP LINE census here (outside every ⊞-removable pane; mirrored in stop-line.test.tsx's
 * JS8 case): Stop TX (header → halt_tx, never disabled), Tune (header; the carrier it starts),
 * and Esc (keyboard-only, census-only — bound while this is the visible view). The TX-enable
 * latch is NOT a stop in a slotted mode; "Drop queue" is a SENDER-class control.
 *
 * Mounted in a keep-alive host (like RTTY/PSK/SSTV/APRS) so the activity stream keeps its
 * scroll position and selection while the operator is on another section.
 */
export function Js8Cockpit({
  snap,
  onSnap,
  active = true,
  onSetFrequency,
  onSetTxEnabled,
  theme = 'dark',
  wheelSensitivity,
  onOpenLogbook,
  panels,
}: Props) {
  const host = panels
    ? panelHost(panels, {
        menu: JS8_PANEL_IDS,
        side: ['stations', 'inbox'],
        main: 'activity',
        labels: js8PanelLabels(),
      })
    : null
  const shown = (id: Js8PanelId) => (host ? host.shown(id) : true)

  // Live state — polled at 2 Hz while this is the visible view (the PSK pattern; no Tauri
  // events). The backend keeps decoding while we're hidden; the first tick on re-activation
  // catches the display up.
  const [js8, setJs8] = useState<Js8State | null>(null)
  useEffect(() => {
    if (!active) return
    let alive = true
    const tick = () => {
      getJs8State()
        .then((s) => {
          if (alive) setJs8(s)
        })
        .catch(() => {})
    }
    tick()
    const id = window.setInterval(tick, 500)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [active])

  // ENTER the mode on the rising edge of `active` (works unconfigured, spec §Works unconfigured):
  // `js8_enter` sets the tier and the dial. ⚠️ RX ONLY, and the ENGINE guarantees it — the call
  // confers neither TX-enable nor any automatic-origin arm.
  const entered = useRef(false)
  useEffect(() => {
    if (!active) {
      entered.current = false
      return
    }
    if (entered.current) return
    entered.current = true
    void js8Enter()
      .then((s) => setJs8(s))
      .catch(() => {})
  }, [active])

  // JS8 watering holes (JS8Call's FrequencyList), license-filtered.
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    void getLicensedBandPlan('js8').then(setPlan).catch(() => {})
  }, [])

  const commitDial = (mhz: number) => {
    onSetFrequency?.(mhz, bandLabelForMhz(mhz), snap?.radio.sideband || 'USB')
  }

  // STOP TX → halt_tx: the universal stop (unkeys, arms slot_tx_abort, and — from B7 — empties
  // the JS8 queue, the HB schedule and the pending auto-reply).
  const stop = () => {
    void haltTx()
      .then((s) => onSnap?.(s))
      .catch(() => {})
  }
  // Esc stops from anywhere in the cockpit — bound only while this is the VISIBLE view (the
  // cockpit stays mounted in the keep-alive host, so an unconditional listener would fire
  // Stop TX from inside another section).
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        stop()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active])

  // --- The dock's addressee + composer. The ENGINE is the authority on every send. ---
  const [toCall, setToCall] = useState('')
  const [text, setText] = useState('')
  const [cqIdx, setCqIdx] = useState(0)
  const snapRef = useRef(snap)
  snapRef.current = snap
  const selectStation = (call: string) => setToCall(call.toUpperCase())

  /** The two refusals worth a toast BEFORE the round trip; the engine re-checks both. */
  const refuseIfUnready = (): boolean => {
    const mycall = snapRef.current?.mycall?.trim() ?? ''
    if (!mycall) {
      pushToast(t('js8.toast.noCallsign'), 'info', 3500)
      return true
    }
    if (snapRef.current && !snapRef.current.radio.txAllowed) {
      pushToast(t('js8.toast.txLocked'), 'info', 3500)
      return true
    }
    return false
  }
  const send = () => {
    const body = text.trim()
    if (!body) return
    if (refuseIfUnready()) return
    const to = toCall.trim().toUpperCase()
    void withErrorToast(() => js8Send(to || null, body), t('js8.toast.send.failed')).then((s) => {
      if (s) {
        setJs8(s)
        setText('')
      }
    })
  }
  const callCq = () => {
    if (refuseIfUnready()) return
    void withErrorToast(() => js8CallCq(cqIdx), t('js8.toast.cq.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  // HB is the SESSION-ONLY schedule (never persisted): the second act for heartbeat frames.
  // Turning it on never keys by itself — the session TX latch is the first act (B7).
  const toggleHb = () => {
    void withErrorToast(() => js8Arm('hb', js8?.hbOn !== true), t('js8.toast.arm.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const quickQuery = (call: string, cmd: number) => {
    if (refuseIfUnready()) return
    void withErrorToast(() => js8SendCommand(call, cmd, ''), t('js8.toast.command.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const setSpeed = (idx: number) => {
    void withErrorToast(() => js8SetSpeed(idx), t('js8.header.speed.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const markInbox = (id: number, state: Js8InboxState) => {
    void withErrorToast(() => js8InboxMark(id, state), t('js8.inbox.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }
  const deleteInbox = (id: number) => {
    void withErrorToast(() => js8InboxDelete(id), t('js8.inbox.failed')).then((s) => {
      if (s) setJs8(s)
    })
  }

  // REGION TIERS (CW's rule): three tracks only when the activity column, the aux column
  // (stations / inbox) AND the log column all have something to hold; an empty track is the
  // "band of empty black" rebuilt. useRegionCols owns data-cols/data-flow.
  const activityPresent = shown('activity')
  const auxPresent = shown('stations') || shown('inbox')
  const logPresent = shown('log')
  const populated = [activityPresent, auxPresent, logPresent].filter(Boolean).length
  const { ref: panesRef, cols } = useRegionCols<HTMLDivElement>(Math.max(1, populated) as RegionCols)

  const activityPin = usePinnedScroll<HTMLDivElement>()
  const sending = js8?.sending === true
  const rxCount = countBits(js8?.rxSpeeds ?? 0)
  const selectedCall = toCall.trim().toUpperCase()
  const selected = js8?.stations.find((h) => h.call === selectedCall) ?? null
  const now = Date.now()

  const hbTitle =
    js8?.hbOn && js8.armed.hb
      ? t('js8.dock.hb.title.armed')
      : js8?.hbOn
        ? t('js8.dock.hb.title.on')
        : t('js8.dock.hb.title.off')

  // ---- panes ----
  const activityPane = shown('activity') && (
    <CockpitPaneFrame
      title={t('js8.panel.activity')}
      paneId="activity"
      weight={2}
      onRemove={panels ? () => panels.setPanelState('activity', 'removed') : undefined}
    >
      <div
        className="js8-activity"
        ref={activityPin.ref}
        onScroll={activityPin.onScroll}
        title={t('js8.panel.activity.title')}
      >
        {!js8 || js8.activity.length === 0 ? (
          <div className="cw-decode-idle">{t('js8.panel.activity.empty')}</div>
        ) : (
          js8.activity.map((r, i) => (
            <div
              key={`${r.atMs}-${Math.round(r.freqHz)}-${i}`}
              className={`js8-row${r.mine ? ' mine' : ''}${r.directedToMe ? ' directed' : ''}${
                r.lowConf ? ' low' : ''
              }${r.complete ? '' : ' partial'}`}
              onDoubleClick={() => selectStation(r.from)}
              title={t('js8.panel.activity.row.title')}
            >
              <span className="js8-cell js8-time">{utcClock(r.atMs)}</span>
              <span className="js8-cell js8-speed">{JS8_SPEEDS[r.speed].letter}</span>
              <span className="js8-cell js8-freq">{Math.round(r.freqHz)}</span>
              <span className="js8-cell js8-snr">{fmtSnr(r.snrDb)}</span>
              <span className="js8-cell js8-text">{r.text}</span>
            </div>
          ))
        )}
      </div>
    </CockpitPaneFrame>
  )

  const stationsPane = shown('stations') && (
    <CockpitPaneFrame
      title={t('js8.panel.stations')}
      paneId="stations"
      onRemove={panels ? () => panels.setPanelState('stations', 'removed') : undefined}
    >
      <div className="js8-stations">
        {!js8 || js8.stations.length === 0 ? (
          <div className="cw-decode-idle">{t('js8.station.empty')}</div>
        ) : (
          js8.stations.map((h) => (
            <div key={h.call} className={`js8-station${h.call === selectedCall ? ' selected' : ''}`}>
              <button
                type="button"
                className="js8-station-call"
                onClick={() => selectStation(h.call)}
                title={t('js8.station.select.title', { call: h.call })}
              >
                {h.call}
              </button>
              <span className="js8-cell">{h.grid ?? ''}</span>
              <span className="js8-cell js8-snr">{fmtSnr(h.snrDb)}</span>
              <span className="js8-cell">
                {Math.round(h.freqHz)} {HZ}
              </span>
              <span className="js8-cell js8-speed">{JS8_SPEEDS[h.speed].letter}</span>
              <span className="js8-cell js8-age">{ageLabel(now - h.lastMs)}</span>
              {h.lastHb && <span className="js8-chip">{HB}</span>}
              {h.lastCq && <span className="js8-chip">{CQ}</span>}
              {h.storedMsgs > 0 && (
                <span className="js8-chip" title={t('js8.station.stored', { count: h.storedMsgs })}>
                  ✉ {h.storedMsgs}
                </span>
              )}
              <span className="js8-station-acts">
                {JS8_QUICK_QUERIES.map((q) => (
                  <button
                    key={q.id}
                    type="button"
                    className="cw-macro js8-query"
                    onClick={() => quickQuery(h.call, q.id)}
                    title={t('js8.station.query.title', { cmd: q.label, call: h.call })}
                  >
                    {q.label}
                  </button>
                ))}
              </span>
            </div>
          ))
        )}
      </div>
    </CockpitPaneFrame>
  )

  const inboxPane = shown('inbox') && (
    <CockpitPaneFrame
      title={t('js8.panel.inbox')}
      paneId="inbox"
      onRemove={panels ? () => panels.setPanelState('inbox', 'removed') : undefined}
    >
      <div className="js8-inbox">
        {!js8 || js8.inbox.length === 0 ? (
          <div className="cw-decode-idle">{t('js8.inbox.empty')}</div>
        ) : (
          js8.inbox.map((m) => (
            <div key={m.id} className={`js8-inbox-row state-${m.state}`}>
              <span className="js8-chip">{inboxStateLabel(m.state)}</span>
              <span className="js8-cell js8-time">{utcClock(m.atMs)}</span>
              <span className="js8-cell js8-call">
                {m.from} → {m.to}
              </span>
              <span className="js8-cell js8-text">{m.text}</span>
              {m.path.length > 1 && <span className="js8-cell js8-path">{m.path.join('>')}</span>}
              {m.state === 'unread' && (
                <button
                  type="button"
                  className="cw-macro js8-inbox-act"
                  onClick={() => markInbox(m.id, 'read')}
                  title={t('js8.inbox.read.title')}
                >
                  {t('js8.inbox.read.label')}
                </button>
              )}
              <button
                type="button"
                className="cw-macro js8-inbox-act"
                onClick={() => deleteInbox(m.id)}
                title={t('js8.inbox.delete.title')}
              >
                {t('js8.inbox.delete.label')}
              </button>
            </div>
          ))
        )}
      </div>
    </CockpitPaneFrame>
  )

  // THE LOG STRIP — CW's shape (a LogEntry in the log column), prefilled from the selected
  // station through the live machine-fill channel (no focus steal — see PskCockpit for why).
  const logPane = snap && shown('log') && (
    <CockpitPaneFrame
      title={t('js8.panel.log')}
      paneId="log"
      weight={1.5}
      onRemove={panels ? () => panels.setPanelState('log', 'removed') : undefined}
    >
      <LogEntry
        onOpenLogbook={onOpenLogbook}
        snap={snap}
        // The ADIF token: written as MODE=MFSK SUBMODE=JS8 by the logbook (B5).
        mode={JS8}
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        cwLive={
          selected
            ? { call: selected.call, rst: fmtSnr(selected.snrDb), name: null, confirmed: true }
            : null
        }
        fieldDay={snap.fieldDay ?? null}
        fdMode="DIG"
        fdSubmode={JS8}
      />
    </CockpitPaneFrame>
  )

  return (
    <main className="layout single js8-cockpit">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          txActiveLabel="▲ JS8"
          onStopTx={stop}
          onSetTxEnabled={onSetTxEnabled}
          // TX DRIVE, the FT8 header's control: a configuration control on the transmit
          // path, not a transmit control.
          power={{
            value: snap.radio.txLevel,
            unit: 'drive',
            onChange: (v: number) => {
              void setTxLevel(v)
                .then((s) => onSnap?.(s))
                .catch(() => {})
            },
            label: t('js8.header.power.label'),
            title: t('js8.header.power.title'),
          }}
          // TUNE — a steady carrier; also a stop control (it stops the carrier it started),
          // so it is on this cockpit's stop-line census and its sweep.
          onTune={(on) => void setTune(on).then((s) => onSnap?.(s))}
          onAtuTune={() =>
            void atuTune()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
          modeIndicator={
            <>
              <span className="cw-mode-badge" title={t('js8.header.speed.title')}>
                {JS8}
              </span>
              {/* The TRANSMIT speed — the slot clock follows it. Names are the mode's own. */}
              <span className="js8-speeds" role="group" aria-label={t('js8.header.speed.aria')}>
                {JS8_SPEED_LIST.map((s) => (
                  <button
                    key={s.key}
                    type="button"
                    className={`rtty-arm js8-speed-chip${js8?.speed === s.key ? ' on' : ''}`}
                    aria-pressed={js8?.speed === s.key}
                    onClick={() => setSpeed(s.idx)}
                    title={t('js8.header.speed.chip.title', { speed: s.label, period: s.periodS })}
                  >
                    {s.label}
                  </button>
                ))}
              </span>
              {/* MULTI-DECODE: how many of the four speeds the receiver is decoding right now.
                  Which ones is a Settings choice (Settings ▸ Digital ▸ JS8). */}
              <span className="rtty-afc-pill js8-multi" title={t('js8.header.rx.title', { n: rxCount })}>
                {RX_PLATE} {rxCount}/4
              </span>
              {sending && (
                // ⚠️ NOT MIGRATED — the transmit-path deferral: this tooltip states what Stop TX
                // does to a frame in flight. It moves in the JS8 TX batch with the sweeps re-run.
                <span className="rtty-tx-pill" title="JS8 frame on the air (Stop TX aborts it)">
                  {TX_PLATE}
                </span>
              )}
            </>
          }
          bandControl={
            onSetFrequency ? (
              <FrequencyControl
                channels={plan}
                dialMhz={snap.radio.dialMhz}
                band={snap.radio.band}
                mode={snap.radio.sideband}
                variant="compact"
                showReadout={false}
                showModeToggle={false}
                onSet={onSetFrequency}
              />
            ) : (
              <span className="cockpit-ph-pill" title={t('js8.header.band.title')}>
                {bandLabelForMhz(snap.radio.dialMhz) || '— band —'}
              </span>
            )
          }
          onCommitDial={onSetFrequency ? commitDial : undefined}
          digitTune={onSetFrequency != null}
          wheelSensitivity={wheelSensitivity}
          actions={
            host && panels ? (
              <PanelsMenu
                items={host.menuItems}
                onToggle={(id, show) => panels.setPanelState(id as Js8PanelId, show ? 'docked' : 'removed')}
                onUndo={panels.undo}
                canUndo={panels.canUndo}
                onReset={panels.reset}
              />
            ) : undefined
          }
        />
      )}

      {/* THE BAND WATERFALL — ⊞-hideable (SCOPE_PANEL_ID). The RX/TX cursors are the engine's
          audio offsets: a click sets RX, right-click TX, Shift both (the Operate convention).
          It hosts no stop control and no sender. */}
      {shown('scope') && (
        <Waterfall
          theme={theme}
          active={active}
          transmitting={snap?.radio.transmitting ?? false}
          rxOffsetHz={snap?.radio.rxOffsetHz ?? 1500}
          txOffsetHz={snap?.radio.txOffsetHz ?? 1500}
          onTune={(hz, target) => {
            if (target !== 'tx')
              void setRxOffset(hz)
                .then((s) => onSnap?.(s))
                .catch(() => {})
            if (target !== 'rx')
              void setTxOffset(hz)
                .then((s) => onSnap?.(s))
                .catch(() => {})
          }}
        />
      )}

      {js8?.lastError && (
        <div className="cw-keyer-warn" role="alert">
          ⚠ {js8.lastError}
        </div>
      )}

      {/* THE PANE REGION — CW's keyed columns: the log column keeps its key across a 2↔3 flip so
          the LogEntry never remounts mid-entry (the fix-round D1 rule). */}
      <div className="cockpit-panes" ref={panesRef}>
        {cols === 3 ? (
          <>
            <div className="cockpit-col" key="main">
              {activityPane}
            </div>
            <div className="cockpit-col" key="aux">
              {stationsPane}
              {inboxPane}
            </div>
            <div className="cockpit-col" key="log">
              {logPane}
            </div>
          </>
        ) : cols === 2 && !logPresent ? (
          <>
            <div className="cockpit-col" key="main">
              {activityPane}
            </div>
            <div className="cockpit-col" key="aux">
              {stationsPane}
              {inboxPane}
            </div>
          </>
        ) : (
          <>
            {(activityPresent || auxPresent) && (
              <div className="cockpit-col" key="main">
                {activityPane}
                {stationsPane}
                {inboxPane}
              </div>
            )}
            {logPresent && (
              <div className="cockpit-col" key="log">
                {logPane}
              </div>
            )}
          </>
        )}
      </div>

      {/* TX DOCK — every transmit control, pinned OUTSIDE the pane region. None has a ⊞ id.
          Stop TX and Tune are up in the header: THE STOP LINE. Everything down here is a
          SENDER (Send, CQ) or a second-act arm (HB) — never a stop. */}
      <div className="cockpit-txdock">
        <div className="js8-dock-row js8-compose-row" role="group" aria-label={t('js8.dock.aria')}>
          <input
            className="settings-input rtty-hiscall js8-to"
            list="js8-to-list"
            value={toCall}
            onChange={(e) => setToCall(e.target.value.toUpperCase())}
            placeholder={t('js8.dock.to.placeholder')}
            aria-label={t('js8.dock.to.aria')}
            autoComplete="off"
            spellCheck={false}
          />
          <datalist id="js8-to-list">
            {[ALLCALL, ...(js8?.stations.map((h) => h.call) ?? [])].map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
          <input
            className="settings-input cw-type js8-compose"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== 'Enter') return
              e.preventDefault()
              send()
            }}
            placeholder={t('js8.dock.compose.placeholder')}
            aria-label={t('js8.dock.compose.aria')}
            autoComplete="off"
            spellCheck={false}
          />
          <button type="button" className="cw-send-btn js8-send" onClick={send} disabled={!text.trim()}>
            {t('js8.dock.send.label')}
          </button>
        </div>
        <div className="js8-dock-row js8-beacon-row">
          <select
            className="settings-input js8-cq-select"
            value={cqIdx}
            onChange={(e) => setCqIdx(Number(e.target.value))}
            aria-label={t('js8.dock.cq.aria')}
          >
            {JS8_CQS.map((c, i) => (
              <option key={c} value={i}>
                {c}
              </option>
            ))}
          </select>
          <button type="button" className="cw-macro js8-cq" onClick={callCq} title={t('js8.dock.cq.title')}>
            <span className="cw-macro-label">{CQ}</span>
          </button>
          <button
            type="button"
            className={`cw-macro rtty-arm js8-arm js8-hb${js8?.hbOn ? ' on' : ''}${js8?.armed.hb ? ' armed' : ''}`}
            aria-pressed={js8?.hbOn === true}
            onClick={toggleHb}
            title={hbTitle}
          >
            <span className="cw-macro-label">{HB}</span>
          </button>
        </div>
      </div>
    </main>
  )
}
