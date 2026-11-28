//#include "gpu_core.inc"
.DATA
    V_a : .half 1, 2, 3, 4

.kernel v_add :
    LDI r0 V_a

    LDR r10 r0 r1
    LDI r3 1
    ADD r1 r1 r3

    LDR r11 r0 r1
    ADD r1 r1 r3

    LDR r12 r0 r1
    ADD r1 r1 r3

    LDR r13 r0 r1
    
    
