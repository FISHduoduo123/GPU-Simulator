#include "gpu_core.inc"
.DATA
    mat_A : .half 1, 2, 3, 4
    mat_B : .half 1, 2, 3, 4
    mat_C : .space 4*2

//矩阵加法
.kernel mat_add :
    _THREAD_start_
    DONE
