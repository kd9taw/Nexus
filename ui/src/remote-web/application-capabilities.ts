import { APPLICATION_COMMANDS } from './application-protocol'
import { streamVocabulary } from './application-stream-protocol'
import { SSTV_IMAGE_COMMAND, APRS_COMMAND, JS8_CONTEXT_COMMAND, FIELD_DAY_COMMAND, OTA_COMMAND, MEMORIES_COMMAND, DXPEDITIONS_COMMAND, INSIGHTS_COMMAND, QUERY_COMMAND, RECALL_COMMAND } from './application-query-protocol'

export const APPLICATION_VERSIONS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] as const
/** Instrument grammar changes only when a new live topic is negotiated. */
export const applicationStreamVersion = (version: number): 2 | 5 | 11 | 12 => version >= 12 ? 12 : version >= 11 ? 11 : version >= 5 ? 5 : 2
/** Query grammar revisions are independent of instrument-stream revisions. */
export const applicationQueryVersion = (version: number): 3 | 4 | 6 | 7 | 8 | 9 | 10 | 11 | 12 => version >= 12 ? 12 : version >= 11 ? 11 : version >= 10 ? 10 : version >= 9 ? 9 : version >= 8 ? 8 : version >= 7 ? 7 : version >= 6 ? 6 : version >= 4 ? 4 : 3
/** Exact command set for each negotiated application version, shared by both ends. */
export function applicationCommands(version: number): readonly string[] {
  if (version === 1) return APPLICATION_COMMANDS
  if (version === 2) return streamVocabulary(2)
  if (version === 3) return [...streamVocabulary(2), QUERY_COMMAND]
  if (version === 4 || version === 5) return [...streamVocabulary(applicationStreamVersion(version)), QUERY_COMMAND, RECALL_COMMAND]
  if (version === 6) return [...streamVocabulary(5), QUERY_COMMAND, RECALL_COMMAND, INSIGHTS_COMMAND]
  if (version === 7) return [...applicationCommands(6), DXPEDITIONS_COMMAND]
  if (version === 8) return [...applicationCommands(7), MEMORIES_COMMAND]
  if (version === 9) return [...applicationCommands(8), OTA_COMMAND]
  if (version === 10) return [...applicationCommands(9), FIELD_DAY_COMMAND]
  if (version === 11) return [...applicationCommands(10), 'get_js8_state', JS8_CONTEXT_COMMAND]
  if (version === 12) return [...applicationCommands(11), 'get_sstv_state', 'get_remote_aprs_state', SSTV_IMAGE_COMMAND, APRS_COMMAND]
  return []
}
