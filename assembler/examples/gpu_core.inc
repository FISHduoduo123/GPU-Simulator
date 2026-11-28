//sys ---- 816GPU
    .define _gpu_thread_num_            16
    .define _wrap_thread_x_line_        2
    .define _wrap_thread_y_line_        2
    .define _wrap_x_line_               2
    .define _wrap_y_line_               2

//mov-type 
    .define _get_wrap_thread_id_        1
    .define _get_wrap_id_               2
    .define _get_block_x_id_            3
    .define _get_block_y_id_            4
    .define _get_grid_x_line_             5
    .define _get_grid_y_line_             6

//cond
    .define _EQ_                        0
    .define _NE_                        1
    .define _LT_                        2
    .define _GT_                        3
    .define _LE_                        4
    .define _GE_                        5
    .define _ULT_                       6
    .define _UGT_                       7
    .define _ULE_                       8
    .define _UGE_                       9
    .define _FEQ_                       10
    .define _FNE_                       11
    .define _FLT_                       12
    .define _FGT_                       13
    .define _FLE_                       14
    .define _FGE_                       15

////======================= 可用工具 =======================////
    //启动内核
    // _THREAD_start_

    //通用寄存器初始化
    // data : 初始数据 
    // _REGFILE_init_ (data)

    //压栈 
    // rs : 源寄存器
    // _mem_push_ (rs)

    //出栈
    // rs : 目的寄存器
    // _mem_pop_ (rd)

    //传入参数
    // index : 参数列表索引
    // rs r0-r9
    // _mem_macro_write_ (index, rs)

    //读取参数
    // index : 参数列表索引
    // rd r0-r9 
    // _mem_macro_read_ (index, rd)

    //取字节 
    // rd_h r0-r9 : 高字节 
    // rd_l r0-r9 : 低字节
    // rs r0-r9 : 源寄存器
    // _Get_byte_ (rd_h,rd_l,rs)

    //计算线程网格坐标
    // rx r0-r9 : grid_thread_x_id
    // ry r0-r9 : grid_thread_y_id
    // _CALC_grid_thread_id_ (rx,ry)

    //计算线程物理坐标
    // rx r0-r9 : gpu_thread_x_id
    // ry r0-r9 : gpu_thread_y_id
    // _CALC_gpu_thread_id_ (rx,ry)

    //元素地址计算 
    // rd r0-r9 
    // r_basic r0-r9 
    // _CALC_addr_ (rd,r_basic)

////======================= kernel局部内存工具 =======================////
    //====================== 16bit ======================//
    //初始栈
    //num : 栈深度
    .macro _init_mem_stack_ (num):
        param : .space (\(num)) * _gpu_thread_num_
        
        _init_mem_stack_INIT_:
        MOV r0 _get_wrap_thread_id_
        MOV r1 _get_wrap_id_
        
        LDI r10 65280
        LDI r11 256
        AND r2 r0 r10 
        SHR r2 r2 r11   // r2 = wrap_thread_x_id
        AND r3 r0 r11   // r3 = wrap_thread_y_id
        AND r4 r0 r10 
        SHR r4 r4 r11   // r4 = wrap_x_id
        AND r5 r0 r11   // r5 = wrap_y_id

        LDI r6 _wrap_thread_x_line_ 
        LDI r7 _wrap_thread_y_line_
        MUL r0 r4 r6
        MUL r1 r5 r7 
        ADD r0 r0 r2    // r0 = gpu_thread_x_id
        ADD r1 r1 r3    // r1 = gpu_thread_y_id

        MUL r2 r1 r0
        ADD r2 r2 r0    // r2 = gpu_thread_index

        _init_mem_stack_SET_STACK_BASIC_:
        LDI r3 \(num)
        MUL r0 r2 r3
        MOV r15 r0
    //压栈 
    // rs : 源寄存器
    .macro _mem_push_ (rs):
        PUSH \(rs)
    //出栈
    // rs : 目的寄存器
    .macro _mem_pop_ (rd):
        POP \(rd)

    //初始扩展宏参数
    //num : 参数数量
    .macro _mem_macro_parameter_ (num):
        _param_ : .space \(num) * _gpu_thread_num_ + 1
        
    //传入参数
    // index : 参数列表索引
    // rs r0-r9
    .macro _mem_macro_write_ (index, rs):
        _mem_macro_write_SAVE_:
        MOV r12 \(rs)
        _mem_push_ (r0)
        _mem_push_ (r1)
        _mem_push_ (r2)
        _mem_push_ (r3)
        _mem_push_ (r4)
        _mem_push_ (r5)
        _mem_push_ (r6)
        _mem_push_ (r7)
        _mem_macro_write_INIT_:
        MOV r0 _get_wrap_thread_id_
        MOV r1 _get_wrap_id_
        
        LDI r10 65280
        LDI r11 256
        AND r2 r0 r10 
        SHR r2 r2 r11   // r2 = wrap_thread_x_id
        AND r3 r0 r11   // r3 = wrap_thread_y_id
        AND r4 r0 r10 
        SHR r4 r4 r11   // r4 = wrap_x_id
        AND r5 r0 r11   // r5 = wrap_y_id

        LDI r6 _wrap_thread_x_line_ 
        LDI r7 _wrap_thread_y_line_
        MUL r0 r4 r6
        MUL r1 r5 r7 
        ADD r0 r0 r2    // r0 = gpu_thread_x_id
        ADD r1 r1 r3    // r1 = gpu_thread_y_id

        MUL r2 r1 r0
        ADD r2 r2 r0    // r2 = gpu_thread_index

        _mem_macro_write_W_:
        LDI r0 _param_
        LDI r1 0
        LDI r3 \(index)
        LDR r0 r0 r1    // r0 = macro_parameter_size
        MUL r0 r2 r0
        ADD r0 r0 r3
        STR r12 r0 r1   // parameter[index] = rs
        _mem_macro_write_recover_:
        _mem_pop_(r7)
        _mem_pop_(r6)
        _mem_pop_(r5)
        _mem_pop_(r4)
        _mem_pop_(r3)
        _mem_pop_(r2)
        _mem_pop_(r1)
        _mem_pop_(r0)
        
    //读取参数
    // index : 参数列表索引
    // rd r0-r9 
    .macro _mem_macro_read_ (index, rd):
        _mem_macro_read_SAVE_:
        MOV r12 \(rd)
        _mem_push_ (r0)
        _mem_push_ (r1)
        _mem_push_ (r2)
        _mem_push_ (r3)
        _mem_push_ (r4)
        _mem_push_ (r5)
        _mem_push_ (r6)
        _mem_push_ (r7)
        _mem_macro_read_INIT_:
        MOV r0 _get_wrap_thread_id_
        MOV r1 _get_wrap_id_
        
        LDI r10 65280
        LDI r11 256
        AND r2 r0 r10 
        SHR r2 r2 r11   // r2 = wrap_thread_x_id
        AND r3 r0 r11   // r3 = wrap_thread_y_id
        AND r4 r0 r10 
        SHR r4 r4 r11   // r4 = wrap_x_id
        AND r5 r0 r11   // r5 = wrap_y_id

        LDI r6 _wrap_thread_x_line_ 
        LDI r7 _wrap_thread_y_line_
        MUL r0 r4 r6
        MUL r1 r5 r7 
        ADD r0 r0 r2    // r0 = gpu_thread_x_id
        ADD r1 r1 r3    // r1 = gpu_thread_y_id

        MUL r2 r1 r0
        ADD r2 r2 r0    // r2 = gpu_thread_index

        _mem_macro_read_R_:
        LDI r0 _param_
        LDI r1 0
        LDI r3 \(index)
        LDR r0 r0 r1    // r0 = macro_parameter_size
        MUL r0 r2 r0
        ADD r0 r0 r3
        LDR r12 r0 r1   // r12 = parameter[index]
        _mem_macro_read_recover_:
        _mem_pop_(r7)
        _mem_pop_(r6)
        _mem_pop_(r5)
        _mem_pop_(r4)
        _mem_pop_(r3)
        _mem_pop_(r2)
        _mem_pop_(r1)
        _mem_pop_(r0)
        MOV \(rd) r12
////=========================================================////

    //取字节 
    // rd_h r0-r9 : 高字节 
    // rd_l r0-r9 : 低字节
    // rs r0-r9 : 源寄存器
    .macro _Get_byte_ (rd_h,rd_l,rs):
        LDI r10 65280
        LDI r11 256
        AND r10 \(rs) r10 
        SHR \(rd_h) r10 r11     // rd_h
        AND \(rd_l) \(rs)  r11    // rd_l

    //计算线程网格坐标
    // rx r0-r9 : grid_thread_x_id
    // ry r0-r9 : grid_thread_y_id
    .macro _CALC_grid_thread_id_ (rx,ry):

        _CALC_grid_INIT_:
        MOV r10 _get_wrap_thread_id_
        MOV r11 _get_block_x_id_
        MOV r12 _get_block_y_id_
        
        LDI r13 _wrap_thread_x_line_   
        LDI r14 _wrap_thread_y_line_   
        MUL r13 r13 r11     // r13 = warp_thread_x_line * block_x_id
        MUL r14 r14 r12     // r14 = warp_thread_y_line * block_y_id

        // r11 = warp_thread_x_id
        // r12 = warp_thread_y_id
        _Get_byte_ (r11,r12,r10)
        ADD \(rx) r13 r11  // \(x) = warp_thread_x_line * block_x_id + warp_thread_x_id
        ADD \(ry) r14 r12  // \(y) = warp_thread_y_line * block_y_id + warp_thread_y_id

    //计算线程物理坐标
    // rx r0-r9 : gpu_thread_x_id
    // ry r0-r9 : gpu_thread_y_id
    .macro _CALC_gpu_thread_id_ (rx,ry):
        _CALC_gpu_thread_id_SAVE_:
        MOV r12 \(rs)
        _mem_push_ (r0)
        _mem_push_ (r1)
        _mem_push_ (r2)
        _mem_push_ (r3)
        _mem_push_ (r4)
        _mem_push_ (r5)
        _mem_push_ (r6)
        _mem_push_ (r7)

        _CALC_gpu_thread_id_INIT_:
        MOV r0 _get_wrap_thread_id_
        MOV r1 _get_wrap_id_
        
        LDI r10 65280
        LDI r11 256
        AND r2 r0 r10 
        SHR r2 r2 r11   // r2 = wrap_thread_x_id
        AND r3 r0 r11   // r3 = wrap_thread_y_id
        AND r4 r0 r10 
        SHR r4 r4 r11   // r4 = wrap_x_id
        AND r5 r0 r11   // r5 = wrap_y_id

        LDI r6 _wrap_thread_x_line_ 
        LDI r7 _wrap_thread_y_line_
        MUL r0 r4 r6
        MUL r1 r5 r7 
        ADD r10 r0 r2    // r10 = gpu_thread_x_id
        ADD r11 r1 r3    // r11 = gpu_thread_y_id

        _CALC_gpu_thread_id_RECOVER_:
        _mem_pop_(r7)
        _mem_pop_(r6)
        _mem_pop_(r5)
        _mem_pop_(r4)
        _mem_pop_(r3)
        _mem_pop_(r2)
        _mem_pop_(r1)
        _mem_pop_(r0)
        ADD \(rx) r0 r2    // rx = gpu_thread_x_id
        ADD \(ry) r1 r3    // ry = gpu_thread_y_id

    //元素地址计算 
    // rd r0-r9 
    // r_basic r0-r9 
    .macro _CALC_addr_ (rd,r_basic) :
        _CALC_addr_SAVE_:
        _mem_push_(r0)
        _mem_push_(r1)

        _CALC_grid_thread_id_ (r0,r1)

        MOV r10 _get_grid_x_line_
        MUL r10 r1 r10 
        ADD r10 r10 r0

        _CALC_addr_RECOVER_:
        _mem_pop_(r1)
        _mem_pop_(r0)
        ADD \(rd) r10 \(r_basic)

    //通用寄存器初始化
    // data : 初始数据 
    .macro _REGFILE_init_ (data):
        LDI r0 \(data)
        MOV r1 r0
        MOV r2 r0
        MOV r3 r0
        MOV r4 r0
        MOV r5 r0
        MOV r6 r0
        MOV r7 r0
        MOV r8 r0
        MOV r9 r0
        MOV r10 r0
        MOV r11 r0
        MOV r12 r0
        MOV r13 r0
        MOV r14 r0 

////======================= GPU启动内核 =======================////
.macro _THREAD_start_ : 
    _THREAD_start_KERNEL_MEM_INIT_:
    _mem_macro_parameter_ (8)
    _init_mem_stack_ (16)
//bug
    _THREAD_start_INIT_:
    _CALC_grid_thread_id_ (r0,r1)
    //边界检验
        LDI r2 2
        LDI r3 1
        LDI r4 0
        // (r0 <= grid_x_id)? b10 : 0
        SUB r8 r0 r14 
        SEL r10 r2 r4 _ULE_
        // (r1 <= grid_y_id)? r10|b01 : r10|0
        OR r10 r10 r3
        OR r11 r10 r4
        SUB r8 r1 r15
        SEL r10 r10 r11 _ULE_
        LDI r0 3
        SUB r0 r10 r0
    //启用线程 (r10!=3)? 不启用 : 启用
        HLTF _NE_
        _REGFILE_init_ (0) // r0-r14 <= 0
