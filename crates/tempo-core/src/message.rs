//! Standard FT1 QSO messages (build + parse).
//!
//! FT1 reuses the WSJT-X 77-bit message formats. The forms Tempo's
//! auto-sequencer uses all take the shape `<TO> <FROM> <PAYLOAD>` (plus the
//! `CQ <CALL> <GRID>` form), where PAYLOAD is one of:
//! a 4-character Maidenhead grid, a signal report (`-10`, `+05`), a rogered
//! report (`R-12`), or `RR73` / `RRR` / `73`. These all round-trip verbatim
//! through the modem (verified against the FT1 packer).

/// A parsed/buildable standard QSO message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Msg {
    /// `CQ <de> <grid>`
    Cq { de: String, grid: String },
    /// `<to> <de> <grid>` — reply to a CQ / call with grid.
    Grid {
        to: String,
        de: String,
        grid: String,
    },
    /// `<to> <de> <snr>` — signal report.
    Report { to: String, de: String, snr: i32 },
    /// `<to> <de> R<snr>` — rogered signal report.
    RReport { to: String, de: String, snr: i32 },
    /// `<to> <de> RR73`
    Rr73 { to: String, de: String },
    /// `<to> <de> RRR`
    Rrr { to: String, de: String },
    /// `<to> <de> 73`
    Bye73 { to: String, de: String },
    /// ARRL Field Day exchange: `<to> <de> [R] <class> <section>`
    /// (e.g. `W9XYZ K2DEF 3A WI` or `W9XYZ K2DEF R 3A WI`).
    FieldDay {
        to: String,
        de: String,
        roger: bool,
        class: String,
        section: String,
    },
    /// Free text or anything not recognized as a standard form.
    Other(String),
}

/// Format a signal report the way WSJT-X does: sign + two digits, clamped.
pub fn fmt_report(snr: i32) -> String {
    format!("{:+03}", snr.clamp(-30, 30))
}

impl Msg {
    /// Render to the on-air text form.
    pub fn to_text(&self) -> String {
        match self {
            Msg::Cq { de, grid } => format!("CQ {de} {grid}"),
            Msg::Grid { to, de, grid } => format!("{to} {de} {grid}"),
            Msg::Report { to, de, snr } => format!("{to} {de} {}", fmt_report(*snr)),
            Msg::RReport { to, de, snr } => format!("{to} {de} R{}", fmt_report(*snr)),
            Msg::Rr73 { to, de } => format!("{to} {de} RR73"),
            Msg::Rrr { to, de } => format!("{to} {de} RRR"),
            Msg::Bye73 { to, de } => format!("{to} {de} 73"),
            Msg::FieldDay {
                to,
                de,
                roger,
                class,
                section,
            } => {
                if *roger {
                    format!("{to} {de} R {class} {section}")
                } else {
                    format!("{to} {de} {class} {section}")
                }
            }
            Msg::Other(s) => s.clone(),
        }
    }

    /// Parse decoded text into a standard form (falls back to [`Msg::Other`]).
    pub fn parse(s: &str) -> Msg {
        let t: Vec<&str> = s.split_whitespace().collect();
        if t.len() >= 3 && t[0] == "CQ" {
            // "CQ <call> <grid>" (also tolerates "CQ DX <call> <grid>").
            let de = t[t.len() - 2].to_string();
            let grid = t[t.len() - 1].to_string();
            if is_grid(&grid) {
                return Msg::Cq { de, grid };
            }
        }
        if t.len() == 3 {
            let to = t[0].to_string();
            let de = t[1].to_string();
            let p = t[2];
            match p {
                "RR73" => return Msg::Rr73 { to, de },
                "RRR" => return Msg::Rrr { to, de },
                "73" => return Msg::Bye73 { to, de },
                _ => {}
            }
            if let Some(rest) = p.strip_prefix('R') {
                if let Ok(n) = rest.parse::<i32>() {
                    return Msg::RReport { to, de, snr: n };
                }
            }
            if let Ok(n) = p.parse::<i32>() {
                return Msg::Report { to, de, snr: n };
            }
            if is_grid(p) {
                return Msg::Grid {
                    to,
                    de,
                    grid: p.to_string(),
                };
            }
        }
        // ARRL Field Day exchange: "<to> <de> [R] <class> <section>".
        if t.len() == 4 || t.len() == 5 {
            let class_idx = if t.len() == 5 && t[2] == "R" {
                Some(3)
            } else if t.len() == 4 {
                Some(2)
            } else {
                None
            };
            if let Some(ci) = class_idx {
                let class = t[ci];
                let section = t.get(ci + 1).copied().unwrap_or("");
                if is_fd_class(class) && is_section(section) {
                    return Msg::FieldDay {
                        to: t[0].to_string(),
                        de: t[1].to_string(),
                        roger: t.len() == 5,
                        class: class.to_string(),
                        section: section.to_string(),
                    };
                }
            }
        }
        Msg::Other(s.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    /// The callsign this message is directed to, if any.
    pub fn addressee(&self) -> Option<&str> {
        match self {
            Msg::Grid { to, .. }
            | Msg::Report { to, .. }
            | Msg::RReport { to, .. }
            | Msg::Rr73 { to, .. }
            | Msg::Rrr { to, .. }
            | Msg::Bye73 { to, .. }
            | Msg::FieldDay { to, .. } => Some(to),
            _ => None,
        }
    }

    /// The callsign that sent this message, if identifiable.
    pub fn sender(&self) -> Option<&str> {
        match self {
            Msg::Cq { de, .. }
            | Msg::Grid { de, .. }
            | Msg::Report { de, .. }
            | Msg::RReport { de, .. }
            | Msg::Rr73 { de, .. }
            | Msg::Rrr { de, .. }
            | Msg::Bye73 { de, .. }
            | Msg::FieldDay { de, .. } => Some(de),
            _ => None,
        }
    }
}

/// True for an ARRL Field Day class like `3A`, `12A`, `1B`, `3H` (1–2 digits + letter).
fn is_fd_class(s: &str) -> bool {
    let b = s.as_bytes();
    let n = b.len();
    (2..=3).contains(&n)
        && b[..n - 1].iter().all(|c| c.is_ascii_digit())
        && b[n - 1].is_ascii_uppercase()
}

/// True for an ARRL/RAC section abbreviation (2–5 uppercase letters, e.g. WI, ENY, STX).
fn is_section(s: &str) -> bool {
    (2..=5).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_uppercase())
}

/// True for a 4-character Maidenhead grid like `EN37`.
fn is_grid(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 4
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_uppercase()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_roundtrip_through_text() {
        let cases = [
            Msg::Cq {
                de: "W9XYZ".into(),
                grid: "EN37".into(),
            },
            Msg::Grid {
                to: "W9XYZ".into(),
                de: "K2DEF".into(),
                grid: "FN31".into(),
            },
            Msg::Report {
                to: "K2DEF".into(),
                de: "W9XYZ".into(),
                snr: -10,
            },
            Msg::Report {
                to: "K2DEF".into(),
                de: "W9XYZ".into(),
                snr: 5,
            },
            Msg::RReport {
                to: "W9XYZ".into(),
                de: "K2DEF".into(),
                snr: -12,
            },
            Msg::Rr73 {
                to: "K2DEF".into(),
                de: "W9XYZ".into(),
            },
            Msg::Rrr {
                to: "K2DEF".into(),
                de: "W9XYZ".into(),
            },
            Msg::Bye73 {
                to: "K2DEF".into(),
                de: "W9XYZ".into(),
            },
        ];
        for c in cases {
            assert_eq!(Msg::parse(&c.to_text()), c, "roundtrip failed for {c:?}");
        }
    }

    #[test]
    fn parses_known_text() {
        assert_eq!(
            Msg::parse("CQ W9XYZ EN37"),
            Msg::Cq {
                de: "W9XYZ".into(),
                grid: "EN37".into()
            }
        );
        assert_eq!(
            Msg::parse("K2DEF W9XYZ +05"),
            Msg::Report {
                to: "K2DEF".into(),
                de: "W9XYZ".into(),
                snr: 5
            }
        );
        assert_eq!(
            Msg::parse("W9XYZ K2DEF R-12"),
            Msg::RReport {
                to: "W9XYZ".into(),
                de: "K2DEF".into(),
                snr: -12
            }
        );
        assert_eq!(Msg::parse("K2DEF W9XYZ RR73").addressee(), Some("K2DEF"));
        assert_eq!(Msg::parse("CQ W9XYZ EN37").sender(), Some("W9XYZ"));
    }

    #[test]
    fn report_formatting() {
        assert_eq!(fmt_report(5), "+05");
        assert_eq!(fmt_report(-10), "-10");
        assert_eq!(fmt_report(0), "+00");
        assert_eq!(fmt_report(-99), "-30"); // clamped
    }

    #[test]
    fn field_day_forms() {
        let fd = Msg::FieldDay {
            to: "W9XYZ".into(),
            de: "K2DEF".into(),
            roger: false,
            class: "3A".into(),
            section: "WI".into(),
        };
        assert_eq!(fd.to_text(), "W9XYZ K2DEF 3A WI");
        assert_eq!(Msg::parse("W9XYZ K2DEF 3A WI"), fd);

        let fdr = Msg::FieldDay {
            to: "W9XYZ".into(),
            de: "K2DEF".into(),
            roger: true,
            class: "12A".into(),
            section: "IL".into(),
        };
        assert_eq!(fdr.to_text(), "W9XYZ K2DEF R 12A IL");
        assert_eq!(Msg::parse("W9XYZ K2DEF R 12A IL"), fdr);

        // Not confused with adjacent forms.
        assert!(matches!(
            Msg::parse("W9XYZ K2DEF R-12"),
            Msg::RReport { .. }
        ));
        assert!(matches!(Msg::parse("CQ FD W9XYZ EN37"), Msg::Cq { .. }));
        assert_eq!(Msg::parse("W9XYZ K2DEF 3A WI").addressee(), Some("W9XYZ"));
        assert_eq!(Msg::parse("W9XYZ K2DEF 3A WI").sender(), Some("K2DEF"));
    }
}
