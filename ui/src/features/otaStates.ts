// The state a POTA/SOTA activator is in, as the hunter board shows it. Pure, no React.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). A place's NAME comes from
// the catalog; its CODE ("ND") is an invariant token, the ADIF STATE value, shown as it is. The
// station sends codes as pota.app writes them, country first ("US-ND", "CA-ON"), and only for a US
// state, DC or a Canadian province: nothing else ever reaches this file.
import { t } from '../i18n'

/** Every place a row can carry, by code, spelled out key by key: a template-literal key is
 *  invisible to both catalog scanners (see the i18n guard). */
const PLACE_NAMES: Record<string, () => string> = {
  'US-AL': () => t('place.us.al'),
  'US-AK': () => t('place.us.ak'),
  'US-AZ': () => t('place.us.az'),
  'US-AR': () => t('place.us.ar'),
  'US-CA': () => t('place.us.ca'),
  'US-CO': () => t('place.us.co'),
  'US-CT': () => t('place.us.ct'),
  'US-DE': () => t('place.us.de'),
  'US-FL': () => t('place.us.fl'),
  'US-GA': () => t('place.us.ga'),
  'US-HI': () => t('place.us.hi'),
  'US-ID': () => t('place.us.id'),
  'US-IL': () => t('place.us.il'),
  'US-IN': () => t('place.us.in'),
  'US-IA': () => t('place.us.ia'),
  'US-KS': () => t('place.us.ks'),
  'US-KY': () => t('place.us.ky'),
  'US-LA': () => t('place.us.la'),
  'US-ME': () => t('place.us.me'),
  'US-MD': () => t('place.us.md'),
  'US-MA': () => t('place.us.ma'),
  'US-MI': () => t('place.us.mi'),
  'US-MN': () => t('place.us.mn'),
  'US-MS': () => t('place.us.ms'),
  'US-MO': () => t('place.us.mo'),
  'US-MT': () => t('place.us.mt'),
  'US-NE': () => t('place.us.ne'),
  'US-NV': () => t('place.us.nv'),
  'US-NH': () => t('place.us.nh'),
  'US-NJ': () => t('place.us.nj'),
  'US-NM': () => t('place.us.nm'),
  'US-NY': () => t('place.us.ny'),
  'US-NC': () => t('place.us.nc'),
  'US-ND': () => t('place.us.nd'),
  'US-OH': () => t('place.us.oh'),
  'US-OK': () => t('place.us.ok'),
  'US-OR': () => t('place.us.or'),
  'US-PA': () => t('place.us.pa'),
  'US-RI': () => t('place.us.ri'),
  'US-SC': () => t('place.us.sc'),
  'US-SD': () => t('place.us.sd'),
  'US-TN': () => t('place.us.tn'),
  'US-TX': () => t('place.us.tx'),
  'US-UT': () => t('place.us.ut'),
  'US-VT': () => t('place.us.vt'),
  'US-VA': () => t('place.us.va'),
  'US-WA': () => t('place.us.wa'),
  'US-WV': () => t('place.us.wv'),
  'US-WI': () => t('place.us.wi'),
  'US-WY': () => t('place.us.wy'),
  'US-DC': () => t('place.us.dc'),
  'CA-AB': () => t('place.ca.ab'),
  'CA-BC': () => t('place.ca.bc'),
  'CA-MB': () => t('place.ca.mb'),
  'CA-NB': () => t('place.ca.nb'),
  'CA-NL': () => t('place.ca.nl'),
  'CA-NS': () => t('place.ca.ns'),
  'CA-NT': () => t('place.ca.nt'),
  'CA-NU': () => t('place.ca.nu'),
  'CA-ON': () => t('place.ca.on'),
  'CA-PE': () => t('place.ca.pe'),
  'CA-QC': () => t('place.ca.qc'),
  'CA-SK': () => t('place.ca.sk'),
  'CA-YT': () => t('place.ca.yt'),
}

/** The code a row shows: "ND" from "US-ND". */
export function placeCode(code: string): string {
  return code.slice(code.indexOf('-') + 1)
}

/** The place's name in the operator's language, or its code for one the catalog lacks. */
export function placeName(code: string): string {
  return PLACE_NAMES[code]?.() ?? placeCode(code)
}

/** The most codes a row spells out: a park on a state line ("MT·ND") or a three-state corner. */
const SHOWN = 3

/**
 * The row's state label: each code, up to three ("MT·ND"). A long trail can span twenty states,
 * which would widen the row, so past three it shows two and how many more ("AL·CT +12"), the
 * needed ones first so a lit label shows why it is lit. The full list is in the tooltip.
 */
export function placeLabel(codes: string[], needed: string[] = []): string {
  if (codes.length <= SHOWN) return codes.map(placeCode).join('·')
  const first = [...codes.filter((c) => needed.includes(c)), ...codes.filter((c) => !needed.includes(c))]
  return `${first.slice(0, SHOWN - 1).map(placeCode).join('·')} +${codes.length - (SHOWN - 1)}`
}
