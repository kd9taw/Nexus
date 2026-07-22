! MODIFIED FOR NEXUS (KD9TAW, 2026): SAVEd locals hoisted into a module so a per-radio
! decoder context can save/restore them. `x`/`cx` (EQUIVALENCEd) hold this chain's wideband
! spectrum and are refreshed only under the caller-owned `newdata` flag — per-chain state.
! A subroutine-local SAVE is not addressable from another compilation unit, which is why the
! hoist is required rather than cosmetic. `first`/`window` come along because the first-call
! gate belongs with them; both are chain-INDEPENDENT and are not part of the context.
module ft4_downsample_state
   include 'ft4_params.f90'
   parameter (NFFT2S=NMAX/NDOWN)
   complex cx(0:NMAX/2)
   real x(NMAX), window(0:NFFT2S-1)
   logical first
   data first/.true./
   equivalence (x,cx)
end module ft4_downsample_state

subroutine ft4_downsample(dd,newdata,f0,c)

   use ft4_downsample_state   ! also re-exports ft4_params (NMAX, NDOWN, NSPS, ...)
   parameter (NFFT2=NMAX/NDOWN)
   real dd(NMAX)
   complex c(0:NMAX/NDOWN-1)
   complex c1(0:NFFT2-1)
   logical newdata

   df=12000.0/NMAX
   baud=12000.0/NSPS
   if(first) then
      bw_transition = 0.5*baud
      bw_flat = 4*baud
      iwt = bw_transition / df
      iwf = bw_flat / df
      pi=4.0*atan(1.0)
      window(0:iwt-1) = 0.5*(1+cos(pi*(/(i,i=iwt-1,0,-1)/)/iwt))
      window(iwt:iwt+iwf-1)=1.0
      window(iwt+iwf:2*iwt+iwf-1) = 0.5*(1+cos(pi*(/(i,i=0,iwt-1)/)/iwt))
      window(2*iwt+iwf:)=0.0
      iws = baud / df
      window=cshift(window,iws)
      first=.false.
   endif

   if(newdata) then
      x=dd
      call four2a(cx,NMAX,1,-1,0)             !r2c FFT to freq domain
   endif
   i0=nint(f0/df)
   c1=0.
   if(i0.ge.0 .and. i0.le.NMAX/2) c1(0)=cx(i0)
   do i=1,NFFT2/2
      if(i0+i.ge.0 .and. i0+i.le.NMAX/2) c1(i)=cx(i0+i)
      if(i0-i.ge.0 .and. i0-i.le.NMAX/2) c1(NFFT2-i)=cx(i0-i)
   enddo
   c1=c1*window/NFFT2
   call four2a(c1,NFFT2,1,1,1)            !c2c FFT back to time domain
   c=c1(0:NMAX/NDOWN-1)

   return
end subroutine ft4_downsample
