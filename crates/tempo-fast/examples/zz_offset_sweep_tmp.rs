//! TEMPORARY diagnostic: sweep the FT1 signal's placement inside the 4 s capture
//! frame and report whether the FULL acquisition path (decode_frame, the one the
//! live RX uses) recovers it, and what dt it reports.

struct Awgn {
    state: u64,
}
impl Awgn {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
    fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 32) as u32
    }
    fn uniform01(&mut self) -> f64 {
        (self.next_u32() as f64 + 1.0) / (u32::MAX as f64 + 2.0)
    }
    fn gaussian(&mut self) -> f32 {
        let u1 = self.uniform01();
        let u2 = self.uniform01();
        ((-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()) as f32
    }
}

fn main() {
    let msg = "CQ W9XYZ EN37";
    let f0 = 1500.0f32;
    let fs = tempo_fast::SAMPLE_RATE;
    let snr_db: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(-8.0);

    let tones = tempo_fast::encode(msg);
    let wave = tempo_fast::gen_wave(&tones, fs, f0);
    println!(
        "wave = {} samples ({:.4} s), NMAX = {} ({:.3} s)",
        wave.len(),
        wave.len() as f32 / fs,
        tempo_fast::NMAX,
        tempo_fast::NMAX as f32 / fs
    );
    println!("SNR = {snr_db} dB\n");

    let bw_ratio = 2500.0f32 / (fs / 2.0);
    let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr_db);

    // Offsets in ms, including negative (signal ARRIVES EARLY => leading edge is
    // chopped off the front of the receiver's frame).
    let offsets_ms: Vec<i32> = vec![
        -400, -200, -100, -50, -20, 0, 20, 50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 1000,
        1100, 1200, 1400,
    ];

    for off_ms in offsets_ms {
        let noff = (off_ms as f32 / 1000.0 * fs).round() as i64;
        let mut dd = vec![0f32; tempo_fast::NMAX];
        for (i, &s) in wave.iter().enumerate() {
            let k = i as i64 + noff;
            if k >= 0 && (k as usize) < tempo_fast::NMAX {
                dd[k as usize] = s;
            }
        }
        let mut rng = Awgn::new(12345);
        let iwave: Vec<i16> = dd
            .iter()
            .map(|&s| ((sig * s + rng.gaussian()) * 1000.0).clamp(-32768.0, 32767.0) as i16)
            .collect();

        tempo_fast::harq_reset();
        let decs = tempo_fast::decode_frame(&iwave, 200, 2900, 3, "", "", 0, 0);
        let hit = decs.iter().find(|d| d.message == msg);
        match hit {
            Some(d) => println!(
                "off {:>6} ms ({:>7} sa)  DECODED   dt={:+.3}  freq={:.1}  snr={}",
                off_ms, noff, d.dt, d.freq, d.snr
            ),
            None => println!(
                "off {:>6} ms ({:>7} sa)  ---FAIL---  ({} other decodes)",
                off_ms,
                noff,
                decs.len()
            ),
        }
    }
}
