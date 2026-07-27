MODULE prog_args
  CHARACTER(len=80) :: shm_key
  CHARACTER(len=500) :: exe_dir = '.', data_dir = '.', temp_dir = '.',    &
       share_dir = '.' !ft8md added share_dir
  LOGICAL :: lquiet = .false.
END MODULE prog_args
