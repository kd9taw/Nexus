//! DARC's Worked All Europe (WAE) award: the WAE country list, and the band-point rule.
//!
//! THE LIST IS DARC'S, TRANSCRIBED. "Offizielle WAE-Länderliste" on
//! <https://www.darc.de/der-club/referate/dx/diplome/wae-diplom/wae-laenderliste/> (read
//! 2026-10-24; the page carries no version or date) prints 73 current countries, prefix and name.
//! [`COUNTRIES`] is that list in the page's order, each beside the name the embedded cty.dat
//! gives the same country — its European entities are exactly these 73, the WAE-only ones
//! (Sicily, Shetland, Bear Island, Vienna Intl Ctr, European Turkey) included, and a test holds
//! the two to each other. The countries DARC lists as deleted (Saarland, Trieste, the GDR …) are
//! not counted.
//!
//! The rules, from the award page <https://www.darc.de/der-club/referate/dx/diplome/wae-diplom/>
//! (read 2026-10-24, German):
//! * "Jedes bestätigte europäische Land zählt einen Punkt je Band, es dürfen aber maximal 5 Bänder
//!   je Land gewertet werden." — each confirmed country is one point per band, at most 5 bands a
//!   country. These band points are the award's multiplier-like rule; the band weights of the WAE
//!   DX Contest belong to the contest, not to the award, and are not used here.
//! * WAE III: 40 countries and 100 points; WAE II: 50 and 150; WAE I: 60 and 200; WAE-TOP: 70 and
//!   300 ([`CLASSES`]). WAE-Trophy: every country on 5 bands.
//! * Issued for CW, SSB, phone, RTTY, FT8, digital and mixed — the Awards view shows mixed.
//!
//! ⚠️ NOT VERIFIED, and stated so:
//! * Which bands count. The page says "Alle Amateurfunkbänder sind zugelassen" (all bands) in the
//!   Trophy paragraph only; Nexus counts every band for every class.
//! * Confirmation. DARC takes applications through its DCL and asks for proof of the cards; the
//!   page says nothing about LoTW or eQSL. Nexus counts what the ARRL awards count (LoTW or a
//!   card) as confirmed, so a country confirmed only by LoTW may need a card for the award.
//! * Vienna Intl Ctr (4U1V): the embedded cty.dat lists each of its calls under Austria as well,
//!   and the resolver answers Austria, so a contact with the Vienna centre counts as Austria (OE)
//!   here. The country is on the list and shows as needed until the resolver names it.
//! * The "valid since" dates the list prints for a few countries (Kosovo 2008, Montenegro 2006 …)
//!   are not applied: a contact made before a country existed is credited as the country its
//!   call resolves to today.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::awards::{counted_entity, RecordFields};
use crate::dxcc::DxccInfo;

/// `(DARC prefix, the name the embedded cty.dat gives the country)`, in the DARC list's order.
pub const COUNTRIES: &[(&str, &str)] = &[
    ("1A", "Sov Mil Order of Malta"),
    ("3A", "Monaco"),
    ("4O", "Montenegro"),
    ("4U1I", "ITU HQ"),
    ("4U1V", "Vienna Intl Ctr"),
    ("9A", "Croatia"),
    ("9H", "Malta"),
    ("C3", "Andorra"),
    ("CT", "Portugal"),
    ("CU", "Azores"),
    ("DL", "Fed. Rep. of Germany"),
    ("E7", "Bosnia-Herzegovina"),
    ("EA", "Spain"),
    ("EA6", "Balearic Islands"),
    ("EI", "Ireland"),
    ("ER", "Moldova"),
    ("ES", "Estonia"),
    ("EU", "Belarus"),
    ("F", "France"),
    ("G", "England"),
    ("GD", "Isle of Man"),
    ("GI", "Northern Ireland"),
    ("GJ", "Jersey"),
    ("GM", "Scotland"),
    ("GM/S", "Shetland Islands"),
    ("GU", "Guernsey"),
    ("GW", "Wales"),
    ("HA", "Hungary"),
    ("HB", "Switzerland"),
    ("HB0", "Liechtenstein"),
    ("HV", "Vatican City"),
    ("I", "Italy"),
    ("IS", "Sardinia"),
    ("IT", "Sicily"),
    ("JW", "Svalbard"),
    ("JW/B", "Bear Island"),
    ("JX", "Jan Mayen"),
    ("LA", "Norway"),
    ("LX", "Luxembourg"),
    ("LY", "Lithuania"),
    ("LZ", "Bulgaria"),
    ("OE", "Austria"),
    ("OH", "Finland"),
    ("OH0", "Aland Islands"),
    ("OJ0", "Market Reef"),
    ("OK", "Czech Republic"),
    ("OM", "Slovak Republic"),
    ("ON", "Belgium"),
    ("OY", "Faroe Islands"),
    ("OZ", "Denmark"),
    ("PA", "Netherlands"),
    ("R1FJ", "Franz Josef Land"),
    ("S5", "Slovenia"),
    ("SM", "Sweden"),
    ("SP", "Poland"),
    ("SV", "Greece"),
    ("SV/A", "Mount Athos"),
    ("SV5", "Dodecanese"),
    ("SV9", "Crete"),
    ("T7", "San Marino"),
    ("TA1", "European Turkey"),
    ("TF", "Iceland"),
    ("TK", "Corsica"),
    ("UA", "European Russia"),
    ("UA2", "Kaliningrad"),
    ("UR", "Ukraine"),
    ("YL", "Latvia"),
    ("YO", "Romania"),
    ("YU", "Serbia"),
    ("Z6", "Republic of Kosovo"),
    ("Z3", "North Macedonia"),
    ("ZA", "Albania"),
    ("ZB", "Gibraltar"),
];

/// The award classes, highest first: `(name, countries, band points)`.
pub const CLASSES: &[(&str, usize, usize)] = &[
    ("TOP", 70, 300),
    ("I", 60, 200),
    ("II", 50, 150),
    ("III", 40, 100),
];

/// At most this many bands a country count toward its band points (and the Trophy's per-country
/// goal).
pub const MAX_BANDS_PER_COUNTRY: usize = 5;

static INDEX: LazyLock<HashMap<&'static str, u16>> = LazyLock::new(|| {
    COUNTRIES
        .iter()
        .enumerate()
        .map(|(i, (_, name))| (*name, i as u16))
        .collect()
});

/// The WAE country a contact counts toward, as an index into [`COUNTRIES`], or `None` outside
/// Europe.
///
/// The country is the entity the contact counts toward for DXCC ([`counted_entity`]: its record's
/// number first, else its call) — refined to the WAE-only area its call names when that area lies
/// in the same DXCC entity: IT9 is Sicily when the record says Italy, but an IT9 call whose record
/// names another entity keeps the record's.
pub fn wae_country(resolved: Option<&DxccInfo>, rec: &RecordFields) -> Option<u16> {
    let counted = counted_entity(resolved, rec)?;
    if counted.deleted {
        return None;
    }
    if let Some(info) = resolved.filter(|i| !i.is_dxcc) {
        if info.dxcc.is_some() && info.dxcc == counted.code {
            if let Some(&i) = INDEX.get(info.entity) {
                return Some(i);
            }
        }
    }
    INDEX.get(counted.name).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxcc;

    fn country(call: &str, rec: RecordFields) -> Option<&'static str> {
        wae_country(dxcc::resolve(call).as_ref(), &rec).map(|i| COUNTRIES[i as usize].0)
    }

    /// The DARC list is the 73 the page prints, and every name is a European entity of the
    /// embedded cty.dat — and every European entity there is on the list.
    #[test]
    fn the_list_is_darcs_73_and_matches_cty_europe() {
        assert_eq!(COUNTRIES.len(), 73);
        assert_eq!(INDEX.len(), 73, "a country appears twice");
        let europe: std::collections::BTreeSet<&str> = dxcc::entity_continents()
            .into_iter()
            .filter(|(_, c)| *c == "EU")
            .map(|(n, _)| n)
            .collect();
        let listed: std::collections::BTreeSet<&str> = COUNTRIES.iter().map(|c| c.1).collect();
        assert_eq!(listed, europe);
    }

    /// Calls land on their country; the WAE-only areas count as themselves, not their parent.
    #[test]
    fn calls_land_on_their_wae_country() {
        let none = RecordFields::default();
        for (call, want) in [
            ("DL1ABC", "DL"),
            ("IT9ABC", "IT"),
            ("I2ABC", "I"),
            ("GM3ABC", "GM"),
            ("TA1ABC", "TA1"),
            ("UA3ABC", "UA"),
            ("OH0ABC", "OH0"),
        ] {
            assert_eq!(country(call, none), Some(want), "{call}");
        }
        for outside in ["W1AW", "JA1ABC", "UA9ABC", "VK2ABC"] {
            assert_eq!(country(outside, none), None, "{outside}");
        }
    }

    /// The record's DXCC number decides the entity, as it does for DXCC: an IT9 call whose record
    /// names Italy is still Sicily, but one whose record names another entity is that entity.
    #[test]
    fn the_record_number_decides_and_the_wae_area_refines_it() {
        let italy = RecordFields {
            dxcc: Some(248),
            ..RecordFields::default()
        };
        assert_eq!(country("IT9ABC", italy), Some("IT"));
        let malta = RecordFields {
            dxcc: Some(257),
            ..RecordFields::default()
        };
        assert_eq!(country("IT9ABC", malta), Some("9H"));
        let usa = RecordFields {
            dxcc: Some(291),
            ..RecordFields::default()
        };
        assert_eq!(country("DL1ABC", usa), None);
    }
}
