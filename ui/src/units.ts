// Display units (F4MQS: "a single global Units: metric / imperial setting").
//
// The app computes everything in native units — distances in km (haversine), APRS weather
// in the °F/mph the APRS spec transmits, RepeaterBook radii in km — and converts ONLY at
// the display edge. So this module is pure formatting; nothing here touches a wire value.
//
// The setting is `settings.units` ('auto' | 'metric' | 'imperial'); 'auto' follows the OS
// locale, imperial only for the three countries that use it (US, Liberia, Myanmar). It is
// mirrored to localStorage on settings load/save so the display helpers — many of which are
// called from components that never receive the full settings object — can resolve it
// synchronously through `useUnits()` without threading a prop through every surface, the
// same app-global pattern `useCountryExclude` uses.

import { useEffect, useState } from 'react'

// APP-GLOBAL, bare localStorage (NOT surfaceGet/surfaceSet): the unit preference is one
// value every window shares, like the country exclude — not a per-surface view state.
function readKey(): string | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage.getItem(UNITS_KEY) : null
  } catch {
    return null
  }
}
function writeKey(v: string): void {
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(UNITS_KEY, v)
  } catch {
    /* ignore */
  }
}

export type UnitsSetting = 'auto' | 'metric' | 'imperial'
export type Units = 'metric' | 'imperial'

/** localStorage key the setting is mirrored to (App writes it on load/save). */
export const UNITS_KEY = 'nexus.units'

/**
 * A BCP-47 tag `Intl.Locale` will actually accept, from whatever the OS handed the webview.
 *
 * ⚠️ `Intl.Locale` THROWS on a tag it cannot parse, and the shapes a Linux desktop produces are
 * exactly the ones it rejects: `C`, `C.UTF-8` and the POSIX underscore form `en_US` all raise
 * `RangeError`. Only an EMPTY string reached the `|| 'en-US'` fallback, so a non-empty invalid
 * tag went straight into the constructor — and since this runs while the units hook renders, the
 * throw took the whole Operate view down with it ("OPERATE HIT AN ERROR — invalid language tag").
 * Found 2026-08-21 running the app under `LANG=C.UTF-8`.
 *
 * Normalising rather than only catching, because the difference is a WRONG ANSWER, not a crash:
 * `en_US` carries a real region, and swallowing it would quietly hand a US operator metric units.
 */
function usableLocaleTag(raw: string | undefined): string {
  const tag = (raw ?? '').trim()
  // `C`/`POSIX` describe the absence of a locale, not a place — there is no region to find.
  if (!tag || /^(C|POSIX)(\.|@|$)/i.test(tag)) return 'en-US'
  // `en_US.UTF-8@euro` → `en-US`: underscores are the POSIX separator, and the charset and
  // modifier suffixes are not part of a language tag at all.
  return tag.split(/[.@]/)[0].replace(/_/g, '-')
}

/** Exported for the test only — the normaliser is the whole of the fix and deserves pinning. */
export const __test_usableLocaleTag = usableLocaleTag

/** Countries whose OS locale implies imperial display; everyone else is metric. */
function localeIsImperial(): boolean {
  if (typeof navigator === 'undefined') return true // SSR/tests: the historical default
  let region: string | undefined
  try {
    region = new Intl.Locale(usableLocaleTag(navigator.language)).maximize().region
  } catch {
    // Still unparseable after normalising: an unknown locale is far more likely to be one of
    // the ~190 metric countries than one of the three that are not. Never throw from here —
    // deciding between miles and kilometres must not be able to take a screen down.
    return false
  }
  return region === 'US' || region === 'LR' || region === 'MM'
}

/** Resolve the stored setting to a concrete system. 'auto' consults the OS locale. */
export function resolveUnits(setting: string | null | undefined): Units {
  if (setting === 'imperial') return 'imperial'
  if (setting === 'metric') return 'metric'
  return localeIsImperial() ? 'imperial' : 'metric' // 'auto' / unknown
}

/** Mirror the persisted setting so the hook can read it synchronously everywhere. Fires a
 *  same-window event too — a `storage` event only reaches OTHER windows. */
export function setUnitsMirror(setting: string | null | undefined): void {
  const v: UnitsSetting =
    setting === 'metric' || setting === 'imperial' ? setting : 'auto'
  const prev = readKey()
  writeKey(v)
  if (prev !== v && typeof window !== 'undefined') {
    window.dispatchEvent(new Event('nexus-units-changed'))
  }
}

/** The resolved units, re-rendering when the setting changes (storage or same-window save). */
export function useUnits(): Units {
  const read = () => resolveUnits(readKey())
  const [units, setUnits] = useState<Units>(read)
  useEffect(() => {
    const on = () => setUnits(read())
    window.addEventListener('storage', on)
    window.addEventListener('nexus-units-changed', on)
    return () => {
      window.removeEventListener('storage', on)
      window.removeEventListener('nexus-units-changed', on)
    }
  }, [])
  return units
}

const KM_PER_MI = 1.609344

/** Distance from km → the display string ("128 km" / "80 mi"). */
export function fmtDistanceKm(km: number, u: Units): string {
  return u === 'imperial' ? `${Math.round(km / KM_PER_MI)} mi` : `${Math.round(km)} km`
}

/** Temperature from °F (APRS-native) → the display string. */
export function fmtTempF(f: number, u: Units): string {
  return u === 'metric' ? `${Math.round((f - 32) / 1.8)}°C` : `${Math.round(f)}°F`
}

/** Speed from mph (APRS-native) → the display string. */
export function fmtSpeedMph(mph: number, u: Units): string {
  return u === 'metric' ? `${Math.round(mph * KM_PER_MI)} km/h` : `${Math.round(mph)} mph`
}

/** Rain/precip from inches (APRS-native) → the display string. */
export function fmtRainIn(inches: number, u: Units): string {
  return u === 'metric' ? `${(inches * 25.4).toFixed(1)} mm` : `${inches.toFixed(2)} in`
}
