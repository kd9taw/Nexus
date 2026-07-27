subroutine tweak1(ca,jz,f0,cb)

! Shift frequency of analytic signal ca, with output to cb

  complex ca(jz),cb(jz)
  real*8 twopi
  complex*16 w,wstep
  complex w4
  data twopi/0.d0/
  save twopi

  if(twopi.eq.0.d0) twopi=8.d0*atan(1.d0)
  w=1.d0
  dphi=twopi*f0/12000.d0
  wstep=cmplx(cos(dphi),sin(dphi))
  do i=1,jz
     w=w*wstep
     w4=w
     cb(i)=w4*ca(i)
  enddo

  return
end subroutine tweak1
