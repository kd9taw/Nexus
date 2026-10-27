// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog under `jtty.*`. What does not: the mode's own name, the RX/TX
// plates, `Rx`, `Tx`, `F Tol` and `Hz` (WSJT-X's own control names and a unit), the key names
// (`F1`…`F8`, `Esc`), WSJT-X's own F-key templates and the operator's (message text, sent as
// written), the decoded lines, which are the station's text in WSJT-X's columns, and the engine's
// own words.
import { Fragment, useEffect, useMemo, useRef, useState, type ReactElement } from 'react'
import type { AppSnapshot, BandChannel, JttyLine, JttyLogPromptAnswer, JttyMessageStyle, JttySent, JttyState, Settings } from '../types'
import { CockpitHeader } from './CockpitHeader'
import { CockpitTxStrip } from './CockpitTxStrip'
import { CockpitPaneFrame } from './panes/CockpitPaneFrame'
import { PaneSeam } from './PaneSeam'
import { WATERFALL_SPLIT_MAX, WATERFALL_SPLIT_MIN } from '../features/paneSeam'
import { PanelsMenu } from './PanelsMenu'
import { panelHost } from '../features/panelHost'
import { JTTY_PANEL_IDS, JTTY_PANELS, type JttyPanelId, type PanelLayoutApi } from '../features/panelState'
import { FrequencyControl } from './FrequencyControl'
import { LogEntry } from './LogEntry'
import { ContestScoreChip } from './ContestScoreChip'
import { Waterfall } from './Waterfall'
import { RfScopePane } from './RfScopePane'
import {
  atuBypass,
  atuTune,
  getJttyState,
  getLicensedBandPlan,
  haltTx,
  jttyArm,
  jttyAutoArm,
  jttyClear,
  jttyDecodeAgain,
  jttyFkey,
  jttyLiveCommit,
  jttyLiveStart,
  jttyLogPromptAnswer,
  jttyLogPromptRelease,
  jttySend,
  jttySetBand,
  jttySetRx,
  jttySetSerial,
  jttySetTx,
  jttySignOff,
  jttyStop,
  setTune,
} from '../api'
import { bandLabelForMhz } from '../band'
import { pushToast } from '../toast'
import { pollSingleFlight } from '../singleFlight'
import { usePinnedScroll } from '../usePinnedScroll'
import { t } from '../i18n'
import { surfaceGet, surfaceSet } from '../features/windowScope'
import { jttyAllPane, jttyPaneText, jttyRxPane, jttySegments, type JttyLineOptions } from '../features/jttyLines'
import { useRegionCols, type RegionCols } from '../useRegionCols'
import { upperTyped } from '../typedInput'
import { patchSettings } from '../settings/patch'

interface Props {
  /** Open the Logbook filtered to a callsign — handed to the log strip's recall card. */
  onOpenLogbook?: (call: string) => void
  /** Live snapshot — may be absent while the app is still connecting; the panes render without
   *  it, only the header, the TX strip and the log strip need it. */
  snap?: AppSnapshot | null
  /** Apply a snapshot returned by a command without waiting for the poll. */
  onSnap?: (snap: AppSnapshot) => void
  /** True when JTTY is the visible view. The cockpit stays MOUNTED in its keep-alive host (the
   *  engine keeps decoding either way); this pauses the display poll while hidden and drives the
   *  view-entry auto-arm on its rising edge. */
  active?: boolean
  /** QSY to a JTTY channel / a typed dial (App's setFrequency path) — a QSY, never TX. */
  onSetFrequency?: (dialMhz: number, band: string, mode: string) => void
  /** The TX-enable latch, as App passes it to every keyboard cockpit's TX strip. */
  onSetTxEnabled?: (on: boolean) => void
  theme?: string
  wheelSensitivity?: number
  /** Panel visibility record — host-owned (App) so it survives remounts. Optional: without it
   *  every pane shows at its default and there is no ⊞ menu. */
  panels?: PanelLayoutApi<JttyPanelId>
}

/** This cockpit's INVARIANT vocabulary: the mode's name, WSJT-X's own control names, a unit and
 *  the direction plates — tokens, gathered as constants so the i18n guard reads them as such. */
const MODE_NAME = 'JTTY'
const RX_LABEL = 'Rx'
const TX_LABEL = 'Tx'
const FTOL_LABEL = 'F Tol'
const HZ = 'Hz'
const RX_PLATE = 'RX ▼'
const ESC_KEY = 'Esc'
const LIVE_KEY = 'TX'
const SERIAL_LABEL = '#'
/** WSJT-X's Rx→Tx and Tx→Rx copy buttons (`pbR2T_2`, `pbT2R_2`), named by its own control names. */
const R2T_LABEL = 'Rx→Tx'
const T2R_LABEL = 'Tx→Rx'
/** WSJT-X's chat opener (Ctrl+H, `insertJttyChatOpener`): the DX call and mine, expanded on send. */
const CHAT_OPENER = '%H %M '
/** WSJT-X's Rx and Tx spin box ranges for JTTY (`RxFreqSpinBox_2`, `TxFreqSpinBox_2`). */
const RX_MIN_HZ = 200
const RX_MAX_HZ = 5000
const TX_MIN_HZ = 200
const TX_MAX_HZ = 5000
/** WSJT-X's serial number spin box for JTTY (`sbSerialNumber_2`). */
const SERIAL_MIN = 1
const SERIAL_MAX = 5000
/** WSJT-X's two JTTY F-key sets, F1 to F8 (3.3.0-beta4 `widgets/JttyMessages.hpp` 200-233): its
 *  Contest templates (`Jtty::nativeMacroTemplate`) and its FT8-style ones
 *  (`Jtty::ft8StyleMacroTemplate`, F7 and F8 blank), message text with its macro tokens (`%M` my
 *  call, `%H`/`%Q` the Call box, `%E` the exchange, `%N` the serial, `%G` my grid, `%R` the picked
 *  decode's SNR). The engine compiles each as WSJT-X does; these are the defaults a key goes back
 *  to, and the captions before the engine has answered. */
const FKEY_DEFAULTS: Record<JttyMessageStyle, string[]> = {
  contest: ['CQ %M CQ', '%H %E', '%H TU CQ %M CQ', '%M', '%H', 'TU NOW %Q %E', '%H AGN?', '%E'],
  ft8: ['%H %M %G', '%H %M %R', '%H %M R%R', '%H %M RRR', '%H %M 73', 'CQ %M %G', '', ''],
}
/** Where each set is kept in Settings (WSJT-X's `JTTY_msgN` / `JTTY_FT8_msgN`). */
const FKEY_SETTING: Record<JttyMessageStyle, 'jttyFkeys' | 'jttyFt8Fkeys'> = {
  contest: 'jttyFkeys',
  ft8: 'jttyFt8Fkeys',
}
const FKEY_NAMES = ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8']
/** What the cockpit says when Log QSO's answer took nothing, by the engine's reason. No answer at
 *  all (the command failed) says what an expired prompt says: nothing was logged. */
function whyNot(answer: JttyLogPromptAnswer | null): string {
  switch (answer) {
    case 'answered':
      return t('jtty.logPrompt.taken')
    case 'logged':
      return t('jtty.logPrompt.logged')
    case 'replaced':
      return t('jtty.logPrompt.replaced')
    default:
      return t('jtty.logPrompt.expired')
  }
}
/** WSJT-X's typing pause after which a live entry sends what is typed (`kJttyAutoAdvanceIdleMs`). */
const LIVE_IDLE_MS = 1500
/** WSJT-X's F Tol choices for JTTY (`sbFtol_2->values`, on_actionJTTY_triggered). */
const FTOL_CHOICES = [2, 5, 10, 20, 50, 100, 150, 200, 250, 300, 350, 400, 450, 500]
/** The display options' keys, per surface: WSJT-X keeps both between sessions. */
const LOWER_CASE_KEY = 'nexus.jtty.lowerCase'
const INCLUDE_TIME_KEY = 'nexus.jtty.includeTime'

/** Display labels for the ⊞ Panels menu, resolved when the menu is BUILT (a module constant
 *  would freeze the first locale loaded). */
const jttyPanelLabels = (): Record<JttyPanelId, string> => ({
  scope: t('jtty.panel.waterfall'),
  rfScope: t('rfScope.title'),
  all: t('jtty.pane.all.title'),
  rx: t('jtty.pane.rx.title'),
})

/** One pane's lines, each confirmed call a button that fills the log strip's Call (and so its
 *  callbook lookup), and nothing else clickable. */
function JttyPaneLines({
  lines,
  options,
  onCall,
}: {
  lines: JttyLine[]
  options: JttyLineOptions
  onCall: (call: string, line: JttyLine) => void
}) {
  // A line found by decoding again carries its review's number, and each review's first line
  // follows WSJT-X's "JTTY review" heading (`insertLineSpacer`), in each pane.
  const headed = new Set<number>()
  return (
    <>
      {lines.map((l) => {
        const key = l.review != null ? `r${l.review}:${l.id}` : String(l.id)
        const heading = l.review != null && !headed.has(l.review)
        if (l.review != null) headed.add(l.review)
        return (
          <Fragment key={key}>
            {heading && (
              <div className="jtty-review-heading" role="separator">
                {t('jtty.review.heading')}
              </div>
            )}
            <div className={`jtty-line ${l.state}`} data-pin-key={key}>
              {jttySegments(jttyPaneText(l, options), l.confirmedCalls).map((seg, k) =>
                seg.call ? (
                  <button
                    key={k}
                    type="button"
                    className="jtty-call"
                    onClick={() => onCall(seg.call!, l)}
                    title={t('jtty.call.title', { call: seg.call })}
                  >
                    {seg.text}
                  </button>
                ) : (
                  <span key={k}>{seg.text}</span>
                ),
              )}
            </div>
          </Fragment>
        )
      })}
    </>
  )
}

/**
 * JTTY operating cockpit — WSJT-X's keyboard mode, the Keyboard section's other modem beside PSK
 * (operator ruling 2026-10-08: "Keyboard modem + own cockpit"). The engine decodes (auto-armed on
 * entry), this shows WSJT-X's All Decodes and QSO Frequency panes, sets Rx / Tx / F Tol and hands
 * the waterfall's range to the decoder. The dock is WSJT-X's transmitter — the compose bar and
 * Send, F1–F8, the Call and serial, live entry. The ENGINE is the authority on every send: it checks
 * every TX gate and says why a send was refused (a toast, and the line under the dock).
 *
 * A call is clickable only where the engine lists it in `confirmedCalls` — decoded in two separate
 * messages a short time apart — and a click only fills the log strip's Call, which is what looks it
 * up. A call heard once is plain text: a false decode must never reach a callbook.
 *
 * THE STOP LINE census here (all outside every ⊞-removable pane, mirrored in stop-line.test.tsx's
 * JTTY case): Stop TX (TX strip → halt_tx, never disabled; it stops JTTY too), Tune (TX strip; the
 * carrier it started), the TX-enable latch (TX strip; TX Off stops JTTY and drops PSK's queue and
 * arms its abort, the stop for a PSK over still on the air from the PSK cockpit, the same
 * section), the dock's Esc/Stop (→ jtty_stop; live exactly while JTTY is on the air, queued or in
 * a live session), and Esc (keyboard-only, census-only — App binds it to the same halt while this
 * is the visible view). The dock's Live key is a SENDER, not a stop (RTTY's "TX" ruling): clicking
 * it off signs off and lets what is queued go out.
 */
export function JttyCockpit({ snap, onSnap, active = true, onSetFrequency, onSetTxEnabled, theme = 'dark', wheelSensitivity, onOpenLogbook, panels }: Props) {
  const host = panels
    ? panelHost(panels, { menu: JTTY_PANEL_IDS, side: [], main: 'all', labels: jttyPanelLabels(), shipsHidden: JTTY_PANELS.defaultRemoved })
    : null
  // No panel record: every pane at its vocabulary's default — shown, but the RF scope pane.
  const shown = (id: JttyPanelId) => (host ? host.shown(id) : !JTTY_PANELS.defaultRemoved?.includes(id))
  // The pane's own ✕ — the SAME setPanelState the ⊞ tick makes (panelHost.closeProps).
  const closeProps = (id: JttyPanelId) => (host ? host.closeProps(id) : {})
  const wfRef = useRef<HTMLDivElement>(null)

  // The engine's receive state, polled at 2 Hz while this is the visible view; the engine keeps
  // decoding while it is hidden, and the first tick on return catches the display up.
  const [jtty, setJtty] = useState<JttyState | null>(null)
  useEffect(() => {
    if (!active) return
    return pollSingleFlight('jtty state', 500, (owns) =>
      getJttyState()
        .then((s) => {
          if (owns()) setJtty(s)
        })
        .catch(() => {}),
    )
  }, [active])

  // Arm the decoder on ENTERING the view (the PSK/APRS/SSTV doctrine): the rising edge of
  // `active`, not mount. RX ONLY, and the engine is what guarantees it: `jtty_auto_arm` only
  // upgrades from off, refuses once the operator stopped the decoder this session, and honours
  // the Settings opt-out. Arming a decoder keys nothing.
  const autoArmed = useRef(false)
  useEffect(() => {
    if (!active) {
      autoArmed.current = false
      return
    }
    if (autoArmed.current) return
    autoArmed.current = true
    void jttyAutoArm()
      .then(setJtty)
      .catch(() => {})
  }, [active])

  const armed = jtty?.armed === true
  const toggleArm = () => {
    void jttyArm(!armed)
      .then(setJtty)
      .catch(() => pushToast(t('jtty.arm.failed'), 'error'))
  }

  // Rx and F Tol: WSJT-X's two receive controls. Rx is typed here or set by a waterfall click;
  // F Tol is one of WSJT-X's choices, and the engine keeps it between sessions as WSJT-X does.
  const rxHz = jtty?.rxHz ?? 1500
  const ftolHz = jtty?.ftolHz ?? 20
  const setRx = (rx: number, ftol: number) => {
    void jttySetRx(rx, ftol)
      .then(setJtty)
      .catch((e) => pushToast(String(e), 'error'))
  }
  // The Rx box edits a draft and commits on Enter or leaving it, so the poll cannot fight the
  // operator's typing.
  const [rxDraft, setRxDraft] = useState<string | null>(null)
  const commitRx = () => {
    if (rxDraft == null) return
    const v = Math.round(Number(rxDraft))
    setRxDraft(null)
    if (!Number.isFinite(v) || v === rxHz) return
    setRx(Math.min(RX_MAX_HZ, Math.max(RX_MIN_HZ, v)), ftolHz)
  }
  const ftolChoices = FTOL_CHOICES.includes(ftolHz) ? FTOL_CHOICES : [...FTOL_CHOICES, ftolHz].sort((a, b) => a - b)

  // WSJT-X's two display options, kept between sessions as WSJT-X keeps them.
  const [lowerCase, setLowerCase] = useState(() => surfaceGet(LOWER_CASE_KEY) === '1')
  const [includeTime, setIncludeTime] = useState(() => surfaceGet(INCLUDE_TIME_KEY) === '1')
  const toggleLowerCase = () => {
    setLowerCase(!lowerCase)
    surfaceSet(LOWER_CASE_KEY, lowerCase ? '0' : '1')
  }
  const toggleIncludeTime = () => {
    setIncludeTime(!includeTime)
    surfaceSet(INCLUDE_TIME_KEY, includeTime ? '0' : '1')
  }
  const options: JttyLineOptions = { lowerCase, includeTime }

  // The JTTY channels for the band control (WSJT-X's JTTY frequency list).
  const [plan, setPlan] = useState<BandChannel[]>([])
  useEffect(() => {
    void getLicensedBandPlan('jtty').then(setPlan).catch(() => {})
  }, [])

  const commitDial = (mhz: number) => {
    onSetFrequency?.(mhz, bandLabelForMhz(mhz), snap?.radio.sideband || 'USB')
  }

  // A CONFIRMED call, clicked, goes to the log strip's Call: a decoder fill that is sure, so the
  // strip looks it up once (`cwLive`, `confirmed: true`). Nothing else here can start a lookup.
  const [grabbed, setGrabbed] = useState<string | null>(null)

  // Stop TX: the universal halt. This cockpit transmits nothing of its own; the halt also takes a
  // tune carrier, a stray PTT, and a PSK over still on the air from the PSK cockpit. Esc is the
  // same halt, bound by App while this screen is on show (its ESC_HALT_VIEWS, as Phone's and
  // SSTV's, whose Stop TX is the plain halt too).
  const stop = () => {
    void haltTx()
      .then((s) => s && onSnap?.(s))
      .catch(() => {})
  }

  // ---- THE TRANSMITTER (the dock). The ENGINE is the authority on every send — it checks every TX
  // gate and says why a send was refused (a toast). ----
  const tx = jtty?.tx
  const live = tx?.live === true
  // On the air, queued, or in a live session: the Esc/Stop key's predicate (PSK's `sending ||
  // latched`), live from the instant a session starts, before its first frame.
  const going = (tx?.sending ?? false) || (tx?.left ?? 0) > 0 || live
  const txHz = tx?.txHz ?? 1500
  const serial = tx?.serial ?? 1
  // The Call box: WSJT-X's DX Call, what `%H`/`%Q` send. A confirmed call clicked in a pane fills it
  // as it fills the log strip; only a click or typing changes it.
  const [hisCall, setHisCall] = useState('')
  // `%R`: the SNR of the decode the DX call was picked from, as WSJT-X captures it
  // (`m_jttyHisCallSnr`, 3.3.0-beta4 `widgets/mainwindow.cpp` 9276-9283) — the newest decode in
  // the pane clicked that contains the call (`snrForSelectedWord`) — kept WITH the call it was
  // taken for, and sent only while the Call box still holds that call: a report never goes to
  // another station. Null — WSJT-X's -10 — once the Call box is typed in (`mainwindow.cpp`
  // 10565) or for a call picked from our own line.
  const [picked, setPicked] = useState<{ call: string; snr: number } | null>(null)
  const hisSnr = picked && picked.call === hisCall.trim().toUpperCase() ? picked.snr : null
  const pickCall = (call: string, line: JttyLine, pane: JttyLine[]) => {
    const up = call.toUpperCase()
    setGrabbed(call)
    setHisCall(up)
    if (line.tx) {
      setPicked(null)
      return
    }
    const named = (l: JttyLine) => !l.tx && l.text.toUpperCase().split(/\s+/).includes(up)
    const newest = pane.filter(named).reduce<JttyLine>((a, l) => (l.startUtcMs >= a.startUtcMs ? l : a), line)
    setPicked({ call: up, snr: newest.snr })
  }
  // The compose box holds what is not yet sent; `committed` is what this transmission has sent of
  // it (WSJT-X's locked, painted prefix), shown before it until the transmission is over.
  const [draft, setDraft] = useState('')
  const draftRef = useRef('')
  const [committed, setCommitted] = useState('')
  useEffect(() => {
    if (!going) setCommitted('')
  }, [going])
  // Sends run one after another, in the order they were made: a commit planned on text an earlier
  // one already took would send it twice, and an F-key pressed meanwhile must be neither lost nor
  // allowed to overtake what was sent before it. A send reads the box when its turn comes.
  const chain = useRef<Promise<unknown>>(Promise.resolve())
  const idle = useRef<number | null>(null)
  const applySent = (r: JttySent | null, fromBox: boolean) => {
    if (!r) return
    setJtty(r.state)
    if (!fromBox) return
    draftRef.current = draftRef.current.slice(r.consumed)
    setDraft(draftRef.current)
    if (r.sent) setCommitted((c) => c + r.sent)
  }
  const sendWith = (f: () => Promise<JttySent | null>, fromBox: boolean) => {
    chain.current = chain.current
      .then(f)
      .then((r) => applySent(r, fromBox))
      .catch((e) => pushToast(String(e), 'error'))
  }
  // A live commit of each kind waits in the chain at most once: typing faster than the engine
  // answers queues one look at the box, not one per keystroke.
  const pendingCommit = useRef({ advance: false, flush: false })
  // Send (WSJT-X's Ctrl+K): all of it, ending the message, and a live session with it.
  const sendTyped = () => {
    if (!draftRef.current.trim()) return
    sendWith(() => (draftRef.current.trim() ? jttySend(draftRef.current, hisCall, hisSnr) : Promise.resolve(null)), true)
  }
  // WHILE THE KEYS ARE OPEN FOR EDITING, F1–F8 SEND NOTHING (pressed or clicked): a refusal is
  // said, and sending comes back when editing closes. What a key would send is being changed.
  const editingRef = useRef(false)
  const sendFkey = (n: number) => {
    if (editingRef.current) {
      pushToast(t('jtty.fkeys.editing.refused'), 'info')
      return
    }
    sendWith(() => jttyFkey(n, hisCall, hisSnr), false)
  }
  // Live (WSJT-X's Alt+J), and its sign-off (Alt+K): `DE <call> K`, and the session ends.
  const toggleLive = () =>
    sendWith(
      () => (live ? jttySignOff(draftRef.current, hisCall, hisSnr) : jttyLiveStart(draftRef.current, hisCall, hisSnr)),
      true,
    )
  // While live: what is typed goes as WSJT-X sends it — a batch once five words are complete beyond
  // the three it holds back, and everything after a pause of LIVE_IDLE_MS.
  const commitLive = (flush: boolean) => {
    const kind = flush ? 'flush' : 'advance'
    if (!live || !draftRef.current || pendingCommit.current[kind]) return
    pendingCommit.current[kind] = true
    sendWith(() => {
      pendingCommit.current[kind] = false
      return draftRef.current ? jttyLiveCommit(draftRef.current, hisCall, flush, hisSnr) : Promise.resolve(null)
    }, true)
  }
  const onDraft = (v: string) => {
    draftRef.current = v
    setDraft(v)
    if (!live) return
    commitLive(false)
    if (idle.current != null) window.clearTimeout(idle.current)
    idle.current = window.setTimeout(() => commitLive(true), LIVE_IDLE_MS)
  }
  useEffect(
    () => () => {
      if (idle.current != null) window.clearTimeout(idle.current)
    },
    [],
  )
  const stopJtty = () => {
    if (idle.current != null) window.clearTimeout(idle.current)
    void jttyStop()
      .then(setJtty)
      .catch(() => {})
  }
  // Ctrl+H (WSJT-X's `insertJttyChatOpener`, 3.3.0-beta4 `widgets/mainwindow_jtty.cpp` 232-236):
  // `%H %M ` typed where the caret is, as ordinary typing, expanded when it is sent.
  const insertOpener = (box: HTMLInputElement) => {
    const start = box.selectionStart ?? box.value.length
    const end = box.selectionEnd ?? start
    onDraft(box.value.slice(0, start) + CHAT_OPENER + box.value.slice(end))
    requestAnimationFrame(() => box.setSelectionRange(start + CHAT_OPENER.length, start + CHAT_OPENER.length))
  }
  // Ctrl+Shift+K (WSJT-X's `clearJttyLiveEntry`, `mainwindow_jtty.cpp` 183-203): the box emptied,
  // what it sent cleared, and anything still going stopped, as WSJT-X aborts it.
  const clearBox = () => {
    if (going) stopJtty()
    draftRef.current = ''
    setDraft('')
    setCommitted('')
  }
  // Tx (WSJT-X's Tx spin box) and the serial: drafts committed on Enter or leaving the box.
  const [txDraft, setTxDraft] = useState<string | null>(null)
  const commitTx = (hz: number) => {
    if (!Number.isFinite(hz)) return
    void jttySetTx(Math.min(TX_MAX_HZ, Math.max(TX_MIN_HZ, Math.round(hz))))
      .then(setJtty)
      .catch((e) => pushToast(String(e), 'error'))
  }
  const commitTxDraft = () => {
    if (txDraft == null) return
    const v = Number(txDraft)
    setTxDraft(null)
    if (Number.isFinite(v) && Math.round(v) !== txHz) commitTx(v)
  }
  const [serialDraft, setSerialDraft] = useState<string | null>(null)
  const commitSerial = () => {
    if (serialDraft == null) return
    const v = Math.round(Number(serialDraft))
    setSerialDraft(null)
    if (!Number.isFinite(v) || v === serial) return
    void jttySetSerial(Math.min(SERIAL_MAX, Math.max(SERIAL_MIN, v)))
      .then(setJtty)
      .catch((e) => pushToast(String(e), 'error'))
  }
  // F1–F8 on the keyboard, while this is the VISIBLE view (the cockpit stays mounted in its
  // keep-alive host): the same send a click makes, one press one message, never with Alt, Ctrl or
  // Cmd (the system's), and the webview's own F-key actions cancelled here.
  const keys = useRef({ sendFkey })
  keys.current = { sendFkey }
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      const n = FKEY_NAMES.indexOf(e.key)
      if (n < 0 || e.altKey || e.ctrlKey || e.metaKey) return
      e.preventDefault()
      if (!e.repeat) keys.current.sendFkey(n + 1)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [active])
  // WSJT-X'S TWO F-KEY SETS, EDITABLE (its `msg1`..`msg8` line edits and "Message style" box,
  // `on_comboBoxJttyStyle_currentIndexChanged`, 3.3.0-beta4 `widgets/mainwindow_jtty.cpp`
  // 1037-1060): the style picks the set the keys send, each set keeps its own eight, and ✎ turns
  // the keys into their templates to edit. Both are Settings fields, written through the patch
  // seam (one field over the live settings, never a held copy); the engine reads them at each
  // press, so a key sends what it shows. Editing never sends.
  const style: JttyMessageStyle = tx?.style ?? 'contest'
  const fkeys = tx?.fkeys ?? FKEY_DEFAULTS[style]
  const [editingKeys, setEditingKeys] = useState(false)
  editingRef.current = editingKeys
  const [keyDrafts, setKeyDrafts] = useState<Record<number, string>>({})
  // THE LOG QSO PROMPT. When a send starting "TU " goes out, WSJT-X opens its Log QSO dialog
  // (`handleJttyContestSerial` starts `logQSOTimer`, 3.3.0-beta4 `widgets/mainwindow_jtty.cpp`
  // 701-709). Here the engine raises a prompt naming the station the TU thanked, and this asks.
  // ASKING CHANGES NOTHING: the Call box and the log strip stay as the operator has them. Log QSO
  // hands the strip a request for THAT station: it logs through its own Log, with every check that
  // makes, only if it holds that call (an empty strip is filled for the operator to check and press
  // again; another call is left alone and nothing is logged). The engine takes the first answer
  // (Log QSO or Not now) for every screen, so a reload or a Remote client does not ask again and
  // the contact is never logged twice; it stops asking after two minutes.
  // An answer that takes nothing says why (WHY_NOT): another screen answered it, the contact is in
  // the log already, a newer TU replaced it, or it is too old. A Log QSO the strip then could not
  // log gives the prompt back, so it can still be answered.
  const logPrompt = tx?.logPrompt ?? null
  const [promptDone, setPromptDone] = useState<number | null>(null)
  const askToLog = logPrompt && logPrompt.id !== promptDone ? logPrompt : null
  const [logRequest, setLogRequest] = useState<{ id: number; call: string; claim: () => Promise<boolean> } | null>(null)
  // The request handed to the strip, with the prompt it answers and the engine's answer once asked:
  // an outcome is matched to it by id, and one for an earlier request changes nothing.
  const requested = useRef<{ id: number; promptId: number; call: string; answer: JttyLogPromptAnswer | null } | null>(null)
  const requests = useRef(0)
  const logPrompted = () => {
    if (!askToLog) return
    const req = { id: ++requests.current, promptId: askToLog.id, call: askToLog.call, answer: null as JttyLogPromptAnswer | null }
    requested.current = req
    setLogRequest({
      id: req.id,
      call: req.call,
      claim: () =>
        jttyLogPromptAnswer(req.promptId).then((a) => {
          req.answer = a
          return a === 'taken'
        }),
    })
  }
  const onLogRequest = (
    id: number,
    outcome: { kind: 'logging' | 'filled' | 'not-ready' | 'taken' | 'refused' } | { kind: 'other-call'; held: string },
  ) => {
    const req = requested.current
    if (!req || req.id !== id) return
    // Handled: the strip is asked nothing more by this request.
    setLogRequest(null)
    const call = req.call
    switch (outcome.kind) {
      case 'logging':
        setPromptDone(req.promptId)
        return
      case 'refused':
        // Taken, but nothing logged (the strip said why): asked again, here and on every screen.
        requested.current = null
        setPromptDone((d) => (d === req.promptId ? null : d))
        void jttyLogPromptRelease(req.promptId).catch(() => {})
        return
      case 'taken':
        setPromptDone(req.promptId)
        pushToast(whyNot(req.answer), 'info')
        return
      case 'filled':
        pushToast(t('jtty.logPrompt.filled', { call }), 'info')
        return
      case 'other-call':
        pushToast(t('jtty.logPrompt.otherCall', { call, held: outcome.held }), 'info')
        return
      case 'not-ready':
        pushToast(t('jtty.logPrompt.notReady'), 'info')
    }
  }
  const notNow = () => {
    if (!askToLog) return
    setPromptDone(askToLog.id)
    void jttyLogPromptAnswer(askToLog.id).catch(() => {})
  }

  const refreshAfterSave = (s: AppSnapshot) => {
    onSnap?.(s)
    return getJttyState().then(setJtty)
  }
  const setStyle = (next: JttyMessageStyle) => {
    if (next === style) return
    void patchSettings(() => ({ jttyMessageStyle: next }))
      .then(refreshAfterSave)
      .catch(() => pushToast(t('jtty.fkeys.saveFailed'), 'error'))
  }
  const saveKey = (i: number) => {
    const text = keyDrafts[i]
    setKeyDrafts((d) => {
      const rest = { ...d }
      delete rest[i]
      return rest
    })
    if (text == null || text === fkeys[i]) return
    const field = FKEY_SETTING[style]
    const defaults = FKEY_DEFAULTS[style]
    void patchSettings((cur: Settings) => {
      const list = defaults.map((d, k) => cur[field]?.[k] ?? d)
      list[i] = text
      return { [field]: list }
    })
      .then(refreshAfterSave)
      .catch(() => pushToast(t('jtty.fkeys.saveFailed'), 'error'))
  }
  const resetKeys = () => {
    setKeyDrafts({})
    void patchSettings(() => ({ [FKEY_SETTING[style]]: [...FKEY_DEFAULTS[style]] }))
      .then(refreshAfterSave)
      .catch(() => pushToast(t('jtty.fkeys.saveFailed'), 'error'))
  }

  // The line under the dock: the transmitter's notice, else why a send would be refused right now
  // (the engine's words), else how to send.
  const dockHint = tx?.notice ?? tx?.refusal ?? t('jtty.compose.hint')

  const lines = jtty?.lines ?? []
  const allLines = useMemo(() => jttyAllPane(lines), [lines])
  const rxLines = useMemo(() => jttyRxPane(lines), [lines])
  const allPin = usePinnedScroll<HTMLDivElement>()
  const rxPin = usePinnedScroll<HTMLDivElement>()
  const erase = () => {
    void jttyClear()
      .then(setJtty)
      .catch(() => {})
    // A wipe re-pins (the RTTY/PSK rule): the emptied panes follow the next copy.
    allPin.repin()
    rxPin.repin()
  }
  // DECODE AGAIN: WSJT-X's Decode button (`jtty_again`) decodes the receive audio still kept, and a
  // double-click on a waterfall row decodes 40 s of it from 5 s before that row
  // (`jttyDecodeAgainAtSample`, 3.3.0-beta4 `widgets/plotter.cpp` 1011-1021). Receive only: what it
  // finds shows under a heading of its own; the engine says why when the audio is gone.
  const decodeAgain = (atUtcMs: number | null) => {
    void jttyDecodeAgain(atUtcMs)
      .then(setJtty)
      .catch((e) => pushToast(String(e), 'info'))
  }

  // ---- panes ----
  // THE RF SCOPE PANE — the radio's own panadapter, hidden until ticked (JTTY_PANELS ships it
  // hidden); it heads the leading column, JS8's place for it. Display only: no stop control, no
  // sender.
  const rfScopePane = shown('rfScope') ? (
    <RfScopePane
      key="rfScope"
      closeProps={closeProps('rfScope')}
      dialMhz={snap?.radio.dialMhz ?? 0}
      keyed={snap?.radio.tuning ?? false}
      theme={theme}
      active={active}
      privilegeMode={snap?.radio.operatingMode}
    />
  ) : null

  // ALL DECODES — WSJT-X's left pane: every message, where it started. Its head holds the decoder's
  // controls and the two display options; none touches PTT.
  const allPane = shown('all') ? (
    <CockpitPaneFrame key="all" title={t('jtty.pane.all.title')} paneId="all" {...closeProps('all')}>
      <div className="cw-decode jtty-pane" title={t('jtty.pane.all.hint')}>
        <div className="cw-decode-head">
          <span className="cw-decode-label">{RX_PLATE}</span>
          <button
            type="button"
            className={`rtty-arm${armed ? ' on' : ''}`}
            aria-pressed={armed}
            onClick={toggleArm}
            title={armed ? t('jtty.arm.on.title') : t('jtty.arm.off.title')}
          >
            {armed ? t('jtty.arm.on.label') : t('jtty.arm.off.label')}
          </button>
          <button
            type="button"
            className={`rtty-arm jtty-option${lowerCase ? ' on' : ''}`}
            aria-pressed={lowerCase}
            onClick={toggleLowerCase}
            title={t('jtty.lowerCase.title')}
          >
            {t('jtty.lowerCase.label')}
          </button>
          <button
            type="button"
            className={`rtty-arm jtty-option${includeTime ? ' on' : ''}`}
            aria-pressed={includeTime}
            onClick={toggleIncludeTime}
            title={t('jtty.includeTime.title')}
          >
            {t('jtty.includeTime.label')}
          </button>
          <button type="button" className="cw-decode-clear" onClick={erase} title={t('jtty.erase.title')}>
            {t('jtty.erase.label')}
          </button>
          <button type="button" className="cw-decode-clear" onClick={() => decodeAgain(null)} title={t('jtty.decode.title')}>
            {t('jtty.decode.label')}
          </button>
        </div>
        <div className="cw-decode-text jtty-lines" ref={allPin.ref} onScroll={allPin.onScroll}>
          {allLines.length > 0 ? (
            <JttyPaneLines lines={allLines} options={options} onCall={(c, l) => pickCall(c, l, allLines)} />
          ) : (
            <span className="cw-decode-idle">{armed ? t('jtty.all.listening') : t('jtty.all.idle')}</span>
          )}
        </div>
      </div>
    </CockpitPaneFrame>
  ) : null

  // QSO FREQUENCY — WSJT-X's right pane: the messages first heard within F Tol of Rx.
  const rxPane = shown('rx') ? (
    <CockpitPaneFrame key="rx" title={t('jtty.pane.rx.title')} paneId="rx" {...closeProps('rx')}>
      <div className="cw-decode jtty-pane" title={t('jtty.pane.rx.hint')}>
        <div className="cw-decode-text jtty-lines" ref={rxPin.ref} onScroll={rxPin.onScroll}>
          {rxLines.length > 0 ? (
            <JttyPaneLines lines={rxLines} options={options} onCall={(c, l) => pickCall(c, l, rxLines)} />
          ) : (
            <span className="cw-decode-idle">{t('jtty.rx.empty')}</span>
          )}
        </div>
      </div>
    </CockpitPaneFrame>
  ) : null

  // THE LOG STRIP — PSK's, in a column of its own and not ⊞-removable (`paneId` is a test handle,
  // not a vocabulary id). A confirmed call clicked in a pane lands in its Call, which is the one
  // lookup this cockpit makes. It hosts no stop control.
  const logPane = snap ? (
    <CockpitPaneFrame
      key="log"
      title={t('jtty.pane.log.title')}
      paneId="log"
      actions={snap.fieldDay ? <ContestScoreChip place="head" fieldDay={snap.fieldDay} /> : undefined}
      weight={1.5}
    >
      {askToLog && (
        <div className="jtty-log-prompt" role="group" aria-label={t('jtty.logPrompt.aria')}>
          <span role="status">
            {askToLog.call
              ? t('jtty.logPrompt.text', { call: askToLog.call })
              : t('jtty.logPrompt.textNoCall')}
          </span>
          {askToLog.replaced && (
            <span className="jtty-log-prompt-note">{t('jtty.logPrompt.replacedNote', { call: askToLog.replaced })}</span>
          )}
          <button type="button" className="le-qrz jtty-log-prompt-log" onClick={logPrompted}>
            {t('jtty.logPrompt.log')}
          </button>
          <button type="button" className="le-qrz" onClick={notNow}>
            {t('jtty.logPrompt.dismiss')}
          </button>
        </div>
      )}
      <LogEntry
        onOpenLogbook={onOpenLogbook}
        snap={snap}
        mode={MODE_NAME}
        defaultRst="599"
        exchange="terrestrial"
        titled={false}
        cwLive={grabbed ? { call: grabbed, rst: null, name: null, confirmed: true } : null}
        onReset={() => setGrabbed(null)}
        logRequest={logRequest}
        onLogRequest={onLogRequest}
        onSnap={onSnap}
        fieldDay={snap.fieldDay ?? null}
        scoreChip={false}
        active={active}
        fdMode="DIG"
        fdSubmode={MODE_NAME}
        // The dial plus JTTY's Tx frequency, as WSJT-X logs a JTTY contact (the dial plus its Tx
        // box). A station that sends no transmitter logs the dial, as before.
        logOffsetHz={tx?.txHz}
      />
    </CockpitPaneFrame>
  ) : null

  // THE PANE REGION — CW's and JS8's grid; useRegionCols measures the region and owns its
  // data-cols / data-flow. On a wide window (three tracks) WSJT-X's two panes stand side by side,
  // as WSJT-X draws them, with the log column beside them; below three they share the leading
  // column, so the log strip keeps a column's full height for its Log button; on a narrow region
  // the same two groups stack and the region scrolls. Only panes on screen take a track (the ⊞
  // menu caps the count: an empty track is a band of dead space), and the columns are KEYED, so a
  // tier flip never remounts the log strip mid-entry.
  const [lead, aux, logCol] = [[rfScopePane, allPane], [rxPane], [logPane]].map((g) => g.filter((p) => p != null))
  const populated = [lead, aux, logCol].filter((g) => g.length > 0).length
  const { ref: panesRef, cols } = useRegionCols<HTMLDivElement>(Math.max(1, populated) as RegionCols)
  const tracks: Array<[key: string, panes: ReactElement[]]> =
    cols === 3
      ? [['main', lead], ['aux', aux], ['log', logCol]]
      : logCol.length > 0
        ? [['main', [...lead, ...aux]], ['log', logCol]]
        : [['main', lead], ['aux', aux]]

  return (
    <main className="layout single jtty-cockpit">
      {snap && (
        <CockpitHeader
          snap={snap}
          onSnap={onSnap}
          modeIndicator={
            <span className="cw-mode-badge" title={t('jtty.header.mode.title')}>
              {MODE_NAME}
            </span>
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
              <span className="cockpit-ph-pill" title={t('jtty.header.band.title')}>
                {bandLabelForMhz(snap.radio.dialMhz) || '—'}
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
                onToggle={(id, show) => panels.setPanelState(id as JttyPanelId, show ? 'docked' : 'removed')}
                onUndo={panels.undo}
                canUndo={panels.canUndo}
                onReset={panels.reset}
              />
            ) : undefined
          }
        >
          {/* WSJT-X's two receive controls, always on screen: they decide what is decoded. */}
          <span className="jtty-rx-controls" role="group" aria-label={t('jtty.rx.group.aria')}>
            <label className="jtty-rx-field" title={t('jtty.rx.title')}>
              <span>{RX_LABEL}</span>
              <input
                className="settings-input jtty-rx-input"
                type="number"
                min={RX_MIN_HZ}
                max={RX_MAX_HZ}
                step={1}
                aria-label={t('jtty.rx.aria')}
                value={rxDraft ?? String(Math.round(rxHz))}
                onFocus={() => setRxDraft(String(Math.round(rxHz)))}
                onChange={(e) => setRxDraft(e.target.value)}
                onBlur={commitRx}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    commitRx()
                  }
                }}
              />
              <span>{HZ}</span>
            </label>
            <label className="jtty-rx-field" title={t('jtty.tx.title')}>
              <span>{TX_LABEL}</span>
              <input
                className="settings-input jtty-rx-input"
                type="number"
                min={TX_MIN_HZ}
                max={TX_MAX_HZ}
                step={1}
                disabled={going}
                aria-label={t('jtty.tx.aria')}
                value={txDraft ?? String(Math.round(txHz))}
                onFocus={() => setTxDraft(String(Math.round(txHz)))}
                onChange={(e) => setTxDraft(e.target.value)}
                onBlur={commitTxDraft}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    commitTxDraft()
                  }
                }}
              />
              <span>{HZ}</span>
            </label>
            <button
              type="button"
              className="rtty-arm"
              onClick={() => commitTx(rxHz)}
              disabled={going}
              title={t('jtty.r2t.title')}
            >
              {R2T_LABEL}
            </button>
            <button
              type="button"
              className="rtty-arm"
              onClick={() => setRx(Math.min(RX_MAX_HZ, Math.max(RX_MIN_HZ, Math.round(txHz))), ftolHz)}
              title={t('jtty.t2r.title')}
            >
              {T2R_LABEL}
            </button>
            <label className="jtty-rx-field" title={t('jtty.ftol.title')}>
              <span>{FTOL_LABEL}</span>
              <select
                className="settings-input jtty-ftol-select"
                aria-label={t('jtty.ftol.aria')}
                value={String(Math.round(ftolHz))}
                onChange={(e) => setRx(rxHz, Number(e.target.value))}
              >
                {ftolChoices.map((v) => (
                  <option key={v} value={v}>
                    {v}
                  </option>
                ))}
              </select>
              <span>{HZ}</span>
            </label>
          </span>
        </CockpitHeader>
      )}

      {/* THE BAND WATERFALL — ⊞-hideable, PSK's strip (`.jtty-cockpit .waterfall-wrap` rides the
          RTTY/PSK rule). It hosts no stop control and no sender: the cursors mark Rx and Tx, a click
          sets Rx, Shift+click (or a right-click) Tx and Ctrl+click both (WSJT-X's wide graph), and
          the window it shows is the range the decoder searches its fixed channels in. Its divider
          sits under it and sets its height, stored per surface. */}
      {jtty && shown('scope') && (
        <>
          <Waterfall
            stripRef={wfRef}
            {...closeProps('scope')}
            paneTitle={jttyPanelLabels().scope}
            theme={theme}
            active={active}
            rowMs={50}
            transmitting={snap?.radio.transmitting ?? false}
            keyed={snap?.radio.tuning ?? false}
            rxOffsetHz={rxHz}
            txOffsetHz={txHz}
            cursors={[
              { hz: rxHz, color: 'var(--rx)', label: 'RX' },
              { hz: txHz, color: 'var(--tx)', label: 'TX' },
            ]}
            hint={t('jtty.waterfall.hint')}
            onTune={(hz, target) => {
              if (target !== 'rx') commitTx(hz)
              if (target !== 'tx') setRx(Math.min(RX_MAX_HZ, Math.max(RX_MIN_HZ, Math.round(hz))), ftolHz)
            }}
            onRowDoubleClick={(at) => decodeAgain(at)}
            onViewChange={(lo, hi) => {
              void jttySetBand(Math.floor(lo), Math.ceil(hi))
                .then(setJtty)
                .catch(() => {})
            }}
          />
          <PaneSeam
            axis="y"
            varName="--rtty-wf-h"
            strip={wfRef}
            storageKey="nexus.split.jtty.waterfall"
            min={WATERFALL_SPLIT_MIN}
            max={WATERFALL_SPLIT_MAX}
            defaultPct={25}
            label={t('jtty.waterfall.splitter.label')}
          />
        </>
      )}

      {/* THE TX STRIP — FT's cluster under the waterfall: the TX-enable latch, Tune, the rig's ATU
          and Stop TX, sticky so they never leave the window. A shell child with no ⊞ id. */}
      {snap && (
        <CockpitTxStrip
          radio={snap.radio}
          onStopTx={stop}
          onSetTxEnabled={onSetTxEnabled}
          onTune={(on) => {
            void setTune(on)
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }}
          onAtuTune={() =>
            void atuTune()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
          onAtuBypass={() =>
            void atuBypass()
              .then((s) => onSnap?.(s))
              .catch((e) => pushToast(String(e), 'error'))
          }
        />
      )}

      {jtty?.notice && (
        <div className="cw-keyer-warn jtty-notice" role="status">
          ⚠ {jtty.notice}
        </div>
      )}

      <div className="cockpit-panes" ref={panesRef}>
        {tracks
          .filter(([, panes]) => panes.length > 0)
          .map(([key, panes]) => (
            <div className="cockpit-col" key={key}>
              {panes}
            </div>
          ))}
      </div>

      {/* THE TX DOCK — WSJT-X's transmitter, in RTTY's measured dock shape (`.rtty-dock-keys`): the
          eight F-keys and the fixed row (the Call box, the serial number and Live), the compose row
          (the compose box, Send and Esc/Stop), and a line under it for what the transmitter did or
          would refuse. No ⊞ id; Stop TX, Tune and the latch are up in the TX strip, and Esc/Stop
          here, at the end of the compose row, is on the stop line too (THE STOP LINE). */}
      <div className="cockpit-txdock">
        <div className="rtty-dock-keys jtty-dock-keys">
          <div className="cw-macros rtty-macros jtty-macros" role="group" aria-label={t('jtty.macros.aria')}>
            {FKEY_NAMES.map((name, i) => {
              const template = fkeys[i] ?? ''
              return editingKeys ? (
                <label key={name} className="jtty-fkey-edit">
                  <span className="cw-macro-key">{name}</span>
                  <input
                    className="settings-input mono"
                    value={keyDrafts[i] ?? template}
                    onChange={(e) => setKeyDrafts((d) => ({ ...d, [i]: e.target.value }))}
                    onBlur={() => saveKey(i)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault()
                        saveKey(i)
                      }
                    }}
                    aria-label={t('jtty.fkeys.edit.aria', { key: name })}
                    autoComplete="off"
                    spellCheck={false}
                  />
                </label>
              ) : (
                <button
                  key={name}
                  type="button"
                  className={`cw-macro${template.trim() ? '' : ' rtty-macro-empty'}`}
                  onClick={() => sendFkey(i + 1)}
                  disabled={!template.trim()}
                  title={template.trim() ? t('jtty.fkey.title', { key: name, template }) : t('jtty.fkey.empty.title', { key: name })}
                >
                  <span className="cw-macro-key">{name}</span>
                  <span className="cw-macro-label">{template.trim() ? template : t('jtty.fkey.empty.label')}</span>
                </button>
              )
            })}
          </div>
          <div className="rtty-dock-row jtty-dock-row">
            <input
              className="settings-input rtty-hiscall"
              value={hisCall}
              onChange={(e) => {
                setHisCall(upperTyped(e.target))
                setPicked(null)
              }}
              placeholder={t('jtty.hisCall.placeholder')}
              aria-label={t('jtty.hisCall.aria')}
              autoComplete="off"
              spellCheck={false}
            />
            <label className="jtty-serial" title={t('jtty.serial.title')}>
              <span>{SERIAL_LABEL}</span>
              <input
                className="settings-input jtty-serial-input"
                type="number"
                min={SERIAL_MIN}
                max={SERIAL_MAX}
                step={1}
                aria-label={t('jtty.serial.aria')}
                value={serialDraft ?? String(serial)}
                onFocus={() => setSerialDraft(String(serial))}
                onChange={(e) => setSerialDraft(e.target.value)}
                onBlur={commitSerial}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    commitSerial()
                  }
                }}
              />
            </label>
            <label className="jtty-style" title={t('jtty.style.title')}>
              <span>{t('jtty.style.label')}</span>
              <select
                className="settings-input"
                aria-label={t('jtty.style.aria')}
                value={style}
                onChange={(e) => setStyle(e.target.value as JttyMessageStyle)}
              >
                <option value="contest">{t('jtty.style.contest')}</option>
                <option value="ft8">{t('jtty.style.ft8')}</option>
              </select>
            </label>
            <button
              type="button"
              className={`le-qrz jtty-fkeys-edit${editingKeys ? ' on' : ''}`}
              aria-pressed={editingKeys}
              onClick={() => setEditingKeys(!editingKeys)}
              title={editingKeys ? t('jtty.fkeys.done.title') : t('jtty.fkeys.edit.title')}
            >
              {editingKeys ? t('jtty.fkeys.done.label') : t('jtty.fkeys.edit.label')}
            </button>
            {editingKeys && (
              <button type="button" className="le-qrz" onClick={resetKeys} title={t('jtty.fkeys.reset.title')}>
                {t('jtty.fkeys.reset.label')}
              </button>
            )}
            {/* A SENDER, not a stop: clicking it off signs off and lets what is queued go out. */}
            <button
              type="button"
              className={`cw-macro rtty-tx-latch jtty-live${live ? ' on' : ''}`}
              aria-pressed={live}
              onClick={toggleLive}
              title={live ? t('jtty.live.off.title') : t('jtty.live.on.title')}
            >
              <span className="cw-macro-key">{LIVE_KEY}</span>
              <span className="cw-macro-label">{live ? t('jtty.live.off.label') : t('jtty.live.on.label')}</span>
            </button>
          </div>
        </div>
        <div className="cw-send">
          {committed && (
            <span className="jtty-committed" title={t('jtty.compose.committed.title')}>
              {committed}
            </span>
          )}
          <input
            className="settings-input cw-type"
            value={draft}
            onChange={(e) => onDraft(e.target.value)}
            onKeyDown={(e) => {
              const key = e.key.toLowerCase()
              if (e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey && key === 'h') {
                e.preventDefault()
                insertOpener(e.currentTarget)
                return
              }
              if (e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey && key === 'k') {
                e.preventDefault()
                clearBox()
                return
              }
              if (e.key !== 'Enter') return
              e.preventDefault()
              if (live) commitLive(true)
              else sendTyped()
            }}
            placeholder={live ? t('jtty.compose.placeholder.live') : t('jtty.compose.placeholder')}
            aria-label={t('jtty.compose.aria')}
            autoComplete="off"
            spellCheck={false}
          />
          {(tx?.framesTotal ?? 0) > 0 && (
            <span className="jtty-frames" title={t('jtty.frames.title')}>
              {t('jtty.frames', { sent: tx?.framesSent ?? 0, total: tx?.framesTotal ?? 0 })}
            </span>
          )}
          <button
            type="button"
            className="cw-send-btn"
            onClick={sendTyped}
            disabled={!draft.trim()}
            title={tx?.pending ? t('jtty.compose.pending.title', { text: tx.pending }) : t('jtty.compose.send.title')}
          >
            {(tx?.left ?? 0) > 0 ? t('jtty.compose.send.left', { n: tx?.left ?? 0 }) : t('jtty.compose.send.label')}
          </button>
          <button
            type="button"
            className="cw-macro rtty-stop jtty-stop"
            onClick={stopJtty}
            disabled={!going}
            title={t('jtty.stop.title')}
          >
            <span className="cw-macro-key">{ESC_KEY}</span>
            <span className="cw-macro-label">{t('jtty.stop.label')}</span>
          </button>
        </div>
        {dockHint && (
          <div className="psk-drive-hint" role="status">
            {dockHint}
          </div>
        )}
      </div>
    </main>
  )
}
