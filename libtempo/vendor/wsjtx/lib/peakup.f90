subroutine peakup(ym,y0,yp,dx)

  real*4 b,c,dx,yp,ym,y0

  b=(yp-ym)
  c=(yp+ym-2.0*y0)
  dx=-b/(2.0*c)

  return
end subroutine peakup
