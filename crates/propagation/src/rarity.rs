//! Grid-rarity scoring, ported verbatim from weak-signal-sleuth's
//! `public/rare_seed_weighted.csv` (grid → rarity_score). Feeds the ML model's
//! `rarity_sum` feature and lets the UI flag rare-grid spots.

use std::collections::HashMap;
use std::sync::OnceLock;

/// weak-signal-sleuth's tuned rarity seed table (`grid,score`), embedded so the
/// ported model behaves identically without a data-file dependency.
const RARITY_CSV: &str = "\
FN57,78\nFN56,70\nFN55,68\nFN65,72\nFN66,75\nFN67,78\nFN51,60\nEN92,55\nEN93,58\n\
CN78,73\nCN77,70\nCN76,65\nCN75,63\nCN74,60\nCN73,55\nCN72,52\nCN71,67\nCN70,65\n\
CM79,69\nCM78,65\nCM77,60\nCM76,58\nEL84,41\nEL89,45\nEL79,48\nDN48,65\nDN38,60\n\
DN28,58\nDN18,56\nDN08,54\nDN58,62\nEN29,70\nEN18,66\nEN38,64\nEN58,60\nEM17,54\n\
EM19,52\nEM18,51\nEM27,55\nEM26,54\nDM72,57\nDM73,56\nDM82,59\nDM83,58";

/// The rarity map (grid → score), parsed once and cached.
pub fn rarity_map() -> &'static HashMap<String, f32> {
    static MAP: OnceLock<HashMap<String, f32>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for line in RARITY_CSV.lines() {
            let mut it = line.split(',');
            if let (Some(grid), Some(score)) = (it.next(), it.next()) {
                if let Ok(v) = score.trim().parse::<f32>() {
                    m.insert(grid.trim().to_uppercase(), v);
                }
            }
        }
        m
    })
}

/// Rarity score for a grid (0.0 if unlisted). Matches the `(rare[grid]||0)`
/// lookup in weak-signal-sleuth, using the 4-char square prefix.
pub fn rarity_of(grid: &str) -> f32 {
    let g = grid.trim().to_uppercase();
    let key = if g.len() >= 4 { &g[..4] } else { g.as_str() };
    rarity_map().get(key).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rarity_lookup() {
        assert_eq!(rarity_of("FN67"), 78.0);
        assert_eq!(rarity_of("fn67xx"), 78.0); // case + 6-char → 4-char prefix
        assert_eq!(rarity_of("ZZ99"), 0.0); // unlisted
        assert_eq!(rarity_map().len(), 44);
    }
}
