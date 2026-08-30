// THE FIELD DAY COCKPIT'S PUSH-TO-TALK ROW — Phone's row, LIFTED VERBATIM.
//
// Every word, class, handler wiring and state branch below is copied character for character
// from `PhoneCockpit.tsx`'s `.ph-ptt-row` (the block its own comment calls the cockpit's
// stop-line census). It is a SECOND SITE for the app's most safety-critical markup, and the
// only defensible way to add one is to copy it rather than to re-derive it: the four labels
// are what `components/stop-line.test.tsx` matches by accessible name, the three-state answer
// (#81) is what makes "PUSH TO TALK over a rig that cannot be keyed" reportable, and the
// pointer-leave unkey is what saves a held button bumped off a tent table at 2 AM.
//
// WHAT IS NOT HERE, and deliberately: the PTT STATE. `keyed` and `lock` live in FdCockpit,
// because the voice-keyer pane beside this row must know whether a live over is in progress
// (playing a canned message over a held mic key fights it), and because the window Space
// listener and the force-unkey-on-unmount belong with the shell that owns the cockpit's
// lifetime. Those handlers are lifted verbatim too — see FdCockpit's `key`/`onPttDown`/
// `onPttUp` and the Space effect. This file is the row's MARKUP and nothing else.
//
// ⚠️ THIS FILE IS ON THE **PARTIAL** LIST (i18n/hardcoded-strings.test.ts), for exactly the
// reason `PhoneCockpit.tsx` is: nothing in this row is migrated. PTT is this cockpit's
// stop-line census and the sweeps find it by ACCESSIBLE NAME; its tooltip IS that control's
// description, naming the switch that is down and the mic the operator talks on; the Lock
// toggle beside it decides whether the window's Space keyup is a PTT release at all. All of
// it moves in the transmit-path batch, with the stop-line sweeps re-run — and it moves
// TOGETHER with Phone's copy or the two rows would come to read as different controls.
// That deferral is the whole reason this row is its own file: FdCockpit is born MIGRATED.

export function FdPttRow({
  txAllowed,
  txEnabled,
  keyed,
  lock,
  onLockChange,
  onPttDown,
  onPttUp,
  onToggleKey,
  fdExchange,
}: {
  /** `snap.radio.txAllowed` — licence privileges for this dial and mode. */
  txAllowed: boolean
  /** `snap.radio.txEnabled` — the TX-enable latch. */
  txEnabled: boolean
  /** Is the rig keyed by THIS row right now (the shell's `keyed` state). */
  keyed: boolean
  /** Hands-free: click once to key, again to unkey. */
  lock: boolean
  onLockChange: (on: boolean) => void
  onPttDown: () => void
  onPttUp: () => void
  /** Lock-mode keyboard toggle (Enter/Space on the focused button). */
  onToggleKey: () => void
  /** "3A WI" — what the operator reads aloud. Empty ⇒ the Give chip is not drawn. */
  fdExchange: string
}) {
  return (
    <div className="ph-ptt-row">
      {fdExchange && (
        <span
          className="ph-fd-give"
          title="Field Day exchange — read this to the station you're working (your class + section)."
        >
          <span className="ph-fd-give-lbl">Give</span>
          <span className="ph-fd-give-exch mono">{fdExchange}</span>
        </span>
      )}
      {/* THE BUTTON ANSWERS THREE STATES, NOT TWO (#81). The engine drops a key on either of
          `tx_enabled` / `tx_allowed` and they are different facts with different remedies:
          the lock is a licence question (change band or licence class), TX-off is a switch
          the operator himself, the watchdog or a UDP HaltTx put down. One label covering both
          is what made #81 unreportable — "PUSH TO TALK" over a rig that could not be keyed,
          with the operator left to guess between his cable and his software. */}
      <button
        type="button"
        className={`ph-ptt${keyed ? ' keyed' : ''}${txAllowed && !txEnabled ? ' txoff' : ''}`}
        aria-pressed={lock ? keyed : undefined}
        onPointerDown={onPttDown}
        onPointerUp={onPttUp}
        onPointerLeave={onPttUp}
        // Keyboard: in Lock (hands-free) mode a focused Enter/Space toggles TX.
        // preventDefault suppresses the synthetic click so it can't double-fire
        // against the pointer handlers on a real mouse click. In hold mode the
        // window-level Space handler owns push-to-talk (a keypress can't hold).
        onKeyDown={(e) => {
          if (lock && (e.key === 'Enter' || e.key === ' ')) {
            e.preventDefault()
            onToggleKey()
          }
        }}
        disabled={!txAllowed}
        title={
          !txAllowed
            ? 'TX locked — outside your license privileges (pick a band, or change your license in Settings)'
            : !txEnabled
              ? "Transmit is switched OFF, so keying is discarded — Stop TX, the TX watchdog or a logger's Halt Tx turns it off. Click to enable transmit, then hold to talk. You talk on the rig's mic."
              : "Hold to talk (or Space). Toggle 'Lock' for hands-free (then Enter keys/unkeys). You talk on the rig's mic."
        }
      >
        {!txAllowed
          ? '🔒 TX LOCKED'
          : !txEnabled
            ? '■ TX OFF — CLICK TO ENABLE'
            : keyed
              ? 'ON AIR — release to stop'
              : 'PUSH TO TALK'}
      </button>
      <label className="ph-lock" title="Hands-free: click PTT once to key, again to unkey">
        <input type="checkbox" checked={lock} onChange={(e) => onLockChange(e.target.checked)} />
        <span>Lock</span>
      </label>
    </div>
  )
}
