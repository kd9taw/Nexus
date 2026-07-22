! MODIFIED FOR NEXUS (KD9TAW, 2026): the SAVEd locals `x`/`cx` (the wideband spectrum, 768 kB
! via the EQUIVALENCE) were hoisted into a module so a per-radio decoder context can save and
! restore them. They are per-chain state: `x` is refreshed only under `newdat`, which is a
! CALLER-owned flag that both call sites (ft8b.f90, ft8_a7.f90) pass as .false., so most calls
! consume a spectrum they did not compute. Shared between two radios that decodes one chain's
! audio at the other's frequency and logs its stations on the wrong band. A subroutine-local
! SAVE is not addressable from another compilation unit, so hoisting is what makes the context
! reachable at all. `taper`/`first` moved with them only because the EQUIVALENCE and the
! first-call gate belong together; they are chain-INDEPENDENT and are not part of the context.
module ft8_downsample_state
  parameter (NFFT1=192000)
  logical first
  complex cx(0:NFFT1/2)
  real x(NFFT1+2),taper(0:100)
  data first/.true./
  equivalence (x,cx)
end module ft8_downsample_state

subroutine ft8_downsample(dd,newdat,f0,c1)

! Downconvert to complex data sampled at 200 Hz ==> 32 samples/symbol

  use ft8_downsample_state
  parameter (NMAX=15*12000,NSPS=1920)
  parameter (NFFT2=3200)                   !192000/60 = 3200

  logical newdat
  complex c1(0:NFFT2-1)
  real dd(NMAX)

  if(first) then
     pi=4.0*atan(1.0)
     do i=0,100
       taper(i)=0.5*(1.0+cos(i*pi/100))
     enddo
     first=.false.
  endif
  if(newdat) then
! Data in dd have changed, recompute the long FFT
     x(1:NMAX)=dd
     x(NMAX+1:NFFT1+2)=0.                       !Zero-pad the x array
     call four2a(cx,NFFT1,1,-1,0)             !r2c FFT to freq domain
     newdat=.false.
  endif
  df=12000.0/NFFT1
  baud=12000.0/NSPS
  i0=nint(f0/df)
  ft=f0+8.5*baud
  it=min(nint(ft/df),NFFT1/2)
  fb=f0-1.5*baud
  ib=max(1,nint(fb/df))
  k=0
  c1=0.
  do i=ib,it
   c1(k)=cx(i)
   k=k+1
  enddo
  c1(0:100)=c1(0:100)*taper(100:0:-1)
  c1(k-1-100:k-1)=c1(k-1-100:k-1)*taper
  c1=cshift(c1,i0-ib)
  call four2a(c1,NFFT2,1,1,1)            !c2c FFT back to time domain
  fac=1.0/sqrt(float(NFFT1)*NFFT2)
  c1=fac*c1

  return
end subroutine ft8_downsample
