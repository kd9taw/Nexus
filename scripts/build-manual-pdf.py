#!/usr/bin/env python3
"""build-manual-pdf.py — the Nexus manual as one printable PDF.

Concatenates the quick start and every guide page, in the order docs/guide/index.md
lists them, and prints it through Chrome DevTools Protocol.

WHY CDP AND NOT `--print-to-pdf`. The CLI flag cannot set a header or footer template,
so a CLI-built PDF has no page numbers — and a 115-page reference with no page numbers
is not a manual, it is a scroll. Driving Page.printToPDF over CDP gives a real running
footer. Chrome does not implement CSS `@page` margin boxes, so this is the only route.

WHY THE BODY IS LIGHT AND ONLY THE COVER IS DARK. The app is a dark-theme cockpit and
the brand is near-black, but this document gets printed and read at a bench. A dark
body would drink ink, grey out on any home printer, and lose the screenshots into the
page. The cover carries the brand; the body serves the reader.

Palette is the site's own (`hamtoolssite/src/styles/global.css` @theme): signal green
#3ddc8c, amber #ffc233, teal #4cc9f0, ink #0a0e13.
"""

from __future__ import annotations

import argparse
import base64
import html
import json
import os
import pathlib
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

REPO = pathlib.Path(__file__).resolve().parent.parent
GUIDE = REPO / "docs/guide"

INK, SIGNAL, AMBER, TEAL = "#0a0e13", "#3ddc8c", "#ffc233", "#4cc9f0"

CSS = """
@page { size: A4; margin: 20mm 17mm 18mm 17mm; }
@page :first { margin: 0; }

body { font: 10.5pt/1.55 "Inter", "Segoe UI", Roboto, -apple-system, sans-serif;
       color: #14181d; margin: 0; -webkit-font-smoothing: antialiased; }

/* ---------- cover: the only dark surface in the document ---------- */
.cover { background: #0a0e13; color: #eaf0f7; height: 297mm; width: 210mm;
         box-sizing: border-box; padding: 30mm 24mm 18mm;
         display: flex; flex-direction: column;
         page-break-after: always; break-after: page; overflow: hidden; }
.cover .kicker { font: 600 9pt/1 "JetBrains Mono", ui-monospace, monospace;
                 letter-spacing: .32em; text-transform: uppercase; color: #3ddc8c; }
.cover h1 { font-size: 62pt; line-height: .95; margin: 7mm 0 0; letter-spacing: -.02em;
            font-weight: 800; border: 0; color: #eaf0f7; }
.cover .rule { height: 3px; width: 42mm; background: #3ddc8c; margin: 7mm 0 6mm; }
.cover .sub { font-size: 15pt; line-height: 1.35; color: #cbd8e6; max-width: 118mm; font-weight: 400; }
.cover .lede { font-size: 10.5pt; line-height: 1.6; color: #9fb0c6; max-width: 122mm; margin-top: 7mm; }

/* The modes, named. On a ham manual this is the most informative thing a cover can
   carry — it answers "will it do what I operate?" before the reader opens it. */
.modes { margin-top: 13mm; display: flex; flex-wrap: wrap; gap: 2mm 2.5mm; max-width: 150mm; }
.modes span { font: 500 8pt/1 "JetBrains Mono", ui-monospace, monospace; letter-spacing: .06em;
              color: #cbd8e6; border: 1px solid #2a3a55; border-radius: 3px;
              padding: 1.7mm 2.4mm; background: #111b28; }
.modes .note { border: 0; background: none; padding: 1.7mm 0 1.7mm 1mm; color: #3ddc8c; }

/* What is actually in the book, by group — substance, not filler. */
.inside { margin-top: 12mm; display: flex; gap: 9mm; max-width: 152mm; }
.inside div { flex: 1; }
.inside h4 { font: 600 7.5pt/1 "JetBrains Mono", ui-monospace, monospace; letter-spacing: .22em;
             text-transform: uppercase; color: #4cc9f0; margin: 0 0 2.5mm; }
.inside p { font-size: 8.5pt; line-height: 1.5; color: #8a9cb2; margin: 0; }

/* A panadapter slice: the one visual that says "radio" without a stock photo.
   Full-bleed at the foot so it reads as a baseline rather than a floating object. */
.spectrum { margin: auto 0 0; height: 30mm; display: flex; align-items: flex-end;
            gap: 1.4px; opacity: .45; }
.spectrum i { flex: 1; background: linear-gradient(to top, #3ddc8c 0%, #4cc9f0 60%, rgba(76,201,240,0) 100%);
              border-radius: 1px 1px 0 0; }
.cover .foot { display: flex; justify-content: space-between; align-items: baseline;
               border-top: 1px solid #22304a; padding-top: 4.5mm; margin-top: 5mm;
               font: 9pt/1.4 "JetBrains Mono", ui-monospace, monospace; color: #7d8fa8; }
.cover .foot b { color: #ffc233; font-weight: 600; }

/* ---------- contents ---------- */
.toc { page-break-after: always; break-after: page; }
.toc h2 { font-size: 20pt; color: #0a0e13; border-bottom: 3px solid #3ddc8c;
          padding-bottom: 3mm; margin: 0 0 8mm; }
.toc ol { list-style: none; padding: 0; margin: 0; column-count: 2; column-gap: 12mm; }
.toc li { margin: 0 0 3.4mm; break-inside: avoid; font-size: 10.5pt; }
.toc .n { font: 600 8.5pt "JetBrains Mono", ui-monospace, monospace; color: #3ddc8c;
          margin-right: 3mm; }

/* ---------- chapter openers ---------- */
.chapter { page-break-before: always; break-before: page; }
.chapter:first-of-type { page-break-before: avoid; break-before: avoid; }
.chapter > h1 { font-size: 25pt; line-height: 1.12; margin: 0 0 5mm; letter-spacing: -.015em;
                color: #0a0e13; border: 0; padding: 0; font-weight: 750; }
.chapter > h1::before { content: attr(data-n); display: block;
                        font: 600 8.5pt/1 "JetBrains Mono", ui-monospace, monospace;
                        letter-spacing: .3em; color: #3ddc8c; margin-bottom: 3.5mm; }
.chapter > h1::after { content: ""; display: block; height: 3px; width: 26mm;
                       background: #3ddc8c; margin-top: 5mm; }

h2 { font-size: 13.5pt; margin: 9mm 0 2.5mm; color: #0a0e13; font-weight: 700;
     letter-spacing: -.005em; }
h3 { font-size: 11pt; margin: 6mm 0 1.5mm; color: #2a3442; font-weight: 650; }
h1, h2, h3 { page-break-after: avoid; break-after: avoid; }
p { margin: 0 0 3mm; }
p, li { orphans: 3; widows: 3; }
ul, ol { margin: 0 0 3mm; padding-left: 5.5mm; }
li { margin: 0 0 1.4mm; }
strong { font-weight: 650; color: #0a0e13; }

code { font: 9pt/1.4 "JetBrains Mono", ui-monospace, Consolas, monospace;
       background: #eef2f7; color: #1b3a5c; padding: .08em .32em; border-radius: 3px; }
pre { background: #f6f8fb; border: 1px solid #dde5ef; border-left: 3px solid #4cc9f0;
      padding: 3mm 4mm; border-radius: 4px; overflow-x: auto; margin: 0 0 3.5mm;
      page-break-inside: avoid; break-inside: avoid; }
pre code { background: none; padding: 0; color: #14181d; }

/* An image is evidence, not decoration: never let one straddle a page break. */
img { max-width: 100%; height: auto; display: block; margin: 5mm auto 2mm;
      border: 1px solid #d3dce8; border-radius: 4px;
      page-break-inside: avoid; break-inside: avoid; }

table { border-collapse: collapse; width: 100%; margin: 0 0 4mm; font-size: 9pt;
        page-break-inside: avoid; break-inside: avoid; }
th, td { border: 1px solid #dde5ef; padding: 1.8mm 2.4mm; text-align: left; vertical-align: top; }
th { background: #0a0e13; color: #eaf0f7; font-weight: 600; letter-spacing: .01em; }
tr:nth-child(even) td { background: #f7f9fc; }

blockquote { margin: 0 0 3.5mm; padding: 2.5mm 4mm; border-left: 3px solid #4cc9f0;
             background: #f2f9fd; color: #23394d; }

/* "Honest limits" closes every guide page and is this project's signature — the part
   that says what a thing does NOT do. It earns its own treatment. */
.limits { background: #fffaf0; border: 1px solid #f0dcae; border-left: 3px solid #ffc233;
          border-radius: 4px; padding: 3mm 4mm 1mm; margin: 5mm 0 4mm;
          page-break-inside: avoid; break-inside: avoid; }
.limits h2 { margin: 0 0 2mm; font-size: 10.5pt; color: #7a5200; letter-spacing: .01em; }
.limits h2::before { content: "▲ "; color: #ffc233; }
.limits ul { margin-bottom: 2mm; }
.limits li { color: #4a3a18; }

a { color: #14181d; text-decoration: none; border-bottom: 1px solid #b9c6d6; }
hr { border: 0; border-top: 1px solid #e3e9f1; margin: 6mm 0; }
"""

FOOTER = """
<style>
  div { font: 7.5pt "Inter", sans-serif; color: #8a97a8; width: 100%;
        padding: 0 17mm; display: flex; justify-content: space-between; }
  b { color: #3d5a72; font-weight: 500; }
</style>
<div><span>Nexus __VER__ — Quick start &amp; reference manual</span>
     <span><b class="pageNumber"></b> / <span class="totalPages"></span></span></div>
"""


def guide_order() -> list[pathlib.Path]:
    """Order comes from index.md, so the PDF follows the guide's own narrative."""
    index = (GUIDE / "index.md").read_text(encoding="utf-8")
    seen, out = set(), []
    for m in re.finditer(r"\]\(([a-z0-9-]+)\.md\)", index):
        name = m.group(1)
        if name in seen or name == "index":
            continue
        seen.add(name)
        if (GUIDE / f"{name}.md").exists():
            out.append(GUIDE / f"{name}.md")
    # A page the index forgot still ships. Silently missing from the manual is the
    # failure worth engineering against.
    for p in sorted(GUIDE.glob("*.md")):
        if p.stem not in seen and p.stem != "index":
            out.append(p)
            print(f"  note: {p.name} is not linked from index.md — appended", file=sys.stderr)
    return out


def md_to_html(md: str, base: pathlib.Path) -> str:
    """A small, predictable Markdown subset — the manual uses a narrow set of
    constructs, and staying dependency-free keeps the PDF buildable anywhere."""
    out, lines = [], md.split("\n")
    i, n = 0, len(lines)
    in_code = False
    lst: str | None = None

    def close_list():
        nonlocal lst
        if lst:
            out.append(f"</{lst}>"); lst = None

    def inline(s: str) -> str:
        s = html.escape(s)
        s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
        s = re.sub(r"!\[([^\]]*)\]\(([^)]+)\)",
                   lambda m: f'<img src="{(base / m.group(2)).resolve().as_uri()}" alt="{m.group(1)}">', s)
        s = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r"<a>\1</a>", s)
        s = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", s)
        s = re.sub(r"(?<![\w*])\*([^*\n]+)\*(?![\w*])", r"<em>\1</em>", s)
        return s

    while i < n:
        ln = lines[i]
        if ln.startswith("```"):
            if in_code: out.append("</code></pre>"); in_code = False
            else: close_list(); out.append("<pre><code>"); in_code = True
            i += 1; continue
        if in_code:
            out.append(html.escape(ln)); i += 1; continue
        if ln.strip().startswith("<!--"):
            i += 1; continue

        if re.match(r"^\|.+\|$", ln.strip()) and i + 1 < n and re.match(r"^\|[\s:|-]+\|$", lines[i + 1].strip()):
            close_list()
            cells = [c.strip() for c in ln.strip().strip("|").split("|")]
            out.append("<table><thead><tr>" + "".join(f"<th>{inline(c)}</th>" for c in cells) + "</tr></thead><tbody>")
            i += 2
            while i < n and re.match(r"^\|.+\|$", lines[i].strip()):
                cells = [c.strip() for c in lines[i].strip().strip("|").split("|")]
                out.append("<tr>" + "".join(f"<td>{inline(c)}</td>" for c in cells) + "</tr>")
                i += 1
            out.append("</tbody></table>"); continue

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
        para = [ln]; i += 1
        while i < n and lines[i].strip() and not re.match(r"^(#{1,6}\s|\s*[-*]\s|\s*\d+\.\s|>|\||```)", lines[i]):
            para.append(lines[i]); i += 1
        out.append(f"<p>{inline(' '.join(para))}</p>")

    close_list()
    if in_code: out.append("</code></pre>")
    body = "\n".join(out)

    # Wrap the "Honest limits" section so it can be styled as the callout it is.
    body = re.sub(r"(<h2>Honest limits</h2>)(.*?)(?=<h2>|$)",
                  r'<div class="limits">\1\2</div>', body, flags=re.S)
    return body


def render_pdf(browser: str, src: pathlib.Path, out: pathlib.Path, footer: str) -> None:
    """Print via CDP — the CLI flag cannot set a footer, and a reference manual
    without page numbers is not usable at a bench."""
    import websockets.sync.client as wsc

    with socket.socket() as s:
        s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]
    # mkdtemp + rmtree(ignore_errors) rather than the context manager: headless Chrome can
    # still hold a lock file in its user-data-dir the instant after we terminate it, and the
    # manager's cleanup then raises "Directory not empty" AFTER the PDF is already written —
    # a spurious non-zero exit that would fail the release job over a temp file.
    td = tempfile.mkdtemp()
    try:
        env = dict(os.environ)
        runtime = pathlib.Path(td) / "xdg"; runtime.mkdir(mode=0o700)
        env["XDG_RUNTIME_DIR"] = str(runtime)
        proc = subprocess.Popen(
            [browser, "--headless", "--disable-gpu", "--no-sandbox",
             f"--remote-debugging-port={port}", f"--user-data-dir={pathlib.Path(td)/'p'}",
             "about:blank"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
        try:
            ws_url = None
            for _ in range(120):
                try:
                    tabs = json.loads(urllib.request.urlopen(
                        f"http://127.0.0.1:{port}/json/list", timeout=1).read())
                    page = next((t for t in tabs if t.get("type") == "page"), None)
                    if page:
                        ws_url = page["webSocketDebuggerUrl"]; break
                except Exception:
                    pass
                time.sleep(0.5)
            if not ws_url:
                raise RuntimeError("Chrome never exposed a CDP page target")

            with wsc.connect(ws_url, max_size=200 * 1024 * 1024) as ws:
                mid = iter(range(1, 10_000))

                def call(method, params=None):
                    i = next(mid)
                    ws.send(json.dumps({"id": i, "method": method, "params": params or {}}))
                    while True:
                        msg = json.loads(ws.recv())
                        if msg.get("id") == i:
                            if "error" in msg:
                                raise RuntimeError(f"{method}: {msg['error']}")
                            return msg.get("result", {})

                call("Page.enable")
                call("Page.navigate", {"url": src.as_uri()})
                time.sleep(6)  # images + fonts; the doc is local so this is generous
                res = call("Page.printToPDF", {
                    "printBackground": True, "preferCSSPageSize": True,
                    "displayHeaderFooter": True,
                    "headerTemplate": "<span></span>", "footerTemplate": footer,
                    "marginTop": 0.79, "marginBottom": 0.71,
                    "marginLeft": 0.67, "marginRight": 0.67,
                })
                out.write_bytes(base64.b64decode(res["data"]))
        finally:
            proc.terminate()
            try: proc.wait(timeout=10)
            except subprocess.TimeoutExpired: proc.kill()
    finally:
        shutil.rmtree(td, ignore_errors=True)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=pathlib.Path, default=REPO / "docs/Nexus-Manual.pdf")
    ap.add_argument("--version", default="1.0.0")
    args = ap.parse_args()

    # google-chrome first: the snap chromium cannot create its XDG_RUNTIME_DIR when
    # /run/user/<uid> is root-owned (as on this box) and fails with a generic
    # "internal error" that reads like a rendering fault rather than a sandbox one.
    browser = (shutil.which("google-chrome") or shutil.which("chromium")
               or shutil.which("chromium-browser"))
    if not browser:
        sys.exit("no google-chrome/chromium on PATH — cannot render the PDF")

    pages = [REPO / "docs/quick-start.md"] + guide_order()
    titles = []
    for p in pages:
        first = next((l for l in p.read_text(encoding="utf-8").split("\n") if l.startswith("# ")), p.stem)
        titles.append(first.lstrip("# ").strip())

    bars = "".join(
        f'<i style="height:{h}%"></i>' for h in
        [7, 12, 9, 18, 14, 26, 19, 38, 30, 55, 41, 72, 58, 91, 66, 100, 74, 88, 61, 79,
         52, 68, 44, 57, 36, 48, 29, 39, 23, 31, 17, 25, 13, 19, 10, 15, 8, 12, 6, 9]
    )
    # Every mode the app operates. Transmitting ones are highlighted, which is the
    # distinction a reader actually cares about and the one the docs got wrong for
    # months ("nine transmitting" when nothing ships receive-only).
    modes = ["FT8", "FT4", "FST4", "FST4W", "Q65", "MSK144", "JT65", "WSPR",
             "TempoFast", "TempoDeep", "CW", "SSB", "FM", "RTTY", "SSTV", "APRS"]
    mode_chips = ("".join(f"<span>{m}</span>" for m in modes)
                  + '<span class="note">— all transmit and receive</span>')

    cover = f"""<div class="cover">
      <div class="kicker">Amateur radio operations center</div>
      <h1>Nexus</h1>
      <div class="rule"></div>
      <div class="sub">Quick start &amp; reference manual</div>
      <div class="lede">Every section of the app, screen by screen — what each control does, the
        workflows that use it, and what each part deliberately does <strong style="color:#eaf0f7">
        not</strong> do.</div>
      <div class="modes">{mode_chips}</div>
      <div class="inside">
        <div><h4>Operating</h4><p>The digital cockpit, phone, CW, RTTY, SSTV and APRS —
          each screen walked pane by pane.</p></div>
        <div><h4>Chasing</h4><p>The Needed board and its evidence, spots, DXpeditions,
          propagation, satellites and the logbook.</p></div>
        <div><h4>Station</h4><p>Rig and audio setup, memories, radio programming,
          contesting, and all eight Settings tabs.</p></div>
      </div>
      <div class="spectrum">{bars}</div>
      <div class="foot"><span>Version <b>{html.escape(args.version)}</b> · KD9TAW</span>
        <span>Free software · GPLv3</span></div>
    </div>"""

    toc = ('<div class="toc"><h2>Contents</h2><ol>'
           + "".join(f'<li><span class="n">{i:02d}</span>{html.escape(t)}</li>'
                     for i, t in enumerate(titles, 1))
           + "</ol></div>")

    body = [cover, toc]
    for i, (p, t) in enumerate(zip(pages, titles), 1):
        h = md_to_html(p.read_text(encoding="utf-8"), p.parent)
        # Tag the H1 so the chapter opener can number it.
        h = re.sub(r"<h1>", f'<h1 data-n="Section {i:02d}">', h, count=1)
        body.append(f'<div class="chapter">{h}</div>')
        print(f"  {i:02d}  {p.relative_to(REPO)}")

    doc = (f"<!doctype html><meta charset=utf-8><title>Nexus {args.version} Manual</title>"
           f"<style>{CSS}</style>" + "\n".join(body))

    with tempfile.TemporaryDirectory() as td:
        src = pathlib.Path(td) / "manual.html"
        src.write_text(doc, encoding="utf-8")
        args.out.parent.mkdir(parents=True, exist_ok=True)
        render_pdf(browser, src, args.out, FOOTER.replace("__VER__", html.escape(args.version)))

    if not args.out.exists() or args.out.stat().st_size == 0:
        sys.exit("no PDF produced")
    print(f"\n  {args.out}  —  {args.out.stat().st_size/1048576:.1f} MB, {len(pages)} sections")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
