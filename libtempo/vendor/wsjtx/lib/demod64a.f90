subroutine demod64a(s3,nadd,afac1,mrsym,mrprob,mr2sym,mr2prob,ntest,nlow)

! Demodulate the 64-bin spectra for each of 63 symbols in a frame.

! Parameters
!    nadd     number of spectra already summed
!    mrsym    most reliable symbol value
!    mr2sym   second most likely symbol value
!    mrprob   probability that mrsym was the transmitted value
!    mr2prob  probability that mr2sym was the transmitted value

  implicit real*8 (a-h,o-z)
  real*4 s3(64,63),afac1
  integer mrsym(63),mrprob(63),mr2sym(63),mr2prob(63)

  if(nadd.eq.-999) return
  afac=afac1 * float(nadd)**0.64
  scale=255.999

! Compute average spectral value
  ave=sum(s3)/(64.*63.)
  i1=1                                      !Silence warning
  i2=1

! Compute probabilities for most reliable symbol values
  do j=1,63
     s1=-1.e30
     psum=0. 
     do i=1,64
        x=min(afac*s3(i,j)/ave,50.d0)
        psum=psum+s3(i,j)
        if(s3(i,j).gt.s1) then
           s1=s3(i,j)
           i1=i                              !Most reliable
        endif
     enddo
     if(psum.eq.0.0) psum=1.e-6

     s2=-1.e30
     do i=1,64
        if(i.ne.i1 .and. s3(i,j).gt.s2) then
           s2=s3(i,j)
           i2=i                              !Second most reliable
        endif
     enddo
     p1=s1/psum                              !Symbol metrics for ftrsd
     p2=s2/psum               
     mrsym(j)=i1-1
     mr2sym(j)=i2-1
     mrprob(j)=scale*p1
     mr2prob(j)=scale*p2
  enddo

  nlow=0
  do j=1,63
     if(mrprob(j).le.5) nlow=nlow+1
  enddo
  ntest=sum(mrprob)

  return
end subroutine demod64a
