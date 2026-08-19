import { useEffect, useRef, useState } from 'react'
import { announce } from '../announce'
import { parseOperatorNumber } from '../numInput'

/** Format a dial frequency (MHz) for DISPLAY — 4 decimals (100 Hz resolution). */
export function formatDialMhz(mhz: number): string {
  return mhz.toFixed(4)
}
/** "Essentially unchanged" tolerance for a committed edit (MHz) — 5 Hz. Skips a no-op QSY. */
const UNCHANGED_EPS = 5e-6

/** One character of a formatted dial plus the DECADE (power of ten, in Hz) that one wheel notch
 *  on it moves. `decade: null` = not a digit (the decimal point) — inert by construction. */
export interface DialDigit {
  ch: string
  decade: number | null
}

/**
 * Tag each digit of a formatted dial with its decade, read off the STRING'S OWN SHAPE.
 *
 * Never a fixed index: the integer part is 1 char on 160 m (`1.8340`) and 4 on 23 cm
 * (`1296.1740`), so "the 1 MHz digit is at index 1" is wrong on almost every band. The digit
 * immediately left of the '.' is 10^6 Hz; each step left is one decade up, each step right of
 * the '.' one decade down. `formatDialMhz` is 4 decimals, so 10^2 (100 Hz) is the finest digit
 * that exists.
 */
export function dialDigits(text: string): DialDigit[] {
  const dot = text.indexOf('.')
  const intLen = dot < 0 ? text.length : dot
  return [...text].map((ch, i) =>
    ch < '0' || ch > '9'
      ? { ch, decade: null }
      : { ch, decade: i < intLen ? 6 + (intLen - 1 - i) : 6 - (i - intLen) },
  )
}

/** Keys the per-digit keyboard equivalent claims (and therefore must not let bubble). */
const ARROW_KEYS = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown']

/** A decade as the operator reads it — the label on the hover tooltip and the announcement. */
function stepLabel(decade: number): string {
  const hz = 10 ** decade
  return hz >= 1e6 ? `${hz / 1e6} MHz` : hz >= 1e3 ? `${hz / 1e3} kHz` : `${hz} Hz`
}

interface Props {
  dialMhz: number
  /** Band chip label rendered beside the number; omit to hide (e.g. when the caller shows its own). */
  band?: string
  /** hero = big focal readout (var(--fs-display)); compact = strip-sized (17px). */
  size?: 'hero' | 'compact'
  /** When set, clicking (or Enter/Space) swaps to a MHz entry input; onCommit fires on Enter. */
  editable?: boolean
  onCommit?: (mhz: number) => void
  /** Commit a typed value on blur too (not just Enter). Use for STAGED forms (Settings), where
   * clicking Save blurs the field — Enter-only would silently discard it. Off for a live rig
   * control, where blur should cancel (matching the old goto field). */
  commitOnBlur?: boolean
  /** Out-of-band / TX-inhibited — the number renders in the TX (red) color. */
  txBlocked?: boolean
  title?: string
  /** Disable entry (e.g. CAT down) — still shows the number, just not clickable. */
  disabled?: boolean
  /** PER-DIGIT tuning (the main dial only). Each digit becomes its own hit region carrying
   * `data-decade` — the power of ten in Hz one wheel notch on it moves — and the readout gains
   * Left/Right digit selection + Up/Down stepping for the keyboard.
   *
   * OFF by default, and that is load-bearing: the WHEEL listener lives in the PARENT (one
   * listener, one coalescer — see CockpitHeader), and the readouts the operator excluded
   * (TopBar, Settings, the memory rows) sit inside scrollable lists where capturing the wheel
   * would trap normal page scrolling. With it off this renders the single text node it always
   * did, byte for byte. */
  digitTune?: boolean
  /** Apply a signed Hz delta — the KEYBOARD half of per-digit tuning. Route it through the same
   * coalesced target the wheel uses (CockpitHeader passes `useWheelTune`'s applier): two
   * optimistic targets on one dial is the failure this whole design avoids. */
  onTuneHz?: (hz: number) => void
}

/**
 * The shared, prominent frequency readout for all three mode families (digital, CW, Phone): big
 * accent-colored MHz, monospace + tabular-nums so digits don't jitter while tuning. When `editable`,
 * activating it (click or Enter/Space) swaps in a decimal MHz input — Enter commits, Esc/blur
 * cancels — the same contract the per-cockpit "Go to MHz" fields had, now unified in one place.
 */
export function FrequencyReadout({
  dialMhz,
  band,
  size = 'hero',
  editable = false,
  onCommit,
  commitOnBlur = false,
  txBlocked = false,
  title,
  disabled = false,
  digitTune = false,
  onTuneHz,
}: Props) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const canEdit = editable && !disabled
  const text = formatDialMhz(dialMhz)
  const digits = digitTune ? dialDigits(text) : null
  // The digit the KEYBOARD spins (the wheel takes the one under the pointer instead). null until
  // the operator asks for one, so a mouse user never sees a selection they did not make.
  const [selDecade, setSelDecade] = useState<number | null>(null)
  // Announce the DIAL, never the digit: a spin that reads "four… five… six" is a stutter, and
  // the bus already coalesces a burst into one utterance (announce.ts). Fires off the committed
  // dial, so it says where the rig actually landed, not where we aimed.
  const kbTunedRef = useRef(false)
  useEffect(() => {
    if (!kbTunedRef.current) return
    kbTunedRef.current = false
    announce(`${formatDialMhz(dialMhz)} megahertz`)
  }, [dialMhz])

  const startEdit = () => {
    if (!canEdit) return
    // Seed at 10 Hz (finer than the 100 Hz display) so re-committing an unchanged value doesn't
    // round the rig off-frequency.
    setDraft(dialMhz.toFixed(5))
    setEditing(true)
  }
  const commit = () => {
    // The comma-decimal handling used to live HERE and only here, as a bare
    // `.replace(',', '.')` — correct, undocumented, and unshared, so the three other numeric
    // input sites in the tree each got it wrong (see `numInput.ts`). Same behaviour, one
    // implementation, and now stricter: a valid PREFIX like `14.0.74` is refused instead of
    // silently tuning the rig to 14 MHz.
    const v = parseOperatorNumber(draft)
    setEditing(false)
    // Skip a no-op commit (opened + Enter/blur without changing) so it never fires a spurious QSY.
    if (Number.isFinite(v) && v > 0 && Math.abs(v - dialMhz) >= UNCHANGED_EPS) onCommit?.(v)
  }

  // ── The keyboard equivalent of hover-and-scroll ───────────────────────────────────────────
  // A screen-reader user cannot hover. The readout is ALREADY one tab stop when editable, and it
  // stays one: ARIA gives `button` presentational children, so nine focusable digits inside it
  // would be both a violation and nine tab stops ahead of Stop TX in every cockpit header
  // (PanelsMenu.tsx:139 records what that costs). Left/Right pick the digit, Up/Down spin it.
  const decades = digits?.flatMap((d) => (d.decade == null ? [] : [d.decade])) ?? [] // high→low
  const canTuneDigits = canEdit && digits != null && onTuneHz != null && decades.length > 0
  const stepDigit = (key: string) => {
    if (!canTuneDigits) return
    const hi = decades[0]
    const lo = decades[decades.length - 1]
    // No selection yet ⇒ the finest digit shown (100 Hz — the display floor, and the strip's
    // default step). The FIRST arrow only selects; it never also moves the selection.
    const cur = selDecade == null ? lo : Math.min(hi, Math.max(lo, selDecade))
    if (key === 'ArrowLeft' || key === 'ArrowRight') {
      const next =
        selDecade == null ? cur : Math.min(hi, Math.max(lo, cur + (key === 'ArrowLeft' ? 1 : -1)))
      setSelDecade(next)
      announce(`${stepLabel(next)} digit`)
      return
    }
    setSelDecade(cur)
    kbTunedRef.current = true
    // Exactly `10 ** decade` Hz — the carry a real VFO does falls out of the arithmetic
    // (14.1990 + 1 kHz = 14.2000), with no digit-by-digit logic to get wrong.
    onTuneHz(10 ** cur * (key === 'ArrowUp' ? 1 : -1))
  }

  if (editing) {
    return (
      <span className={`readout ${size} editing`}>
        <input
          className="readout-input mono"
          inputMode="decimal"
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            // Keep typing keys (Space, Esc, PgUp…) from reaching the cockpit's global shortcuts
            // (spacebar-PTT, Esc-abort) while entering a frequency.
            e.stopPropagation()
            if (e.key === 'Enter') {
              e.preventDefault()
              commit()
            } else if (e.key === 'Escape') {
              setEditing(false)
            }
          }}
          onBlur={() => (commitOnBlur ? commit() : setEditing(false))}
          aria-label="Dial frequency (MHz)"
        />
        <span className="readout-unit">MHz</span>
      </span>
    )
  }

  return (
    <span
      className={`readout ${size}${txBlocked ? ' blocked' : ''}${canEdit ? ' editable' : ''}`}
      title={
        title ??
        (canTuneDigits
          ? 'Scroll a digit to tune it (100 Hz … 10 MHz) · ←/→ pick a digit, ↑/↓ spin it · click to type a frequency (MHz)'
          : canEdit
            ? 'Click to enter a frequency (MHz)'
            : 'Dial frequency (MHz)')
      }
      role={canEdit ? 'button' : undefined}
      tabIndex={canEdit ? 0 : undefined}
      onClick={startEdit}
      onKeyDown={(e) => {
        if (canTuneDigits && ARROW_KEYS.includes(e.key)) {
          e.preventDefault()
          // Same reason as the Space guard below: an arrow this readout handled must never ALSO
          // reach the cockpit's window-level shortcuts. Arrows are free there today (CW uses
          // PageUp/PageDown for WPM, Phone uses Space) — that is luck, not a contract.
          e.stopPropagation()
          stepDigit(e.key)
          return
        }
        if (canEdit && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault()
          // CRITICAL: stop Space from reaching the Phone cockpit's window-level spacebar-PTT — the
          // readout is a role=button span (not exempted by its INPUT/TEXTAREA field check), so
          // without this, Space here would key the transmitter (and mounting the edit input moves
          // focus so the keyup is swallowed → a stuck open carrier).
          e.stopPropagation()
          startEdit()
        }
      }}
    >
      <span className="readout-val">
        {/* One `.map()` inside ONE JSX expression: no whitespace text nodes between the digits,
            which is the only way span-splitting could open a visible gap. The '.' stays a bare
            string child — smaller DOM, and it is deliberately not a hit region. */}
        {digits
          ? digits.map((d, i) =>
              d.decade == null ? (
                d.ch
              ) : (
                <span
                  key={i}
                  className={`readout-digit${d.decade === selDecade ? ' sel' : ''}`}
                  data-decade={d.decade}
                >
                  {d.ch}
                </span>
              ),
            )
          : text}
      </span>
      <span className="readout-unit">MHz</span>
      {band && <span className="band-chip active">{band}</span>}
    </span>
  )
}
