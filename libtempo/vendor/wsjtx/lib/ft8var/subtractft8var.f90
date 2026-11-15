! MODIFIED FOR NEXUS (KD9TAW, 2026): cfilt is an ordinary local here; upstream declares it `save`
! and `!$omp threadprivate`. Every call writes all of it before reading it (it carries nothing
! between calls), but as thread-local storage its 1.44 MB became part of EVERY thread of the
! process, carved out of each thread's stack: a program with one decoder and many other threads
! (Nexus) loses 1.44 MB of stack on every one of them (measured: a 2 MiB thread that never
! decodes FT8 overflowed). On the decoding thread's own stack it costs that thread alone, which
! has tempo_fast_sys::OPENMP_THREAD_STACK for it. The FFT plans four2avar keys on the array's
! address are re-planned when the decoding thread changes (ft8var_cabi.f90).
! ALSO MODIFIED FOR NEXUS: JTDX's SWL mode, which WSJT-X's port left out (JTDX 2.2.159
! lib/ft8v2/subtractft8.f90): with SWL on, the SWL subtraction filter (cwswl, a cos window over
! NFILT2) and its end correction; JTDX swaps `cw` itself instead (libtempo/ft8var_swl.f90).
subroutine subtractft8var(itone,f0,dt)

! Subtract an ft8 signal
!
! Measured signal  : dd(t)    = a(t)cos(2*pi*f0*t+theta(t))
! Reference signal : cref(t)  = exp( j*(2*pi*f0*t+phi(t)) )
! Complex amp      : cfilt(t) = LPF[ dd(t)*CONJG(cref(t)) ]
! Subtract         : dd(t)    = dd(t) - 2*REAL{cref*cfilt}

  use ft8_mod1, only : dd8,cw,NFILT1,NFILT2,endcorr
  use ft8var_swl, only : lswl,cwswl,endcorrswl
! NMAX=15*12000,NFFT=15*12000,NFRAME=1920*79
  parameter (NFFT=180000,NMAX=180000,NFRAME=151680)
  complex cref(nframe),cfilt(nmax)
  integer itone(79)

  nstart=dt*12000+1
  call gen_ft8wavevar(itone,79,1920,2.0,12000.0,f0,cref,xjunk,1,NFRAME)
  do i=1,nframe
    id=nstart-1+i 
    if(id.ge.1.and.id.le.NMAX) then
      cfilt(i)=dd8(id)*conjg(cref(i))
    else
      cfilt(i)=0.0
    endif
  enddo
  cfilt(nframe+1:)=0.0
  call four2avar(cfilt,nfft,1,-1,1)
  if(.not.lswl) then
    cfilt(1:nfft)=cfilt(1:nfft)*cw(1:nfft)
  else
    cfilt(1:nfft)=cfilt(1:nfft)*cwswl(1:nfft)
  endif
  call four2avar(cfilt,nfft,1,1,1)
  if(.not.lswl) then
    cfilt(1:NFILT1/2+1)=cfilt(1:NFILT1/2+1)*endcorr
    cfilt(nframe:nframe-NFILT1/2:-1)=cfilt(nframe:nframe-NFILT1/2:-1)*endcorr
  else
    cfilt(1:NFILT2/2+1)=cfilt(1:NFILT2/2+1)*endcorrswl
    cfilt(nframe:nframe-NFILT2/2:-1)=cfilt(nframe:nframe-NFILT2/2:-1)*endcorrswl
  endif
  do i=1,nframe
     j=nstart+i-1
     if(j.ge.1 .and. j.le.NMAX) dd8(j)=dd8(j)-2*REAL(cfilt(i)*cref(i))
  enddo
!!!$omp flush(dd8) ! makes no difference in number of decoded messages

  return
end subroutine subtractft8var
