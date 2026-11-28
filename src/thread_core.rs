//! 指令译码控制模块 + 线程核心接口 —— 参见 `docs/thread_core_spec.md`。
//!
//! 本轮实现两大类指令：
//! - **寄存器类**：MOV（reg⇔reg 及读取线程上下文）、LDI（立即数装载）、SEL（条件选择）
//! - **整数 / 浮点计算类**：委托 [`crate::alu::ALU`] 与 [`crate::fpu::FPU`]
//!
//! 对外接口：
//! - 指令端口（24bit 输入）
//! - 使能端口（1bit；en=0 视为 NOP）
//! - 计算标志端口（16bit 输出：低 10bit 整数 + 高 6bit 浮点）
//! - 访存接口（骨架，协议后续展开）
//!
//! 指令周期：组合取值/译码/执行 + `clk` 上升沿写回（`RegFile::tick`）。

use crate::alu::{IntFlags, ALU};
use crate::fpu::{FloatFlags, FPU};
use crate::regfile::RegFile;

/// 指令操作码（op = bits[23:20]，24bit 指令高位）。
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
    /// warp_thread_id（MOV type=0001）：线程在 warp 内的二维编号，**x 存高 8 位、y 存低 8 位**，
    /// 即 `(x << 8) | y`。4 线程 warp 中核 k(0..3) → `x = k>>1, y = k&1`（见 [`warp_thread_id`]）。
    pub thread_id: u16,
    /// warp_id（MOV type=0010）：warp 二维编号（同样 `(x<<8)|y` 打包），由 warp 级输入端口决定，4 核共享。
    pub warp_id: u16,
    pub block_x_id: u16,
    pub block_y_id: u16,
    pub grid_x_line: u16,
    pub grid_y_line: u16,
}

/// 计算核 `core_index`（0..3）在 4 线程 warp 内的 warp_thread_id：二维编号 (x,y)，x 高 8 位、y 低 8 位。
/// `x = core_index >> 1`，`y = core_index & 1` → 0x0000 / 0x0001 / 0x0100 / 0x0101。
pub fn warp_thread_id(core_index: usize) -> u16 {
    let x = (core_index >> 1) as u16;
    let y = (core_index & 1) as u16;
    (x << 8) | y
}

impl ThreadContext {
    /// MOV type 编码 → 上下文值：0001 warp_thread_id / 0010 warp_id / 0011..0110 block·grid。
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

/// 分支端口输出（核心 → BRU）。无请求有效信号，BRU 以 `op != 0` 判断。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrOut {
    /// 控制流 op：0000 无 / 0110 BRA / 0111 BRAF / 1000 BRAP·REA / 1001 CALL·RET
    pub op: u8,
    /// 跳转目标地址（16bit，来自指令 `bits[19:4]`，即 f1..f4）
    pub jump_addr: u16,
    /// 条件编码 0..15
    pub cond: u8,
    /// BRA 的收敛属性位（0 普通 / 1 收敛）
    pub uncond_attr: bool,
    /// 子类型：CALL/RET(0/1)、BRAP/REA(0/1)
    pub func: bool,
}

/// 访存接口请求（P0 原子 / P1 普通写 / P2 普通读）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemReq {
    /// 是否有访存请求。
    pub valid: bool,
    /// 命令类型：取 op=1010 的 func（0000 STR … 0110 ATCAS）。
    pub kind: u8,
    /// 访存地址（17bit，低 17bit 有效）。
    pub addr: u32,
    /// 写数据（STR/PUSH/原子 value 或 ATCAS 的 cmp）。
    pub wdata: u16,
    /// ATCAS 的 new 值（其余为 0）。
    pub wdata2: u16,
    /// 读/原子操作的目的寄存器。
    pub dst: u8,
    /// 普通写（P1）。
    pub is_write: bool,
    /// 普通读（P2）。
    pub is_read: bool,
    /// 原子操作（P0）。
    pub is_atomic: bool,
}

/// 系统状态寄存器（2bit）取值。
/// 编码：`00 正常` / `01 HLT` / `10 保留（正常情况下不发生）` / `11 DONE`。
#[allow(dead_code)]
pub const STATUS_NORMAL: u8 = 0b00;
#[allow(dead_code)]
pub const STATUS_HLT: u8 = 0b01;
#[allow(dead_code)]
pub const STATUS_DONE: u8 = 0b11;

/// 一个指令周期的对外输出。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CycleOut {
    /// 计算标志端口：低 10bit 整数标志 + 高 6bit 浮点标志。
    pub flags: u16,
    /// 本周期是否发生了比较（标志是否有效）。
    pub flags_valid: bool,
    /// 访存接口请求。
    pub mem: MemReq,
    /// 分支端口输出（给 BRU）。
    pub branch: BrOut,
    /// 系统状态寄存器（2bit）输出端口：`00 正常 / 01 HLT / 11 DONE`。
    pub status: u8,
    /// 是否已停机（= status bit0）。
    pub halted: bool,
    /// 是否已完成（= status bit1）。
    pub done: bool,
}

/// 线程核心：内部含寄存器组、线程上下文、整数/浮点标志、掩码寄存器与译码控制逻辑。
#[derive(Debug, Clone, Default)]
pub struct ThreadCore {
    pub rf: RegFile,
    pub ctx: ThreadContext,
    pub int_flags: IntFlags,
    pub float_flags: FloatFlags,
    pub flags_valid: bool,
    /// 1bit 掩码寄存器（MOV type=0111 写入，输出给 BRU）。
    pub mask: bool,
    /// 系统状态寄存器（2bit）：`00 正常 / 01 HLT / 11 DONE`。
    pub status: u8,
    /// 状态寄存器待提交值：执行 HLT/DONE 当拍写入，于下个上升沿提交。
    status_pending: Option<u8>,
    pub halted: bool,
    pub done: bool,
}

/// 取指令的 4bit 字段：field1 = bits[19:16] … field5 = bits[3:0]（24bit 指令）。
#[inline]
fn fld(instr: u32, n: u32) -> u8 {
    ((instr >> (20 - 4 * n)) & 0xF) as u8
}

/// 分支跳转地址：指令 `bits[19:4]`（16bit，即 field1..field4）。
#[inline]
fn branch_addr(instr: u32) -> u16 {
    ((instr >> 4) & 0xFFFF) as u16
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
        self.mask = false;
        self.status = STATUS_NORMAL;
        self.status_pending = None;
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

    /// 掩码标志端口值（1bit 掩码寄存器）。
    pub fn mask_port(&self) -> bool {
        self.mask
    }

    /// 完成一次「读内存」回写：把读到的数据写入目的寄存器并提交（模拟同拍上升沿）。
    pub fn apply_load(&mut self, dst: u8, data: u16) {
        self.rf.write(true, dst, data);
        self.rf.tick();
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

        let opc = ((instr >> 20) & 0xF) as u8;
        let mut write: Option<(u8, u16)> = None; // (dst, data)
        let mut mem = MemReq::default();
        let mut branch = BrOut::default();
        let mut new_int = false;
        let mut new_float = false;

        match opc {
            op::NOP => { /* 空操作 */ }

            op::MOV => {
                let dst = fld(instr, 1);
                let src_reg = fld(instr, 2);
                let ty = fld(instr, 5);
                if ty == 0 {
                    write = Some((dst, self.rf.read(src_reg))); // reg <=> reg
                } else if ty == 7 {
                    // type=0111: mask <- reg[0]（仅取源寄存器第 0 位）
                    self.mask = (self.rf.read(src_reg) & 1) != 0;
                } else {
                    write = Some((dst, self.ctx.by_mov_type(ty))); // reg <= 线程上下文
                }
            }

            op::LDI => {
                let dst = fld(instr, 1);
                // LDI 立即数为 16bit，占 f2..f5 = bits[15:0]
                let imm = (instr & 0xFFFF) as u16;
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
                // 访存协议中部分细节（响应时序/原子/栈）仍待展开，这里按当前已确认的
                // 字段布局发出请求；地址 = base + offset（低 17bit），无 offset 时 base 直接作地址。
                let f = fld(instr, 5);
                let base = self.rf.read(fld(instr, 3));
                let off = self.rf.read(fld(instr, 4));
                let base_addr = (base as u32) & 0x1FFFF;
                let mut a = (base as u32).wrapping_add(off as u32) & 0x1FFFF;
                mem.valid = true;
                mem.kind = f;
                match f {
                    0b0000 => {
                        // STR：field1 未用，field2=源数据，field3=base，field4=offset
                        mem.is_write = true;
                        mem.wdata = self.rf.read(fld(instr, 2));
                    }
                    0b0001 => {
                        // LDR：field1=dst，field3=base，field4=offset
                        mem.is_read = true;
                        mem.dst = fld(instr, 1);
                    }
                    0b0010 => {
                        // PUSH：field2=入栈数据；地址用 r15（栈基址寄存器）
                        mem.is_write = true;
                        mem.wdata = self.rf.read(fld(instr, 2));
                        a = (self.rf.read(15) as u32) & 0x1FFFF;
                    }
                    0b0011 => {
                        // POP：field1=dst；地址用 r15（栈基址寄存器）
                        mem.is_read = true;
                        mem.dst = fld(instr, 1);
                        a = (self.rf.read(15) as u32) & 0x1FFFF;
                    }
                    0b0100 => {
                        // ATADD：field1=dst，field2=value，field3=base，field4=offset
                        mem.is_atomic = true;
                        mem.dst = fld(instr, 1);
                        mem.wdata = self.rf.read(fld(instr, 2));
                    }
                    0b0101 => {
                        // ATSWAP：field1=dst，field2=value，field3=base，field4=offset
                        mem.is_atomic = true;
                        mem.dst = fld(instr, 1);
                        mem.wdata = self.rf.read(fld(instr, 2));
                    }
                    0b0110 => {
                        // ATCAS：field1=dst，field2=cmp，field3=base，field4=new
                        mem.is_atomic = true;
                        mem.dst = fld(instr, 1);
                        mem.wdata = self.rf.read(fld(instr, 2));
                        mem.wdata2 = self.rf.read(fld(instr, 4));
                        a = base_addr;
                    }
                    _ => mem.valid = false,
                }
                mem.addr = a;
            }

            // 执行流控制：仅产生分支端口输出，是否跳转由外部 BRU 决定。
            op::BRA => {
                branch = BrOut {
                    op: opc,
                    jump_addr: branch_addr(instr),
                    cond: 0,
                    uncond_attr: fld(instr, 5) & 1 != 0,
                    func: false,
                };
            }
            op::BRAF => {
                branch = BrOut {
                    op: opc,
                    jump_addr: branch_addr(instr),
                    cond: fld(instr, 5),
                    uncond_attr: false,
                    func: false,
                };
            }
            op::BRAP_REA => {
                // field5：BRAP=0000，REA=0001
                branch = BrOut {
                    op: opc,
                    jump_addr: branch_addr(instr),
                    cond: 0,
                    uncond_attr: false,
                    func: fld(instr, 5) & 1 != 0,
                };
            }
            op::RET => {
                // op=1001：field5 0=CALL、1=RET（CALL/RET）
                branch = BrOut {
                    op: opc,
                    jump_addr: branch_addr(instr),
                    cond: 0,
                    uncond_attr: false,
                    func: fld(instr, 5) & 1 != 0,
                };
            }
            op::SYNC => {
                // 全局栅栏：指令集备注“不允许在分支路径内加入全局栅栏”。
                // 本轮仅透传为待实现标记（不阻塞）。
            }

            op::HLT => {
                // 停机：待提交，下个上升沿才置位状态寄存器（本拍状态仍为旧值）。
                self.status_pending = Some(STATUS_HLT);
            }
            op::DONE => {
                // 完成：本拍待提交，下个上升沿置为 11（含 HLT 位）。
                self.status_pending = Some(STATUS_DONE);
            }
            op::HLTF | op::DONEF => {
                let cond = fld(instr, 5);
                if self.test_cond(cond) {
                    self.status_pending = Some(if opc == op::DONEF {
                        STATUS_DONE
                    } else {
                        STATUS_HLT
                    });
                }
            }

            // 执行流控制（BRA/BRAF/BRAP/RET）与 SYNC 尚未实现，暂作空操作。
            _ => { /* TODO: 流控 / 系统控制后续展开 */ }
        }

        // 2. 标志端口有效性：仅比较指令刷新标志
        self.flags_valid = new_int || new_float;

        // 3. 回写 + 时钟上升沿提交（状态寄存器与其它寄存器一样于上升沿更新）
        if let Some((dst, data)) = write {
            self.rf.write(true, dst, data);
        }
        // 本拍对外可见的状态 = 提交前的旧值（无旁路，下一拍才出现新值）。
        let out_status = self.status;
        if let Some(v) = self.status_pending.take() {
            self.status = v & 0x3;
        }
        self.rf.tick();
        self.halted = self.status & 0x1 != 0;
        self.done = self.status & 0x2 != 0;

        CycleOut {
            flags: self.flags_port(),
            flags_valid: self.flags_valid,
            mem,
            branch,
            status: out_status,
            halted: out_status & 0x1 != 0,
            done: out_status & 0x2 != 0,
        }
    }
}
