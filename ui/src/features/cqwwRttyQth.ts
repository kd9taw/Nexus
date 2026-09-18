// ---------------------------------------------------------------------------
// CQ WW RTTY's W/VE QTH LIST — the `cqww_rtty_qth` domain of the rules seed, mirrored so
// the entry strip's while-typing verdict (and the RTTY cockpit's grab) can test a code with
// no IPC.
//
// ⚠️ A MIRROR, and a guarded one: tempo-core's
// `the_typescript_cqww_rtty_qth_mirror_matches_the_seed_domain` reads this file and fails on
// any code, name or order that differs from the seed. Change the seed and this together.
//
// The codes are INVARIANT TOKENS, exactly as the sponsor prints them (NWT, NF, LB, PEI are
// its own spellings, not the ARRL/RAC ones). The names are display text for a tooltip.
// ---------------------------------------------------------------------------

/** One W/VE QTH: the code sent on the air and the place it names. */
export interface CqwwRttyQth {
  code: string
  name: string
}

/** The 48 contiguous states and DC, by USPS abbreviation (the sponsor's named authority). */
export const CQWW_RTTY_US_QTH: CqwwRttyQth[] = [
  { code: 'AL', name: 'Alabama' },
  { code: 'AZ', name: 'Arizona' },
  { code: 'AR', name: 'Arkansas' },
  { code: 'CA', name: 'California' },
  { code: 'CO', name: 'Colorado' },
  { code: 'CT', name: 'Connecticut' },
  { code: 'DE', name: 'Delaware' },
  { code: 'DC', name: 'District of Columbia' },
  { code: 'FL', name: 'Florida' },
  { code: 'GA', name: 'Georgia' },
  { code: 'ID', name: 'Idaho' },
  { code: 'IL', name: 'Illinois' },
  { code: 'IN', name: 'Indiana' },
  { code: 'IA', name: 'Iowa' },
  { code: 'KS', name: 'Kansas' },
  { code: 'KY', name: 'Kentucky' },
  { code: 'LA', name: 'Louisiana' },
  { code: 'ME', name: 'Maine' },
  { code: 'MD', name: 'Maryland' },
  { code: 'MA', name: 'Massachusetts' },
  { code: 'MI', name: 'Michigan' },
  { code: 'MN', name: 'Minnesota' },
  { code: 'MS', name: 'Mississippi' },
  { code: 'MO', name: 'Missouri' },
  { code: 'MT', name: 'Montana' },
  { code: 'NE', name: 'Nebraska' },
  { code: 'NV', name: 'Nevada' },
  { code: 'NH', name: 'New Hampshire' },
  { code: 'NJ', name: 'New Jersey' },
  { code: 'NM', name: 'New Mexico' },
  { code: 'NY', name: 'New York' },
  { code: 'NC', name: 'North Carolina' },
  { code: 'ND', name: 'North Dakota' },
  { code: 'OH', name: 'Ohio' },
  { code: 'OK', name: 'Oklahoma' },
  { code: 'OR', name: 'Oregon' },
  { code: 'PA', name: 'Pennsylvania' },
  { code: 'RI', name: 'Rhode Island' },
  { code: 'SC', name: 'South Carolina' },
  { code: 'SD', name: 'South Dakota' },
  { code: 'TN', name: 'Tennessee' },
  { code: 'TX', name: 'Texas' },
  { code: 'UT', name: 'Utah' },
  { code: 'VT', name: 'Vermont' },
  { code: 'VA', name: 'Virginia' },
  { code: 'WA', name: 'Washington' },
  { code: 'WV', name: 'West Virginia' },
  { code: 'WI', name: 'Wisconsin' },
  { code: 'WY', name: 'Wyoming' },
]

/** The 14 Canadian call areas, in the sponsor's own order and spelling. */
export const CQWW_RTTY_VE_QTH: CqwwRttyQth[] = [
  { code: 'NB', name: 'New Brunswick (VE9)' },
  { code: 'NS', name: 'Nova Scotia (VE1)' },
  { code: 'QC', name: 'Quebec (VE2)' },
  { code: 'ON', name: 'Ontario (VE3)' },
  { code: 'MB', name: 'Manitoba (VE4)' },
  { code: 'SK', name: 'Saskatchewan (VE5)' },
  { code: 'AB', name: 'Alberta (VE6)' },
  { code: 'BC', name: 'British Columbia (VE7)' },
  { code: 'NWT', name: 'Northwest Territories (VE8)' },
  { code: 'NF', name: 'Newfoundland (VO1)' },
  { code: 'LB', name: 'Labrador (VO2)' },
  { code: 'NU', name: 'Nunavut (VY0)' },
  { code: 'YT', name: 'Yukon (VY1)' },
  { code: 'PEI', name: 'Prince Edward Island (VY2)' },
]
