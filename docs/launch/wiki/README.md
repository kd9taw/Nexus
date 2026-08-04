# The Nexus wikis — what is generated and what is not

This directory is the master copy of the **GitHub wiki** and the **SourceForge
wiki**. Neither wiki is a git remote we push to: the pages here are pasted in by
hand, one at a time, through each site's editor. Nothing outside this directory
is affected by editing a page here, and nothing here reaches an operator until
somebody pastes it.

**`docs/` is the source of truth.** These pages are the shop window; the docs tree
is the stock room. When a fact changes, change it in `docs/` — three of the pages
here then pick it up automatically, and the other four tell you plainly that they
need a human.

---

## Generated — do not hand-edit

| Page | Generated from |
|---|---|
| `FAQ.md` | `docs/faq.md` |
| `Install.md` | `docs/install.md` |
| `Quick-Start.md` | `docs/quick-start.md` |

```
node scripts/gen-wiki.mjs           # rewrite the three pages
node scripts/gen-wiki.mjs --check   # exit 1 if they are behind their source
```

An edit made directly to one of these three is **lost on the next run.** Edit the
`docs/` source instead. If the wiki needs to say something the docs page should not
say, that difference belongs in `scripts/gen-wiki.mjs` as a transform, where it is
applied every run and reviewed once — not typed into the page, where it survives
exactly until the next regeneration.

The generator only does the mechanical things: it drops the screenshot markers,
turns docs-relative links into wiki page names or absolute GitHub URLs, points the
download at GitHub Releases (the docs point at SourceForge), applies the wiki's
en-GB spellings, condenses the heading time estimates, and swaps in each page's own
nav block. It rewrites no prose. The transforms are keyed on exact source text and
the run **fails loudly** if a key stops matching, so a source rewrite cannot quietly
produce a page with a transform missing.

## Hand-maintained — no generator touches these

| Page | Why it is written by hand |
|---|---|
| `Home.md` | The positioning document: what Nexus is, the four problems it answers, what it does not claim. Written for the wiki landing page; no `docs/` page says this. |
| `Rig-Setup.md` | Per-brand CAT/audio setup condensed from `docs/rigs/*.md` into one page. It is where an operator lands when the radio will not talk, so it is ordered by what breaks, not by what the rig docs cover. |
| `_ProjectSummary.md` | The SourceForge project summary. Different length limit, different audience, and its links point at the SourceForge wiki rather than the GitHub one. |
| `Documentation.md` | The link hub. It is a table of contents for pages that live elsewhere, so there is nothing to derive it from. It links `docs/guide/` (canonical) and deliberately never `docs/manual/`. |

These carry facts that go stale the same way any doc does — Settings paths, pane
names, per-rig status. Sweep them when the thing they describe changes; a generator
that overwrote them would destroy copy nobody can regenerate.

`Documentation.md` needs a new row every time a guide is added to `docs/guide/`.
It currently lists all eighteen.

---

## Mirroring a change

1. Edit the `docs/` source (generated pages) or the page itself (hand-maintained).
2. `node scripts/gen-wiki.mjs`, and commit whatever changed.
3. Paste the changed pages into the GitHub wiki and the SourceForge wiki.

Both editors take Markdown, and SourceForge's is CodeMirror — paste the **whole**
page rather than editing in place, so the wiki cannot drift line by line from the
copy here. Wiki page names come from the filenames: `Quick-Start.md` is the page
`Quick-Start`, which is what the `[Quick Start](Quick-Start)` links resolve to.
