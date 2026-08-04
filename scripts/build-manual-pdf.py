#!/usr/bin/env python3
"""build-manual-pdf.py — the Nexus manual as one printable PDF.

Concatenates the quick start and every guide page, in the order docs/guide/index.md
lists them, renders to a single HTML file, and prints it with headless Chromium.

WHY CHROMIUM. pandoc, weasyprint and wkhtmltopdf are not on this box; chromium is.
Its print-to-PDF also renders WebP, which every manual image is, and honours the
@page CSS below — so the page breaks and margins are controlled rather than left
to a converter's defaults.

WHAT IT IS FOR. An operator at the bench with no internet, or someone who wants the
manual on paper. So: images never split across a page break, every section starts on
a fresh page, and links stay visible as text because a printed link that only exists
as a colour is lost.
"""

from __future__ import annotations

import argparse
import os
import html
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
GUIDE = REPO / "docs/guide"

CSS = """
@page { size: A4; margin: 18mm 16mm 20mm 16mm; }
body { font: 10.5pt/1.5 -apple-system, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
       color: #16191d; max-width: none; }
h1 { font-size: 20pt; margin: 0 0 .4em; padding-bottom: .25em; border-bottom: 2px solid #2b6cb0; }
h2 { font-size: 14pt; margin: 1.6em 0 .4em; color: #1a365d; }
h3 { font-size: 11.5pt; margin: 1.2em 0 .3em; color: #2a4365; }
h1, h2, h3 { page-break-after: avoid; break-after: avoid; }
p, li { orphans: 3; widows: 3; }
code { font: 9.5pt/1.4 "SF Mono", Consolas, "Liberation Mono", monospace;
       background: #f2f4f7; padding: .1em .3em; border-radius: 3px; }
pre { background: #f2f4f7; padding: .7em .9em; border-radius: 5px; overflow-x: auto;
      page-break-inside: avoid; break-inside: avoid; }
pre code { background: none; padding: 0; }
/* An image is evidence, not decoration: never let one straddle a page break. */
img { max-width: 100%; height: auto; display: block; margin: 1em auto;
      border: 1px solid #cbd5e0; border-radius: 4px;
      page-break-inside: avoid; break-inside: avoid; }
table { border-collapse: collapse; width: 100%; margin: 1em 0; font-size: 9.5pt;
        page-break-inside: avoid; break-inside: avoid; }
th, td { border: 1px solid #cbd5e0; padding: .4em .6em; text-align: left; vertical-align: top; }
th { background: #edf2f7; }
blockquote { margin: 1em 0; padding: .6em 1em; border-left: 3px solid #2b6cb0;
             background: #f7fafc; }
/* A printed link that is only a colour is lost. Keep the text visible. */
a { color: #2b6cb0; text-decoration: none; }
.page { page-break-before: always; break-before: page; }
.page:first-of-type { page-break-before: avoid; break-before: avoid; }
.cover { text-align: center; padding-top: 60mm; }
.cover h1 { font-size: 34pt; border: 0; }
.cover .sub { font-size: 13pt; color: #4a5568; margin-top: .3em; }
.cover .meta { font-size: 10pt; color: #718096; margin-top: 28mm; }
.toc ol { padding-left: 1.4em; }
.toc li { margin: .25em 0; }
"""


def guide_order() -> list[pathlib.Path]:
    """Page order comes from index.md, so the PDF matches the guide's own narrative."""
    index = (GUIDE / "index.md").read_text(encoding="utf-8")
    seen, out = set(), []
    for m in re.finditer(r"\]\(([a-z0-9-]+)\.md\)", index):
        name = m.group(1)
        if name in seen or name == "index":
            continue
        seen.add(name)
        p = GUIDE / f"{name}.md"
        if p.exists():
            out.append(p)
    # Anything in the directory the index forgot still ships — a page missing from
    # the PDF because a link was dropped is the silent failure worth avoiding.
    for p in sorted(GUIDE.glob("*.md")):
        if p.stem not in seen and p.stem != "index":
            out.append(p)
            print(f"  note: {p.name} is not linked from index.md — appended", file=sys.stderr)
    return out


def md_to_html(md: str, base: pathlib.Path) -> str:
    """A small, predictable Markdown subset. Deliberately not a full parser: the manual
    uses a narrow set of constructs, and a dependency-free converter keeps the PDF
    buildable on any box that has Python and a browser."""
    out, lines = [], md.split("\n")
    i, n = 0, len(lines)
    in_code = in_table = False
    lst: str | None = None

    def close_list():
        nonlocal lst
        if lst:
            out.append(f"</{lst}>")
            lst = None

    def inline(s: str) -> str:
        s = html.escape(s)
        s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
        s = re.sub(r"!\[([^\]]*)\]\(([^)]+)\)",
                   lambda m: f'<img src="{(base / m.group(2)).resolve().as_uri()}" alt="{m.group(1)}">', s)
        s = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r"<a>\1</a>", s)  # links flattened: no targets in print
        s = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", s)
        s = re.sub(r"(?<![\w*])\*([^*\n]+)\*(?![\w*])", r"<em>\1</em>", s)
        return s

    while i < n:
        ln = lines[i]
        if ln.startswith("```"):
            if in_code:
                out.append("</code></pre>"); in_code = False
            else:
                close_list(); out.append("<pre><code>"); in_code = True
            i += 1; continue
        if in_code:
            out.append(html.escape(ln)); i += 1; continue
        if ln.strip().startswith("<!--"):
            i += 1; continue

        if re.match(r"^\|.+\|$", ln.strip()) and i + 1 < n and re.match(r"^\|[\s:|-]+\|$", lines[i + 1].strip()):
            close_list()
            cells = [c.strip() for c in ln.strip().strip("|").split("|")]
            out.append("<table><thead><tr>" + "".join(f"<th>{inline(c)}</th>" for c in cells) + "</tr></thead><tbody>")
            i += 2; in_table = True
            while i < n and re.match(r"^\|.+\|$", lines[i].strip()):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                out.append("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in cells) + "</tr>")
                i += 1
            out.append("</tbody></table>"); in_table = False; continue

        if m := re.match(r"^(#{1,6})\s+(.*)", ln):
            close_list(); lvl = len(m.group(1))
            out.append(f"<h{lvl}>{inline(m.group(2))}</h{lvl}>"); i += 1; continue
        if m := re.match(r"^\s*[-*]\s+(.*)", ln):
            if lst != "ul": close_list(); out.append("<ul>"); lst = "ul"
            out.append(f"<li>{inline(m.group(1))}</li>"); i += 1; continue
        if m := re.match(r"^\s*\d+\.\s+(.*)", ln):
            if lst != "ol": close_list(); out.append("<ol>"); lst = "ol"
            out.append(f"<li>{inline(m.group(1))}</li>"); i += 1; continue
        if m := re.match(r"^>\s?(.*)", ln):
            close_list(); out.append(f"<blockquote>{inline(m.group(1))}</blockquote>"); i += 1; continue
        if re.match(r"^\s*---+\s*$", ln):
            close_list(); i += 1; continue
        if not ln.strip():
            close_list(); i += 1; continue

        close_list()
        para = [ln]
        i += 1
        while i < n and lines[i].strip() and not re.match(r"^(#{1,6}\s|\s*[-*]\s|\s*\d+\.\s|>|\||```)", lines[i]):
            para.append(lines[i]); i += 1
        out.append(f"<p>{inline(' '.join(para))}</p>")

    close_list()
    if in_code:
        out.append("</code></pre>")
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=pathlib.Path, default=REPO / "docs/Nexus-Manual.pdf")
    ap.add_argument("--version", default="1.0.0")
    args = ap.parse_args()

    # google-chrome FIRST, deliberately. The snap chromium cannot create its
    # XDG_RUNTIME_DIR under /run/user/<uid> when that directory is root-owned (as it is
    # on this WSL2 box), and fails with a confusing "internal error" that looks like a
    # rendering fault rather than a sandbox one.
    browser = (shutil.which("google-chrome") or shutil.which("chromium")
               or shutil.which("chromium-browser"))
    if not browser:
        sys.exit("no google-chrome/chromium on PATH — cannot render the PDF")

    pages = [REPO / "docs/quick-start.md"] + guide_order()
    body = [
        '<div class="page cover"><h1>Nexus</h1>'
        f'<div class="sub">Quick start and reference manual</div>'
        f'<div class="meta">Version {args.version} · KD9TAW · GPLv3</div></div>'
    ]

    toc = []
    for p in pages:
        first = next((l for l in p.read_text(encoding="utf-8").split("\n") if l.startswith("# ")), p.stem)
        toc.append(html.escape(first.lstrip("# ").strip()))
    body.append('<div class="page toc"><h1>Contents</h1><ol>'
                + "".join(f"<li>{t}</li>" for t in toc) + "</ol></div>")

    for p in pages:
        body.append(f'<div class="page">{md_to_html(p.read_text(encoding="utf-8"), p.parent)}</div>')
        print(f"  + {p.relative_to(REPO)}")

    doc = (f"<!doctype html><meta charset=utf-8><title>Nexus {args.version} Manual</title>"
           f"<style>{CSS}</style>" + "\n".join(body))

    with tempfile.TemporaryDirectory() as td:
        src = pathlib.Path(td) / "manual.html"
        src.write_text(doc, encoding="utf-8")
        args.out.parent.mkdir(parents=True, exist_ok=True)
        # Give the browser a runtime dir it can actually write to, and its own profile,
        # so a root-owned /run/user/<uid> cannot fail the build.
        env = dict(os.environ)
        runtime = pathlib.Path(td) / "xdg"
        runtime.mkdir(mode=0o700)
        env["XDG_RUNTIME_DIR"] = str(runtime)
        r = subprocess.run(
            [browser, "--headless", "--disable-gpu", "--no-sandbox", "--virtual-time-budget=30000",
             f"--user-data-dir={pathlib.Path(td) / 'profile'}",
             f"--print-to-pdf={args.out}", "--no-pdf-header-footer", src.as_uri()],
            capture_output=True, text=True, timeout=300, env=env,
        )
        if not args.out.exists() or args.out.stat().st_size == 0:
            sys.exit(f"chromium produced no PDF.\n{r.stderr[-2000:]}")

    print(f"\n  {args.out}  —  {args.out.stat().st_size/1048576:.1f} MB, {len(pages)} sections")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
