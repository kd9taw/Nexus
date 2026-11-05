import { useLayoutEffect, useRef, type ReactNode, type RefObject } from 'react'
import { t } from '../i18n'
import type { RadioStatus } from '../types'
import { isOnAir } from '../types'
import { RadioTuneNote } from './RadioTuneNote'
import { AtuSwitchOutMenu, useAtuSwitchOutMenu } from './AtuSwitchOutMenu'
// ⚠️ THIS FILE IS **PARTIAL** ON THE i18n LIST (i18n/hardcoded-strings.test.ts), for the same
// deferral `CockpitHeader` carried until these controls moved here: THE TX-ENABLE LATCH (and its
// read-only rendering), TUNE, ATU AND STOP TX stay written here, labels and tooltips. One strip
// draws them for EIGHT screens, so a typo in one of the four labels is an eight-screen safety
// regression, and three of them are what `components/stop-line.test.tsx` matches by accessible
// name (/^stop tx$/i, /^tune$/i, /^tx on$|^tx off$/i). Transmit-path controls and their accessible
// names move in their own batch, with the stop-line sweeps re-run. Everything else here — the
// group's name, the TX state, Hold Tx — is catalogued.

export interface CockpitTxStripProps {
  radio: RadioStatus
  /** Arm/disarm TX (WSJT-X "Enable Tx"). Omit ⇒ the first slot shows the latch READ-ONLY (Phone
   *  and CW arm it themselves: the mode change arms it, Phone's PTT offers to). Phone and CW pass it
   *  on a computer driving a station, where entering the section arms nothing: from afar the button
   *  arms it. */
  onSetTxEnabled?: (on: boolean) => void
  /** Tune (key a steady carrier). */
  onTune?: (on: boolean) => void
  /** Run the RADIO's own built-in ATU. Rendered only when the rig reports a tuner (`radio.atu`). */
  onAtuTune?: () => void
  /** Switch the radio's tuner OUT of line — "Switch tuner out" in the ATU button's menu (#344;
   *  right-click, or a long press on touch). A click always runs a tune-up. Without it, or where the
   *  radio's path has no switch-out (`radio.atuSwitchOut`), no menu is offered. */
  onAtuBypass?: () => void
  /** Stop TX — the screen's own stop (CW passes stopCw()+haltTx(), RTTY rttyStop()+haltTx()). */
  onStopTx?: () => void
  /** Hold Tx, the slot modes' fifth control (Tempo; FT draws its own strip). */
  hold?: { on: boolean; onChange: (on: boolean) => void }
  /** The READ-ONLY transmit meters, in the strip's empty right side (Phone and CW on a window of
   *  `md` and up, the compact dock: Operate's QSO strip already carries its meters this way). A
   *  readout and never a control that keys or stops anything: the screen's send area stays in its
   *  dock. Omitted: the strip is exactly what it was. */
  meters?: ReactNode
  /** A screen's own line beside Tune (Phone's newcat Yaesu note, #441): text only, drawn last and
   *  only where Tune is, after RadioTuneNote, so it never moves a control or the TX state. */
  tuneNote?: ReactNode
  /** A screen's own small marker right after the cluster (Phone's ⓘ for the Tune note, #441): after
   *  Stop TX, never between the controls, so Tune, ATU and Stop TX keep their places on every screen. */
  tuneHint?: ReactNode
}

/**
 * THE TRANSMIT STRIP — FT's transmit cluster, in FT's order and FT's look, on every operating
 * screen (2026-10-01: "FT's strip under the scope in every mode, FT's order,
 * staying in view; Stop TX at the same spot on all 8 screens"). TX On/Off · Tune · ATU · Stop TX
 * (· Hold Tx), then the TX state. Each screen keeps its own send area in its dock.
 *
 * A SHELL CHILD, NEVER A PANE: it renders as a direct child of the cockpit shell, right under the
 * scope, with no id in any ⊞ vocabulary — so hiding or moving it is unrepresentable, as for the
 * dock (THE STOP LINE, `cockpit-panes.css`). It is STICKY on both edges (`.cockpit-txstrip`), so
 * when the shell's deficit valve scrolls, the strip parks at the top of the window, or just above
 * the sticky dock, and never leaves the window: at the 150–175 % pins the header Stop TX it
 * replaces sat under the dock or below the window.
 *
 * THE FIRST SLOT IS THE LATCH, AND KEEPS ONE WIDTH: a button where the screen arms TX here (RTTY,
 * PSK, JS8, SSTV, Tempo, APRS), its read-only twin where the screen arms it itself (Phone, CW, at the station), so
 * Tune, ATU and Stop TX sit at the same x on every screen. The button keys on the SLOT flag, as
 * the header's did: in RTTY and SSTV the latch is a STOP (set_tx_enabled(false) arms the mode's
 * abort) and must stay a button through their overs, which `radio.transmitting` never marks.
 */
export function CockpitTxStrip({ radio, onSetTxEnabled, onTune, onAtuTune, onAtuBypass, onStopTx, hold, meters, tuneNote, tuneHint }: CockpitTxStripProps) {
  const onAir = isOnAir(radio)
  // THE SIGN WAITS WITH AN ARMED OVER (the operator's pick "Header ON AIR waits too",
  // 2026-09-28; the sign moved here from the header with the rest of the strip). A streamed
  // operator's PTT, whichever they hold, arms an over that owns the transmitter from the arm, but
  // nothing is on the air until the voice keys it, so while the station reports it armed
  // (`streamMic`, which says so only while that over owns the transmitter) the sign reads as it
  // does before a key. A key read back from the radio is not one of the arbiter's owners, and the
  // FT slot flag and the tune carrier are checked here too, for a station that reports `armed`
  // whatever else holds the transmitter: any of them lights it. DISPLAY ONLY: `onAir` stays the
  // arbiter's answer, and the header's amplifier strip reads it; Stop TX is not touched.
  const lit =
    onAir &&
    !(radio.streamMic === 'armed' && !radio.transmitting && !radio.tuning && radio.rigKeyed !== true)
  const ref = useRef<HTMLDivElement>(null)
  useDockClearance(ref)
  // ⭐ ATU ON (#344, operator ruling): the label says when the radio reports its tuner in line; a
  // click ALWAYS runs a tune-up, as it always did. Switching the tuner out is the button's menu
  // (right-click / long press), offered only where Nexus can send it and only while the tuner is
  // in. It keys nothing, so the tune-up's lockouts do not grey the button while the menu is offered
  // (`aria-disabled` keeps the click a no-op and the right-click alive); the backend refuses the
  // switch-out on the air and says so.
  const atuMenu = useAtuSwitchOutMenu(radio.atu === true && radio.atuSwitchOut === true && !!onAtuBypass)
  const atuTuneBlocked = !radio.txAllowed || !!radio.atuStartTuneUnsupported

  return (
    <div className="cockpit-txstrip" ref={ref}>
      <div className="op-controls cq-txctl" role="group" aria-label={t('operate.strip.txControls.aria')}>
        {/* ⚠️ DEFERRED (i18n), the four controls below — see this file's header. */}
        {onSetTxEnabled && !radio.transmitting ? (
          <button
            type="button"
            className={`op-btn monitor${radio.txEnabled ? ' on' : ''}`}
            aria-pressed={radio.txEnabled}
            onClick={() => onSetTxEnabled(!radio.txEnabled)}
            title={
              radio.txEnabled
                ? 'Transmit ARMED (WSJT-X "Enable Tx") — sends will key the rig. Click to disable.'
                : 'Transmit is OFF — click to ARM so this screen’s sends can key the rig (WSJT-X "Enable Tx").'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </button>
        ) : (
          <span className={`op-btn monitor readonly${radio.txEnabled ? ' on' : ''}`}
            title={
              !radio.transmitting && radio.txBusyReason
                ? radio.txBusyReason
                : radio.txEnabled
                  ? 'Transmit is enabled (WSJT-X "Enable Tx"). Stop TX is the immediate halt.'
                  : 'Transmit is off (WSJT-X "Enable Tx"). This screen arms it from its own transmit control.'
            }
          >
            {radio.txEnabled ? 'TX On' : 'TX Off'}
          </span>
        )}
        {onTune && (
          <button
            type="button"
            className={`op-btn tune${radio.tuning ? ' keyed' : ''}`}
            aria-pressed={radio.tuning}
            onClick={() => onTune(!radio.tuning)}
            disabled={!radio.txAllowed}
            title={radio.txRefusal ?? "Key a steady carrier to tune an ATU/amp (auto-stops on the tune watchdog). Click again to stop."}
          >
            Tune
          </button>
        )}
        {/* ⭐ DISABLED, NOT HIDDEN, when this CAT path cannot START a tune (Hamlib's Icom and
            Kenwood backends clamp `set_func TUNER 2` to "tuner in line"; `icom.c:7085`): hiding it
            answers "where did my ATU go?" with nothing. It is the ACTION that is unavailable.
            Disabled on the licence lockout exactly like Tune; every other refusal (TX off, busy)
            comes back from the backend with its reason. */}
        {onAtuTune && radio.atu != null && (
          <button
            type="button"
            ref={atuMenu.buttonRef}
            className={`op-btn atu${radio.atu ? ' on' : ''}`}
            aria-pressed={radio.atu}
            aria-haspopup={atuMenu.offered ? 'menu' : undefined}
            onClick={atuMenu.click(atuTuneBlocked ? undefined : onAtuTune)}
            disabled={!atuMenu.offered && atuTuneBlocked}
            aria-disabled={atuMenu.offered && atuTuneBlocked ? true : undefined}
            {...atuMenu.handlers}
            title={`${
              radio.atuStartTuneUnsupported
                ? "This CAT connection can't start the radio's tuner — press TUNER on the radio itself."
                : "Run the radio's built-in antenna tuner (it transmits its own carrier for a second or two)."
            } ${radio.atu ? 'The tuner is switched in.' : 'The tuner is currently bypassed.'}${
              atuMenu.offered ? ' Right-click or long-press to switch it out.' : ''
            }`}
          >
            {radio.atu ? 'ATU ON' : 'ATU'}
            {atuMenu.offered && onAtuBypass && (
              <AtuSwitchOutMenu
                open={atuMenu.open}
                onOpenChange={atuMenu.setOpen}
                onSwitchOut={onAtuBypass}
                buttonRef={atuMenu.buttonRef}
              />
            )}
          </button>
        )}
        {/* THE stop control of every screen that draws this strip; its label is the accessible
            name every stop-line sweep looks for (/^stop tx$/i). */}
        {onStopTx && (
          <button type="button" className="op-btn stop"
            onClick={onStopTx}
            title="Stop TX (Esc)">
            Stop TX
          </button>
        )}
        {hold && (
          <button
            type="button"
            className={`op-btn hold${hold.on ? ' on' : ''}`}
            aria-pressed={hold.on}
            onClick={() => hold.onChange(!hold.on)}
            title={t('topbar.holdTx.title')}
          >
            {t('topbar.holdTx.label')}
          </button>
        )}
      </div>
      {onTune && tuneHint}
      {/* The ON AIR sign reads the arbiter (`isOnAir`), never the FT slot flag alone, so a voice
          over, CW, RTTY, a tune or a key held at the radio all light it; an over armed through the
          stream lights it once the voice keys it (`lit`, above). Paint only. */}
      {/* A connection that transmits nothing says so here, with its reason on hover, rather
          than "receiving" over a TX that cannot key. A key the radio itself holds still lights
          the sign: what is on the air is shown first. */}
      <span className={`cq-statecap ${lit ? 'tx' : radio.txRefusal ? 'off' : radio.txEnabled ? 'rx' : 'off'}`}
        title={!lit && radio.txRefusal ? radio.txRefusal : undefined}>
        {lit
          ? t('operate.strip.state.transmitting')
          : radio.txRefusal
            ? t('operate.strip.state.connectionNoTx')
            : radio.txEnabled
              ? t('operate.strip.state.receiving')
              : t('operate.strip.state.txOff')}
      </span>
      {/* THE METERS' CELL, at the strip's right end (`.cockpit-txstrip-meters`): after the cluster and
          the TX state, so nothing it shows can move a control. Two rows at most, the strip's own
          height, and the same readings before and after a key-down (TxMeters `strip`). */}
      {meters != null && <div className="cockpit-txstrip-meters">{meters}</div>}
      {/* What the radio reports beside Tune while Tune is the Flex radio's own carrier: the last
          child, on a line of its own, so it never moves the controls or the TX state. */}
      {onTune && <RadioTuneNote radio={radio} />}
      {onTune && tuneNote}
    </div>
  )
}

/** THE STICKY EDGES' PLACEMENT INPUTS, measured because CSS cannot read a sibling's box.
 *  · `--cockpit-txstrip-bottom` on the strip: the dock's height. The dock is sticky at `bottom: 0`,
 *    so a strip parked at 0 would park UNDER it — Stop TX covered by the PTT row, the exact defect
 *    at the 150–175 % pins. A dock that is not sticky clears nothing. A FOLDED dock (decideFold below)
 *    parks only its keep row, so the strip clears the keep row's height.
 *  · `--cockpit-txstrip-top` on the strip: the height of a PINNED header (`.cockpit-header-pinned`,
 *    sticky at `top: 0` — SSTV, RTTY, PSK). A strip parked at 0 would park OVER it and cover the
 *    dial and the band; it parks right under it instead. A header that is not sticky clears nothing.
 *  · `--cockpit-txstrip-h` on the SHELL: what is parked at the top while the strip is sticky — the
 *    strip's rendered height, plus a pinned header's — which the shell's `scroll-padding-top` reads
 *    so whatever is scrolled to the start lands below it. RENDERED, not offsetHeight: under the UI
 *    zoom Chrome applies `scroll-padding` unzoomed (measured: at 175 % a 42 px offsetHeight padded
 *    42 screen px under a 74 px strip), and more padding than the strip only scrolls a target a
 *    little lower.
 *  · `--cockpit-txdock-keep-h` on the SHELL, while the dock is folded: the keep row's height, which the shell's
 *    `scroll-padding-bottom` reads so a focus or a `scrollIntoView` never lands behind the parked row. RENDERED, for the
 *    strip's reason: with offsetHeight (89 px for RTTY's 156 px row at 175 %) it cleared CW and PSK by luck and not RTTY.
 *  Re-read on every render as well as on a resize. */
const isDock = (c: Element): c is HTMLElement =>
  c instanceof HTMLElement && (c.classList.contains('cockpit-txdock') || c.classList.contains('sstv-tx-bar'))
/** What is parked at the top ABOVE the strip: a sticky shell child before it — the pinned header
 *  (CockpitHeader `pinned`). Read off the computed style rather than a class, so the strip clears
 *  whatever actually sticks there and nothing that does not. */
function pinnedAbove(shell: HTMLElement, strip: HTMLElement): HTMLElement | undefined {
  const kids = Array.from(shell.children)
  const at = kids.findIndex((c) => c === strip || c.contains(strip))
  return kids
    .slice(0, Math.max(0, at))
    .filter((c): c is HTMLElement => c instanceof HTMLElement && getComputedStyle(c).position === 'sticky')
    .pop()
}

/** A header that ASKS to be pinned (CockpitHeader `pinned` renders `data-pin`), before the strip. */
function pinCandidate(shell: HTMLElement, strip: HTMLElement): HTMLElement | undefined {
  const kids = Array.from(shell.children)
  const at = kids.findIndex((c) => c === strip || c.contains(strip))
  return kids.slice(0, Math.max(0, at)).find((c): c is HTMLElement => c instanceof HTMLElement && c.hasAttribute('data-pin'))
}

/** How much of the shell's scrollport must stay free for what scrolls (the scope and the panes) before
 *  a header may pin itself, in the shell's em. */
export const PIN_ROOM_EM = 6

/** PIN THE HEADER ONLY WHERE THE THREE PARKED SURFACES FIT. Pinned, the header parks at the top, the
 *  strip under it and the dock at the bottom; on a small window at a large zoom (900×600 or 700×800 at
 *  175 %) the three are taller than the scrollport, and the header — first and topmost — sat over Stop
 *  TX, Send and Esc/Stop at every scroll position (measured). So the header is pinned (`data-pin="on"`)
 *  only while its padding, its height, the strip's and the dock's leave PIN_ROOM_EM of the scrollport
 *  free, and otherwise unpinned (`"off"`: it scrolls, as every other cockpit's header does). The
 *  decision reads the header's height, which pinning does not change, so it cannot flap. */
function decidePin(shell: HTMLElement, head: HTMLElement, strip: HTMLElement, dockH: number): void {
  const cs = getComputedStyle(shell)
  const em = parseFloat(cs.fontSize) || 16
  const need = (parseFloat(cs.paddingTop) || 0) + head.offsetHeight + strip.offsetHeight + dockH + PIN_ROOM_EM * em
  const pin = need <= shell.clientHeight ? 'on' : 'off'
  if (head.getAttribute('data-pin') !== pin) head.setAttribute('data-pin', pin)
}

/** How much of the shell's scrollport must stay free between the strip and a dock parked WHOLE, in the
 *  shell's em, before the dock folds. Two: the height of the header's ⊞ Panels button with room to
 *  spare, so scrolling brings it out between the parked surfaces where a pointer can reach it. */
export const FOLD_ROOM_EM = 2

/** THE DOCK'S KEEP ROW: the dock child that parks at the bottom while the dock is folded (Send, PTT,
 *  Esc/Stop and the latch). A dock without one never folds. */
const keepRow = (dock: HTMLElement): HTMLElement | undefined =>
  Array.from(dock.children).find(
    (c): c is HTMLElement => c instanceof HTMLElement && c.classList.contains('cockpit-txdock-keep'),
  )

/** FOLD THE DOCK ONLY WHERE IT DOES NOT FIT WHOLE (`.cockpit-txdock[data-fold='on']`, cockpit-panes.css):
 *  where the strip, the whole dock and FOLD_ROOM_EM are taller than the shell's scrollport (inside its
 *  padding, where sticky boxes park). Returns the dock's UNFOLDED height, which is also what the pin
 *  decision reads. Folded, the dock has no box at all (`display: contents`), so it is unfolded for this
 *  one measurement and folded again before the frame is painted. A shell with no height (a keep-alive
 *  host under `[hidden]`, or jsdom) leaves the state as it is. The keep row carries the same attribute:
 *  that is what parks it.
 *  A HYSTERESIS BAND, ON PURPOSE. Only the dock is unfolded for the measurement; the keep row keeps its
 *  folded padding and border, so a folded dock measures that much taller (7–8 layout px, measured) than
 *  it does unfolded. The window must therefore grow that much more to unfold it than it shrank to fold it
 *  (CW at 900 px wide: folds at 776 px tall, unfolds at 792). That is the anti-flap: at no height can
 *  unfolding make it fit and folding make it not.
 *  RE-MEASURED ONLY WHEN SOMETHING IT READS HAS CHANGED. The strip renders on every snapshot (several a
 *  second), and a folded dock's measurement is two attribute writes and two forced layouts. So a folded
 *  dock keeps the decision until the shell's size or zoom, its padding or font, the strip's height or the
 *  dock's rows (their number, classes and sizes) change; then it is measured again. */
const foldMemo = new WeakMap<HTMLElement, { key: string; natural: number }>()
function foldKey(shell: HTMLElement, strip: HTMLElement, dock: HTMLElement, cs: CSSStyleDeclaration): string {
  const rows = Array.from(dock.children, (c) =>
    c instanceof HTMLElement ? `${c.className}:${c.offsetWidth}x${c.offsetHeight}` : c.tagName,
  )
  return [shell.clientWidth, shell.clientHeight, shell.getBoundingClientRect().height, cs.paddingTop, cs.paddingBottom,
    cs.fontSize, strip.offsetHeight, ...rows].join('|')
}
function decideFold(shell: HTMLElement, strip: HTMLElement, dock: HTMLElement): number {
  const keep = keepRow(dock)
  const was = dock.getAttribute('data-fold') === 'on'
  if (shell.clientHeight === 0) return was ? (foldMemo.get(dock)?.natural ?? 0) : dock.offsetHeight
  const cs = getComputedStyle(shell)
  let natural: number
  const memo = was ? foldMemo.get(dock) : undefined
  const key = was ? foldKey(shell, strip, dock, cs) : ''
  if (memo && memo.key === key) {
    natural = memo.natural
  } else {
    if (was) dock.removeAttribute('data-fold')
    natural = dock.offsetHeight
  }
  const em = parseFloat(cs.fontSize) || 16
  const room = shell.clientHeight - (parseFloat(cs.paddingTop) || 0) - (parseFloat(cs.paddingBottom) || 0)
  const fold =
    keep != null &&
    getComputedStyle(dock).position === 'sticky' &&
    strip.offsetHeight + natural + FOLD_ROOM_EM * em > room
  if (fold) {
    if (dock.getAttribute('data-fold') !== 'on') dock.setAttribute('data-fold', 'on')
    if (keep.getAttribute('data-fold') !== 'on') keep.setAttribute('data-fold', 'on')
    // Keyed on the FOLDED layout, the one the next render reads.
    foldMemo.set(dock, { key: foldKey(shell, strip, dock, cs), natural })
  } else {
    dock.removeAttribute('data-fold')
    keep?.removeAttribute('data-fold')
    foldMemo.delete(dock)
  }
  return natural
}

/** The focus a Tab moves follows its keydown at once; this only allows for a loaded machine. */
const TAB_FOCUS_MS = 100
/** What a press holds the shell for. */
const PRESS_HOLDS = 'input, select, textarea, button, label, [tabindex]'
const PRESS_HOLD_MAX_MS = 1000
const LABEL_HOLD_MS = 100

/** Below this many layout px of column, a notice on the parked row gets a third line. */
const REASON_NARROW_PX = 160

/** What is parked at the bottom, from its top edge down: the keep row, and a notice standing on it (JTTY's, out of flow on
 *  its top edge, in the row's `[role=status]`), which hides what is under it just as the row does. */
function parkedTop(keep: HTMLElement): { top: number; height: number } {
  const k = keep.getBoundingClientRect()
  const reason = keep.querySelector<HTMLElement>(':scope > [role="status"]')?.getBoundingClientRect()
  return reason && reason.height > 0 && reason.top < k.top ? { top: reason.top, height: k.bottom - reason.top } : { top: k.top, height: k.height }
}

/** When a pointer last pressed in a shell, for what a click opens. */
let lastPressAt = -Infinity
/** The longest gap a double click's second press can follow its first (Windows' default). */
const DOUBLE_CLICK_MS = 500
/** Run `reveal`, which scrolls the shell to show something a click or a key just opened or returned to. After a pointer's
 *  click it waits out the second press of a double click, which would otherwise land on whatever the scroll put under the
 *  pointer, a macro key among them. By keyboard there is no second press, so it runs at once. If the operator has scrolled
 *  the shell in that gap, or `el` has gone, it does not run: the operator's scroll is theirs. */
export function revealAfterPress(el: HTMLElement, reveal: () => void): void {
  const since = performance.now() - lastPressAt
  if (since >= DOUBLE_CLICK_MS) {
    reveal()
    return
  }
  const shell = el.closest('main')
  const from = shell?.scrollTop
  setTimeout(() => {
    if (el.isConnected && shell?.scrollTop === from) reveal()
  }, DOUBLE_CLICK_MS - since)
}

/** A FOCUSED CONTROL THE PADDING COULD NOT CLEAR. Where the room between the strip and the keep row is shorter than the
 *  control (JS8 at 900×600 at 175 %: 22 px for its 38 px buttons), no `scroll-padding` fits it and Chrome top-aligns it,
 *  its centre behind the keep row, so Enter would send a key the operator cannot see. Its centre is brought to the middle
 *  of that room instead, after the browser's own reveal. The parked surfaces hold their own focus. */
function centreIfBehind(shell: HTMLElement, strip: HTMLElement, dock: HTMLElement, el: HTMLElement): void {
  const keep = dock.getAttribute('data-fold') === 'on' ? keepRow(dock) : undefined
  if (!keep || keep.contains(el) || strip.contains(el) || el.closest('.panels-menu-pop')) return
  const q = el.getBoundingClientRect()
  const s = strip.getBoundingClientRect()
  const parked = parkedTop(keep).top
  const cy = q.top + q.height / 2
  if (cy < parked && (cy < s.top || cy > s.bottom)) return
  if (parked <= s.bottom || shell.offsetHeight === 0) return
  shell.scrollTop += (cy - (s.bottom + parked) / 2) / (shell.getBoundingClientRect().height / shell.offsetHeight)
}

/** The same, for something opened rather than focused (JS8's heartbeat menu): where it scrolls with a folded dock's
 *  rows, its centre is not left behind the keep row or the strip. */
export function keepInSight(el: HTMLElement): void {
  const shell = el.closest('main')
  const strip = shell?.querySelector<HTMLElement>(':scope > .cockpit-txstrip')
  const dock = shell && Array.from(shell.children).find(isDock)
  if (shell && strip && dock) centreIfBehind(shell, strip, dock, el)
}

/** The cockpit shell the strip stands in: its parent — or, where layout wrappers stand between them
 *  (Phone's stage and the left side's row, 2026-10-03), the nearest ancestor holding the dock, up to
 *  the cockpit's own `<main>`. With no dock that far up, the parent, as before. */
function shellOf(strip: HTMLElement): HTMLElement | null {
  for (let p = strip.parentElement; p; p = p.parentElement) {
    if (Array.from(p.children).some(isDock)) return p
    if (p.tagName === 'MAIN') break
  }
  return strip.parentElement
}

function useDockClearance(ref: RefObject<HTMLDivElement | null>) {
  const apply = () => {
    const strip = ref.current
    const shell = strip && shellOf(strip)
    if (!strip || !shell) return
    const dock = Array.from(shell.children).find(isDock)
    const dockH = dock ? decideFold(shell, strip, dock) : 0
    const candidate = pinCandidate(shell, strip)
    if (candidate) decidePin(shell, candidate, strip, dockH)
    const head = pinnedAbove(shell, strip)
    // What is parked at the bottom: the whole dock, or — folded — its keep row alone.
    const keep = dock?.getAttribute('data-fold') === 'on' ? keepRow(dock) : undefined
    strip.style.setProperty('--cockpit-txstrip-bottom',
      dock && getComputedStyle(dock).position === 'sticky' ? `${keep ? keep.offsetHeight : dock.offsetHeight}px` : '0px')
    if (keep) shell.style.setProperty('--cockpit-txdock-keep-h', `${Math.ceil(parkedTop(keep).height)}px`)
    else shell.style.removeProperty('--cockpit-txdock-keep-h')
    // A notice standing on the parked row's top edge (JTTY's) keeps clear of Stop TX: the column to its right.
    const stop = strip.querySelector<HTMLElement>('.op-btn.stop')
    if (keep && stop && keep.offsetWidth > 0) {
      const k = keep.getBoundingClientRect()
      const z = k.width / keep.offsetWidth
      const s = stop.getBoundingClientRect()
      keep.style.setProperty('--cockpit-txdock-reason-left', `${Math.max(0, s.right - k.left) / z + 4}px`)
      // About two lines, so the room left for what scrolls stays usable; three in a narrow column, or even "TX is off"
      // is cut. The whole text is in the notice's tooltip and the toast its click raises.
      keep.style.setProperty('--cockpit-txdock-reason-lines', (k.right - s.right) / z < REASON_NARROW_PX ? '3' : '2')
      // Its height, once it is clamped to those lines (the sheet applies them before this reads).
      const reason = keep.querySelector<HTMLElement>(':scope > [role="status"]')
      if (reason) keep.style.setProperty('--cockpit-txdock-reason-room', `${reason.offsetHeight}px`)
      else keep.style.removeProperty('--cockpit-txdock-reason-room')
    } else if (dock) {
      keepRow(dock)?.style.removeProperty('--cockpit-txdock-reason-left')
      keepRow(dock)?.style.removeProperty('--cockpit-txdock-reason-lines')
      keepRow(dock)?.style.removeProperty('--cockpit-txdock-reason-room')
    }
    // Only where something is pinned above: every other cockpit's strip keeps exactly the inline style
    // it always had (the sheet's fallback is 0), so their documents are unchanged.
    if (head) strip.style.setProperty('--cockpit-txstrip-top', `${head.offsetHeight}px`)
    else strip.style.removeProperty('--cockpit-txstrip-top')
    shell.style.setProperty('--cockpit-txstrip-h',
      getComputedStyle(strip).position === 'sticky'
        ? `${Math.ceil(strip.getBoundingClientRect().height + (head ? head.getBoundingClientRect().height : 0))}px`
        : '0px')
  }
  useLayoutEffect(apply)
  useLayoutEffect(() => {
    const strip = ref.current
    const shell = strip && shellOf(strip)
    if (!strip || !shell) return
    const dock = Array.from(shell.children).find(isDock)
    const head = pinCandidate(shell, strip) ?? pinnedAbove(shell, strip)
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(apply)
    observer?.observe(strip)
    if (dock) observer?.observe(dock)
    // Folded, the dock has no box to observe; its keep row is what the strip clears.
    const keep = dock && keepRow(dock)
    if (keep) observer?.observe(keep)
    if (head) observer?.observe(head)
    // The pin and fold decisions are against the shell's own height: a shorter window may unpin the
    // header or fold the dock without any of the parked surfaces changing size.
    if (head || keep) observer?.observe(shell)
    // ONLY A REAL TAB CENTRES. `:focus-visible` was not enough: it matches a text field on a mouse press, so a press re-centred
    // the field under the held pointer and a double click's second press reached a sender (21 send calls, measured), and
    // focusin fires again on a window regain, so the screen jumped under the next click. The focus Tab moves follows its
    // keydown within this task; nothing else counts.
    let tabAt = -Infinity
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Tab') tabAt = e.timeStamp
    }
    const onFocus = (e: FocusEvent) => {
      const el = e.target
      if (dock && el instanceof HTMLElement && e.timeStamp - tabAt < TAB_FOCUS_MS) {
        requestAnimationFrame(() => {
          if (document.activeElement === el) centreIfBehind(shell, strip, dock, el)
        })
      }
    }
    // A PRESS MOVES NOTHING WHILE THE DOCK IS FOLDED. Chrome reveals a field or a <select> on a mouse press, honouring the
    // shell's scroll padding, so the keys moved under the pointer: 87 px on a <select>, 102 px on a field even before the
    // padding (measured). The scroll is held where the press found it until the release. A press on the shell itself is on
    // its scrollbar, and only a control can take focus; a label's focus comes with its click, just after the release.
    let pinnedAt: number | null = null
    let letGo = 0
    const unpin = () => {
      pinnedAt = null
      clearTimeout(letGo)
    }
    const hold = () => {
      if (pinnedAt !== null && shell.scrollTop !== pinnedAt) shell.scrollTop = pinnedAt
    }
    const onPress = (e: PointerEvent) => {
      lastPressAt = performance.now()
      const t = e.target
      if (dock?.getAttribute('data-fold') !== 'on' || !(t instanceof Element) || t === shell || !t.closest(PRESS_HOLDS)) return
      pinnedAt = shell.scrollTop
      clearTimeout(letGo)
      // A <select>'s native list can swallow the release.
      letGo = window.setTimeout(unpin, PRESS_HOLD_MAX_MS)
      labelHeld = t.closest('label') != null
    }
    let labelHeld = false
    const onRelease = () => {
      if (pinnedAt === null) return
      hold()
      clearTimeout(letGo)
      if (labelHeld) letGo = window.setTimeout(unpin, LABEL_HOLD_MS)
      else unpin()
    }
    window.addEventListener('keydown', onKey, true)
    shell.addEventListener('focusin', onFocus)
    shell.addEventListener('pointerdown', onPress, true)
    shell.addEventListener('scroll', hold)
    window.addEventListener('pointerup', onRelease, true)
    window.addEventListener('pointercancel', onRelease, true)
    return () => {
      observer?.disconnect()
      unpin()
      window.removeEventListener('keydown', onKey, true)
      shell.removeEventListener('focusin', onFocus)
      shell.removeEventListener('pointerdown', onPress, true)
      shell.removeEventListener('scroll', hold)
      window.removeEventListener('pointerup', onRelease, true)
      window.removeEventListener('pointercancel', onRelease, true)
      shell.style.removeProperty('--cockpit-txstrip-h')
      shell.style.removeProperty('--cockpit-txdock-keep-h')
    }
    // `apply` reads only refs and the DOM; the observer is set up once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref])
}
