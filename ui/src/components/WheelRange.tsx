// A SLIDER THE WHEEL MOVES (#384): an `<input type="range">` that a mouse wheel or a trackpad moves too.
//
// The operator's ask (KR4FQG, 2026-09-28): "could the sliders also change with a mouse scroll?" One
// notch moves the slider by `wheelStep`, which its host picks per slider, and the value reaches the
// slider's own `onChange` as a drag's does: written to the element, then an `input` event. So every
// gate a drag meets (`disabled`, a capability check, the rig's power ceiling behind the handler) is
// met the same way, and nothing in this file knows what a slider controls. It renders exactly the
// `<input>` it replaces, so no document a golden holds changes.
//
// ⭐ A WHEEL BURST IS A DRAG. The first wheel event the slider takes calls the slider's own
// `onPointerDown`, and once the wheel has rested for WHEEL_REST_MS, its `onPointerUp`. Those two
// ends are what stop the rig's read-back pulling a slider back mid-gesture (the hosts' `…Dragging`
// flags), and what make a Remote browser's movement ONE draft submitted once (`useRadioLevels`, the
// Sub strip), so the wheel inherits both rather than copying either. They are typed `() => void` so
// no host can hand this component an end that reads a pointer event the wheel never has.
//
// ⚠️ THE SLIDER UNDER THE POINTER TAKES THE WHEEL, NOT THE FOCUSED ONE. Focus alone would let a
// scroll over the band list or the log move an AF gain that was clicked a minute ago.
//
// ⚠️ A SCROLL THAT BEGAN ELSEWHERE KEEPS SCROLLING. The pane region scrolls on a narrow window, and
// a slider passing under the pointer mid-scroll would otherwise stop the scroll and take the rest
// of it: AF gain to full on the way down the page. A wheel event the page took less than
// WHEEL_LATCH_MS ago latches the gesture to the page, as a browser latches a scroll to its first
// scroller.
//
// ⚠️ A NATIVE, NON-PASSIVE LISTENER. React's `onWheel` is passive, so it cannot stop the pane
// scrolling under a slider being wheeled (the reason `useWheelTune` attaches its own). The event
// also stops at the slider, so a slider inside the scope strip (the scope's G and Z) is not
// wheel-tuning the rig underneath it at the same time.
import { useEffect, useRef, type InputHTMLAttributes } from 'react'

/** Accumulated scroll, in pixel-equivalents, per notch: `useWheelTune`'s normalisation, so a
 *  trackpad's many small events and a mouse's notches move a slider alike. */
const PX_PER_NOTCH = 100
/** At most this many notches from ONE event, so a free-spinning wheel's flick is bounded. */
const MAX_NOTCHES_PER_EVENT = 4
/** The burst ends, as a drag does on release, once the wheel has rested this long. */
export const WHEEL_REST_MS = 400
/** A wheel event the page took this recently keeps the gesture on the page. */
export const WHEEL_LATCH_MS = 300

/** Decimal places in a step attribute ("0.05" → 2), so the next value lands on its grid exactly. */
function decimals(step: string): number {
  const dot = step.indexOf('.')
  return dot < 0 ? 0 : step.length - dot - 1
}

/** `value` put on the slider's own grid: its step from its min, inside its range. */
function onGrid(el: HTMLInputElement, value: number): number {
  const min = el.min === '' ? 0 : Number(el.min)
  const max = el.max === '' ? 100 : Number(el.max)
  const inRange = Math.min(max, Math.max(min, value))
  if (el.step === 'any') return inRange
  const step = el.step === '' ? 1 : Number(el.step)
  const snapped = min + Math.round((inRange - min) / step) * step
  return Number(Math.min(max, Math.max(min, snapped)).toFixed(decimals(el.step === '' ? '1' : el.step)))
}

/** Hand `value` to the slider the way the browser's own drag does. React keeps the value it last
 *  wrote on the element itself, so writing through the prototype's setter is what it reads as a
 *  change, and the `input` event is what fires `onChange` on a range input. */
function setLikeADrag(el: HTMLInputElement, value: number) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set?.call(el, String(value))
  el.dispatchEvent(new Event('input', { bubbles: true }))
}

type Props = Omit<InputHTMLAttributes<HTMLInputElement>, 'onPointerDown' | 'onPointerUp'> & {
  /** How far one notch moves the slider, in its own units. Omitted: no wheel, a plain range input. */
  wheelStep?: number
  /** The two ends of a drag, which a wheel burst shares (see the header). */
  onPointerDown?: () => void
  onPointerUp?: () => void
}

export function WheelRange({ wheelStep, onPointerDown, onPointerUp, ...input }: Props) {
  const ref = useRef<HTMLInputElement>(null)
  // The listener attaches once; it reads the step and the drag's ends as they are now.
  const live = useRef({ wheelStep, onPointerDown, onPointerUp })
  live.current = { wheelStep, onPointerDown, onPointerUp }
  const wheel = wheelStep != null && wheelStep > 0
  useEffect(() => {
    const el = ref.current
    if (!el || !wheel) return
    let pageWheelAt = Number.NEGATIVE_INFINITY
    let accum = 0
    let rest: number | null = null // the burst's release timer; null between bursts
    const end = () => {
      rest = null
      accum = 0
      live.current.onPointerUp?.()
    }
    // Bubble phase at the window: a wheel event a slider took stops there, so this sees only the
    // ones the page took.
    const onPageWheel = () => {
      pageWheelAt = performance.now()
    }
    const onWheel = (e: WheelEvent) => {
      const step = live.current.wheelStep
      if (!step || el.disabled) return
      if (rest == null && performance.now() - pageWheelAt < WHEEL_LATCH_MS) return
      // The dominant axis: Shift+wheel arrives as horizontal scroll in Chromium and WebKit.
      const raw = Math.abs(e.deltaY) >= Math.abs(e.deltaX) ? e.deltaY : e.deltaX
      if (raw === 0) return
      e.preventDefault()
      e.stopPropagation()
      if (rest == null) live.current.onPointerDown?.()
      else window.clearTimeout(rest)
      rest = window.setTimeout(end, WHEEL_REST_MS)
      accum += e.deltaMode === 1 ? raw * PX_PER_NOTCH : e.deltaMode === 2 ? raw * PX_PER_NOTCH * 8 : raw
      const whole = Math.trunc(accum / PX_PER_NOTCH)
      if (whole === 0) return // still short of a notch
      accum -= whole * PX_PER_NOTCH
      const notches = Math.max(-MAX_NOTCHES_PER_EVENT, Math.min(MAX_NOTCHES_PER_EVENT, whole))
      const from = Number(el.value)
      // Scroll up (a negative delta) moves the slider up, as it tunes the dial up.
      const to = onGrid(el, from - notches * step)
      if (to !== from) setLikeADrag(el, to)
    }
    window.addEventListener('wheel', onPageWheel, { passive: true })
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => {
      window.removeEventListener('wheel', onPageWheel)
      el.removeEventListener('wheel', onWheel)
      // Unmounted or switched off mid-burst: the drag still ends, or its host's flag stays up.
      if (rest != null) {
        window.clearTimeout(rest)
        end()
      }
    }
  }, [wheel])
  return <input ref={ref} {...input} onPointerDown={onPointerDown} onPointerUp={onPointerUp} />
}
