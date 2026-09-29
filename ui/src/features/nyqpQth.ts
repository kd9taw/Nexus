// ---------------------------------------------------------------------------
// THE NEW YORK QSO PARTY'S TWO EXCHANGE UNIVERSES — the `ny_counties` and `ny_mults`
// domains of the rules seed, mirrored so the entry strip can answer with NO IPC: the
// while-typing verdict, the county type-ahead ("Monroe" → MON) and the multiplier board
// all read these tables on every keystroke.
//
// ⚠️ A MIRROR, and a guarded one: tempo-core's
// `the_typescript_nyqp_mirror_matches_the_seed_domains` reads this file and fails on any
// code, name or order that differs from the seed. Change the seed and this together.
//
// The codes are INVARIANT TOKENS — the sponsor's own three-letter county abbreviations and
// the state and province codes its log robot lists. The names are display text: they feed
// the type-ahead and a tooltip, and they are never sent on the air.
// ---------------------------------------------------------------------------

/** One legal exchange value: the code sent on the air and the place it names. */
export interface NyqpValue {
  code: string
  name: string
}

/** The 62 counties of the sponsor's own county list (the 2026 rules PDF, repeated code for
 *  code by its county checklist), in its alphabetical order. */
export const NYQP_COUNTIES: NyqpValue[] = [
  { code: 'ALB', name: 'Albany' },
  { code: 'ALL', name: 'Allegany' },
  { code: 'BRX', name: 'Bronx' },
  { code: 'BRM', name: 'Broome' },
  { code: 'CAT', name: 'Cattaraugus' },
  { code: 'CAY', name: 'Cayuga' },
  { code: 'CHA', name: 'Chautauqua' },
  { code: 'CHE', name: 'Chemung' },
  { code: 'CGO', name: 'Chenango' },
  { code: 'CLI', name: 'Clinton' },
  { code: 'COL', name: 'Columbia' },
  { code: 'COR', name: 'Cortland' },
  { code: 'DEL', name: 'Delaware' },
  { code: 'DUT', name: 'Dutchess' },
  { code: 'ERI', name: 'Erie' },
  { code: 'ESS', name: 'Essex' },
  { code: 'FRA', name: 'Franklin' },
  { code: 'FUL', name: 'Fulton' },
  { code: 'GEN', name: 'Genesee' },
  { code: 'GRE', name: 'Greene' },
  { code: 'HAM', name: 'Hamilton' },
  { code: 'HER', name: 'Herkimer' },
  { code: 'JEF', name: 'Jefferson' },
  { code: 'KIN', name: 'Kings' },
  { code: 'LEW', name: 'Lewis' },
  { code: 'LIV', name: 'Livingston' },
  { code: 'MAD', name: 'Madison' },
  { code: 'MON', name: 'Monroe' },
  { code: 'MTG', name: 'Montgomery' },
  { code: 'NAS', name: 'Nassau' },
  { code: 'NEW', name: 'New York' },
  { code: 'NIA', name: 'Niagara' },
  { code: 'ONE', name: 'Oneida' },
  { code: 'ONO', name: 'Onondaga' },
  { code: 'ONT', name: 'Ontario' },
  { code: 'ORA', name: 'Orange' },
  { code: 'ORL', name: 'Orleans' },
  { code: 'OSW', name: 'Oswego' },
  { code: 'OTS', name: 'Otsego' },
  { code: 'PUT', name: 'Putnam' },
  { code: 'QUE', name: 'Queens' },
  { code: 'REN', name: 'Rensselaer' },
  { code: 'RIC', name: 'Richmond' },
  { code: 'ROC', name: 'Rockland' },
  { code: 'SAR', name: 'Saratoga' },
  { code: 'SCH', name: 'Schenectady' },
  { code: 'SCO', name: 'Schoharie' },
  { code: 'SCU', name: 'Schuyler' },
  { code: 'SEN', name: 'Seneca' },
  { code: 'STL', name: 'St. Lawrence' },
  { code: 'STE', name: 'Steuben' },
  { code: 'SUF', name: 'Suffolk' },
  { code: 'SUL', name: 'Sullivan' },
  { code: 'TIO', name: 'Tioga' },
  { code: 'TOM', name: 'Tompkins' },
  { code: 'ULS', name: 'Ulster' },
  { code: 'WAR', name: 'Warren' },
  { code: 'WAS', name: 'Washington' },
  { code: 'WAY', name: 'Wayne' },
  { code: 'WES', name: 'Westchester' },
  { code: 'WYO', name: 'Wyoming' },
  { code: 'YAT', name: 'Yates' },
]

/** What a station outside New York sends: the 49 other US states (New York's own stations
 *  send a county), in the order of the sponsor's log robot, then the 13 Canadian provinces
 *  and territories of the rules' "Canadian Multiplier List". DX stations send `DX`, which
 *  counts for no multiplier and is not listed here. */
export const NYQP_MULTS: NyqpValue[] = [
  { code: 'AL', name: 'Alabama' },
  { code: 'AK', name: 'Alaska' },
  { code: 'AZ', name: 'Arizona' },
  { code: 'AR', name: 'Arkansas' },
  { code: 'CA', name: 'California' },
  { code: 'CO', name: 'Colorado' },
  { code: 'CT', name: 'Connecticut' },
  { code: 'DE', name: 'Delaware' },
  { code: 'FL', name: 'Florida' },
  { code: 'GA', name: 'Georgia' },
  { code: 'HI', name: 'Hawaii' },
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
  { code: 'AB', name: 'Alberta' },
  { code: 'BC', name: 'British Columbia' },
  { code: 'MB', name: 'Manitoba' },
  { code: 'NB', name: 'New Brunswick' },
  { code: 'NL', name: 'Newfoundland and Labrador' },
  { code: 'NT', name: 'Northwest Territories' },
  { code: 'NS', name: 'Nova Scotia' },
  { code: 'NU', name: 'Nunavut' },
  { code: 'ON', name: 'Ontario' },
  { code: 'PE', name: 'Prince Edward Island' },
  { code: 'QC', name: 'Quebec' },
  { code: 'SK', name: 'Saskatchewan' },
  { code: 'YT', name: 'Yukon' },
]
