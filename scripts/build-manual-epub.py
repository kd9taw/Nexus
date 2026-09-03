#!/usr/bin/env python3
"""build-manual-epub.py — the Nexus manual as one reflowable, valid EPUB 3.

The companion to build-manual-pdf.py: the SAME source (docs/quick-start.md + the guide
pages, in the order docs/guide/index.md lists them), emitted as EPUB 3 instead of PDF. EPUB
is the format that reflows on a phone and that a Kindle accepts through Send to Kindle
(Amazon retired MOBI), so EPUB + the PDF cover desktop, mobile and Kindle with no third
format.

VALIDATED WITH epubcheck. The first cut failed the official validator three ways, each a
real defect an e-reader can trip on, all fixed here:
  • images — the guide's screenshots are .webp referenced relatively; fed to pandoc raw they
    became <img> links pointing OUTSIDE the book (RSC + a Kindle that would not open it).
    Each is resolved against its page dir, converted to PNG (Kindle-safe) with Pillow, and
    embedded; a missing/unconvertible source drops to its alt text, never a broken link.
  • cross-references — `](settings-reference.md#features)`-style links across pages. Merged
    into one book those file targets vanish, and pandoc's own auto-ids collide ("Features"
    on three pages), so bare `#features` was undefined (RSC-012 ×87). Every heading is given
    a unique id namespaced by its page (`stem__slug`) via header_attributes, and every link
    is resolved against a map of those ids — a target that does not exist drops to plain
    text rather than a dangling anchor.
  • any remaining link that is not an in-book `#anchor`, an `http(s)://` URL or a `mailto:`
    points outside the book (the old desktop `manual/` tree, `guide/`, `install.md`); those
    drop to their text too.
  • dc:date is a real ISO date, not the version string (RSC-005 / OPF-053).

  ./scripts/build-manual-epub.py --version 1.10.2 --out docs/Nexus-Manual.epub
  PANDOC=/path/to/pandoc ./scripts/build-manual-epub.py …
"""
from __future__ import annotations

import argparse
import datetime
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
GUIDE = REPO / "docs/guide"
HEADING = re.compile(r"^(#{1,6})[ \t]+(.+?)[ \t]*$", re.M)
LINK = re.compile(r"(?<!!)\[([^\]]*)\]\(([^)]+)\)")  # (?<!!) so it never matches an ![image]
IMG = re.compile(r"!\[([^\]]*)\]\(([^)]+)\)")


def slugify(text: str) -> str:
    """GitHub's heading slug: strip inline markdown, lowercase, drop punctuation, spaces→-."""
    t = re.sub(r"`([^`]*)`", r"\1", text)          # code spans → their content
    t = re.sub(r"[*_]", "", t)                     # emphasis marks
    t = t.strip().lower()
    t = re.sub(r"[^\w\s-]", "", t)
    return re.sub(r"\s+", "-", t).strip("-")


def guide_order() -> list[pathlib.Path]:
    """Page order from index.md's links — identical to build-manual-pdf.py."""
    index = (GUIDE / "index.md").read_text(encoding="utf-8")
    out: list[pathlib.Path] = []
    seen: set[str] = set()
    for m in re.finditer(r"\]\(([a-z0-9-]+)\.md\)", index):
        name = m.group(1)
        if name not in seen and (GUIDE / f"{name}.md").exists():
            seen.add(name)
            out.append(GUIDE / f"{name}.md")
    for p in sorted(GUIDE.glob("*.md")):
        if p.name != "index.md" and p not in out:
            print(f"  note: {p.name} unlinked from index.md — appended", file=sys.stderr)
            out.append(p)
    return out


def assign_heading_ids(md: str, stem: str) -> tuple[str, dict[str, str], str | None]:
    """Give every heading an explicit id `stem__slug` (deduped within the page), returning the
    rewritten markdown, a {base-slug: first-id} map for link resolution, and the first id."""
    counts: dict[str, int] = {}
    slug_to_id: dict[str, str] = {}
    first: str | None = None

    def repl(m: re.Match) -> str:
        nonlocal first
        hashes, text = m.group(1), m.group(2)
        base = slugify(text)
        n = counts.get(base, 0)
        counts[base] = n + 1
        hid = f"{stem}__{base}" if n == 0 else f"{stem}__{base}-{n}"
        if base not in slug_to_id:
            slug_to_id[base] = hid   # a link to this slug lands on the first occurrence (gh rule)
        if first is None:
            first = hid
        return f"{hashes} {text} {{#{hid}}}"

    return HEADING.sub(repl, md), slug_to_id, first


def resolve_links(md: str, stem: str, ids: dict, first: dict) -> str:
    """Rewrite links to in-book anchors where they resolve, and strip the rest to plain text.
    `ids[stem][slug]` is the id for that page's heading; `first[stem]` its first id."""
    def repl(m: re.Match) -> str:
        label, target = m.group(1), m.group(2).split()[0].strip("<>")
        # in-page anchor: `#frag`
        if target.startswith("#"):
            frag = target[1:]
            dest = ids.get(stem, {}).get(frag)
            return f"[{label}](#{dest})" if dest else (label or "")
        # cross-page: `other.md#frag` / `other.md`
        mm = re.match(r"([a-z0-9-]+)\.md(?:#([\w-]+))?$", target)
        if mm:
            other, frag = mm.group(1), mm.group(2)
            if frag:
                dest = ids.get(other, {}).get(frag)
            else:
                dest = first.get(other)
            return f"[{label}](#{dest})" if dest else (label or "")
        # external links stay; anything else points outside the book → plain text.
        if re.match(r"(https?:|mailto:)", target):
            return m.group(0)
        return label or ""

    return LINK.sub(repl, md)


def embed_images(md: str, base: pathlib.Path, media: pathlib.Path) -> str:
    """Resolve `![alt](rel)` against `base`, convert to PNG in `media`, rewrite to that path so
    pandoc embeds it. Missing/unconvertible → alt text, never a broken link."""
    from PIL import Image

    def one(m: re.Match) -> str:
        alt, rel = m.group(1), m.group(2).split()[0].strip("<>")
        if re.match(r"https?:", rel):
            return m.group(0)
        src = (base / rel).resolve()
        if not src.exists():
            print(f"    image missing, kept as text: {rel}", file=sys.stderr)
            return f"*{alt}*" if alt else ""
        dst = media / (src.stem + ".png")
        try:
            if not dst.exists():
                Image.open(src).convert("RGB").save(dst, "PNG", optimize=True)
        except Exception as e:  # noqa: BLE001
            print(f"    image {rel} unconvertible ({e}); kept as text", file=sys.stderr)
            return f"*{alt}*" if alt else ""
        return f"![{alt}]({dst})"

    return IMG.sub(one, md)


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

    # Pass A — assign every heading a namespaced id and build the resolution maps.
    annotated: list[tuple[pathlib.Path, str]] = []
    ids: dict[str, dict[str, str]] = {}
    first: dict[str, str] = {}
    for p in pages:
        stem = p.stem
        md, slug_map, first_id = assign_heading_ids(p.read_text(encoding="utf-8"), stem)
        annotated.append((p, md))
        ids[stem] = slug_map
        if first_id:
            first[stem] = first_id

    # Pass B — comments out, images embedded, links resolved against the maps.
    media = pathlib.Path(tempfile.mkdtemp(prefix="nexus-manual-img-"))
    parts = []
    for i, (p, md) in enumerate(annotated, 1):
        md = re.sub(r"<!--.*?-->", "", md, flags=re.DOTALL)
        md = embed_images(md, p.parent, media)
        md = resolve_links(md, p.stem, ids, first)
        parts.append(md.strip())
        print(f"  {i:02d}  {p.relative_to(REPO)}")
    merged = "\n\n".join(parts) + "\n"

    meta = (
        "---\n"
        'title: "Nexus — Quick Start & Reference Manual"\n'
        f'subtitle: "Version {args.version}"\n'
        'author: "KD9TAW"\n'
        f'date: "{datetime.date.today().isoformat()}"\n'
        'lang: en-US\n'
        'rights: "Free software · GPL-3.0-only"\n'
        "---\n\n"
    )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        pandoc,
        "--from=commonmark_x",  # CommonMark+extensions: honours our explicit {#id} on headings, GFM-compatible
        "--to=epub3",
        "--toc", "--toc-depth=1",
        "--split-level=1",
        "-o", str(args.out),
    ]
    subprocess.run(cmd, input=meta + merged, text=True, check=True)
    shutil.rmtree(media, ignore_errors=True)

    if not args.out.exists() or args.out.stat().st_size == 0:
        sys.exit("EPUB was not produced")
    print(f"EPUB: {args.out} ({args.out.stat().st_size // 1024} KB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
