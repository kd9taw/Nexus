! Nexus: which ft8var decodes are not error-checked.
!
! JTDX's FT8S/FT8SD and matched-filter decoders (WSJT-X 3.0.2 lib/ft8var: ft8svar, ft8sdvar,
! ft8sd1var, ft8mf1var, ft8mfcqvar) accept a message on correlation with the one they expected,
! with no LDPC decode and no CRC. JTDX marks them '^'. The decoder's callback does not say which
! decodes they are, so ft8_decodevar.f90 (MODIFIED FOR NEXUS there) sets this flag just before each
! callback, from its own lft8s/lft8sd, exactly where JTDX picks its '^' (JTDX ft8_decode.f90:291),
! and ft8var_cabi.f90's callback reads it.
!
! `!$omp threadprivate`: with several threads each decodes and calls back on its own, and the flag
! is set and read on the same thread. One logical: no real cost to a thread's stack.
module ft8var_hint
  implicit none
  logical :: lhintdec = .false.
  !$omp threadprivate(lhintdec)
end module ft8var_hint
