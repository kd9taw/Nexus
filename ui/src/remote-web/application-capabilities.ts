import { APPLICATION_COMMANDS } from './application-protocol'
import { streamVocabulary } from './application-stream-protocol'
import { MEMORIES_COMMAND, DXPEDITIONS_COMMAND, INSIGHTS_COMMAND, QUERY_COMMAND, RECALL_COMMAND } from './application-query-protocol'

export const APPLICATION_VERSIONS = [1, 2, 3, 4, 5, 6, 7, 8] as const
/** Query grammar revisions are independent of instrument-stream revisions. */
export const applicationQueryVersion = (version: number): 3 | 4 | 6 | 7 | 8 => version >= 8 ? 8 : version >= 7 ? 7 : version >= 6 ? 6 : version >= 4 ? 4 : 3
/** Exact command set for each negotiated application version, shared by both ends. */
export function applicationCommands(version: number): readonly string[] {
  if (version === 1) return APPLICATION_COMMANDS
  if (version === 2) return streamVocabulary(2)
  if (version === 3) return [...streamVocabulary(2), QUERY_COMMAND]
  if (version === 4 || version === 5) return [...streamVocabulary(version), QUERY_COMMAND, RECALL_COMMAND]
  if (version === 6) return [...streamVocabulary(5), QUERY_COMMAND, RECALL_COMMAND, INSIGHTS_COMMAND]
  if (version === 7) return [...applicationCommands(6), DXPEDITIONS_COMMAND]
  if (version === 8) return [...applicationCommands(7), MEMORIES_COMMAND]
  return []
}
