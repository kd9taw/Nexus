//! WWFF (World Wide Flora & Fauna): the references a hunter has worked, counted from the log.
//!
//! THE COUNT IS BUILT ON THE LOGGED REFERENCE, NOT ON A DIRECTORY. WWFF publishes its reference
//! directory at <https://wwff.co/directory/> under "all rights reserved … No part of this
//! directory may be reproduced … without the prior permission of WWFF" (read 2026-10-24), so no
//! copy of it ships in Nexus. What counts is the reference the operator logged (ADIF `WWFF_REF`,
//! or `SIG`=WWFF with its `SIG_INFO`), which Nexus checks for the shape every published reference
//! has — a programme prefix, `FF-`, four digits (`KFF-1234`, `DLFF-0001`, `GMFF-0001`) — and
//! upper-cases. The shape is read from the references WWFF prints; no rule page states it.
//!
//! The award rules, from <https://wwff.co/awards/global-awards/> (read 2026-10-24):
//! * "All awards are based on QSO information in the WWFF LogSearch database. We cannot accept
//!   other sources (like QSLs, LoTW, eQSL)." So a WWFF reference has no confirmation in the log's
//!   sense; Nexus shows references WORKED, and the award is the activator's uploaded log.
//! * WWFF-H (hunter) levels: "Entry level at 10 references worked", "Between 44 and 444, in steps
//!   of 44", "Between 444 and 4444, in steps of 100", "Between 4,444 and 8,444, in steps of 200",
//!   "Above 8,444, in steps of 400" ([`hunter_levels_reached`]).
//! * DXFF-H: "at least 10 different DX Entities" — the entities of the worked references are
//!   counted beside them.
//!
//! ⚠️ NOT VERIFIED: the Global Rules PDF could not be read as text, so whether repeater or
//! satellite contacts count is not known; Nexus counts every logged reference.

/// A WWFF reference in the shape every published one has (`KFF-1234`), upper case, or `None`.
pub fn valid_wwff_ref(s: &str) -> Option<String> {
    let r = s.trim().to_ascii_uppercase();
    let (head, num) = r.split_once("FF-")?;
    let ok = (1..=4).contains(&head.len())
        && head.bytes().all(|b| b.is_ascii_alphanumeric())
        && num.len() == 4
        && num.bytes().all(|b| b.is_ascii_digit());
    ok.then_some(r)
}

/// The programme a reference belongs to: its prefix with `FF` ("KFF", "DLFF").
pub fn wwff_program(reference: &str) -> &str {
    reference.split_once('-').map_or(reference, |(p, _)| p)
}

/// The WWFF-H level steps a hunter with `worked` references has reached, highest last.
pub fn hunter_levels_reached(worked: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut level = 10;
    while level <= worked {
        out.push(level);
        level = next_level_after(level);
    }
    out
}

/// The WWFF-H level after `level`.
pub fn next_level_after(level: usize) -> usize {
    match level {
        0..=9 => 10,
        10..=43 => 44,
        // 44 … 440 in steps of 44, then 444, where the steps of 100 begin.
        44..=443 => ((level / 44 + 1) * 44).min(444),
        444..=4443 => 444 + ((level - 444) / 100 + 1) * 100,
        4444..=8443 => 4444 + ((level - 4444) / 200 + 1) * 200,
        _ => 8444 + ((level - 8444) / 400 + 1) * 400,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_have_the_published_shape() {
        for (raw, want) in [
            ("KFF-1234", "KFF-1234"),
            (" dlff-0001 ", "DLFF-0001"),
            ("GMFF-0001", "GMFF-0001"),
            ("VKFF-0123", "VKFF-0123"),
        ] {
            assert_eq!(valid_wwff_ref(raw).as_deref(), Some(want), "{raw}");
        }
        for junk in [
            "",
            "K-1234",
            "KFF-123",
            "KFF-12345",
            "US-1234",
            "ABCDEFF-0001",
            "FF-0001",
        ] {
            assert_eq!(valid_wwff_ref(junk), None, "{junk:?}");
        }
        assert_eq!(wwff_program("KFF-1234"), "KFF");
    }

    /// The levels as the global-awards page states them, across each change of step.
    #[test]
    fn hunter_levels_follow_the_published_steps() {
        assert!(hunter_levels_reached(9).is_empty());
        assert_eq!(hunter_levels_reached(10), vec![10]);
        assert_eq!(hunter_levels_reached(88), vec![10, 44, 88]);
        let to_444 = hunter_levels_reached(444);
        assert_eq!(
            to_444.len(),
            12,
            "10, then 44 … 440 in steps of 44, then 444"
        );
        assert_eq!(next_level_after(440), 444);
        assert_eq!(*to_444.last().unwrap(), 444);
        assert_eq!(next_level_after(444), 544);
        assert_eq!(next_level_after(4344), 4444);
        assert_eq!(next_level_after(4444), 4644);
        assert_eq!(next_level_after(8244), 8444);
        assert_eq!(next_level_after(8444), 8844);
        assert_eq!(next_level_after(0), 10);
        assert_eq!(next_level_after(10), 44);
    }
}
