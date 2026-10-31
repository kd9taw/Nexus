//! Compare NOAA OVATION's aurora edge with the Kp-fit fallback (dev only).
//!
//!   cargo run -p propagation --example oval_compare --features live             # once
//!   cargo run -p propagation --example oval_compare --features live -- 8640     # every 5 min for 30 days
//!
//! The map draws OVATION's equatorward edge (its 10 % line) while OVATION is current, and the
//! Kp-fit edge (Starkov's diffuse-aurora boundary, crate::oval) only when it is stale. This
//! measures the plan's success measure for that fallback on every pass: the Kp-fit sits within
//! ±3° of OVATION's edge (a pass "agrees" when the median over the longitude bands is ≤ 3°; the
//! target is ≥ 80 % of passes), and OVATION's own edge follows Kp (the correlation of its mean
//! latitude with Kp, target |r| ≥ 0.7 over a month). The fit is evaluated at OVATION's forecast
//! time with the Kp of the pass. One request per product per pass (OVATION refreshes every 5 min).

use propagation::live::{aurora, swpc};
use propagation::oval::{edge_bin_lon, kp_fit_lat_at};

fn main() {
    let passes: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1)
        .max(1);
    let (mut agree, mut done) = (0u32, 0u32);
    let mut pairs: Vec<(f64, f64)> = Vec::new(); // (Kp, mean |edge latitude|)
    println!("forecast for | Kp   | bands | within 3° | median |Δ| | verdict");
    for pass in 0..passes {
        if pass > 0 {
            std::thread::sleep(std::time::Duration::from_secs(300));
        }
        let (oval, wx) = match (aurora::fetch_aurora(), swpc::fetch_space_wx()) {
            (Ok(o), Ok(w)) => (o, w),
            (o, w) => {
                eprintln!("fetch failed: ovation {:?} / kp {:?}", o.err(), w.err());
                continue;
            }
        };
        let (Some(at), Some(_)) = (oval.forecast_at, wx.stamps.kp_at) else {
            eprintln!("no forecast time or no measured Kp: not a test");
            continue;
        };
        let kp = f64::from(wx.wx.kp);
        let mut diffs = Vec::new();
        let mut lats = Vec::new();
        for (side, north) in [(&oval.edge.north, true), (&oval.edge.south, false)] {
            for (i, lat) in side.iter().enumerate() {
                let Some(lat) = lat else { continue };
                lats.push(f64::from(lat.abs()));
                if let Some(fit) = kp_fit_lat_at(kp, edge_bin_lon(i), north, at) {
                    diffs.push((fit.abs() - f64::from(lat.abs())).abs());
                }
            }
        }
        if diffs.is_empty() {
            println!(
                "{} | {kp:4.2} | 0 | - | - | no OVATION edge: not a test",
                hm(at)
            );
            continue;
        }
        diffs.sort_by(f64::total_cmp);
        let median = diffs[diffs.len() / 2];
        let within = diffs.iter().filter(|d| **d <= 3.0).count();
        done += 1;
        if median <= 3.0 {
            agree += 1;
        }
        pairs.push((kp, lats.iter().sum::<f64>() / lats.len() as f64));
        println!(
            "{} | {kp:4.2} | {:5} | {within:9} | {median:10.2} | {}",
            hm(at),
            diffs.len(),
            if median <= 3.0 { "agrees" } else { "DIFFERS" }
        );
    }
    if done > 0 {
        println!(
            "passes agreeing: {agree} of {done} ({:.0} %); Kp vs OVATION edge r = {}",
            100.0 * f64::from(agree) / f64::from(done),
            pearson(&pairs).map_or("n/a".into(), |r| format!("{r:.2}"))
        );
    }
}

/// Pearson's r over (x, y) pairs; `None` for fewer than 3 or no spread.
fn pearson(p: &[(f64, f64)]) -> Option<f64> {
    if p.len() < 3 {
        return None;
    }
    let n = p.len() as f64;
    let (mx, my) = (
        p.iter().map(|q| q.0).sum::<f64>() / n,
        p.iter().map(|q| q.1).sum::<f64>() / n,
    );
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (x, y) in p {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    (sxx > 0.0 && syy > 0.0).then(|| sxy / (sxx * syy).sqrt())
}

/// `HH:MMZ` for a Unix time.
fn hm(unix: i64) -> String {
    let s = unix.rem_euclid(86_400);
    format!("{:02}:{:02}Z     ", s / 3600, s / 60 % 60)
}
