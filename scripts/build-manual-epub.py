#!/usr/bin/env python3
"""build-manual-epub.py — the Nexus manual as one reflowable EPUB.

The companion to build-manual-pdf.py: the SAME source (docs/quick-start.md + the guide
pages, in the order docs/guide/index.md lists them), the SAME narrative order, emitted as
EPUB 3 instead of PDF. EPUB is the format that reflows on a phone and that a Kindle accepts
directly through Send to Kindle — Amazon retired MOBI, so EPUB + the PDF cover desktop,
mobile and Kindle between them with no third format.

WHY PANDOC AND NOT THE PDF's PRINT HTML. The PDF concatenates everything into one print-CSS
document and renders it through Chrome — right for a fixed A4 page, wrong for a reflowable
reader, which wants real chapter navigation (each section a nav entry the reader can jump
to) and no page furniture. Pandoc builds that navigation natively from the top-level
headings, so this goes markdown → EPUB directly rather than reusing the PDF's HTML.

INTER-PAGE LINKS. The guide cross-links pages as `](settings-reference.md#features)`. Merged
into one EPUB those file targets do not exist, so they are rewritten to in-document anchors:
`](page.md#anchor)` → `](#anchor)`, and a bare `](page.md)` → the id pandoc gives that page's
first heading. Pandoc derives heading ids the GitHub way (lowercase, spaces→hyphens), so the
anchors line up.

  ./scripts/build-manual-epub.py --version 1.10.2 --out docs/Nexus-Manual.epub
  PANDOC=/path/to/pandoc ./scripts/build-manual-epub.py …   # override the binary
"""
from __future__ import annotations

import argparse
import os
import pathlib
import re
import shutil
import subprocess
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
GUIDE = REPO / "docs/guide"


def guide_order() -> list[pathlib.Path]:
    """The page order, read from index.md's link list — identical to build-manual-pdf.py so
    the two documents can never fall out of step."""
    index = (GUIDE / "index.md").read_text(encoding="utf-8")
    out: list[pathlib.Path] = []
    seen: set[str] = set()
    for m in re.finditer(r"\]\(([a-z0-9-]+)\.md\)", index):
        name = m.group(1)
        if name in seen:
            continue
        seen.add(name)
        p = GUIDE / f"{name}.md"
        if p.exists():
            out.append(p)
    # Anything shipped but not linked from the index still belongs in the book.
    for p in sorted(GUIDE.glob("*.md")):
        if p.name != "index.md" and p not in out:
            print(f"  note: {p.name} is not linked from index.md — appended", file=sys.stderr)
            out.append(p)
    return out


def heading_id(text: str) -> str:
    """Pandoc's gfm auto-identifier: lowercase, drop punctuation, spaces → hyphens."""
    t = text.strip().lower()
    t = re.sub(r"[^\w\s-]", "", t)
    t = re.sub(r"\s+", "-", t)
    return t.strip("-")


def first_heading_id(md: str) -> str | None:
    for line in md.split("\n"):
        if line.startswith("# "):
            return heading_id(line[2:])
    return None


def preprocess(md: str, page_ids: dict[str, str]) -> str:
    # Drop HTML comments (the TODO screenshot markers) — they render as raw text otherwise.
    md = re.sub(r"<!--.*?-->", "", md, flags=re.DOTALL)
    # `](page.md#anchor)` → `](#anchor)`
    md = re.sub(r"\]\(([a-z0-9-]+)\.md#([\w-]+)\)", r"](#\2)", md)
    # `](page.md)` → the id of that page's first heading (else drop to a top anchor)
    def bare(m: re.Match) -> str:
        pid = page_ids.get(m.group(1))
        return f"](#{pid})" if pid else "](#)"
    md = re.sub(r"\]\(([a-z0-9-]+)\.md\)", bare, md)
    # ANY remaining `.md` link points OUTSIDE this book — the old desktop `manual/` tree,
    # which the guide-based EPUB does not include (these are quick-start's "further reading"
    # pointers, redundant here since every chapter is already present). Drop the link, keep
    # the text: `[label](whatever.md#x)` → `label`.
    md = re.sub(r"\[([^\]]+)\]\([^)]*\.md[^)]*\)", r"\1", md)
    return md


def find_pandoc() -> str:
    for c in (os.environ.get("PANDOC"), shutil.which("pandoc")):
        if c and pathlib.Path(c).exists():
            return c
    sys.exit("pandoc not found — set PANDOC=/path/to/pandoc or install it")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", type=pathlib.Path, default=REPO / "docs/Nexus-Manual.epub")
    ap.add_argument("--version", default="1.0.0")
    args = ap.parse_args()

    pandoc = find_pandoc()
    pages = [REPO / "docs/quick-start.md"] + guide_order()

    # Map each stem to its first-heading id, so a bare `page.md` link lands on that chapter.
    page_ids: dict[str, str] = {}
    for p in pages:
        hid = first_heading_id(p.read_text(encoding="utf-8"))
        if hid:
            page_ids[p.stem] = hid

    parts = []
    for i, p in enumerate(pages, 1):
        body = preprocess(p.read_text(encoding="utf-8"), page_ids)
        parts.append(body.strip())
        print(f"  {i:02d}  {p.relative_to(REPO)}")
    merged = "\n\n".join(parts) + "\n"

    # A metadata block pandoc turns into the EPUB title page + package metadata.
    meta = (
        "---\n"
        'title: "Nexus — Quick Start & Reference Manual"\n'
        'author: "KD9TAW"\n'
        f'date: "Version {args.version}"\n'
        'lang: en-US\n'
        'rights: "Free software · GPL-3.0-only"\n'
        "---\n\n"
    )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        pandoc,
        "--from=gfm",
        "--to=epub3",
        "--toc",
        "--toc-depth=1",       # one nav entry per section (each page's H1)
        "--split-level=1",     # each H1 opens a new EPUB chapter file
        "--metadata", "title=Nexus — Quick Start & Reference Manual",
        "-o", str(args.out),
    ]
    subprocess.run(cmd, input=meta + merged, text=True, check=True)

    if not args.out.exists() or args.out.stat().st_size == 0:
        sys.exit("EPUB was not produced")
    print(f"EPUB: {args.out} ({args.out.stat().st_size // 1024} KB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
