#include "gpu_core.txt"
.DATA
    mat_A : .half 1, 2, 3, 4    
    mat_B : .half 1, 2, 3, 4   
    mat_C : .space 4*2  


//矩阵加法
.kernel mat_add :
    THREAD_start
    //获取元素地址
    LDI r0 mat_A
    CALC_addr(r0,r0)
    LDI r1 mat_B
    CALC_addr(r1,r1)
    LDI r2 mat_C
    CALC_addr(r2,r2)
    //取mat_A, mat_B
    LDR r0 r0 r4
    LDR r1 r1 r4
    ADD r3 r0 r1
    //写入mat_C
    STR r3 r2 r4
    DONE






