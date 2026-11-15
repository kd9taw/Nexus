#!/usr/bin/env python3
"""FT8 decoder A/B bench: Nexus's decoder against WSJT-X's own jt9, on the same WAVs.

Decoders (each WAV in its own process, so no state carries between files):
  nexus      Nexus's standard decoder, one period's early/47/full steps
             (crates/ft8/examples/decode_bench.rs, `decode 3`)
  jt9        WSJT-X 3.0.2 `jt9 -8 -d 3`        (the standard decoder, Deep)
  jt9var     WSJT-X 3.0.2 `jt9 -8 -M -C 3 -E 3` (the multithreaded JTDX-derived decoder)
  easy, keen, ludicrous (--presets)
             Nexus's JTDX-style decoder (`decodevar`) at each preset's settings: 3 cycles,
             sensitivity 3; Easy 1 thread, Keen 1 thread + SWL, Ludicrous 12 threads + SWL

Corpus:
  --real DIR   real off-air WAVs (yymmdd_hhmmss.wav). No ground truth: a decode one decoder
               alone reports, holding a callsign that is not in ALLCALL7.TXT, is counted as
               "likely false" and listed for a human to judge.
  --synth DIR  a synthetic busy band written by `decode_bench synth` (ground truth in
               truth.tsv): exact true/false/missed counts, recall by SNR, and noise-only files.

CPU is each decoder process's user+sys time (getrusage of the child), wall its elapsed time.
jt9 is built from the WSJTX/wsjtx v3.0.2 tag (`make jt9`); pass its path with --jt9.

Example:
  cargo build --release -p ft8 --example decode_bench
  target/release/examples/decode_bench synth /tmp/syn libtempo/.../ALLCALL7.TXT 24 30 1
  scripts/ft8-decoder-bench.py --bench target/release/examples/decode_bench \\
      --jt9 /path/to/jt9 --share /path/with/ALLCALL7.TXT --real samples/ --synth /tmp/syn
"""

import argparse
import json
import os
import re
import resource
import subprocess
import sys
import tempfile
import time
from pathlib import Path

JT9_LINE = re.compile(r"^\s*\d{6}\s*(-?\d+)\s+(-?[\d.]+)\s+(\d+)\s+~\s+(.*)$")
GRID = re.compile(r"^[A-R]{2}\d{2}([A-X]{2})?$")
REPORT = re.compile(r"^R?[+-]\d{2}$")


def run_timed(cmd, cwd=None):
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    t = time.monotonic()
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    wall = time.monotonic() - t
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    cpu = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
    if p.returncode != 0:
        sys.exit(f"{cmd[0]} failed on {cmd[-1]}: {p.stderr[-2000:]}")
    return p.stdout, cpu, wall


def decode_nexus(bench, wav, depth):
    out, cpu, wall = run_timed([bench, "decode", str(depth), str(wav)])
    decs = []
    for line in out.splitlines():
        f = line.split("\t")
        if f[0] == "D":
            decs.append({"snr": int(f[1]), "dt": float(f[2]), "freq": float(f[3]), "msg": f[4]})
    return decs, cpu, wall


def decode_var(bench, share, wav, threads, swl):
    cmd = [bench, "decodevar", str(threads), share, str(wav), "3", "3"] + (["swl"] if swl else [])
    out, cpu, wall = run_timed(cmd)
    decs, seen, ms = [], set(), None
    for line in out.splitlines():
        f = line.split("\t")
        if f[0] == "D" and f[4] not in seen:
            seen.add(f[4])
            decs.append({"snr": int(f[1]), "dt": float(f[2]), "freq": float(f[3]), "msg": f[4]})
        elif f[0] == "T":
            ms = float(f[1])
    return decs, cpu, wall, ms


def decode_jt9(jt9, share, wav, extra, work):
    out, cpu, wall = run_timed(
        [jt9, "-8", *extra, "-r", share, "-a", work, "-t", work, str(wav)], cwd=work
    )
    decs, seen = [], set()
    for line in out.splitlines():
        m = JT9_LINE.match(line)
        if not m:
            continue
        msg = m.group(4)[:37].rstrip().rstrip("?").strip()
        if msg in seen:
            continue
        seen.add(msg)
        decs.append({"snr": int(m.group(1)), "dt": float(m.group(2)),
                     "freq": float(m.group(3)), "msg": msg})
    return decs, cpu, wall


def load_allcall(share):
    calls = set()
    with open(Path(share) / "ALLCALL7.TXT", encoding="latin-1") as f:
        for line in f:
            if not line.startswith("//") and "," in line:
                calls.add(line.split(",", 1)[0].strip())
    return calls


def call_tokens(msg):
    """Words of a message that look like callsigns (not CQ words, grids, reports or acks)."""
    out = []
    for w in msg.replace(";", " ").split():
        w = w.strip("<>")
        if w in ("CQ", "DE", "QRZ", "RR73", "RRR", "73", "TU", "DX") or GRID.match(w) or REPORT.match(w):
            continue
        if any(c.isdigit() for c in w) and any(c.isalpha() for c in w) and 3 <= len(w) <= 11:
            out.append(w.split("/")[0] if "/" in w and len(w.split("/")[0]) > 2 else w)
    return out


def load_truth(synth):
    truth = {}
    with open(Path(synth) / "truth.tsv") as f:
        for line in f:
            file, snr, _dt, _f0, msg = line.rstrip("\n").split("\t")
            truth.setdefault(file, [])
            if msg:
                truth[file].append((float(snr), msg))
    return truth


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bench", required=True, help="decode_bench example binary")
    ap.add_argument("--jt9", help="WSJT-X 3.0.2 jt9 binary (omit to bench Nexus alone)")
    ap.add_argument("--share", required=True, help="directory holding ALLCALL7.TXT")
    ap.add_argument("--real", action="append", default=[], help="directory of real WAVs")
    ap.add_argument("--synth", help="synthetic corpus directory (with truth.tsv)")
    ap.add_argument("--depth", type=int, default=3)
    ap.add_argument("--json", help="write per-decode results here")
    ap.add_argument("--presets", action="store_true", help="also run Nexus's JTDX-style presets")
    a = ap.parse_args()

    decoders = {"nexus": lambda w, work: decode_nexus(a.bench, w, a.depth)}
    if a.jt9:
        decoders["jt9"] = lambda w, work: decode_jt9(a.jt9, a.share, w, ["-d", str(a.depth)], work)
        decoders["jt9var"] = lambda w, work: decode_jt9(a.jt9, a.share, w, ["-M", "-C", "3", "-E", "3"], work)
    if a.presets:
        for name, threads, swl in (("easy", 1, False), ("keen", 1, True), ("ludicrous", 12, True)):
            decoders[name] = (lambda t, s: lambda w, work: decode_var(a.bench, a.share, w, t, s))(threads, swl)

    real = sorted(p for d in a.real for p in Path(d).glob("*.wav"))
    synth = sorted(Path(a.synth).glob("*.wav")) if a.synth else []
    results = {name: {} for name in decoders}
    with tempfile.TemporaryDirectory() as work:
        for wav in real + synth:
            for name, fn in decoders.items():
                r = fn(wav, work)
                decs, cpu, wall = r[:3]
                results[name][wav.name] = {"decodes": decs, "cpu": cpu, "wall": wall,
                                           "ms": r[3] if len(r) > 3 else None}

    allcall = load_allcall(a.share)
    names = list(decoders)
    lines = []
    say = lines.append
    say(f"FT8 decoder bench, depth {a.depth}; {len(real)} real + {len(synth)} synthetic WAVs")
    say(f"host: {os.cpu_count()} logical CPUs")

    if real:
        say("")
        say("== Real off-air WAVs (no ground truth)")
        say(f"{'file':28}" + "".join(f"{n:>10}" for n in names) + f"{'union':>8}")
        for wav in real:
            sets = {n: {d['msg'] for d in results[n][wav.name]['decodes']} for n in names}
            union = set().union(*sets.values())
            say(f"{wav.name:28}" + "".join(f"{len(sets[n]):>10}" for n in names) + f"{len(union):>8}")
        say("")
        say("per decoder over the real set: decodes / unique to it / likely false (unique AND a call not in ALLCALL7)")
        for n in names:
            tot = uniq = lf = 0
            flagged = []
            for wav in real:
                sets = {m: {d['msg'] for d in results[m][wav.name]['decodes']} for m in names}
                others = set().union(*(sets[m] for m in names if m != n)) if len(names) > 1 else set()
                tot += len(sets[n])
                for msg in sets[n] - others:
                    uniq += 1
                    unknown = [c for c in call_tokens(msg) if c not in allcall]
                    if unknown:
                        lf += 1
                        flagged.append(f"{wav.name}: {msg}  [not in ALLCALL7: {' '.join(unknown)}]")
            say(f"  {n:8} {tot:5} decodes  {uniq:4} unique  {lf:4} likely false")
            for f in flagged:
                say(f"      {f}")
        say("")
        say("pairwise over the real set (A only / both / B only):")
        for i, x in enumerate(names):
            for y in names[i + 1:]:
                ox = both = oy = 0
                for wav in real:
                    sx = {d['msg'] for d in results[x][wav.name]['decodes']}
                    sy = {d['msg'] for d in results[y][wav.name]['decodes']}
                    ox += len(sx - sy)
                    both += len(sx & sy)
                    oy += len(sy - sx)
                say(f"  {x} vs {y}: {ox} / {both} / {oy}")
                for wav in real:
                    sx = {d['msg'] for d in results[x][wav.name]['decodes']}
                    sy = {d['msg'] for d in results[y][wav.name]['decodes']}
                    for m in sorted(sx - sy):
                        say(f"      {x} only  {wav.name}: {m}")
                    for m in sorted(sy - sx):
                        say(f"      {y} only  {wav.name}: {m}")

    if synth:
        truth = load_truth(a.synth)
        busy = [w for w in synth if truth.get(w.name)]
        noise = [w for w in synth if w.name in truth and not truth[w.name]]
        ntrue = sum(len(truth[w.name]) for w in busy)
        buckets = [(-99, -21), (-21, -16), (-16, 99)]
        say("")
        say(f"== Synthetic busy band: {len(busy)} files, {ntrue} signals (SNR -24..-6 dB); {len(noise)} noise-only files")
        say(f"{'decoder':10}{'true':>7}{'recall':>8}{'false':>7}{'noise-false':>12}"
            + "".join(f"{f'recall {lo}..{hi}':>18}" for lo, hi in buckets))
        for n in names:
            tp = fp = nf = 0
            bt = [[0, 0] for _ in buckets]
            for w in busy:
                want = {m: s for s, m in truth[w.name]}
                got = {d['msg'] for d in results[n][w.name]['decodes']}
                tp += len(got & want.keys())
                fp += len(got - want.keys())
                for m, s in want.items():
                    for i, (lo, hi) in enumerate(buckets):
                        if lo < s <= hi:
                            bt[i][1] += 1
                            bt[i][0] += m in got
            for w in noise:
                nf += len(results[n][w.name]['decodes'])
            say(f"{n:10}{tp:>7}{tp / max(ntrue, 1):>8.1%}{fp:>7}{nf:>12}"
                + "".join(f"{f'{b[0]}/{b[1]} {b[0] / max(b[1], 1):.0%}':>18}" for b in bt))
        say("")
        say("false decodes on the synthetic busy band (decoded, not transmitted):")
        for n in names:
            for w in busy:
                want = {m for _, m in truth[w.name]}
                for d in results[n][w.name]['decodes']:
                    if d['msg'] not in want:
                        say(f"  {n:8} {w.name}: {d['msg']}  ({d['snr']} dB, {d['freq']:.0f} Hz)")
            for w in noise:
                for d in results[n][w.name]['decodes']:
                    say(f"  {n:8} {w.name} (noise only): {d['msg']}")

    say("")
    say("== CPU per period (one process per WAV: user+sys, and wall)")
    say("(decode = the decoder's own time, without loading ALLCALL7; the JTDX-style presets only.")
    say(" Nexus's budget for it is 2.1 s: JTDX_FULL_PASS_BUDGET.)")
    say(f"{'decoder':10}{'cpu mean':>10}{'cpu max':>10}{'wall mean':>11}{'wall max':>10}{'decode mean':>13}{'decode max':>12}{'> 2.1 s':>9}")
    for n in names:
        rs = list(results[n].values())
        cpu = [r['cpu'] for r in rs]
        wall = [r['wall'] for r in rs]
        ms = [r['ms'] for r in rs if r.get('ms') is not None]
        dec = (f"{sum(ms) / len(ms) / 1000:>12.2f}s{max(ms) / 1000:>11.2f}s{sum(m > 2100 for m in ms):>9}"
               if ms else "")
        say(f"{n:10}{sum(cpu) / len(cpu):>9.2f}s{max(cpu):>9.2f}s{sum(wall) / len(wall):>10.2f}s{max(wall):>9.2f}s" + dec)

    print("\n".join(lines))
    if a.json:
        Path(a.json).write_text(json.dumps(results, indent=1))


if __name__ == "__main__":
    main()
