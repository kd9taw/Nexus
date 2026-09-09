import { APPLICATION_COMMANDS } from './application-protocol'
import { streamVocabulary } from './application-stream-protocol'
import { QUERY_COMMAND, RECALL_COMMAND } from './application-query-protocol'

export const APPLICATION_VERSIONS = [1, 2, 3, 4, 5] as const
/** Exact command set for each negotiated application version, shared by both ends. */
export function applicationCommands(version: number): readonly string[] {
  if (version === 1) return APPLICATION_COMMANDS
  if (version === 2) return streamVocabulary(2)
  if (version === 3) return [...streamVocabulary(2), QUERY_COMMAND]
  if (version === 4 || version === 5) return [...streamVocabulary(version), QUERY_COMMAND, RECALL_COMMAND]
  return []
}
