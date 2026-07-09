import { useState } from 'react'

/** Format a dial frequency (MHz) for DISPLAY — 4 decimals (100 Hz resolution). */
export function formatDialMhz(mhz: number): string {
  return mhz.toFixed(4)
}
/** "Essentially unchanged" tolerance for a committed edit (MHz) — 5 Hz. Skips a no-op QSY. */
const UNCHANGED_EPS = 5e-6

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
}: Props) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const canEdit = editable && !disabled

  const startEdit = () => {
    if (!canEdit) return
    // Seed at 10 Hz (finer than the 100 Hz display) so re-committing an unchanged value doesn't
    // round the rig off-frequency.
    setDraft(dialMhz.toFixed(5))
    setEditing(true)
  }
  const commit = () => {
    const v = parseFloat(draft.trim().replace(',', '.'))
    setEditing(false)
    // Skip a no-op commit (opened + Enter/blur without changing) so it never fires a spurious QSY.
    if (Number.isFinite(v) && v > 0 && Math.abs(v - dialMhz) >= UNCHANGED_EPS) onCommit?.(v)
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
      title={title ?? (canEdit ? 'Click to enter a frequency (MHz)' : 'Dial frequency (MHz)')}
      role={canEdit ? 'button' : undefined}
      tabIndex={canEdit ? 0 : undefined}
      onClick={startEdit}
      onKeyDown={(e) => {
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
      <span className="readout-val">{formatDialMhz(dialMhz)}</span>
      <span className="readout-unit">MHz</span>
      {band && <span className="band-chip active">{band}</span>}
    </span>
  )
}
