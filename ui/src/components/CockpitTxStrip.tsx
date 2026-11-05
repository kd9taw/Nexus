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
 *    at the 150–175 % pins. A dock that is not sticky clears nothing.
 *  · `--cockpit-txstrip-top` on the strip: the height of a PINNED header (`.cockpit-header-pinned`,
 *    sticky at `top: 0` — SSTV, RTTY, PSK). A strip parked at 0 would park OVER it and cover the
 *    dial and the band; it parks right under it instead. A header that is not sticky clears nothing.
 *  · `--cockpit-txstrip-h` on the SHELL: what is parked at the top while the strip is sticky — the
 *    strip's rendered height, plus a pinned header's — which the shell's `scroll-padding-top` reads
 *    so whatever is scrolled to the start lands below it. RENDERED, not offsetHeight: under the UI
 *    zoom Chrome applies `scroll-padding` unzoomed (measured: at 175 % a 42 px offsetHeight padded
 *    42 screen px under a 74 px strip), and more padding than the strip only scrolls a target a
 *    little lower.
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
function decidePin(shell: HTMLElement, head: HTMLElement, strip: HTMLElement, dock: HTMLElement | undefined): void {
  const cs = getComputedStyle(shell)
  const em = parseFloat(cs.fontSize) || 16
  const need = (parseFloat(cs.paddingTop) || 0) + head.offsetHeight + strip.offsetHeight + (dock?.offsetHeight ?? 0) + PIN_ROOM_EM * em
  const pin = need <= shell.clientHeight ? 'on' : 'off'
  if (head.getAttribute('data-pin') !== pin) head.setAttribute('data-pin', pin)
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
    const candidate = pinCandidate(shell, strip)
    if (candidate) decidePin(shell, candidate, strip, dock)
    const head = pinnedAbove(shell, strip)
    strip.style.setProperty('--cockpit-txstrip-bottom',
      dock && getComputedStyle(dock).position === 'sticky' ? `${dock.offsetHeight}px` : '0px')
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
    if (head) {
      observer?.observe(head)
      // The pin decision is against the shell's own height: a shorter window may unpin it.
      observer?.observe(shell)
    }
    return () => {
      observer?.disconnect()
      shell.style.removeProperty('--cockpit-txstrip-h')
    }
    // `apply` reads only refs and the DOM; the observer is set up once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ref])
}
