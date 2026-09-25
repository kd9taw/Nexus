// A TEST'S ENGINE (SPEC-2 v3 C17). A window asks the engine for what it shows — `askLog`, one
// question at a time, through `features/askingLogSource` — and holds only the answers. A test holds
// its log instead and answers each question as the engine does: `answerFrom`, the reference the
// engine's queries are held to by the goldens (logAnswers.golden.test.ts).
//
//     const engineLog = vi.hoisted(() => vi.fn())
//     vi.mock('../api', () => ({
//       askLog: vi.fn(async (q: LogQuestion) => (await import('../features/logAnswers.testkit')).answerAs(q, await engineLog())),
//     }))
//     …
//     engineLog.mockResolvedValue([contact, contact])
//
// A page's rows get the edit keys `ek:<id>`: the engine's are hashes of the contact's editable
// fields; a test needs them only distinct and stable (Logbook.byId.test.tsx pins what the view
// does with them).

import type { LoggedQso } from '../types'
import { answerFrom, type AnswerTo, type LogPage, type LogQuestion } from './logAnswers'

/** The engine's answer to `q` over `log`, at `revision`. */
export function answerAs<Q extends LogQuestion>(q: Q, log: LoggedQso[], revision = 1): AnswerTo<Q> {
  const answer = answerFrom(log, q, revision)
  if (q.kind !== 'page') return answer
  const page = answer as LogPage
  return { ...page, editKeys: page.keys.map((key) => `ek:${key}`) } as AnswerTo<Q>
}
