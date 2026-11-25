//! 顶层装配 —— 四核心 SIMT warp + 共享 PC + 指令内存 + BRU + 数据内存。
//!
//! 连接关系（对应 `docs/architecture_modules.mmd`）：
//! ```text
//!   PC ──addr──▶ IMem ──instr──▶ 4× ThreadCore（同一指令广播）
//!                                       │ 分支端口/标志/掩码
//!                                       ▼
//!                                   BRU ──en[3:0]──▶ 各核心
//!                                    │ jump_req/jump_addr
//!                                    ▼
//!                                   PC（顺延 +2 或跳转）
//!   ThreadCore 访存请求 ──▶ Memory
//! ```
//!
//! 单周期流程（`step`）：四核心取同一指令各自执行 → 处理访存 → BRU 决策 → 更新 PC。

use crate::bru::{BRU, BruCoreIn, BruOut};
use crate::memory::Memory;
use crate::thread_core::{CycleOut, ThreadCore};

/// 一次 `step` 的对外观察结果。
#[derive(Debug, Clone)]
pub struct StepOut {
    pub cycle: u64,
    pub pc: u16,
    pub instr: u32,
    pub en: [bool; 4],
    pub per_core: [CycleOut; 4],
    pub jump_req: bool,
    pub jump_addr: u16,
    pub halted: bool,
    pub done: bool,
}

/// 顶层系统。
#[derive(Debug, Clone)]
pub struct GpuSystem {
    /// 程序计数器（16bit；每条指令占用 2 个地址单位）。
    pub pc: u16,
    /// 指令内存（按 word 存放；`imem[pc >> 1]`）。
    pub imem: Vec<u32>,
    /// 4 个线程核心。
    pub cores: [ThreadCore; 4],
    /// 分支单元。
    pub bru: BRU,
    /// 数据内存。
    pub mem: Memory,
    /// 汇总：任一核心停机/完成。
    pub halted: bool,
    pub done: bool,
    /// 已执行的周期数。
    pub cycle: u64,
}

impl GpuSystem {
    /// 用一段程序与数据内存字数构造系统；核心线程号依次 0..3。
    pub fn new(program: Vec<u32>, mem_words: usize) -> Self {
        let mut cores = std::array::from_fn(|_| ThreadCore::new());
        for (k, c) in cores.iter_mut().enumerate() {
            c.ctx.thread_id = k as u16;
        }
        Self {
            pc: 0,
            imem: program,
            cores,
            bru: BRU::new(),
            mem: Memory::new(mem_words),
            halted: false,
            done: false,
            cycle: 0,
        }
    }

    pub fn reset(&mut self) {
        self.pc = 0;
        self.halted = false;
        self.done = false;
        self.cycle = 0;
        self.bru.reset();
        for c in self.cores.iter_mut() {
            c.reset();
        }
    }

    /// 取指：`imem[pc >> 1]`，越界返回 0（NOP）。
    pub fn fetch(&self) -> u32 {
        let idx = (self.pc as usize) >> 1;
        self.imem.get(idx).copied().unwrap_or(0)
    }

    /// 执行一个周期。
    pub fn step(&mut self) -> StepOut {
        let instr = self.fetch();

        // 1) 四核心执行同一指令（各自按 bru.en 决定是否 NOP）
        let mut per_core = [CycleOut::default(); 4];
        for k in 0..4 {
            per_core[k] = self.cores[k].cycle(self.bru.en[k], instr);
        }

        // 2) 数据访存（P0/P1/P2 排队语义待展开，当前逐核心顺序处理）
        self.handle_mem(&per_core);

        // 3) BRU 决策
        let inv: [BruCoreIn; 4] = std::array::from_fn(|k| {
            BruCoreIn::from_out(&per_core[k], self.cores[k].mask_port(), self.bru.en[k])
        });
        let bo: BruOut = self.bru.step(&inv, self.pc);

        // 4) 停机/完成汇总
        for out in &per_core {
            if out.halted {
                self.halted = true;
            }
            if out.done {
                self.done = true;
            }
        }

        // 5) 更新 PC
        let (jreq, jaddr) = (bo.jump_req, bo.jump_addr);
        if jreq {
            self.pc = jaddr;
        } else {
            self.pc = self.pc.wrapping_add(2);
        }

        self.cycle += 1;
        StepOut {
            cycle: self.cycle,
            pc: self.pc,
            instr,
            en: bo.en,
            per_core,
            jump_req: jreq,
            jump_addr: jaddr,
            halted: self.halted,
            done: self.done,
        }
    }

    /// 处理一批访存请求（当前为顺序模型；各核心请求对应各自核心）。
    fn handle_mem(&mut self, outs: &[CycleOut; 4]) {
        for k in 0..4 {
            let m = outs[k].mem;
            if !m.valid {
                continue;
            }
            if m.is_write {
                self.mem.write(m.addr, m.wdata);
            }
            if m.is_read {
                let d = self.mem.read(m.addr);
                // 同拍上升沿回写
                self.cores[k].apply_load(m.dst, d);
            }
            if m.is_atomic {
                self.exec_atomic(k, &outs[k]);
            }
        }
    }

    /// 执行原子操作（RMW）。语义为**临时模型，待协议展开**。
    fn exec_atomic(&mut self, k: usize, out: &CycleOut) {
        let m = out.mem;
        let old = self.mem.read(m.addr);
        match m.kind {
            0b0100 => {
                // ATADD：dst = old; mem = old + value
                self.mem.write(m.addr, old.wrapping_add(m.wdata));
                self.cores[k].apply_load(m.dst, old);
            }
            0b0101 => {
                // ATSWAP：dst = old; mem = value
                self.mem.write(m.addr, m.wdata);
                self.cores[k].apply_load(m.dst, old);
            }
            0b0110 => {
                // ATCAS：if mem==cmp { mem=new; dst=1 } else { dst=0 }
                if old == m.wdata {
                    self.mem.write(m.addr, m.wdata2);
                    self.cores[k].apply_load(m.dst, 1);
                } else {
                    self.cores[k].apply_load(m.dst, 0);
                }
            }
            _ => {}
        }
    }
}

/// 编码一条指令（测试辅助）：op + 7×4bit 字段。
#[allow(dead_code)]
pub fn enc(opc: u8, f: &[u8]) -> u32 {
    let mut v = (opc as u32) << 28;
    for (i, &x) in f.iter().enumerate().take(7) {
        v |= ((x as u32) & 0xF) << (24 - 4 * i);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread_core::op;

    #[test]
    fn fetch_and_nop_advance() {
        let mut s = GpuSystem::new(vec![enc(op::NOP, &[]), enc(op::NOP, &[])], 16);
        let so = s.step();
        assert_eq!(so.pc, 2); // 顺延 +2
        assert!(!so.jump_req);
        assert_eq!(s.pc, 2);
    }

    #[test]
    fn bra_jumps_to_target() {
        // BRA → 0x0040：op=0110，addr=bits[27:12]=0x0040
        let bra = (0x6u32 << 28) | ((0x0040u32) << 12) | (0u32); // field5=attr 0
        let mut s = GpuSystem::new(vec![bra], 16);
        let so = s.step();
        assert!(so.jump_req);
        assert_eq!(so.jump_addr, 0x0040);
        assert_eq!(s.pc, 0x0040);
    }

    #[test]
    fn ldi_executes_in_all_cores() {
        // LDI r1 <= 5 on all 4 cores
        let mut s = GpuSystem::new(vec![enc(op::LDI, &[1, 5])], 16);
        s.step();
        for k in 0..4 {
            assert_eq!(s.cores[k].rf.read(1), 5);
        }
    }

    #[test]
    fn str_then_ldr() {
        // LDI r1<=0x100 (addr base)
        // MOV r2 <- 5 via LDI:
        //   LDI r1 0x10 (addr low nibble) ... use LDI imm only 4bit → build base in reg
        // 改用：数据地址由 base 寄存器提供，base=LDI r1, 0x8 (addr 8)
        // STR r2(=5)@mem[r1]
        // LDR r3 <- mem[r1]
        let mut s = GpuSystem::new(
            vec![
                enc(op::LDI, &[1, 0x8]),                       // r1 = 8
                enc(op::LDI, &[2, 0x5]),                       // r2 = 5
                enc(op::MEM, &[0, 2, 1, 0, 0b0000]),           // STR: data=r2, base=r1
                enc(op::MEM, &[3, 0, 1, 0, 0b0001]),           // LDR: dst=r3, base=r1
            ],
            16,
        );
        // run 4 cycles
        for _ in 0..4 {
            s.step();
        }
        assert_eq!(s.mem.peek(8), 5);
        assert_eq!(s.cores[0].rf.read(3), 5);
    }
}