#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
make-kit.py — build the Italian translation kit fresh from the application's own catalog.

    python3 translations/it/make-kit.py                    # write the kit
    python3 translations/it/make-kit.py --check            # write nothing; exit 1 if it is stale
    python3 translations/it/make-kit.py --carry filled.csv # rebuild, keeping a translator's work

FOR THE MAINTAINER, not the translator. It writes `nexus-it-translation.csv`, the pin block in
`verify-it.py`, and the counts in `README.md` and `GLOSSARY.md`, and touches nothing else.

WHY A GENERATOR, AND NOT A SECOND HAND-KEPT CSV. The Portuguese kit is kept by hand: every string
change is meant to add its row. That is one more obligation on every commit that touches a string,
and measured on 2026-10-01 it had not held — 1155 of the catalog's 6391 rows were missing from it,
22 of its rows named keys the catalog no longer has, and 27 carried English that had since
changed. A second hand-kept kit would double that cost and drift the same way. So the Italian CSV
is never edited: it is BUILT, from `ui/src/i18n/en.ts` and the splash page, whenever a translator
is about to start, and rebuilt with `--carry` when their work comes back. Nothing between those two
moments needs an Italian row, and a stale CSV in the tree between them is harmless — the verifier's
pins describe the file they were written with, not the catalog of the day.

WHERE EACH COLUMN COMES FROM
  key, english      en.ts itself (through Node's type stripping, so a value is exactly what the
                    application renders), plus the splash page's strings under `splashscreen.`.
                    A plural becomes one row per form, `<key>::one` and `<key>::other`.
  priority          the Portuguese kit's tier for the same key, which a person chose; a key the
                    Portuguese kit lacks takes the most common tier among its nearest neighbours
                    there (the longest shared dotted prefix), and tier 6 when no area matches.
                    The splash page is first-run text, tier 2.
  do_not_translate  the Portuguese kit's list where the key and its English are unchanged (a person
                    curated those); otherwise computed: every {{slot}}, <marker> and {MACRO} token,
                    plus every term the Portuguese kit keeps in English at least 90% of the time it
                    appears, and every term in GLOSSARY.md's list A.
  notes             the Portuguese kit's notes where the key and its English are unchanged, with
                    the language named for Italian; the counted sentences (how many keys share an
                    English text, the plural pair) are always recomputed against this catalog.
                    A new or changed row gets only the notes a rule can make honestly: shared text,
                    plurals, CW macro tokens, and a quote of another control by name.

Needs Python 3 and Node 22.6 or later (the one already used to build ui/). No other dependency.
"""

import argparse
import csv
import importlib.util
import io
import json
import re
import subprocess
import sys
from collections import Counter, OrderedDict
from pathlib import Path

sys.dont_write_bytecode = True        # importing verify-it.py must not leave __pycache__ in the kit

KIT = Path(__file__).resolve().parent
ROOT = KIT.parent.parent

LANG_COLUMN = 'italian'
CSV_NAME = 'nexus-it-translation.csv'
COLUMNS = ['priority', 'key', 'english', LANG_COLUMN, 'do_not_translate', 'notes']
SPLASH_PREFIX = 'splashscreen.'
SPLASH_TIER = '2'
DEFAULT_TIER = '6'
# Areas the Portuguese kit has no key in, so no neighbour can lend a tier. Each was placed by
# reading its strings against the README's tier definitions; everything else falls to tier 6
# (on 2026-10-01 that is the remote browser station's 342 rows).
AREA_TIERS = {
    'quit': '1',        # the logbook-saving dialog at quit, its failure included — errors, tier 1
    'bandMenu': '1',    # the band menu's condition word — band controls, tier 1
    'qso': '3',         # the contact detail view — the logbook, tier 3
}
PLURAL_FORMS = ('zero', 'one', 'two', 'few', 'many', 'other')
CONSISTENT = 0.9                      # a term the Portuguese kit keeps in English this often
# Terms the Italian translator has said ARE translated on the air in Italy. Each is taken off every
# row's do_not_translate list, kept or computed, so the checker stops demanding it. Empty until
# the translator says otherwise; move the term out of GLOSSARY.md's list A in the same change.
RELEASED = set()

RE_PLACEHOLDER = re.compile(r'\{\{\w+\}\}')
RE_OPEN_MARKER = re.compile(r'<[a-zA-Z][a-zA-Z0-9]*>')
RE_CWTOKEN = re.compile(r'(?<!\{)\{[A-Z][A-Z0-9_]*\}(?!\})')
RE_QUOTE = re.compile(r'“([^”]+)”|"([^"]+)"')

# The Portuguese kit's notes name their language in exactly these ways (10 sentence shapes,
# measured); anything else that still names it after this is refused, never shipped.
LANGUAGE_WORDS = (('a Brazilian', 'an Italian'), ('Brazilian', 'Italian'), ('Portuguese', 'Italian'))

CW_NOTE = ('Contains CW macro tokens in SINGLE braces ({NAME}, {RST}...). These are matched '
           'literally by the macro expander — copy them exactly, do not translate or reorder them '
           'against the words that explain them.')
SPLASH_NOTE = ('Shown on the splash page while Nexus moves the logbook into its new database, '
               'before the program itself opens. The splashscreen rows travel together: fill every '
               'one, or none.')


class Refusal(Exception):
    """Something this tool does not understand. It stops rather than guess."""


# ── reading the sources ─────────────────────────────────────────────────────────────────────

def load_catalog(path):
    """en.ts as the application sees it: `EN`, in declaration order."""
    script = ('import(%s).then((m) => process.stdout.write(JSON.stringify(m.EN)))'
              % json.dumps(path.as_uri()))
    attempts = (['node', '--experimental-strip-types', '--no-warnings', '-e', script],
                ['node', '--no-warnings', '-e', script])     # Node that strips types by default
    last = ''
    for cmd in attempts:
        try:
            run = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8')
        except OSError as exc:
            raise Refusal('could not run node (%s). make-kit.py needs Node 22.6 or later.' % exc)
        if run.returncode == 0:
            catalog = json.loads(run.stdout, object_pairs_hook=OrderedDict)
            break
        last = run.stderr.strip()
    else:
        raise Refusal('node could not load %s:\n%s' % (path, last))
    for key, value in catalog.items():
        if isinstance(value, str):
            continue
        if not isinstance(value, dict) or not value or any(
                f not in PLURAL_FORMS or not isinstance(v, str) for f, v in value.items()):
            raise Refusal('%s: an entry that is neither a string nor a plural object' % key)
    return catalog


def load_splash(path):
    """The splash page's English strings, in page order."""
    html = path.read_text(encoding='utf-8')
    m = re.search(r'<script[^>]*id="splash-strings"[^>]*>(.*?)</script>', html, re.S)
    if not m:
        raise Refusal('%s has no splash-strings block' % path)
    strings = json.loads(m.group(1), object_pairs_hook=OrderedDict)
    if 'en' not in strings:
        raise Refusal('%s: the splash-strings block has no "en" entry' % path)
    return strings['en']


def load_csv(path):
    with open(path, encoding='utf-8-sig', newline='') as fh:
        reader = csv.DictReader(fh)
        return reader.fieldnames, list(reader)


def tokens(cell):
    return [t.strip() for t in cell.split('|') if t.strip()]


def glossary_terms(text, section_start, section_end):
    """The terms of one GLOSSARY.md table, as rows of slots; a slot is a list of alternatives.

    `QSO / QSOs` is two slots counted separately; `EN52, EN52XA, EN52XA25` is one slot whose
    count is the rows holding any of them."""
    body = text[text.index(section_start):text.index(section_end)]
    rows = []
    for line in body.splitlines():
        cells = [c.strip() for c in line.strip().strip('|').split('|')]
        if len(cells) < 2 or not line.startswith('|') or set(line) <= set('|-: '):
            continue
        if not re.fullmatch(r'[\d,]+(?: / [\d,]+)*', cells[1]):
            continue                                            # the header row
        rows.append((line, [[a.strip() for a in slot.split(',')] for slot in cells[0].split(' / ')]))
    return rows


# ── building the rows ───────────────────────────────────────────────────────────────────────

def word_re(term, case_sensitive):
    return re.compile(r'(?<![\w-])' + re.escape(term) + r'(?![\w-])', 0 if case_sensitive else re.I)


def consistent_vocabulary(meta_rows):
    """Terms the Portuguese kit keeps in English at least CONSISTENT of the time they appear."""
    plain = {t for r in meta_rows for t in tokens(r['do_not_translate'])
             if not t.startswith(('{', '<'))}
    keep = []
    for term in sorted(plain):
        pat = word_re(term, True)
        has = [r for r in meta_rows if pat.search(r['english'])]
        if has and sum(term in tokens(r['do_not_translate']) for r in has) >= CONSISTENT * len(has):
            keep.append(term)
    return keep


def computed_dnt(english, vocabulary):
    found = []
    for rx in (RE_PLACEHOLDER, RE_OPEN_MARKER, RE_CWTOKEN):
        found += [(m.start(), m.group(0)) for m in rx.finditer(english)]
    for term in vocabulary:
        m = word_re(term, True).search(english)
        if m:
            found.append((m.start(), term))
    out = []
    for _, tok in sorted(found):
        if tok not in out:
            out.append(tok)
    return ' | '.join(out)


def kept_dnt(cell, english):
    """The Portuguese kit's list, if every token on it is still in this English."""
    return cell if all(t in english for t in tokens(cell)) else None


def neighbour_tiers(meta_rows):
    """prefix -> Counter of tiers, over every dotted prefix of the Portuguese kit's keys."""
    index = {}
    for r in meta_rows:
        seg = r['key'].split('::')[0].split('.')
        for n in range(1, len(seg)):
            index.setdefault('.'.join(seg[:n]) + '.', Counter())[r['priority']] += 1
    return index


def inherit_tier(key, index):
    seg = key.split('::')[0].split('.')
    for n in range(len(seg) - 1, 0, -1):
        tiers = index.get('.'.join(seg[:n]) + '.')
        if tiers:
            best = max(tiers.values())
            return min(t for t, c in tiers.items() if c == best), '.'.join(seg[:n])
    if seg[0] in AREA_TIERS:
        return AREA_TIERS[seg[0]], '%s (AREA_TIERS)' % seg[0]
    return DEFAULT_TIER, None


def adapt_note(note, english):
    """A Portuguese-kit note, with its counted sentences removed and its language named Italian."""
    s = re.sub(r'The same English text "' + re.escape(english) + r'" is (?:used by \d+ different '
               r'keys \(e\.g\. [^)]*\)|also a colour choice \([^)]*\)) — each can take a different '
               r'Portuguese word(?:, so choose the one that fits THIS screen)?\.\s*', '', note)
    s = re.sub(r'Plural form "[a-z]+" — this key has one row per form \(::one = exactly 1, '
               r'::other = every other count\)\.\s*', '', s)
    s = re.sub(r'Fill both rows separately; never merge them into one sentence\.\s*', '', s)
    for old, new in LANGUAGE_WORDS:
        s = s.replace(old, new)
    s = s.strip()
    # Refuse what the removals above should have taken: the language still named, or one of the
    # COUNTED sentences left behind (a hand-written "the same English text is used by <key>"
    # carries no count and stays).
    if re.search(r'Portugu|Brazil|pt-BR|is used by \d+ different keys|Plural form "', s):
        raise Refusal('a Portuguese-kit note this tool cannot adapt:\n    %s' % note)
    return s


def build_rows(catalog, splash, meta, vocabulary):
    meta_by_key = {r['key']: r for r in meta}
    index = neighbour_tiers(meta)
    rows, stats, inherited = [], Counter(), Counter()
    for order, (key, value) in enumerate(catalog.items()):
        if key.startswith(SPLASH_PREFIX):
            raise Refusal('%s: a catalog key uses the prefix reserved for the splash page' % key)
        forms = [(key, value)] if isinstance(value, str) else [
            ('%s::%s' % (key, f), value[f]) for f in PLURAL_FORMS if f in value]
        for k, english in forms:
            rows.append({'key': k, 'english': english, 'order': order})
    for n, (sid, english) in enumerate(splash.items()):
        rows.append({'key': SPLASH_PREFIX + sid, 'english': english, 'order': len(catalog) + n,
                     'splash': True})

    for row in rows:
        m = meta_by_key.get(row['key'])
        row['same'] = bool(m) and m['english'] == row['english']
        if row.get('splash'):
            row['priority'] = SPLASH_TIER
        elif m:
            row['priority'] = m['priority']
        else:
            row['priority'], area = inherit_tier(row['key'], index)
            inherited[(area or '(no area in the Portuguese kit)', row['priority'])] += 1
        dnt = kept_dnt(m['do_not_translate'], row['english']) if row['same'] else None
        dnt = dnt if dnt is not None else computed_dnt(row['english'], vocabulary)
        row['do_not_translate'] = ' | '.join(t for t in tokens(dnt) if t not in RELEASED)
        row['base_note'] = adapt_note(m['notes'], row['english']) if row['same'] else ''
        stats['kept' if row['same'] else ('changed' if m else 'new')] += 1

    rows.sort(key=lambda r: (int(r['priority']), r['order']))

    by_text = OrderedDict()
    for r in rows:
        by_text.setdefault(r['english'], []).append(r['key'])
    labels = {}
    for r in rows:
        if '::' not in r['key'] and not r.get('splash'):
            labels.setdefault(r['english'], []).append(r['key'])

    for r in rows:
        parts = [r['base_note']] if r['base_note'] else []
        if not r['same'] and not r.get('splash'):
            for m in RE_QUOTE.finditer(r['english']):
                quoted = m.group(1) or m.group(2)
                others = [k for k in labels.get(quoted, []) if k != r['key']][:3]
                if others:
                    parts.append('Quotes another control by name: "%s" is the on-screen text of %s. '
                                 'Translate the quote and that control exactly the same way, or the '
                                 'hint names a control the screen does not show.'
                                 % (quoted, ' / '.join(others)))
            if RE_CWTOKEN.search(r['english']):
                parts.append(CW_NOTE)
        same_text = by_text[r['english']]
        if len(same_text) > 1:
            parts.append('The same English text "%s" is used by %d different keys (e.g. %s) — each '
                         'can take a different Italian word, so choose the one that fits THIS screen.'
                         % (r['english'], len(same_text),
                            ', '.join([k for k in same_text if k != r['key']][:3])))
        if '::' in r['key']:
            parts.append('Plural form "%s" — this key has one row per form (::one = exactly 1, '
                         '::other = every other count, 0 included). Fill both rows separately; '
                         'never merge them into one sentence.' % r['key'].rsplit('::', 1)[1])
        if r.get('splash'):
            parts.append(SPLASH_NOTE)
        r['notes'] = ' '.join(parts)
        r[LANG_COLUMN] = ''
    return rows, stats, inherited


def carry(rows, path):
    """Copy a translator's cells across, by key, where the English has not changed since."""
    header, old = load_csv(path)
    if header != COLUMNS:
        raise Refusal('%s does not have the kit columns: %s' % (path, ', '.join(header or [])))
    prev = {r['key']: r for r in old}
    now = {r['key']: r for r in rows}
    # A plural travels whole: if either form's English changed, neither form is carried.
    stale_bases = {k.rsplit('::', 1)[0] for k, r in now.items()
                   if '::' in k and k in prev and prev[k]['english'] != r['english']}
    report = Counter()
    for r in rows:
        p = prev.get(r['key'])
        if not p or not p[LANG_COLUMN].strip():
            continue
        if p['english'] != r['english'] or r['key'].rsplit('::', 1)[0] in stale_bases:
            report['English changed since it was translated — left empty'] += 1
            continue
        r[LANG_COLUMN] = p[LANG_COLUMN]
        report['carried'] += 1
    report['translated rows whose key is gone'] = sum(
        1 for k, p in prev.items() if p[LANG_COLUMN].strip() and k not in now)
    return report


# ── writing ─────────────────────────────────────────────────────────────────────────────────

def csv_text(rows):
    buf = io.StringIO()
    out = csv.writer(buf, quoting=csv.QUOTE_ALL, lineterminator='\n')
    out.writerow(COLUMNS)
    for r in rows:
        out.writerow([r[c] for c in COLUMNS])
    return buf.getvalue()


def verifier_module(path):
    spec = importlib.util.spec_from_file_location('verify_it', path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def with_pins(verifier_text, pins):
    block = ('# >>> make-kit pins\n'
             'EXPECTED_ROWS = %d\n'
             "EXPECTED_KEYS_SHA = '%s'\n"
             "EXPECTED_ENGLISH_SHA = '%s'\n"
             "EXPECTED_DNT_SHA = '%s'\n"
             'EXPECTED_TIERS = %r\n'
             '# <<< make-kit pins\n'
             % (pins['rows'], pins['keys'], pins['english'], pins['dnt'], pins['tiers']))
    new, n = re.subn(r'# >>> make-kit pins\n.*?# <<< make-kit pins\n', lambda _: block,
                     verifier_text, flags=re.S)
    if n != 1:
        raise Refusal('verify-it.py must hold exactly one make-kit pin block (found %d)' % n)
    return new


def with_values(text, values, where):
    """Rewrite every `<!-- make-kit:NAME -->…<!-- /make-kit -->` with the value for NAME."""
    def sub(m):
        if m.group(1) not in values:
            raise Refusal('%s: no value for make-kit:%s' % (where, m.group(1)))
        return '<!-- make-kit:%s -->%s<!-- /make-kit -->' % (m.group(1), values[m.group(1)])
    return re.sub(r'<!-- make-kit:(\w+) -->.*?<!-- /make-kit -->', sub, text)


def with_tier_counts(text, tiers):
    """Rewrite the Rows cell of the README's tier table, between its markers."""
    start, end = text.index('<!-- make-kit:tiers -->'), text.index('<!-- /make-kit:tiers -->')

    def row(m):
        n = '{:,}'.format(tiers.get(int(m.group(1)), 0))
        return '| **%s** | %s |' % (m.group(1), '**%s**' % n if m.group(2) else n)
    table = re.sub(r'^\| \*\*(\d)\*\* \| (\*\*)?[\d,]+(?:\*\*)? \|', row, text[start:end], flags=re.M)
    return text[:start] + table + text[end:]


def with_glossary_counts(text, rows):
    english = [r['english'] for r in rows]

    def count(slot, case_sensitive):
        pats = [word_re(alt, case_sensitive) for alt in slot]
        return sum(1 for e in english if any(p.search(e) for p in pats))
    for start, end, cs in (('## A.', '## B.', True), ('## B.', '## If I have this wrong', False)):
        for line, slots in glossary_terms(text, start, end):
            cells = line.split('|')
            cells[2] = ' %s ' % ' / '.join('{:,}'.format(count(s, cs)) for s in slots)
            text = text.replace(line, '|'.join(cells), 1)
    return text


def main(argv):
    ap = argparse.ArgumentParser(description='Build the Italian translation kit from the catalog.')
    ap.add_argument('--check', action='store_true', help='write nothing; exit 1 if the kit is stale')
    ap.add_argument('--carry', help='a filled CSV whose Italian cells to carry into the new build')
    ap.add_argument('--catalog', default=str(ROOT / 'ui/src/i18n/en.ts'))
    ap.add_argument('--splash', default=str(ROOT / 'ui/public/splashscreen.html'))
    ap.add_argument('--metadata', default=str(ROOT / 'translations/pt-BR/nexus-ptbr-translation.csv'))
    ap.add_argument('--out-dir', default=str(KIT), help='write the kit here (default: beside this file)')
    args = ap.parse_args(argv[1:])

    catalog_path, splash_path = Path(args.catalog).resolve(), Path(args.splash).resolve()
    meta_path, out_dir = Path(args.metadata).resolve(), Path(args.out_dir).resolve()
    print('catalog:  %s' % catalog_path, file=sys.stderr)
    print('splash:   %s' % splash_path, file=sys.stderr)
    print('metadata: %s' % meta_path, file=sys.stderr)
    print('kit:      %s' % out_dir, file=sys.stderr)

    try:
        catalog = load_catalog(catalog_path)
        splash = load_splash(splash_path)
        meta = load_csv(meta_path)[1] if meta_path.exists() else []
        glossary_src = (KIT / 'GLOSSARY.md').read_text(encoding='utf-8')
        listed = [a for _, slots in glossary_terms(glossary_src, '## A.', '## B.')
                  for slot in slots for a in slot]
        vocabulary = sorted((set(consistent_vocabulary(meta)) | set(listed)) - RELEASED,
                            key=lambda t: (-len(t), t))
        rows, stats, inherited = build_rows(catalog, splash, meta, vocabulary)
        carried = carry(rows, Path(args.carry)) if args.carry else None

        verifier = verifier_module(KIT / 'verify-it.py')
        pins = verifier.pins([{c: r[c] for c in COLUMNS} for r in rows])
        tier1 = [r for r in rows if r['priority'] == '1']
        chars = sum(len(r['english']) for r in rows)
        values = {
            'rows': '{:,}'.format(len(rows)),
            'plurals': '{:,}'.format(sum(1 for v in catalog.values() if not isinstance(v, str))),
            'tier1': '{:,}'.format(len(tier1)),
            'tier12': '{:,}'.format(sum(1 for r in rows if r['priority'] in ('1', '2'))),
            'tier1chars': '{:,}'.format(int(round(sum(len(r['english']) for r in tier1), -3))),
            'tier1pct': '%d' % round(100.0 * sum(len(r['english']) for r in tier1) / chars),
        }
        outputs = {
            CSV_NAME: csv_text(rows),
            'verify-it.py': with_pins((KIT / 'verify-it.py').read_text(encoding='utf-8'), pins),
            'README.md': with_values(with_tier_counts((KIT / 'README.md').read_text(encoding='utf-8'),
                                                      pins['tiers']), values, 'README.md'),
            'GLOSSARY.md': with_values(with_glossary_counts(glossary_src, rows), values, 'GLOSSARY.md'),
        }
    except Refusal as exc:
        print('REFUSING — %s' % exc, file=sys.stderr)
        return 1

    print('rows: %d  (%d with the Portuguese kit\'s metadata, %d whose English changed, %d new)'
          % (len(rows), stats['kept'], stats['changed'], stats['new']), file=sys.stderr)
    print('tiers: %s' % ', '.join('%d:%d' % kv for kv in pins['tiers'].items()), file=sys.stderr)
    for (area, tier), n in sorted(inherited.items(), key=lambda kv: (-kv[1], kv[0])):
        print('  new rows, tier %s from %s: %d' % (tier, area, n), file=sys.stderr)
    if carried is not None:
        for what, n in sorted(carried.items()):
            print('carry: %s: %d' % (what, n), file=sys.stderr)

    stale = [name for name, text in outputs.items()
             if not (out_dir / name).exists()
             or (out_dir / name).read_text(encoding='utf-8') != text]
    if args.check:
        for name in stale:
            print('STALE: %s' % (out_dir / name), file=sys.stderr)
        print('the kit is %s' % ('STALE' if stale else 'current'), file=sys.stderr)
        return 1 if stale else 0
    out_dir.mkdir(parents=True, exist_ok=True)
    for name, text in outputs.items():
        with open(out_dir / name, 'w', encoding='utf-8', newline='\n') as fh:
            fh.write(text)
        print('%s %s' % ('wrote' if name in stale else 'unchanged', out_dir / name), file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
