// Whether resting the pointer on a call opens its station card (components/CallCard.tsx). Default
// OFF: a click, or Enter on a focused call, always opens it, and a card that pops up under a pointer
// crossing a busy list would cover the rows the operator is reading.
//
// APP-GLOBAL, bare localStorage, the same classification as the Logbook globe
// (features/logbookGlobe.ts): a standing statement about how this operator wants the app to behave,
// not a property of one window. The switch lives in Settings ▸ Logbook ▸ Logbook display; the
// card's triggers read it at the moment the pointer arrives, so there is nothing to subscribe to
// there.

import { useEffect, useState } from 'react'

export const CALL_CARD_HOVER_KEY = 'nexus.callCard.hover'
const CHANGED_EVENT = 'nexus-call-card-hover-changed'

/** Is hover-to-open on? Off unless the operator turned it on. */
export function callCardHoverOn(): boolean {
  try {
    return localStorage.getItem(CALL_CARD_HOVER_KEY) === 'on'
  } catch {
    return false // blocked storage: keep the default
  }
}

/** Turn hover-to-open on or off, and tell every mounted reader in this window at once. */
export function setCallCardHover(on: boolean): void {
  try {
    localStorage.setItem(CALL_CARD_HOVER_KEY, on ? 'on' : 'off')
  } catch {
    /* blocked storage: the change still applies to the switch for this session */
  }
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent<boolean>(CHANGED_EVENT, { detail: on }))
  }
}

/** The current setting for the Settings switch, re-rendering when it changes here or in another window. */
export function useCallCardHover(): [boolean, (on: boolean) => void] {
  const [on, setOn] = useState<boolean>(callCardHoverOn)
  useEffect(() => {
    const onChange = (e: Event) => setOn((e as CustomEvent<boolean>).detail === true)
    const onStorage = (e: StorageEvent) => {
      if (e.key === CALL_CARD_HOVER_KEY) setOn(callCardHoverOn())
    }
    window.addEventListener(CHANGED_EVENT, onChange)
    window.addEventListener('storage', onStorage)
    return () => {
      window.removeEventListener(CHANGED_EVENT, onChange)
      window.removeEventListener('storage', onStorage)
    }
  }, [])
  return [on, setCallCardHover]
}
