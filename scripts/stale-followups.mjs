#!/usr/bin/env node
// List open issues where WE asked the last question and nobody answered.
//
// The maintainer rule (2026-08-21): a follow-up that goes unanswered for more than five days
// is closed. This finds the candidates; it does NOT close anything. Closing somebody's bug
// report is a judgement — "no reply" can mean they lost interest, or that they are away for a
// week, or that the question was unanswerable as asked — and a script cannot tell those apart.
//
// WHAT IT WILL NOT DO, and this is the whole reason it exists rather than a one-liner: it will
// not count a question asked TODAY as stale. When the rule was written, four of the candidates
// had been asked twelve hours earlier; closing those would have read as "we asked and shut the
// door", which is worse than never asking. Age is measured from OUR last comment, and only
// when ours is the last word.
//
// It also reports the OPPOSITE case, because that list matters more: issues where the REPORTER
// spoke last and is waiting on us. Those are never stale — they are ours to answer, and an
// automated sweep that only ever looks one way quietly turns our own backlog into their fault.
//
// Usage:  node scripts/stale-followups.mjs [days]     (default 5)
import { execFileSync } from 'node:child_process'

const DAYS = Number(process.argv[2] ?? 5)
const REPO = 'kd9taw/Nexus'
const MAINTAINER = 'kd9taw'

/** A question mark in our last comment is the signal that we asked for something. */
const looksLikeAQuestion = (body) => body.includes('?')

const raw = execFileSync(
  'gh',
  ['issue', 'list', '--repo', REPO, '--state', 'open', '--limit', '200', '--json',
   'number,title,comments,createdAt,author'],
  { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 },
)
const issues = JSON.parse(raw)
if (issues.length === 0) {
  console.error('no open issues came back — check `gh auth status` before believing this')
  process.exit(1)
}

const now = Date.now()
const ageDays = (iso) => (now - Date.parse(iso)) / 86_400_000

const stale = []
const waitingOnUs = []
const untouched = []

for (const it of issues) {
  const last = it.comments.at(-1)
  if (!last) {
    untouched.push({ n: it.number, title: it.title, age: ageDays(it.createdAt) })
    continue
  }
  const age = ageDays(last.createdAt)
  if (last.author.login === MAINTAINER) {
    if (looksLikeAQuestion(last.body) && age > DAYS) {
      stale.push({ n: it.number, title: it.title, age })
    }
  } else {
    waitingOnUs.push({ n: it.number, title: it.title, who: last.author.login, age })
  }
}

const show = (rows, fmt) => rows.sort((a, b) => b.age - a.age).forEach((r) => console.log(fmt(r)))

console.log(`\n── Unanswered for more than ${DAYS} days — candidates to close ──`)
if (stale.length === 0) console.log('  (none)')
show(stale, (r) => `  #${r.n}  ${r.age.toFixed(1)}d  ${r.title.slice(0, 62)}`)

console.log('\n── Waiting on US (the reporter spoke last) ──')
if (waitingOnUs.length === 0) console.log('  (none)')
show(waitingOnUs, (r) => `  #${r.n}  ${r.age.toFixed(1)}d  ${r.who} — ${r.title.slice(0, 50)}`)

console.log('\n── No comments at all ──')
if (untouched.length === 0) console.log('  (none)')
show(untouched, (r) => `  #${r.n}  ${r.age.toFixed(1)}d  ${r.title.slice(0, 62)}`)
console.log()
