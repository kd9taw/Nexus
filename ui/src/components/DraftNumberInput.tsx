import { useState, type InputHTMLAttributes } from 'react'
import { parseOperatorNumber } from '../numInput'

/**
 * A `<input type="number">` with a range that keeps what the operator TYPES until they are done.
 *
 * Clamping a controlled number box on every keystroke makes most values untypeable: in a
 * 300–1200 Hz pitch box the "6" of "650" became 300 at once, and the next keys built 3005 → 1200,
 * so the box could only ever hold its two ends (the spinner worked, which is why it went unseen).
 * Here the typed text stays in the box while it is edited; `onValue` hears it only once it is a
 * number inside `min..max` (so a spinner or arrow step still applies at once), and on blur or
 * Enter whatever is in the box is clamped into range and committed. Text that is not a number
 * (blank, "-") is dropped on blur and the box shows the stored value again; Escape drops it too.
 */
export function DraftNumberInput({
  value,
  onValue,
  onBlur,
  onKeyDown,
  ...rest
}: {
  value: number
  min: number
  max: number
  onValue: (v: number) => void
} & Omit<InputHTMLAttributes<HTMLInputElement>, 'value' | 'min' | 'max' | 'onChange' | 'type'>) {
  // `min`/`max` stay in `rest` so the box renders its attributes in the caller's order.
  const { min, max } = rest
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    setDraft(null)
    const n = parseOperatorNumber(draft)
    if (!Number.isFinite(n)) return
    const v = Math.max(min, Math.min(max, n))
    if (v !== value) onValue(v)
  }
  return (
    <input
      type="number"
      {...rest}
      value={draft ?? value}
      onChange={(e) => {
        setDraft(e.target.value)
        const n = parseOperatorNumber(e.target.value)
        if (Number.isFinite(n) && n >= min && n <= max) onValue(n)
      }}
      onBlur={(e) => {
        commit()
        onBlur?.(e)
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit()
        else if (e.key === 'Escape') setDraft(null)
        onKeyDown?.(e)
      }}
    />
  )
}
