//! GPU 模拟器 —— 基础线程核心。
//!
//! 当前包含以下模块（按对应规格实现，均为纯组合、单周期出结果）：
//! - [`regfile`] / [`RegFile`]：16×16bit 寄存器组（4 读 1 写）
//! - [`alu`] / [`ALU`]：`op=0100` 整数算数逻辑单元
//! - [`fpu`] / [`FPU`]：`op=0101` FP16 浮点单元
//! - [`thread_core`] / [`ThreadCore`]：指令译码控制模块 + 核心接口
//!   （指令端口 / 使能端口 / 计算标志端口 / 访存接口骨架）
//!
//! 上升沿写回由调用方统一提交：见 [`regfile::RegFile::tick`] 与 [`thread_core::ThreadCore::cycle`]。
//!
//! ## 线程核心上的连接方式（规格摘录）
//! ```text
//! a/b/c ──▶ ALU/FPU ── result ──▶ WB MUX ──▶ regfile.write(wen, dst, data)
//! 上升沿 tick() 提交写回；同时按需更新整数/浮点标志寄存器。
//! ```

pub mod alu;
pub mod bru;
pub mod fpu;
pub mod memory;
pub mod regfile;
pub mod system;
pub mod thread_core;

pub use alu::{func as alu_func, ALU, AluResult, IntFlags};
pub use bru::{BruCoreIn, BruOut, BRU};
pub use fpu::{f32_to_half, ffunc, half_to_f32, FloatFlags, FpuResult, FPU};
pub use memory::Memory;
pub use regfile::RegFile;
pub use system::{enc, enc_ldi, GpuSystem, StepOut};
pub use thread_core::{op, warp_thread_id, BrOut, CycleOut, MemReq, ThreadContext, ThreadCore};

/// 一个把寄存器组、ALU、FPU 按规格连接的最小示意图（仅供说明，非流水线实现）。
///
/// 返回 `(整型结果及标志是否更新, 浮点结果及标志是否更新, 周期后的寄存器快照)`，
/// 并演示：源操作数从读端口取、结果经回写 MUX 交由 RegFile 在 `tick` 时提交。
#[allow(clippy::type_complexity)]
pub fn wired_cycle(
    rf: &mut RegFile,
    src_addrs: [u8; 4],
    dalu: u16,
    alu_func_code: u8,
    dfpu: u16,
    fpu_func_code: u8,
    int_flag_update: &mut IntFlags,
    float_flag_update: &mut FloatFlags,
) -> ([u16; 16], bool, bool) {
    // 组合读：寄存器组读端口 A/B/C 提供源操作数
    let ra = rf.reads4(src_addrs);
    let (a, b, c) = (ra[0], ra[1], ra[2]);

    // 整数 ALU / 浮点 FPU（纯组合、单周期）
    let ar = ALU.exec(a, b, alu_func_code);
    let fr = FPU.exec(a, b, c, fpu_func_code);

    // 结果经回写 MUX → 写端口（后写覆盖前写，模拟同拍写冲突）
    if ALU::writes_back(alu_func_code) {
        rf.write(true, dalu as u8, ar.value);
    }
    if FPU::writes_back(fpu_func_code) {
        rf.write(true, dfpu as u8, fr.value);
    }

    let int_sub = ar.sub_cmp;
    if ar.sub_cmp {
        *int_flag_update = ar.flags;
    }
    let float_sub = fr.sub_cmp;
    if fr.sub_cmp {
        *float_flag_update = fr.flags;
    }

    // 时钟上升沿：提交写回
    rf.tick();

    (rf.snapshot(), int_sub, float_sub)
}