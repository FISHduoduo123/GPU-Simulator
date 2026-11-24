//! 指令译码控制模块 + 线程核心接口 —— 参见 `docs/thread_core_spec.md`。
//!
//! 本轮实现两大类指令：
//! - **寄存器类**：MOV（reg⇔reg 及读取线程上下文）、LDI（立即数装载）、SEL（条件选择）
//! - **整数 / 浮点计算类**：委托 [`crate::alu::ALU`] 与 [`crate::fpu::FPU`]
//!
//! 对外接口：
//! - 指令端口（32bit 输入）
//! - 使能端口（1bit；en=0 视为 NOP）
//! - 计算标志端口（16bit 输出：低 10bit 整数 + 高 6bit 浮点）
//! - 访存接口（骨架，协议后续展开）
//!
//! 指令周期：组合取值/译码/执行 + `clk` 上升沿写回（`RegFile::tick`）。

use crate::alu::{IntFlags, ALU};
use crate::fpu::{FloatFlags, FPU};
use crate::regfile::RegFile;

/// 指令操作码（op = bits[31:28]）。
pub mod op {
    pub const NOP: u8 = 0x0;
    pub const MOV: u8 = 0x1;
    pub const LDI: u8 = 0x2;
    pub const SEL: u8 = 0x3;
    pub const ALU: u8 = 0x4; // op=0100 整数算逻
    pub const FPU: u8 = 0x5; // op=0101 FP16
    pub const BRA: u8 = 0x6;
    pub const BRAF: u8 = 0x7;
    pub const BRAP_REA: u8 = 0x8;
    pub const RET: u8 = 0x9;
    pub const MEM: u8 = 0xA; // op=1010 访存/原子
    pub const SYNC: u8 = 0xB;
    pub const HLT: u8 = 0xC;
    pub const HLTF: u8 = 0xD;
    pub const DONE: u8 = 0xE;
    pub const DONEF: u8 = 0xF;
}

/// 线程上下文（供 MOV 的 type 字段读取的只读来源）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreadContext {
    pub thread_id: u16,
    pub warp_id: u16,
    pub block_x_id: u16,
    pub block_y_id: u16,
    pub grid_x_line: u16,
    pub grid_y_line: u16,
}

impl ThreadContext {
    /// MOV type 编码 → 上下文值：0001 thread_id … 0110 grid_y_line。
    pub fn by_mov_type(&self, ty: u8) -> u16 {
        match ty & 0xF {
            0x1 => self.thread_id,
            0x2 => self.warp_id,
            0x3 => self.block_x_id,
            0x4 => self.block_y_id,
            0x5 => self.grid_x_line,
            0x6 => self.grid_y_line,
            _ => 0,
        }
    }
}

/// 访存接口请求骨架（协议后续展开）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemReq {
    /// 是否有访存请求。
    pub valid: bool,
    /// 命令类型：取 op=1010 的 func 字段（0000 STR … 0110 ATCAS），具体语义待协议展开。
    pub kind: u8,
    /// 访存地址（协议展开前为占位）。
    pub addr: u16,
    /// 写数据（协议展开前为占位）。
    pub wdata: u16,
}

/// 一个指令周期的对外输出。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CycleOut {
    /// 计算标志端口：低 10bit 整数标志 + 高 6bit 浮点标志。
    pub flags: u16,
    /// 本周期是否发生了比较（标志是否有效）。
    pub flags_valid: bool,
    /// 访存接口请求（骨架）。
    pub mem: MemReq,
    /// 是否已停机。
    pub halted: bool,
    /// 是否已完成（DONE/DONEF）。
    pub done: bool,
}

/// 线程核心：内部含寄存器组、线程上下文、整数/浮点标志与译码控制逻辑。
#[derive(Debug, Clone, Default)]
pub struct ThreadCore {
    pub rf: RegFile,
    pub ctx: ThreadContext,
    pub int_flags: IntFlags,
    pub float_flags: FloatFlags,
    pub flags_valid: bool,
    pub halted: bool,
    pub done: bool,
}

/// 取指令的 4bit 字段：field1 = bits[27:24] … field7 = bits[3:0]。
#[inline]
fn fld(instr: u32, n: u32) -> u8 {
    ((instr >> (28 - 4 * n)) & 0xF) as u8
}

/// 打包计算标志端口：低 10bit 整数 + 高 6bit 浮点。
pub fn pack_flags(i: &IntFlags, f: &FloatFlags) -> u16 {
    let ib = [i.eq, i.ne, i.lt, i.gt, i.le, i.ge, i.ult, i.ugt, i.ule, i.uge];
    let mut b = 0u16;
    for (k, &v) in ib.iter().enumerate() {
        if v {
            b |= 1 << k;
        }
    }
    let fb = [f.feq, f.fne, f.flt, f.fgt, f.fle, f.fge];
    for (k, &v) in fb.iter().enumerate() {
        if v {
            b |= 1 << (10 + k);
        }
    }
    b
}

impl ThreadCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 异步复位：清寄存器组、标志、停机状态（线程上下文保留）。
    pub fn reset(&mut self) {
        self.rf.reset();
        self.int_flags = IntFlags::default();
        self.float_flags = FloatFlags::default();
        self.flags_valid = false;
        self.halted = false;
        self.done = false;
    }

    /// 按 4bit Cond 编码测试条件（0..=9 取整数标志，10..=15 取浮点标志）。
    pub fn test_cond(&self, cond: u8) -> bool {
        let c = cond & 0xF;
        if c <= 9 {
            self.int_flags.test(c)
        } else {
            self.float_flags.test(c)
        }
    }

    /// 当前计算标志端口值。
    pub fn flags_port(&self) -> u16 {
        pack_flags(&self.int_flags, &self.float_flags)
    }

    /// 执行一个指令周期。
    ///
    /// - `en=false`：使能端口为 0，本周期按 NOP 处理（不写回、不改标志）。
    /// - `en=true`：读取指令端口 `instr_port` 并译码执行。
    ///
    /// 周期结束时（模拟时钟上升沿）提交寄存器写回，并返回对外输出端口值。
    pub fn cycle(&mut self, en: bool, instr_port: u32) -> CycleOut {
        // 1. 取值：使能=0 时默认 NOP
        let instr = if en { instr_port } else { 0 };

        let opc = ((instr >> 28) & 0xF) as u8;
        let mut write: Option<(u8, u16)> = None; // (dst, data)
        let mut mem = MemReq::default();
        let mut new_int = false;
        let mut new_float = false;

        match opc {
            op::NOP => { /* 空操作 */ }

            op::MOV => {
                let dst = fld(instr, 1);
                let src_reg = fld(instr, 2);
                let ty = fld(instr, 5);
                let v = if ty == 0 {
                    self.rf.read(src_reg) // reg <=> reg
                } else {
                    self.ctx.by_mov_type(ty) // reg <= 线程上下文
                };
                write = Some((dst, v));
            }

            op::LDI => {
                let dst = fld(instr, 1);
                let imm = fld(instr, 2) as u16; // 4bit 立即数，零扩展
                write = Some((dst, imm));
            }

            op::SEL => {
                let dst = fld(instr, 1);
                let a = fld(instr, 2);
                let b = fld(instr, 3);
                let cond = fld(instr, 5);
                let v = if self.test_cond(cond) {
                    self.rf.read(a)
                } else {
                    self.rf.read(b)
                };
                write = Some((dst, v));
            }

            op::ALU => {
                let dst = fld(instr, 1);
                let a = self.rf.read(fld(instr, 2));
                let b = self.rf.read(fld(instr, 3));
                let f = fld(instr, 5);
                let r = ALU.exec(a, b, f);
                if ALU::writes_back(f) {
                    write = Some((dst, r.value));
                }
                if r.sub_cmp {
                    self.int_flags = r.flags;
                    new_int = true;
                }
            }

            op::FPU => {
                let dst = fld(instr, 1);
                let a = self.rf.read(fld(instr, 2));
                let b = self.rf.read(fld(instr, 3));
                let c = self.rf.read(fld(instr, 4));
                let f = fld(instr, 5);
                let r = FPU.exec(a, b, c, f);
                if FPU::writes_back(f) {
                    write = Some((dst, r.value));
                }
                if r.sub_cmp {
                    self.float_flags = r.flags;
                    new_float = true;
                }
            }

            op::MEM => {
                // 访存协议后续展开：本轮仅产生接口请求骨架，暂不写回。
                mem = MemReq {
                    valid: true,
                    kind: fld(instr, 5),
                    addr: 0,
                    wdata: 0,
                };
            }

            op::HLT => {
                self.halted = true;
            }
            op::DONE => {
                self.done = true;
                self.halted = true;
            }
            op::HLTF | op::DONEF => {
                let cond = fld(instr, 5);
                if self.test_cond(cond) {
                    self.halted = true;
                    if opc == op::DONEF {
                        self.done = true;
                    }
                }
            }

            // 执行流控制（BRA/BRAF/BRAP/RET）与 SYNC 尚未实现，暂作空操作。
            _ => { /* TODO: 流控 / 系统控制后续展开 */ }
        }

        // 2. 标志端口有效性：仅比较指令刷新标志
        self.flags_valid = new_int || new_float;

        // 3. 回写 + 时钟上升沿提交
        if let Some((dst, data)) = write {
            self.rf.write(true, dst, data);
        }
        self.rf.tick();

        CycleOut {
            flags: self.flags_port(),
            flags_valid: self.flags_valid,
            mem,
            halted: self.halted,
            done: self.done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alu::func as alu_func;
    use crate::fpu::ffunc;

    fn enc(opc: u8, f1: u8, f2: u8, f3: u8, f4: u8, f5: u8) -> u32 {
        ((opc as u32) << 28)
            | ((f1 as u32) << 24)
            | ((f2 as u32) << 20)
            | ((f3 as u32) << 16)
            | ((f4 as u32) << 12)
            | ((f5 as u32) << 8)
    }

    /// 直接预置某寄存器值（模拟上一拍已提交）。
    fn preset(core: &mut ThreadCore, idx: u8, v: u16) {
        core.rf.write(true, idx, v);
        core.rf.tick();
    }

    #[test]
    fn ldi_then_mov() {
        let mut c = ThreadCore::new();
        c.cycle(true, enc(op::LDI, 1, 5, 0, 0, 0));
        assert_eq!(c.rf.read(1), 5);
        c.cycle(true, enc(op::MOV, 2, 1, 0, 0, 0));
        assert_eq!(c.rf.read(2), 5);
    }

    #[test]
    fn mov_from_context() {
        let mut c = ThreadCore::new();
        c.ctx.thread_id = 0x1234;
        c.cycle(true, enc(op::MOV, 1, 0, 0, 0, 0x1));
        assert_eq!(c.rf.read(1), 0x1234);
    }

    #[test]
    fn alu_add() {
        let mut c = ThreadCore::new();
        preset(&mut c, 1, 5);
        preset(&mut c, 2, 3);
        let out = c.cycle(true, enc(op::ALU, 3, 1, 2, 0, alu_func::ADD));
        assert_eq!(c.rf.read(3), 8);
        assert!(!out.flags_valid); // ADD 不更新标志
    }

    #[test]
    fn alu_sub_updates_flag_port() {
        let mut c = ThreadCore::new();
        preset(&mut c, 1, 5);
        preset(&mut c, 2, 3);
        let out = c.cycle(true, enc(op::ALU, 3, 1, 2, 0, alu_func::SUB_CMP));
        assert_eq!(c.rf.read(3), 2); // a-b
        assert!(out.flags_valid);
        // GT 位于 bit3；a=5 > b=3
        assert_ne!(out.flags & (1 << 3), 0);
        assert!(c.int_flags.gt);
    }

    #[test]
    fn sel_by_cond() {
        let mut c = ThreadCore::new();
        preset(&mut c, 1, 5);
        preset(&mut c, 2, 3);
        // 先 SUB(5,3) 产生 gt 标志
        c.cycle(true, enc(op::ALU, 3, 1, 2, 0, alu_func::SUB_CMP));
        // SEL cond=GT(3) → 取 f2(=r1=5)
        c.cycle(true, enc(op::SEL, 4, 1, 2, 0, 0x3));
        assert_eq!(c.rf.read(4), 5);
        // SEL cond=LT(2) 不成立 → 取 f3(=r2=3)
        c.cycle(true, enc(op::SEL, 4, 1, 2, 0, 0x2));
        assert_eq!(c.rf.read(4), 3);
    }

    #[test]
    fn fpu_hadd() {
        let mut c = ThreadCore::new();
        preset(&mut c, 1, 0x3C00); // 1.0
        preset(&mut c, 2, 0x4000); // 2.0
        c.cycle(true, enc(op::FPU, 3, 1, 2, 0, ffunc::HADD));
        assert_eq!(c.rf.read(3), 0x4200); // 3.0
    }

    #[test]
    fn fpu_hsub_updates_float_flag_port() {
        let mut c = ThreadCore::new();
        preset(&mut c, 1, 0x4000); // 2.0
        preset(&mut c, 2, 0x3C00); // 1.0
        let out = c.cycle(true, enc(op::FPU, 3, 1, 2, 0, ffunc::HSUB_CMP));
        assert!(out.flags_valid);
        // FGT 位于 bit(10+3)=13
        assert_ne!(out.flags & (1 << 13), 0);
    }

    #[test]
    fn disabled_cycle_is_nop() {
        let mut c = ThreadCore::new();
        c.cycle(false, enc(op::LDI, 1, 9, 0, 0, 0));
        assert_eq!(c.rf.read(1), 0);
        assert!(!c.flags_valid);
    }

    #[test]
    fn mem_request_skeleton() {
        let mut c = ThreadCore::new();
        let out = c.cycle(true, enc(op::MEM, 0, 0, 0, 0, 0));
        assert!(out.mem.valid);
    }

    #[test]
    fn flags_port_packing() {
        let mut i = IntFlags::default();
        i.eq = true; // bit0
        i.ge = true; // bit5
        let mut f = FloatFlags::default();
        f.fle = true; // bit(10+4)=14
        let p = pack_flags(&i, &f);
        assert_ne!(p & 0x1, 0);
        assert_ne!(p & (1 << 5), 0);
        assert_ne!(p & (1 << 14), 0);
    }
}