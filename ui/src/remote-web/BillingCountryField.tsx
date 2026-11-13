// The Billing country question beside Subscribe (2026-11-12): Remote is not sold to the EU or the UK yet. The
// page asks it before Checkout; the relay passes it to the billing Worker, which refuses a country Remote
// is not sold in yet (`countryNotServed`) before Checkout opens. The page holds no refused list of its own.
//
// No default: the list starts empty. A country the browser's language tag names (`de-AT` → Austria) is
// preselected as a convenience only, and still has to be confirmed (the box below the list) before
// Subscribe works; picking a country by hand is itself the confirmation.
//
// Country names are prose and come from the browser (`Intl.DisplayNames`) in the page's language; the
// value sent is the ISO code, an invariant token. A browser without `Intl.DisplayNames` shows the codes.
import { useMemo } from 'react'
import { documentLang, getLocale, t } from '../i18n'
import { BILLING_COUNTRIES } from './billing-country'

/** What the operator has said about their billing country: the code, whether it is the browser's guess
 *  rather than their pick, and whether they chose or confirmed it. Only a confirmed code goes to Checkout. */
export interface BillingCountryChoice { code: string; guessed: boolean; confirmed: boolean }

/** The country in the browser's own language tag, preselected but not confirmed; none when the tag names
 *  no region, or one that is not a country (`es-419` is Latin America). */
export function initialBillingCountry(tag = typeof navigator === 'undefined' ? '' : navigator.language): BillingCountryChoice {
  let region: string | undefined
  try { region = new Intl.Locale(tag).region } catch { region = undefined }
  const code = region !== undefined && BILLING_COUNTRIES.has(region) ? region : ''
  return { code, guessed: code !== '', confirmed: false }
}

/** A country's name in the page's language, or its code where the browser cannot name it. */
export function billingCountryName(code: string): string {
  try { return new Intl.DisplayNames([documentLang(getLocale())], { type: 'region', fallback: 'code' }).of(code) ?? code }
  catch { return code }
}

export function BillingCountryField({ choice, onChange, disabled }: {
  choice: BillingCountryChoice; onChange: (choice: BillingCountryChoice) => void; disabled: boolean
}) {
  const locale = getLocale()
  const options = useMemo(() => [...BILLING_COUNTRIES].map(code => ({ code, name: billingCountryName(code) }))
    .sort((a, b) => a.name.localeCompare(b.name, documentLang(locale))), [locale])
  return <>
    <label>{t('remote.billing.country')}
      <select value={choice.code} disabled={disabled} required
        onChange={event => onChange({ code: event.target.value, guessed: false, confirmed: event.target.value !== '' })}>
        <option value="">{t('remote.billing.countryChoose')}</option>
        {options.map(option => <option key={option.code} value={option.code}>{option.name}</option>)}
      </select>
    </label>
    {choice.guessed && <label className="remote-site-confirm">
      <input type="checkbox" checked={choice.confirmed} disabled={disabled} onChange={event => onChange({ ...choice, confirmed: event.target.checked })} />
      {t('remote.billing.countryConfirm')}
    </label>}
  </>
}
