subroutine msgparservar(msg37,msg37_2)

  character msg37*37,msg37_2*37,call1*12,call2*12,call3*12,report*6

! ES1JA RR73; TA1BM <...> -22 
! ES1JA RR73; TA1BM <SX60RAAG> -22

  call1='            '; call2='            '; call3='            '; report='      '
  ispc1=index(msg37,' '); ispc2=index(msg37((ispc1+1):),' ')+ispc1
  ispc3=index(msg37((ispc2+1):),' ')+ispc2; ispc4=index(msg37((ispc3+1):),' ')+ispc3
  ispc5=index(msg37((ispc4+1):),' ')+ispc4
  if(ispc5.gt.20) then
    call1=msg37(1:ispc1-1); call2=msg37(ispc2+1:ispc3-1)
    if(msg37(ispc3+1:ispc3+2).eq.'<.') then
      call3='<...>'
    else
      call3=msg37(ispc3+1:ispc4-1)
    endif
    if(len_trim(call3).lt.11) then; report=msg37(ispc4+1:ispc5-1); else; report=msg37(ispc4+1:37); endif
! build standard messages
    msg37=trim(call1)//' RR73; '//trim(call2)//' '//trim(call3)//' '//trim(report)  !ft8md reformat msg37
    msg37_2=trim(call2)//' '//trim(call3)//' '//trim(report)
  endif

  return
end subroutine msgparservar
