#!/usr/bin/env python3
"""The ft8var merge gate: Nexus's multithreaded FT8 decoder on one thread must decode exactly what
WSJT-X 3.0.2's own `jt9 -8 -M -C 3 -E 3 -R 3 -N 1` decodes, record for record, in the same order,
with the same SNR, DT and frequency.

    scripts/ft8var-parity.py --bench <decode_bench> --jt9 <jt9> --share <dir with ALLCALL7.TXT> \\
        <wav or dir>...

decode_bench is crates/ft8/examples/decode_bench.rs (`decodevar 1`), which gives the decoder
jt9's from-disk parameters. Each WAV runs in a fresh process on both sides (decoder state carries
between periods). Exit status 0 when every file matches; otherwise every difference is listed.
"""

import argparse
import math
import re
import subprocess
import sys
import tempfile
from pathlib import Path

JT9_LINE = re.compile(r"^\s*\d{6}\s*(-?\d+)\s+(-?[\d.]+)\s+(\d+)\s+~\s+(.*)$")


def nint(x):
    """Fortran's nint: halves away from zero."""
    return int(math.floor(abs(x) + 0.5)) * (1 if x >= 0 else -1)


def jt9(jt9, share, wav, threads, work):
    out = subprocess.run(
        [jt9, "-8", "-M", "-C", "3", "-E", "3", "-R", "3", "-N", str(threads),
         "-r", share, "-a", work, "-t", work, str(wav)],
        cwd=work, capture_output=True, text=True, check=True).stdout
    recs = []
    for line in out.splitlines():
        m = JT9_LINE.match(line)
        if m:
            recs.append((int(m.group(1)), float(m.group(2)), int(m.group(3)),
                         m.group(4)[:37].rstrip().rstrip("?").strip()))
    return recs


def nexus(bench, share, wav, threads):
    out = subprocess.run([bench, "decodevar", str(threads), share, str(wav)],
                         capture_output=True, text=True, check=True).stdout
    recs = []
    for line in out.splitlines():
        f = line.split("\t")
        if f[0] == "D":
            recs.append((int(f[1]), float(f[2]), nint(float(f[3])), f[4]))
    return recs


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bench", required=True)
    ap.add_argument("--jt9", required=True)
    ap.add_argument("--share", required=True)
    ap.add_argument("--threads", type=int, default=1)
    ap.add_argument("-j", "--jobs", type=int, default=1, help="files decoded at once")
    ap.add_argument("wavs", nargs="+")
    a = ap.parse_args()
    wavs = sorted(w for p in map(Path, a.wavs) for w in (p.glob("*.wav") if p.is_dir() else [p]))
    bad = 0

    def one(w):
        with tempfile.TemporaryDirectory() as work:   # jt9 writes its files beside it
            return w, jt9(a.jt9, a.share, w, a.threads, work), nexus(a.bench, a.share, w, a.threads)

    from concurrent.futures import ThreadPoolExecutor
    with ThreadPoolExecutor(max_workers=max(1, a.jobs)) as pool:
        for w, b, n in pool.map(one, wavs):
            same = b == n
            print(f"{'OK  ' if same else 'DIFF'} {w.name}: jt9 {len(b)}, nexus {len(n)}")
            if not same:
                bad += 1
                for i in range(max(len(b), len(n))):
                    x = b[i] if i < len(b) else None
                    y = n[i] if i < len(n) else None
                    if x != y:
                        print(f"     #{i + 1}: jt9 {x}  nexus {y}")
    print(f"{len(wavs) - bad}/{len(wavs)} files identical")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
