! MODIFIED FOR NEXUS (KD9TAW, 2026): JTDX 2.2.159's normalizebmet, taken from lines 2022-2035 of
! its lib/ft8b.f90 (the rest of that file is not vendored) and renamed with a _jtdx suffix. It
! scales the bit metrics by their RMS, where WSJT-X 3.0.2's normalizebmet (already linked, from
! ft8b.f90) scales by their standard deviation, so the JTDX FT4 bit metrics need this copy.

subroutine normalizebmet_jtdx(bmet,n)
  real bmet(n)

!  bmetav=sum(bmet)/real(n)
  bmet2av=sum(bmet*bmet)/real(n)
!  var=bmet2av-bmetav*bmetav
!  if( var .gt. 0.0 ) then
!     bmetsig=sqrt(var)
!  else
     bmetsig=sqrt(bmet2av)
!  endif
  bmet=bmet/bmetsig
  return
end subroutine normalizebmet_jtdx
