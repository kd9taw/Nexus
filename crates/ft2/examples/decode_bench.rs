// Measures one FT2 decode at depth 3 — the early-pass budget question.
fn main() {
    let itone = ft2::encode("CQ KD9TAW EN52").unwrap();
    let wave = ft2::gen_wave(&itone, 1500.0).unwrap();
    let start = (0.5 * ft2::SAMPLE_RATE) as usize;
    let mut iwave = vec![0i16; ft2::NMAX];
    for (i, &s) in wave.iter().enumerate() {
        iwave[start + i] = (s * 3000.0) as i16;
    }
    // Warm the init-once tables, then time 5 decodes.
    let _ = ft2::decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "", 1500);
    let t = std::time::Instant::now();
    for _ in 0..5 {
        let _ = ft2::decode_frame(&iwave, 200, 2900, 3, "KD9TAW", "", 1500);
    }
    println!(
        "mean decode: {:.0} ms",
        t.elapsed().as_millis() as f64 / 5.0
    );
}
