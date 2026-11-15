//! FT8 decoder A/B bench: decodes WAV files the way a period runs in Nexus, and writes the
//! synthetic busy-band corpus the comparison script also feeds to WSJT-X's `jt9`.
//!
//! Driven by `scripts/ft8-decoder-bench.py`, which runs this once per WAV (so no decoder state
//! carries between files, as with one `jt9` process per file) and tabulates the result against
//! `jt9 -8 -d 3` and `jt9 -8 -M`. Run it through the script, in release (debug numbers are
//! meaningless):
//!
//!   cargo build --release -p ft8 --example decode_bench
//!
//! Subcommands:
//! - `decode <depth> <wav>`: the standard decoder's three steps of a period, as WSJT-X runs them
//!   from disk (`jt9.f90`: the early pass on the first 41 x 3456 samples, the 47 step's
//!   subtraction, then the full pass) through the same entry points the engine uses
//!   ([`ft8::decode_frame_carry`], [`ft8::subtract_early`]). Same parameters as `jt9`'s
//!   defaults: 200-4000 Hz, RX 1500 Hz, MyCall K1ABC, DxCall W9XYZ, AP on. Prints one
//!   `D<TAB>snr<TAB>dt<TAB>freq<TAB>message` line per decode and `T<TAB>ms` for the decode time.
//! - `synth <outdir> <allcall> <nfiles> <nsig> <seed>`: writes `nfiles` busy-band WAVs of `nsig`
//!   signals each (real callsigns drawn from `ALLCALL7.TXT`, SNR -24..-6 dB, 250-2850 Hz, DT
//!   -0.3..+1.0 s) plus a `truth.tsv` of what each holds, and 6 noise-only WAVs.

use std::io::Write;
use std::time::Instant;

const MYCALL: &str = "K1ABC";
const HISCALL: &str = "W9XYZ";
const NFA: i32 = 200;
const NFB: i32 = 4000;
const NFQSO: i32 = 1500;

fn read_wav_i16(path: &str) -> Vec<i16> {
    let b = std::fs::read(path).expect("read wav");
    let mut i = 12usize;
    while i + 8 <= b.len() {
        let sz = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = i + 8;
        if &b[i..i + 4] == b"data" {
            let end = (body + sz).min(b.len());
            let mut s: Vec<i16> = b[body..end]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            s.resize(s.len().max(ft8::NMAX), 0);
            return s;
        }
        i = body + sz + (sz & 1);
    }
    panic!("no data chunk in {path}");
}

fn write_wav_i16(path: &std::path::Path, s: &[i16]) {
    let data = s.len() as u32 * 2;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&12_000u32.to_le_bytes());
    b.extend_from_slice(&24_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for v in s {
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b).expect("write wav");
}

/// The slot's UTC seconds of day from a WSJT-X `yymmdd_hhmmss.wav` name, as `jt9` reads it.
fn nutc_of(path: &str) -> i32 {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let hms = stem.rsplit('_').next().unwrap_or("");
    if hms.len() != 6 || !hms.bytes().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    let n: i32 = hms.parse().unwrap_or(0);
    (n / 10_000) * 3600 + (n / 100 % 100) * 60 + n % 100
}

fn zeroed_after(iwave: &[i16], n: usize) -> Vec<i16> {
    let mut v = iwave.to_vec();
    v[n..].fill(0);
    v
}

fn decode(depth: i32, path: &str) {
    let iwave = read_wav_i16(path);
    let nutc = nutc_of(path);
    let t = Instant::now();
    let mut carry = ft8::Carry::default();
    let mut all: Vec<ft8::Decode> = Vec::new();
    let run = |iw: &[i16], partial: bool, carry: &mut ft8::Carry| {
        ft8::decode_frame_carry(
            iw,
            NFA,
            NFB,
            depth,
            MYCALL,
            HISCALL,
            0,
            NFQSO,
            NFQSO,
            nutc,
            !partial,
            partial,
            true,
            false,
            false,
            &mut |_| false,
            carry,
        )
    };
    // jt9.f90's from-disk sequence: 41, 47, 50 half-symbols.
    all.extend(run(&zeroed_after(&iwave, 41 * 3456), true, &mut carry));
    ft8::subtract_early(
        &zeroed_after(&iwave, 47 * 3456),
        depth,
        &mut carry,
        &mut || false,
    );
    all.extend(run(&iwave, false, &mut carry));
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let mut seen = std::collections::HashSet::new();
    let out = std::io::stdout();
    let mut out = out.lock();
    for d in all {
        if seen.insert(d.message.trim().to_string()) {
            writeln!(
                out,
                "D\t{}\t{:.1}\t{:.0}\t{}",
                d.snr,
                d.dt,
                d.freq,
                d.message.trim()
            )
            .unwrap();
        }
    }
    writeln!(out, "T\t{ms:.1}").unwrap();
}

/// One period through the multithreaded decoder with exactly the parameters
/// `jt9 -8 -M -C <cycles> -E <sensitivity> -R 3 -N <threads>` gives it from disk (jt9.f90:336-412: nzhsym 49 for
/// the default decoder start, napwid 75, K1ABC / W9XYZ / EN37, RX 1500 Hz, 200-4000 Hz, AP on,
/// and every field jt9 does not set left zero: no TX frequency, calls not flagged standard, NUL
/// base calls). Run on a thread with the stack OpenMP code needs. `swl`: JTDX's SWL mode on
/// (Nexus's Keen and Ludicrous; jt9 has no such option).
fn decode_var(threads: i32, share: &str, path: &str, cycles: i32, sensitivity: i32, swl: bool) {
    let iwave = read_wav_i16(path);
    let nutc_s = nutc_of(path);
    let threads_c = threads;
    let share = share.to_string();
    // FT8VAR_STACK (bytes) overrides the decoding thread's stack, to measure what it needs.
    let stack = std::env::var("FT8VAR_STACK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(ft8::OPENMP_THREAD_STACK);
    let (decodes, ms) = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            assert!(
                ft8::var_load_callsign_db(std::path::Path::new(&share)),
                "ALLCALL7.TXT not found or short in {share}"
            );
            let mut p = ft8::Ft8VarParams {
                nzhsym: 49,
                nutc: (nutc_s / 3600) * 10_000 + (nutc_s / 60 % 60) * 100 + nutc_s % 60,
                nfa: NFA,
                nfb: NFB,
                nfqso: NFQSO,
                napwid: 75,
                nmt: threads_c,
                nft8cycles: cycles,
                nft8rxfsens: 3,
                ncandthin: 100,
                lft8lowth: (sensitivity >= 2).into(),
                lft8subpass: (sensitivity >= 3).into(),
                lft8apon: 1,
                lcommonft8b: 1,
                lwidedxcsearch: 1,
                lswl: swl.into(),
                ..ft8::Ft8VarParams::default()
            };
            p.mycall = *b"K1ABC       ";
            p.hiscall = *b"W9XYZ       ";
            p.mybcall = [0; 12];
            p.hisbcall = [0; 12];
            p.hisgrid = *b"EN37  ";
            let t = Instant::now();
            let d = ft8::decode_frame_var_raw(&iwave, &p, &mut |_| {});
            (d, t.elapsed().as_secs_f64() * 1000.0)
        })
        .expect("spawn decode thread")
        .join()
        .expect("decode thread");
    // Every record as the decoder reported it, in its order (jt9 prints duplicates too unless
    // told to hide them), with the frequency unrounded: jt9 prints Fortran's nint, which rounds
    // halves away from zero, and the comparison does the same.
    let out = std::io::stdout();
    let mut out = out.lock();
    for d in decodes {
        writeln!(
            out,
            "D\t{}\t{:.1}\t{:.3}\t{}",
            d.snr,
            d.dt,
            d.freq,
            d.message.trim()
        )
        .unwrap();
    }
    writeln!(out, "T\t{ms:.1}").unwrap();
}

/// Deterministic LCG + Box-Muller, as the decode-parity tests use.
struct Rng(u64);
impl Rng {
    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[(self.next_f64() * v.len() as f64) as usize % v.len()]
    }
    fn gauss(&mut self) -> f32 {
        let u1 = (self.next_f64() + 1e-12).min(1.0);
        let u2 = self.next_f64();
        ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
    }
}

/// A standard (non-hashed) callsign: 1-2 prefix chars, a digit, 1-3 suffix letters.
fn standard_call(c: &str) -> bool {
    let b = c.as_bytes();
    let Some(d) = b.iter().rposition(|x| x.is_ascii_digit()) else {
        return false;
    };
    (1..=2).contains(&d)
        && b[..d].iter().all(|x| x.is_ascii_alphanumeric())
        && b[..d].iter().any(|x| x.is_ascii_uppercase())
        && (1..=3).contains(&(b.len() - d - 1))
        && b[d + 1..].iter().all(|x| x.is_ascii_uppercase())
}

fn grid(rng: &mut Rng) -> String {
    let l = |r: &mut Rng| (b'A' + (r.next_f64() * 17.0) as u8) as char; // A..Q: never "RR73"
    let n = |r: &mut Rng| (b'0' + (r.next_f64() * 10.0) as u8 % 10) as char;
    format!("{}{}{}{}", l(rng), l(rng), n(rng), n(rng))
}

fn message(rng: &mut Rng, calls: &[String]) -> String {
    let a = rng.pick(calls).clone();
    let mut b = rng.pick(calls).clone();
    while b == a {
        b = rng.pick(calls).clone();
    }
    let rpt = (rng.uniform(-24.0, 10.0)) as i32;
    match (rng.next_f64() * 6.0) as u32 {
        0 => format!("CQ {b} {}", grid(rng)),
        1 => format!("{a} {b} {}", grid(rng)),
        2 => format!("{a} {b} {rpt:+03}"),
        3 => format!("{a} {b} R{rpt:+03}"),
        4 => format!("{a} {b} RR73"),
        _ => format!("{a} {b} 73"),
    }
}

fn synth(outdir: &str, allcall: &str, nfiles: usize, nsig: usize, seed: u64) {
    let text = std::fs::read_to_string(allcall).expect("read ALLCALL7.TXT");
    let calls: Vec<String> = text
        .lines()
        .filter(|l| !l.starts_with("//"))
        .filter_map(|l| l.split(',').next())
        .map(|c| c.trim().to_string())
        .filter(|c| standard_call(c))
        .collect();
    assert!(calls.len() > 1000, "too few standard calls in {allcall}");
    let dir = std::path::Path::new(outdir);
    std::fs::create_dir_all(dir).unwrap();
    let mut truth = std::fs::File::create(dir.join("truth.tsv")).unwrap();
    let mut rng = Rng(seed);
    let bw_ratio = 2500.0 / (ft8::SAMPLE_RATE / 2.0);
    // 12:00:00 onward, one slot apart, so each name parses as a real slot time.
    let mut slot = 12 * 3600;
    let name = |slot: i32| {
        format!(
            "261114_{:02}{:02}{:02}.wav",
            slot / 3600,
            slot / 60 % 60,
            slot % 60
        )
    };
    for f in 0..nfiles + 6 {
        let mut dd = vec![0f32; ft8::NMAX];
        let file = name(slot);
        slot += 15;
        if f < nfiles {
            let mut used = std::collections::HashSet::new();
            let mut k = 0;
            while k < nsig {
                let msg = message(&mut rng, &calls);
                if !used.insert(msg.clone()) {
                    continue;
                }
                let tones = ft8::encode(&msg);
                if tones.len() != ft8::NN {
                    continue;
                }
                let f0 = rng.uniform(250.0, 2850.0) as f32;
                let dt = rng.uniform(-0.3, 1.0) as f32;
                let snr = rng.uniform(-24.0, -6.0) as f32;
                let wave = ft8::gen_wave(&tones, ft8::SAMPLE_RATE, f0);
                let sig = (2.0 * bw_ratio).sqrt() * 10f32.powf(0.05 * snr);
                let noff = ((0.5 + dt) * ft8::SAMPLE_RATE) as isize;
                for (i, &w) in wave.iter().enumerate() {
                    let j = noff + i as isize;
                    if (0..ft8::NMAX as isize).contains(&j) {
                        dd[j as usize] += sig * w;
                    }
                }
                writeln!(truth, "{file}\t{snr:.1}\t{dt:.2}\t{f0:.0}\t{msg}").unwrap();
                k += 1;
            }
        } else {
            writeln!(truth, "{file}\t\t\t\t").unwrap();
        }
        let iwave: Vec<i16> = dd
            .iter()
            .map(|&s| ((s + rng.gauss()) * 100.0).clamp(-32768.0, 32767.0) as i16)
            .collect();
        write_wav_i16(&dir.join(&file), &iwave);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("decode") if args.len() == 3 => decode(args[1].parse().expect("depth"), &args[2]),
        Some("decodevar") if (4..=7).contains(&args.len()) => decode_var(
            args[1].parse().expect("threads"),
            &args[2],
            &args[3],
            args.get(4).map_or(3, |a| a.parse().expect("cycles")),
            args.get(5).map_or(3, |a| a.parse().expect("sensitivity")),
            args.get(6).is_some_and(|a| a == "swl"),
        ),
        Some("synth") if args.len() == 6 => synth(
            &args[1],
            &args[2],
            args[3].parse().expect("nfiles"),
            args[4].parse().expect("nsig"),
            args[5].parse().expect("seed"),
        ),
        _ => {
            eprintln!(
                "usage: decode_bench decode <depth> <wav>\n       \
                 decode_bench decodevar <threads> <dir with ALLCALL7.TXT> <wav> [cycles] [sensitivity] [swl]\n       \
                 decode_bench synth <outdir> <ALLCALL7.TXT> <nfiles> <nsig> <seed>"
            );
            std::process::exit(2);
        }
    }
}
