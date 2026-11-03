// #441: THE TUNE NOTE IN PHONE ON A NEWCAT YAESU. In Phone, Nexus keys the radio in plain USB or LSB.
// A Yaesu on Hamlib's newcat backend (the FTDX10, FT-991A, FT-710, FT-891, FTDX101 and the rest) then
// takes its transmit audio from its SSB modulation source menu, MIC from the factory: the front jack
// only. Tune's tone goes to the radio over USB audio, so with that menu on MIC the radio keys and puts
// out no RF. Nexus never changes a radio's menus, so a line beside Tune says where the menu is. It is
// text only: Tune keys exactly as it does on every other radio. (Settings' computer-microphone hint
// says the same for the PC microphone.)
import type { Settings } from '../types'

/** Hamlib's Yaesu newcat ASCII rigs, the set Rust's `raw_mode_query` names (rigmodels.rs): FT-450,
 *  FT-950, FT-2000, FTDX-9000, FTDX-5000, FTDX-1200, FT-991/991A, FT-891, FTDX-3000, FTDX-101D,
 *  FTDX-10, FTDX-101MP, FT-450D, FT-710. The FTX-1 (1051) has its own backend and is left out there,
 *  so it is here. yaesuTuneNote.test.ts reads the Rust list and holds the two equal. */
export const NEWCAT_MODELS: readonly number[] = [1027, 1028, 1029, 1030, 1032, 1034, 1035, 1036, 1037, 1040, 1042, 1044, 1046, 1049]

/** Whether Phone's Tune note applies to the active radio: a newcat Yaesu on its own CAT link. A TCI
 *  program stores a model of its own (`TCI_RIG_MODEL`), so a model number there is not the radio's. */
export function phoneTuneNeedsRearSource(s: Pick<Settings, 'rigModel' | 'rigConn'>): boolean {
  return NEWCAT_MODELS.includes(s.rigModel) && s.rigConn !== 'tci'
}
