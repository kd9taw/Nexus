// The typed audio-offset entry: WSJT-X's Rx/Tx Hz spinners. FT8/FT4's header has had it since
// the cockpit was built; JS8 takes the same field (#448), so it lives here instead of inside
// OperateCockpit. A display of the engine's offset that commits a typed one: the engine owns
// the clamp per tier and, on JS8, the hold on the TX offset while a message waits or goes out.
import { useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { clampOffsetHz } from '../txMessages'

/** The two fields' labels: direction tokens that also read out in each field's accessible
 *  name. `Hz` is a unit symbol. */
export const DF_RX = 'Rx'
export const DF_TX = 'Tx'
const HZ_UNIT = 'Hz'

/**
 * A compact labeled DF (audio offset) entry: tracks the snapshot value while
 * idle, commits on Enter/blur (rounded + clamped 200–4000 Hz), and reverts on
 * garbage input. Enter just blurs — the single commit happens in onBlur.
 *
 * `disabled` (with `title` saying why) is for an offset the engine would refuse to move: a typed
 * value it then ignored would sit in the box as if it had been taken.
 */
export function DfField({
  label,
  hz,
  onCommit,
  disabled = false,
  title,
}: {
  label: string
  hz: number
  onCommit: (hz: number) => void
  disabled?: boolean
  title?: string
}) {
  const [text, setText] = useState(() => String(Math.round(hz)))
  const [editing, setEditing] = useState(false)
  const editingRef = useRef(editing)
  editingRef.current = editing
  // Track the live prop ONLY when it actually changes — keying the effect on
  // `editing` too made the blur's clamped value flash back to the stale prop
  // for the IPC round-trip (commit sets text + editing in the same render).
  useEffect(() => {
    if (!editingRef.current) setText(String(Math.round(hz)))
  }, [hz])
  const commit = () => {
    setEditing(false)
    const n = Number(text)
    if (text.trim() !== '' && Number.isFinite(n)) {
      const clamped = clampOffsetHz(n)
      setText(String(clamped))
      if (clamped !== Math.round(hz)) onCommit(clamped)
    } else {
      setText(String(Math.round(hz))) // revert garbage
    }
  }
  return (
    <label className="df-field" title={title ?? t('operate.header.df.title', { label })}>
      <span className="df-label">{label}</span>
      <input
        type="number"
        inputMode="numeric"
        min={200}
        max={4000}
        step={1}
        value={text}
        disabled={disabled}
        aria-label={t('operate.header.df.aria', { label })}
        onFocus={() => setEditing(true)}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            ;(e.target as HTMLInputElement).blur()
          }
        }}
      />
      <span className="df-unit">{HZ_UNIT}</span>
    </label>
  )
}
