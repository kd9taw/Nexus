! Nexus: JTDX's SWL mode for the JTDX-style FT8 decoder (WSJT-X 3.0.2 lib/ft8var).
!
! WSJT-X's port of JTDX's decoder left SWL mode out. This is JTDX's own (JTDX 2.2.159,
! jtdx-project/jtdx: lib/ft8_decode.f90, lib/sync8.f90, lib/ft8b.f90, lib/ft8v2/subtractft8.f90,
! lib/cwfilter.f90; GPL v3), carried onto ft8var's files where JTDX has `swl` and ft8var does not,
! each such edit marked MODIFIED FOR NEXUS there:
!   ft8_decodevar  a +/-3.5 s DT search (jzb/jzt +/-86 instead of +/-62); the low sync thresholds of
!                  `lft8lowth` whenever SWL is on; subtraction on the third pass of one cycle;
!   sync8var       candidates over the wider DT window (jpeak -74..101); near-dupe frequency 3 Hz;
!   ft8bvar        deep sync, two weak sub-passes, the fourth metric sub-pass, and none of the CQ
!                  and MyCall signal-level gates;
!   subtractft8var the SWL subtraction filter below (a cos window over NFILT2 = 3400 samples
!                  instead of cos^2 over NFILT1 = 4000) and its end correction.
! JTDX's separate SWL decoding-cycles setting is not carried: SWL decodes the operator's cycles.
!
! JTDX rebuilds its one filter `cw` when SWL is turned on or off (cwfilter's `swlchanged`). Here the
! SWL filter is built once beside it (cwfilter, at the callsign-database load) and subtractft8var
! picks one per call, so nothing process-wide changes when the setting does.
!
! `lswl` is set by ft8var_decode_frame (ft8var_cabi.f90) from its parameters before any decoding
! thread starts, and only read while they run.
module ft8var_swl
  use ft8_mod1, only : NFILT2
  implicit none
  logical :: lswl = .false.
  complex :: cwswl(180000)
  real :: endcorrswl(NFILT2/2+1)
end module ft8var_swl
