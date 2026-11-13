// The billing country Subscribe sends (2026-11-12): Remote is not sold to the EU or the UK yet. Shared by the
// page, which offers these in its Billing country list, and the relay (remote/src/index.ts), which takes
// no other. Whether Remote is sold in a country is the private billing Worker's answer alone (it refuses
// `countryNotServed` for the EU and the UK, before Checkout opens); this file must never grow that list.

/** Every country code ISO 3166-1 assigns (alpha-2), the form Stripe writes an address's country in. No
 *  reserved or user-assigned code (EU, UK, XK, ZZ): the relay refuses one `invalidRequest`. */
export const BILLING_COUNTRIES: ReadonlySet<string> = new Set(`
AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW
BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI
FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY HK HM HN HR HT HU ID IE IL IM IN
IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME
MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF
PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV
SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE
YT ZA ZM ZW`.trim().split(/\s+/))

/** True for a country code from the list above, exactly as written there (upper case, no spaces). */
export function isBillingCountry(value: unknown): value is string {
  return typeof value === 'string' && BILLING_COUNTRIES.has(value)
}
