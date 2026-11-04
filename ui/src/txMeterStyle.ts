import { useEffect, useState } from 'react'

// #343 — HOW THE TX METERS ARE DRAWN: the bars they have always been, or needles (an analog meter
// face with a peak marker, components/TxNeedles.tsx). Settings ▸ Workspace picks it.
//
// BARS BY DEFAULT, so nothing changes for anyone until they pick Needle.
//
// PER MACHINE, bare localStorage, APP-GLOBAL — the classification of nexus-density and the local
// clock: it is how this screen draws an instrument, not a fact about the station, and every window
// (a popped-out panel too) reads the one value. Read synchronously by the meters themselves through
// `useTxMeterStyle()`, so no prop threads through the three cockpits.
//
// CLAMPED ON LOAD: anything stored that is not exactly one of the two names (an old build's value,
// a hand edit, a value from a newer build) reads as the default, and storage that throws reads as
// the default too.

export type TxMeterStyle = 'bars' | 'needle'
export const TX_METER_STYLE_KEY = 'nexus-txmeter-style'
const CHANGED = 'nexus-txmeter-style-changed'

/** The stored style, validated: only the two names are honoured, anything else is Bars. */
export function readTxMeterStyle(): TxMeterStyle {
  try {
    return localStorage.getItem(TX_METER_STYLE_KEY) === 'needle' ? 'needle' : 'bars'
  } catch {
    return 'bars'
  }
}

/** Store the style and tell this window's meters (a `storage` event only reaches OTHER windows). */
export function setTxMeterStyle(style: TxMeterStyle): void {
  const v: TxMeterStyle = style === 'needle' ? 'needle' : 'bars'
  try {
    localStorage.setItem(TX_METER_STYLE_KEY, v)
  } catch {
    /* storage unavailable — nothing to remember it in */
  }
  window.dispatchEvent(new Event(CHANGED))
}

/** The style, re-rendering when it changes here or in another window. */
export function useTxMeterStyle(): TxMeterStyle {
  const [style, setStyle] = useState<TxMeterStyle>(readTxMeterStyle)
  useEffect(() => {
    const on = () => setStyle(readTxMeterStyle())
    window.addEventListener('storage', on)
    window.addEventListener(CHANGED, on)
    return () => {
      window.removeEventListener('storage', on)
      window.removeEventListener(CHANGED, on)
    }
  }, [])
  return style
}

