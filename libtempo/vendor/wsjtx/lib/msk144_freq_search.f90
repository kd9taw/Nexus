subroutine msk144_freq_search(cdat,fc,if1,if2,delf,nframes,navmask,cb,    &
     cdat2,xmax,bestf,cs,xccs)

  parameter (NSPM=864,NZ=7*NSPM)
  complex cdat(NZ)
  complex cdat2(NZ)
  complex c(NSPM)                    !Coherently averaged complex data
  complex ct2(2*NSPM)
  complex cs(NSPM)
  complex cb(42)                     !Complex waveform for sync word 
  complex cc(0:NSPM-1)
  real xcc(0:NSPM-1)
  real xccs(0:NSPM-1)
  integer navmask(nframes)           !Tells which frames to average

  navg=sum(navmask) 
  n=nframes*NSPM
  fac=1.0/(48.0*sqrt(float(navg)))

  do ifr=if1,if2                     !Find freq that maximizes sync
     ferr=ifr*delf
     call tweak1(cdat,n,-(fc+ferr),cdat2)
     c=0
     sumw=0.
     do i=1,nframes
        ib=(i-1)*NSPM+1
        ie=ib+NSPM-1
        if(navmask(i).eq.1) c=c + cdat2(ib:ie)
     enddo

     cc=0
     ct2(1:NSPM)=c
     ct2(NSPM+1:2*NSPM)=c

     do ish=0,NSPM-1
        cc(ish)=dot_product(ct2(1+ish:42+ish)+ct2(337+ish:378+ish),cb(1:42))
     enddo

     xcc=abs(cc)
     xb=maxval(xcc)*fac
     if(xb.gt.xmax) then
        xmax=xb
        bestf=ferr
        cs=c
        xccs=xcc
     endif
  enddo

  return
end subroutine msk144_freq_search
