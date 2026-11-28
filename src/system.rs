//! 顶层装配 —— 四核心 SIMT warp + 共享 PC + 指令内存 + BRU + 数据内存。
//!
//! 连接关系（对应 `docs/architecture_modules.mmd`）：
//! ```text
//!   PC ──addr──▶ 统一内存 ──instr──▶ 4× ThreadCore（同一指令广播）
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
use crate::memory::{Memory, BE_CORE, BE_FETCH};
use crate::thread_core::{op, warp_thread_id, CycleOut, ThreadCore};

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
///
/// 程序与数据共用**一块统一内存**（半字粒度）：指令按大端 2 半字打包存放
/// （`unit[a]=I[23:8]`、`unit[a+1]=低8位 I[7:0] 置高字节`），与
/// `docs/memory/memory_spec.md` §3.5 一致。
#[derive(Debug, Clone)]
pub struct GpuSystem {
    /// 程序计数器（16bit 半字地址；每条指令占用 2 个半字地址，PC 顺延 +2）。
    pub pc: u16,
    /// 4 个线程核心。
    pub cores: [ThreadCore; 4],
    /// 分支单元。
    pub bru: BRU,
    /// 统一内存（数据 + 指令镜像）。
    pub mem: Memory,
    /// 汇总：任一核心停机/完成。
    pub halted: bool,
    pub done: bool,
    /// 已执行的周期数。
    pub cycle: u64,
    /// 停机/完成后是否还需要额外一拍“收尾拍”来显示已锁存的状态寄存器。
    pub flush: bool,
    // ---- warp 级外部输入（2026-11-28 新增；4 核共享同值，写入各核 ThreadContext）----
    /// warp_id：warp 二维编号 `(x<<8)|y`（MOV type=0010 读取）。默认 0。
    pub warp_id: u16,
    /// block_x（MOV type=0011）。默认 0。
    pub block_x: u16,
    /// block_y（MOV type=0100）。默认 0。
    pub block_y: u16,
    /// grid_x（MOV type=0101）。默认 0。
    pub grid_x: u16,
    /// grid_y（MOV type=0110）。默认 0。
    pub grid_y: u16,
}

impl GpuSystem {
    /// 用一段 24bit 程序与数据内存字数构造系统；程序按大端 2 半字地址 0,2,4,… 放入内存。
    pub fn new(program: Vec<u32>, mem_words: usize) -> Self {
        let mut s = Self::with_memory(Memory::new(mem_words.max(1)));
        s.load_program(&program);
        s
    }

    /// 用给定的统一内存构造系统（内存不含程序时取指为 0/NOP）。
    pub fn with_memory(mem: Memory) -> Self {
        let mut cores = std::array::from_fn(|_| ThreadCore::new());
        for (k, c) in cores.iter_mut().enumerate() {
            // warp_thread_id = 二维编号 (x 高8 / y 低8)：核 k → x=k>>1, y=k&1
            c.ctx.thread_id = warp_thread_id(k);
        }
        let mut s = Self {
            pc: 0,
            cores,
            bru: BRU::new(),
            mem,
            halted: false,
            done: false,
            cycle: 0,
            flush: false,
            warp_id: 0,
            block_x: 0,
            block_y: 0,
            grid_x: 0,
            grid_y: 0,
        };
        s.apply_warp_inputs();
        s
    }

    /// 把 warp 级输入写入各核的线程上下文（4 核共享 warp_id/block/grid；thread_id 每核不同）。
    fn apply_warp_inputs(&mut self) {
        for (k, c) in self.cores.iter_mut().enumerate() {
            c.ctx.thread_id = warp_thread_id(k);
            c.ctx.warp_id = self.warp_id;
            c.ctx.block_x_id = self.block_x;
            c.ctx.block_y_id = self.block_y;
            c.ctx.grid_x_line = self.grid_x;
            c.ctx.grid_y_line = self.grid_y;
        }
    }

    /// 设置 warp 级外部输入（warp_id/block_x/block_y/grid_x/grid_y，各 16bit），并同步到 4 核上下文。
    pub fn set_warp_inputs(&mut self, warp_id: u16, block_x: u16, block_y: u16, grid_x: u16, grid_y: u16) {
        self.warp_id = warp_id;
        self.block_x = block_x;
        self.block_y = block_y;
        self.grid_x = grid_x;
        self.grid_y = grid_y;
        self.apply_warp_inputs();
    }

    /// 把 24bit 指令序列装入统一内存（大端打包）：地址 `2i` 放 `I[23:8]`，
    /// 地址 `2i+1` 放 `I[7:0]`（置于高字节、低字节为 0）。
    pub fn load_program(&mut self, prog: &[u32]) {
        for (i, &ins) in prog.iter().enumerate() {
            let a = (2 * i) as u32;
            self.mem.write(a, ((ins >> 8) & 0xFFFF) as u16);
            self.mem.write(a + 1, ((ins & 0xFF) as u16) << 8);
        }
    }

    /// 把 `.hex` 镜像（`(半字地址, 值)` 列表）写入统一内存。
    pub fn load_hex(&mut self, pairs: &[(u16, u16)]) {
        for &(a, w) in pairs {
            self.mem.write(a as u32, w);
        }
    }

    pub fn reset(&mut self) {
        self.pc = 0;
        self.halted = false;
        self.done = false;
        self.cycle = 0;
        self.flush = false;
        self.bru.reset();
        for c in self.cores.iter_mut() {
            c.reset();
        }
    }

    /// 取指：取指读接口 `be=BE_FETCH(11)`、大端重组（`memory_spec.md` §3.5）。
    /// `instr = read32be(pc, 0b11) >> 8 = (unit[pc]<<8) | (unit[pc+1]>>8)`；越界返回 0（NOP）。
    pub fn fetch(&self) -> u32 {
        (self.mem.read32be(self.pc as u32, BE_FETCH) >> 8) as u32
    }

    /// 执行一个周期。
    pub fn step(&mut self) -> StepOut {
        if self.flush {
            return self.finish_step();
        }
        let was_halted = self.halted || self.done;
        let pc_this = self.pc; // 本拍取指/执行的 PC（更新前）
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

        // 4) 停机/完成：状态寄存器在上升沿提交，故取提交后的核心状态
        self.halted = self.cores.iter().any(|c| c.status & 0x1 != 0);
        self.done = self.cores.iter().any(|c| c.status & 0x2 != 0);
        // 本拍刚进入停机/完成 → 下一拍额外走一个收尾拍，把锁存状态显示出来；
        // 该拍不再取指，避免执行 HLT/DONE 之后的下一条指令。
        if (self.halted || self.done) && !was_halted {
            self.flush = true;
        }

        // 5) 更新 PC
        let (jreq, jaddr) = (bo.jump_req, bo.jump_addr);
        if jreq {
            self.pc = jaddr;
        } else {
            self.pc = self.pc.wrapping_add(2);
        }

        // 本拍对外显示的 halt/done 与 per_core.status 一致（提交前旧值）
        let shown_halted = per_core.iter().any(|o| o.halted);
        let shown_done = per_core.iter().any(|o| o.done);

        self.cycle += 1;
        StepOut {
            cycle: self.cycle,
            pc: pc_this, // 对外显示本拍实际执行的地址（而非更新后的下一拍 PC）
            instr,
            en: bo.en,
            per_core,
            jump_req: jreq,
            jump_addr: jaddr,
            halted: shown_halted,
            done: shown_done,
        }
    }

    /// 停机/完成后的“收尾拍”：不再取指/执行，仅把已锁存的状态寄存器输出一拍。
    fn finish_step(&mut self) -> StepOut {
        self.flush = false;
        let instr = 0u32;
        let mut per_core = [CycleOut::default(); 4];
        for k in 0..4 {
            per_core[k] = self.cores[k].cycle(false, instr);
        }
        let halted = per_core.iter().any(|o| o.halted);
        let done = per_core.iter().any(|o| o.done);
        self.halted = halted;
        self.done = done;
        self.cycle += 1;
        StepOut {
            cycle: self.cycle,
            pc: self.pc,
            instr,
            en: [false; 4],
            per_core,
            jump_req: false,
            jump_addr: 0,
            halted,
            done,
        }
    }

    /// 处理一批访存请求（当前为顺序模型；各核心请求对应各自核心，字节使能恒接 `BE_CORE=01`）。
    fn handle_mem(&mut self, outs: &[CycleOut; 4]) {
        for k in 0..4 {
            let m = outs[k].mem;
            if !m.valid {
                continue;
            }
            if m.is_write {
                // 核心侧 be=01：只把低 16 位写入当前单元
                self.mem.write32(m.addr, BE_CORE, m.wdata as u32);
            }
            if m.is_read {
                let d = (self.mem.read32(m.addr, BE_CORE) & 0xFFFF) as u16;
                // 同拍上升沿回写
                self.cores[k].apply_load(m.dst, d);
            }
            if m.is_atomic {
                self.exec_atomic(k, &outs[k]);
            }
        }
    }

    /// 执行原子操作（RMW，字节使能恒接 `BE_CORE=01`）。语义为**临时模型，待协议展开**。
    fn exec_atomic(&mut self, k: usize, out: &CycleOut) {
        let m = out.mem;
        let old = (self.mem.read32(m.addr, BE_CORE) & 0xFFFF) as u16;
        match m.kind {
            0b0100 => {
                // ATADD：dst = old; mem = old + value
                self.mem.write32(m.addr, BE_CORE, old.wrapping_add(m.wdata) as u32);
                self.cores[k].apply_load(m.dst, old);
            }
            0b0101 => {
                // ATSWAP：dst = old; mem = value
                self.mem.write32(m.addr, BE_CORE, m.wdata as u32);
                self.cores[k].apply_load(m.dst, old);
            }
            0b0110 => {
                // ATCAS：if mem==cmp { mem=new; dst=1 } else { dst=0 }
                if old == m.wdata {
                    self.mem.write32(m.addr, BE_CORE, m.wdata2 as u32);
                    self.cores[k].apply_load(m.dst, 1);
                } else {
                    self.cores[k].apply_load(m.dst, 0);
                }
            }
            _ => {}
        }
    }
}

/// 编码一条指令（测试/装载辅助）：op + 至多 5 个 4bit 字段，组 **24bit** `I`。
/// `I = op[23:20] | f1[19:16] | f2[15:12] | f3[11:8] | f4[7:4] | f5[3:0]`。
#[allow(dead_code)]
pub fn enc(opc: u8, f: &[u8]) -> u32 {
    let mut v = ((opc as u32) & 0xF) << 20;
    for (i, &x) in f.iter().enumerate().take(5) {
        v |= ((x as u32) & 0xF) << (16 - 4 * i);
    }
    v
}

/// LDI 编码辅助：f1=dst，f2..f5 合为 16bit immediate。
#[allow(dead_code)]
pub fn enc_ldi(dst: u8, imm: u16) -> u32 {
    ((op::LDI as u32) << 20) | (((dst as u32) & 0xF) << 16) | ((imm as u32) & 0xFFFF)
}
