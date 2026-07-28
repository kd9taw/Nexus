!subroutine xcor(ss,ipk,nsteps,nsym,lag1,lag2,ccf,ccf0,lagpk,flip,fdot,nrobust)
subroutine xcor(ipk,nsteps,nsym,lag1,lag2,ccf,ccf0,lagpk,flip,fdot,nrobust)

! Computes ccf of a row of ss and the pseudo-random array pr.  Returns
! peak of the CCF and the lag at which peak occurs.  For JT65, the 
! CCF peak may be either positive or negative, with negative implying
! the "OOO" message.

  use jt65_mod
  parameter (NHMAX=3413)           !Max length of power spectra
  parameter (NSMAX=552)            !Max number of quarter-symbol steps
  real ss(NSMAX,NHMAX)             !2d spectrum, stepped by half-symbols
  real a(NSMAX)
!  real ccf(-44:118)
  real ccf(lag1:lag2)
  data lagmin/0/                              !Silence g77 warning
!  save
  common/sync/ss

  df=12000.0/8192.
!  dtstep=0.5/df
  dtstep=0.25/df
  fac=dtstep/(60.0*df)
  do j=1,nsteps
     ii=nint((j-nsteps/2)*fdot*fac)+ipk
     if( (ii.ge.1) .and. (ii.le.NHMAX) ) then
       a(j)=ss(j,ii)
     endif
  enddo

  if(nrobust.eq.1) then
! use robust correlation estimator to mitigate AGC attack spikes at beginning
! this reduces the number of spurious candidates overall
    call pctile(a,nsteps,50,xmed)
    do j=1,nsteps
      if( a(j).ge.xmed ) then
        a(j)=1
      else
        a(j)=-1
      endif
    enddo
  endif

! MODIFIED FOR NEXUS (KD9TAW, 2026-07-28): lagpk is initialized here.
! It was assigned ONLY inside `if(ccf(lag).gt.ccfmax)` (ccfmax seeded to 0.) or
! `if(-ccfmin.gt.ccfmax)`, so a cross-correlation that is everywhere zero or NaN
! left it UNASSIGNED — and it is a dummy argument with no intent, aliased to
! sync65's uninitialized local lagpk0, which sync65.f90 then uses as an UNGUARDED
! index into `real ccfblue(-32:82)`. An arbitrary leftover stack word scaled by 4
! is a read gigabytes from the frame: on Windows, 0xC0000005, killing the process
! (an access violation is not a Rust panic, so no catch_unwind contains it).
!
! Both degenerate cases are reachable from this application, which is why this is
! not theoretical: Nexus's RxRing FRONT-ZERO-PADS a partially-filled window, and a
! window that is >28% digital silence drives flat65's percentile reference to zero,
! so symspec65's `ss/ref` is 0/0 = NaN and every comparison below is false. A muted
! or disconnected sound card does the same thing with no padding involved. Upstream
! WSJT-X never hits it because it only ever decodes a full window of live audio.
! Note `lagmin` right above already carries `data lagmin/0/` for the same reason.
!
! lag1 is the natural choice: it is what a flat CCF means (no peak found anywhere),
! it is in range by construction, and sync65's OTHER use of lagpk is already
! bounds-guarded at :74 — this makes the unguarded use at :44 safe too.
  lagpk=lag1
  ccfmax=0.
  ccfmin=0.
  do lag=lag1,lag2
     x=0.
     do i=1,nsym
        j=4*i-3+lag
        if(j.ge.1 .and. j.le.nsteps) x=x+a(j)*pr(i)
     enddo
     ccf(lag)=2*x                        !The 2 is for plotting scale
     if(ccf(lag).gt.ccfmax) then
        ccfmax=ccf(lag)
        lagpk=lag
     endif

     if(ccf(lag).lt.ccfmin) then
        ccfmin=ccf(lag)
        lagmin=lag
     endif
  enddo

  ccf0=ccfmax
  flip=1.0
  if(-ccfmin.gt.ccfmax) then
     do lag=lag1,lag2
        ccf(lag)=-ccf(lag)
     enddo
     lagpk=lagmin
     ccf0=-ccfmin
     flip=-1.0
  endif

  return
end subroutine xcor
