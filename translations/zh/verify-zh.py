#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
verify-zh.py — check a filled-in Nexus Chinese (Simplified) translation CSV.

    python3 verify-zh.py nexus-zh-translation.csv

It checks the MECHANICAL things only: that every row is still there, that the English and
do-not-translate columns were not edited, and that nothing which must survive a translation
byte-for-byte got renamed, dropped, added, or re-punctuated. It says nothing about whether the
Chinese is good — that is your job and it cannot be checked by a program.

Python 3 only. No installation, no dependencies.

Exit code 0 = nothing broken (warnings may still be printed). Exit code 1 = something to fix.
"""

import csv
import hashlib
import re
import sys
from collections import Counter, defaultdict

# ── What the kit shipped, so a missing row can be detected without a second file ────────────
EXPECTED_ROWS = 10515
EXPECTED_KEYS_SHA = 'b04305c30c21c1a04c9c79dfffb07706952aced7943a95b8e8fc5f84babdea6a'
EXPECTED_ENGLISH_SHA = 'fdf711d32d3b623bce65a3731586b8a0be1d08a717c5a5c955f91b2116266e48'
EXPECTED_DNT_SHA = '38d9b7a5f8dc6501b5014d0920fd2f05b7a729a226f329727bca631ab33c75e7'
EXPECTED_TIERS = {1: 1100, 2: 361, 3: 1907, 4: 567, 5: 4101, 6: 2479}
COLUMNS = ['priority', 'key', 'english', 'chinese', 'do_not_translate', 'notes']

# ── Terms the Russian kit keeps in Latin script, which Chinese says in Chinese ──────────────
#
# `do_not_translate` was curated by the Russian translator, and Russian keeps TX, RX, SWR and
# the rest as Latin abbreviations on the air. Chinese does not: the operator reads 发射, 接收,
# 驻波比 on the button, so a hint that still said "TX" would name a control the screen does
# not show. These are the terms this kit translates deliberately — the check below stops
# demanding them and starts demanding every other term on the list.
#
# 'DX' prefixes stay Latin inside the Chinese phrase (DX 远征), so 'DXped'/'DXpedition' are
# here: the row reads 「DX 远征」, which is what the navigation item says.
TRANSLATED = frozenset({
    '6m', 'AF', 'ATU', 'AUTO', 'AUTOREPLY', 'Baudot', 'DX', 'DXped', 'DXpedition',
    'DXpeditions', 'Digital', 'Field Day', 'Hound', 'Lock', 'Maidenhead', 'Nexus', 'PA',
    'QRM', 'QSO', 'RF', 'RX', 'Remote', 'SPORADIC-E', 'SWR', 'Stop TX', 'TX', 'TX On',
    'Tune', 'USB',
})

RE_PLACEHOLDER = re.compile(r'\{\{(\w+)\}\}')
RE_SINGLE_BRACE = re.compile(r'(?<!\{)\{([a-z]\w*)\}(?!\})')
RE_MARKER = re.compile(r'<(/?)([a-zA-Z][a-zA-Z0-9]*)>')
RE_CWTOKEN = re.compile(r'(?<!\{)\{([A-Z][A-Z0-9_]*)\}(?!\})')
RE_DECIMAL_COMMA = re.compile(r'\d,\d')

MAX_EXAMPLES = 8


class Report:
    """Collects problems by kind so the output is a short list, not 4000 lines."""

    def __init__(self):
        self.errors = defaultdict(list)
        self.warnings = defaultdict(list)

    def error(self, kind, key, detail):
        self.errors[kind].append((key, detail))

    def warn(self, kind, key, detail):
        self.warnings[kind].append((key, detail))

    def n_errors(self):
        return sum(len(v) for v in self.errors.values())

    def n_warnings(self):
        return sum(len(v) for v in self.warnings.values())

    def render(self):
        for label, bucket in (('PROBLEM', self.errors), ('WORTH A LOOK', self.warnings)):
            for kind, items in bucket.items():
                print()
                print('%s: %s  (%d row%s)' % (label, kind, len(items), '' if len(items) == 1 else 's'))
                for key, detail in items[:MAX_EXAMPLES]:
                    print('    %s' % key)
                    for line in detail.splitlines():
                        print('        %s' % line)
                if len(items) > MAX_EXAMPLES:
                    print('    ... and %d more like it' % (len(items) - MAX_EXAMPLES))


def sha(text):
    return hashlib.sha256(text.encode('utf-8')).hexdigest()


def markers(text):
    """Counter of marker tokens, e.g. {'<b>': 2, '</b>': 2}."""
    return Counter('<%s%s>' % (slash, name) for slash, name in RE_MARKER.findall(text))


def is_all_invariant(english, required):
    """True when the English is nothing but tokens, numbers and punctuation.

    Such a row is legitimately identical in Chinese ("FT8", "{{call}}", "599 · 20m"),
    so leaving it unchanged is not a sign of an untranslated row.
    """
    stripped = english
    for token in required:
        stripped = stripped.replace(token, ' ')
    stripped = RE_PLACEHOLDER.sub(' ', stripped)
    stripped = RE_MARKER.sub(' ', stripped)
    stripped = RE_CWTOKEN.sub(' ', stripped)
    return not re.search(r'[A-Za-z]{2}', stripped)


def check_row(row, rep, one_needs_count=False):
    key = row['key']
    english = row['english']
    zh = row['chinese']
    dnt = [t.strip() for t in row['do_not_translate'].split('|') if t.strip()]

    if zh == '':
        return False                      # not translated yet: nothing to check, nothing wrong
    if zh.strip() == '':
        rep.error('a cell holds only spaces, which the program treats as untranslated',
                  key, 'Either write the Chinese or clear the cell completely.')
        return False

    # 1. {{placeholders}} — same names, same number of each. Order may change.
    #
    # Chinese `::one` normally carries the same sentence as `::other`, so where the English
    # plural shows the count, a Chinese `::one` that also shows it is right, not a stray slot —
    # even where the English `::one` writes "1 more…" and carries no slot at all. That extra
    # {{count}} is permitted, never required: `Intl.PluralRules('zh')` only ever selects `other`,
    # and the converter emits the `other` row, so `::one` is documentation rather than shipped text.
    want, got = Counter(RE_PLACEHOLDER.findall(english)), Counter(RE_PLACEHOLDER.findall(zh))
    if one_needs_count and 'count' in got and 'count' not in want:
        del got['count']
    if want != got:
        missing = sorted((want - got).elements())
        extra = sorted((got - want).elements())
        bits = []
        if missing:
            bits.append('missing or renamed: ' + ', '.join('{{%s}}' % m for m in missing))
        if extra:
            bits.append('not in the English: ' + ', '.join('{{%s}}' % m for m in extra))
        rep.error('a {{slot}} was renamed, dropped or added', key,
                  'English: %s\nChinese: %s\n%s' % (english, zh, '; '.join(bits)))

    # 1b. single braces where double braces were meant — renders as literal text
    singles = [s for s in RE_SINGLE_BRACE.findall(zh) if s in set(RE_PLACEHOLDER.findall(english))]
    if singles:
        rep.error('a slot was written with single braces instead of double', key,
                  'Chinese: %s\nWrite {{%s}}, not {%s}.' % (zh, singles[0], singles[0]))

    # 2. <markers> — same tags, same count, and each one closed.
    mw, mg = markers(english), markers(zh)
    if mw != mg:
        missing = sorted((mw - mg).elements())
        extra = sorted((mg - mw).elements())
        bits = []
        if missing:
            bits.append('missing: ' + ' '.join(missing))
        if extra:
            bits.append('invented or duplicated: ' + ' '.join(extra))
        rep.error('a <marker> was changed', key,
                  'English: %s\nChinese: %s\n%s\nOnly the marker names the English uses will '
                  'do anything; anything else is printed on screen as literal text.'
                  % (english, zh, '; '.join(bits)))
    stack = []
    for slash, name in RE_MARKER.findall(zh):
        if slash:
            if not stack or stack[-1] != name:
                rep.error('a <marker> is out of order or was never opened', key,
                          'Chinese: %s\n</%s> closes a marker that is not open at that point. '
                          'Each <%s> is closed by the matching </%s>, in order.' % (zh, name, name, name))
                stack = []
                break
            stack.pop()
        else:
            stack.append(name)
    if stack:
        rep.error('a <marker> was left unclosed', key,
                  'Chinese: %s\nEvery <%s> needs a matching </%s>.' % (zh, stack[0], stack[0]))

    # 3. {MACRO} tokens — single braces, all capitals. Literal, always.
    cw, cg = Counter(RE_CWTOKEN.findall(english)), Counter(RE_CWTOKEN.findall(zh))
    if cw != cg:
        rep.error('a CW macro token was changed', key,
                  'English: %s\nChinese: %s\nThese are matched literally by the macro '
                  'expander. Copy them exactly.' % (english, zh))

    # 4. Ham vocabulary and other terms of art listed in do_not_translate.
    plain = [t for t in dnt if not (t.startswith('{') or t.startswith('<')) and t not in TRANSLATED]
    absent = [t for t in plain if t not in zh]
    if absent:
        rep.error('a term that must be copied through is not in the Chinese', key,
                  'English: %s\nChinese: %s\nNot found: %s\nIf you believe one of these really is '
                  'said in Chinese, add it to TRANSLATED at the top of this file and say why.'
                  % (english, zh, ', '.join(absent)))

    # 5. A decimal comma the English did not have. This is the operating fault.
    if RE_DECIMAL_COMMA.search(zh) and not RE_DECIMAL_COMMA.search(english):
        rep.error('a decimal comma appeared in a number', key,
                  'English: %s\nChinese: %s\nAn operator reads these off the screen and '
                  'dials them into a radio. Technical numbers keep the dot.' % (english, zh))

    # 6. Copied English sitting in the Chinese column.
    if zh.strip() == english.strip() and not is_all_invariant(english, plain):
        rep.warn('the Chinese is identical to the English', key,
                 'Text: %s\nFine if that really is the Chinese; otherwise this row is not done.'
                 % english)

    return True


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    path = argv[1]

    try:
        with open(path, encoding='utf-8-sig', newline='') as fh:
            reader = csv.DictReader(fh)
            header = reader.fieldnames
            rows = list(reader)
    except UnicodeDecodeError:
        print('PROBLEM: this file is not UTF-8. Re-save it as CSV UTF-8 and try again.')
        return 1
    except OSError as exc:
        print('PROBLEM: could not open %s (%s)' % (path, exc))
        return 1

    print('Nexus Chinese (Simplified) translation check')
    print('file: %s' % path)

    if header != COLUMNS:
        print()
        print('PROBLEM: the columns are not the ones the kit shipped.')
        print('    expected: %s' % ', '.join(COLUMNS))
        print('    found:    %s' % ', '.join(header or []))
        print('Do not add, remove or reorder columns. Fix that and run this again.')
        return 1

    rep = Report()

    # ── the census: is every row still here, and untouched where it must be ──────────────
    if len(rows) != EXPECTED_ROWS:
        rep.error('the number of rows changed', '(whole file)',
                  'The kit shipped %d rows; this file has %d. %d row%s went missing — most likely '
                  'a sort, a filter left on when saving, or a deleted line.'
                  % (EXPECTED_ROWS, len(rows), abs(EXPECTED_ROWS - len(rows)),
                     '' if abs(EXPECTED_ROWS - len(rows)) == 1 else 's')
                  if len(rows) < EXPECTED_ROWS else
                  'The kit shipped %d rows; this file has %d. There are extra rows.'
                  % (EXPECTED_ROWS, len(rows)))

    dupes = [k for k, n in Counter(r['key'] for r in rows).items() if n > 1]
    if dupes:
        rep.error('the same key appears more than once', '(whole file)',
                  'Duplicated: %s' % ', '.join(sorted(dupes)[:10]))

    ordered = sorted(rows, key=lambda r: r['key'])
    keys_ok = sha('\n'.join(sorted(r['key'] for r in rows))) == EXPECTED_KEYS_SHA
    if not keys_ok:
        rep.error('the key column is not the one the kit shipped', '(whole file)',
                  'Some keys were changed, removed or added. The program matches rows by key, so '
                  'a changed key is a lost string. Start again from the original CSV and paste '
                  'your Chinese column across.\n(The english and do_not_translate columns are '
                  'not checked while rows are missing — fix the rows first.)')
    if keys_ok and sha('\n'.join(r['key'] + '\t' + r['english'] for r in ordered)) != EXPECTED_ENGLISH_SHA:
        rep.error('the english column was edited', '(whole file)',
                  'The English text must stay exactly as shipped — it is what the Chinese is '
                  'matched against. If some English is wrong, tell me rather than fixing it here.')
    if keys_ok and sha('\n'.join(r['key'] + '\t' + r['do_not_translate'] for r in ordered)) != EXPECTED_DNT_SHA:
        rep.error('the do_not_translate column was edited', '(whole file)',
                  'Leave that column alone. If a term on it should be translated after all, say '
                  'so and I will change the kit.')

    # ── per row ─────────────────────────────────────────────────────────────────────────
    # Plural bases whose English `other` form shows {{count}}: their Chinese `::one` must too.
    count_bases = {r['key'].rsplit('::', 1)[0] for r in rows
                   if r['key'].endswith('::other') and '{{count}}' in r['english']}
    filled = 0
    per_tier = Counter()
    for row in rows:
        one_needs_count = (row['key'].endswith('::one')
                           and row['key'].rsplit('::', 1)[0] in count_bases)
        if check_row(row, rep, one_needs_count):
            filled += 1
            try:
                per_tier[int(row['priority'])] += 1
            except (ValueError, KeyError):
                pass

    # ── plural rows travel together ────────────────────────────────────────────────────
    #
    # Chinese has one CLDR category, so every form of a plural carries the same sentence and
    # `csv-to-catalog.py` collapses the four rows into one string (taking `other`). A half-filled
    # plural is still a problem: the converter has nothing to fall back on, and the row that is
    # blank is a sentence the translator never looked at.
    plural = defaultdict(dict)
    for row in rows:
        if '::' in row['key']:
            base, form = row['key'].rsplit('::', 1)
            plural[base][form] = row['chinese']
    for base, forms in sorted(plural.items()):
        done = [f for f, v in forms.items() if v.strip()]
        if done and len(done) != len(forms):
            rep.error('a plural key is only half filled in', base,
                      'Filled: %s. Still empty: %s.\nChinese uses one sentence for every count, '
                      'so all four rows normally read the same — but all four must be filled. '
                      'The converter emits the `other` row.' % (', '.join(sorted(done)),
                                                                 ', '.join(sorted(f for f in forms if f not in done))))

    # A translator who pastes the English column across would otherwise get warnings only.
    copied = len(rep.warnings.get('the Chinese is identical to the English', ()))
    if filled >= 20 and copied * 5 > filled:
        rep.error('most of the Chinese column is still English', '(whole file)',
                  '%d of the %d filled rows are word-for-word the English. That is what a copied '
                  'column looks like, not a translation. If it was deliberate, ignore this; if the '
                  'column got pasted by accident, start again from the original CSV.'
                  % (copied, filled))

    rep.render()

    # ── summary ─────────────────────────────────────────────────────────────────────────
    print()
    print('-' * 72)
    print('Rows in file:      %d   (the kit shipped %d)' % (len(rows), EXPECTED_ROWS))
    pct = (100.0 * filled / len(rows)) if rows else 0.0
    print('Rows translated:   %d   (%.1f%%)' % (filled, pct))
    for tier in sorted(EXPECTED_TIERS):
        total = EXPECTED_TIERS[tier]
        got = per_tier.get(tier, 0)
        bar = '#' * int(round(20.0 * got / total)) if total else ''
        print('    tier %d: %5d of %5d  %-20s %5.1f%%'
              % (tier, got, total, bar, 100.0 * got / total if total else 0.0))
    print('-' * 72)

    if rep.n_errors():
        print()
        print('%d problem%s to fix. Nothing here is about your Chinese — every one is a token, '
              'a marker or a row that moved.' % (rep.n_errors(), '' if rep.n_errors() == 1 else 's'))
        if rep.n_warnings():
            print('%d more thing%s worth a look.'
                  % (rep.n_warnings(), '' if rep.n_warnings() == 1 else 's'))
        return 1

    if rep.n_warnings():
        print()
        print('No problems. %d thing%s worth a look above — none of them will break anything.'
              % (rep.n_warnings(), '' if rep.n_warnings() == 1 else 's'))
        return 0

    print()
    print('No problems found. Every slot, marker and technical term survived.')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
