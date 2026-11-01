//! Compare NOAA's D-RAP absorption grid with Nexus's analytic D-RAP field (dev only).
//!
//!   cargo run -p propagation --example drap_compare --features live            # once
//!   cargo run -p propagation --example drap_compare --features live -- 90      # once a minute for 90 min
//!
//! The map draws NOAA's grid as the truth and the analytic field only between grid updates and
//! offline. This prints how far apart the two are on every pass, against the success measure:
//! median |Δ| ≤ 1 MHz where the sun is high (cos χ ≥ 0.5) and ≤ 3 MHz in the taper toward the
//! terminator. Run it during an M/X flare: on a quiet sun both are ~0 and the numbers say
//! nothing. One request per product per pass (NOAA refreshes both every minute).

use propagation::flare::{agreement, wants_drap};
use propagation::live::swpc;

fn main() {
    let passes: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(1)
        .max(1);
    println!("grid valid  | GOES sample | flux     | grid max | dayside |Δ| (n) | taper |Δ| (n) | verdict");
    for pass in 0..passes {
        if pass > 0 {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        let (grid, x) = match (swpc::fetch_drap_grid(), swpc::fetch_xray_trace()) {
            (Ok(g), Ok(x)) => (g, x),
            (g, x) => {
                eprintln!("fetch failed: grid {:?} / xray {:?}", g.err(), x.err());
                continue;
            }
        };
        let a = agreement(&grid, x.now.value);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let verdict = if !wants_drap(x.now.value, x.now.at, now) {
            "quiet sun (below M1) or a stale GOES sample: not a test"
        } else if a.dayside_median <= 1.0 && a.taper_median <= 3.0 {
            "agrees"
        } else {
            "DIFFERS: the map draws NOAA's grid"
        };
        println!(
            "{} | {} | {:.2e} | {:8.1} | {:7.2} ({:4}) | {:5.2} ({:4}) | {verdict}",
            hm(grid.valid_at),
            hm(x.now.at),
            x.now.value,
            grid.max(),
            a.dayside_median,
            a.dayside_cells,
            a.taper_median,
            a.taper_cells,
        );
    }
}

/// `HH:MMZ` for a Unix time.
fn hm(unix: i64) -> String {
    let s = unix.rem_euclid(86_400);
    format!("{:02}:{:02}Z     ", s / 3600, s / 60 % 60)
}
