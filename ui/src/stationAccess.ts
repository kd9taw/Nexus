// UI affordances follow the session's station authority. Enforcement remains in
// the transport and station; hiding/disabling a button never grants permission.
// The installed desktop keeps its existing authority and transmit guards.
import { createContext, useContext } from 'react'

export const StationControlContext = createContext(true)
export function useStationControl(): boolean { return useContext(StationControlContext) }
export const StationDataContext = createContext(true)
export function useStationData(): boolean { return useContext(StationDataContext) }
