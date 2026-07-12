// ---------------------------------------------------------------------------
// ARRL/RAC Field Day section universe — the worked-sections board's universe.
//
// This is a TS mirror of `ARRL_SECTIONS` in
// crates/tempo-core/src/fd_rules.rs (the authoritative list). Like the
// `FD_BONUSES` mirror in FieldDayView.tsx, this is a deliberate small
// duplication: the sections change rarely (years apart), and mirroring them
// here keeps the frontend board free of a backend round-trip for the static
// universe. Worked-state still comes from the log / DTO `workedSections`.
//
// If the Rust list ever changes, update this table to match (71 US ARRL
// sections + 12 RAC = 83). Grouped by ARRL division so the board renders one
// tidy block per division, preserving the Rust ordering.
// ---------------------------------------------------------------------------

export interface ArrlSection {
  code: string
  name: string
  division: string
}

export interface SectionDivision {
  division: string
  sections: ArrlSection[]
}

/** The section universe, grouped by ARRL/RAC division (Rust ordering preserved). */
export const ARRL_SECTIONS_BY_DIVISION: SectionDivision[] = [
  {
    division: 'Atlantic',
    sections: [
      { code: 'DE', name: 'Delaware', division: 'Atlantic' },
      { code: 'EPA', name: 'Eastern Pennsylvania', division: 'Atlantic' },
      { code: 'MDC', name: 'Maryland-DC', division: 'Atlantic' },
      { code: 'NNY', name: 'Northern New York', division: 'Atlantic' },
      { code: 'SNJ', name: 'Southern New Jersey', division: 'Atlantic' },
      { code: 'WNY', name: 'Western New York', division: 'Atlantic' },
      { code: 'WPA', name: 'Western Pennsylvania', division: 'Atlantic' },
    ],
  },
  {
    division: 'Central',
    sections: [
      { code: 'IL', name: 'Illinois', division: 'Central' },
      { code: 'IN', name: 'Indiana', division: 'Central' },
      { code: 'WI', name: 'Wisconsin', division: 'Central' },
    ],
  },
  {
    division: 'Dakota',
    sections: [
      { code: 'MN', name: 'Minnesota', division: 'Dakota' },
      { code: 'ND', name: 'North Dakota', division: 'Dakota' },
      { code: 'SD', name: 'South Dakota', division: 'Dakota' },
    ],
  },
  {
    division: 'Delta',
    sections: [
      { code: 'AR', name: 'Arkansas', division: 'Delta' },
      { code: 'LA', name: 'Louisiana', division: 'Delta' },
      { code: 'MS', name: 'Mississippi', division: 'Delta' },
      { code: 'TN', name: 'Tennessee', division: 'Delta' },
    ],
  },
  {
    division: 'Great Lakes',
    sections: [
      { code: 'KY', name: 'Kentucky', division: 'Great Lakes' },
      { code: 'MI', name: 'Michigan', division: 'Great Lakes' },
      { code: 'OH', name: 'Ohio', division: 'Great Lakes' },
    ],
  },
  {
    division: 'Hudson',
    sections: [
      { code: 'ENY', name: 'Eastern New York', division: 'Hudson' },
      { code: 'NLI', name: 'New York City-Long Island', division: 'Hudson' },
      { code: 'NNJ', name: 'Northern New Jersey', division: 'Hudson' },
    ],
  },
  {
    division: 'Midwest',
    sections: [
      { code: 'IA', name: 'Iowa', division: 'Midwest' },
      { code: 'KS', name: 'Kansas', division: 'Midwest' },
      { code: 'MO', name: 'Missouri', division: 'Midwest' },
      { code: 'NE', name: 'Nebraska', division: 'Midwest' },
    ],
  },
  {
    division: 'New England',
    sections: [
      { code: 'CT', name: 'Connecticut', division: 'New England' },
      { code: 'EMA', name: 'Eastern Massachusetts', division: 'New England' },
      { code: 'ME', name: 'Maine', division: 'New England' },
      { code: 'NH', name: 'New Hampshire', division: 'New England' },
      { code: 'RI', name: 'Rhode Island', division: 'New England' },
      { code: 'VT', name: 'Vermont', division: 'New England' },
      { code: 'WMA', name: 'Western Massachusetts', division: 'New England' },
    ],
  },
  {
    division: 'Northwestern',
    sections: [
      { code: 'AK', name: 'Alaska', division: 'Northwestern' },
      { code: 'EWA', name: 'Eastern Washington', division: 'Northwestern' },
      { code: 'ID', name: 'Idaho', division: 'Northwestern' },
      { code: 'MT', name: 'Montana', division: 'Northwestern' },
      { code: 'OR', name: 'Oregon', division: 'Northwestern' },
      { code: 'WWA', name: 'Western Washington', division: 'Northwestern' },
    ],
  },
  {
    division: 'Pacific',
    sections: [
      { code: 'EB', name: 'East Bay', division: 'Pacific' },
      { code: 'NV', name: 'Nevada', division: 'Pacific' },
      { code: 'PAC', name: 'Pacific', division: 'Pacific' },
      { code: 'SCV', name: 'Santa Clara Valley', division: 'Pacific' },
      { code: 'SF', name: 'San Francisco', division: 'Pacific' },
      { code: 'SJV', name: 'San Joaquin Valley', division: 'Pacific' },
      { code: 'SV', name: 'Sacramento Valley', division: 'Pacific' },
    ],
  },
  {
    division: 'Roanoke',
    sections: [
      { code: 'NC', name: 'North Carolina', division: 'Roanoke' },
      { code: 'SC', name: 'South Carolina', division: 'Roanoke' },
      { code: 'VA', name: 'Virginia', division: 'Roanoke' },
      { code: 'WV', name: 'West Virginia', division: 'Roanoke' },
    ],
  },
  {
    division: 'Rocky Mountain',
    sections: [
      { code: 'CO', name: 'Colorado', division: 'Rocky Mountain' },
      { code: 'NM', name: 'New Mexico', division: 'Rocky Mountain' },
      { code: 'UT', name: 'Utah', division: 'Rocky Mountain' },
      { code: 'WY', name: 'Wyoming', division: 'Rocky Mountain' },
    ],
  },
  {
    division: 'Southeastern',
    sections: [
      { code: 'AL', name: 'Alabama', division: 'Southeastern' },
      { code: 'GA', name: 'Georgia', division: 'Southeastern' },
      { code: 'NFL', name: 'Northern Florida', division: 'Southeastern' },
      { code: 'PR', name: 'Puerto Rico', division: 'Southeastern' },
      { code: 'SFL', name: 'Southern Florida', division: 'Southeastern' },
      { code: 'VI', name: 'Virgin Islands', division: 'Southeastern' },
      { code: 'WCF', name: 'West Central Florida', division: 'Southeastern' },
    ],
  },
  {
    division: 'Southwestern',
    sections: [
      { code: 'AZ', name: 'Arizona', division: 'Southwestern' },
      { code: 'LAX', name: 'Los Angeles', division: 'Southwestern' },
      { code: 'ORG', name: 'Orange', division: 'Southwestern' },
      { code: 'SB', name: 'Santa Barbara', division: 'Southwestern' },
      { code: 'SDG', name: 'San Diego', division: 'Southwestern' },
    ],
  },
  {
    division: 'West Gulf',
    sections: [
      { code: 'NTX', name: 'North Texas', division: 'West Gulf' },
      { code: 'OK', name: 'Oklahoma', division: 'West Gulf' },
      { code: 'STX', name: 'South Texas', division: 'West Gulf' },
      { code: 'WTX', name: 'West Texas', division: 'West Gulf' },
    ],
  },
  {
    division: 'RAC',
    sections: [
      { code: 'MAR', name: 'Maritime', division: 'RAC' },
      { code: 'NL', name: 'Newfoundland/Labrador', division: 'RAC' },
      { code: 'QC', name: 'Quebec', division: 'RAC' },
      { code: 'ONE', name: 'Ontario East', division: 'RAC' },
      { code: 'ONN', name: 'Ontario North', division: 'RAC' },
      { code: 'ONS', name: 'Ontario South', division: 'RAC' },
      { code: 'GTA', name: 'Greater Toronto Area', division: 'RAC' },
      { code: 'MB', name: 'Manitoba', division: 'RAC' },
      { code: 'SK', name: 'Saskatchewan', division: 'RAC' },
      { code: 'AB', name: 'Alberta', division: 'RAC' },
      { code: 'BC', name: 'British Columbia', division: 'RAC' },
      { code: 'NT', name: 'Northern Territories', division: 'RAC' },
    ],
  },
]

/** Total number of sections in the universe (the board's "N/total" denominator). */
export const ARRL_SECTION_TOTAL: number = ARRL_SECTIONS_BY_DIVISION.reduce(
  (n, d) => n + d.sections.length,
  0,
)
