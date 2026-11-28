//! 分支单元（BRU）—— 规格见 `docs/warp/bru_spec.md`（端口 + 接口 + 内部逻辑）。
//!
//! 收集 **4 个线程核心**的分支端口 + 计算标志 + 掩码 + 使能，按当前确认的收敛 /
//! 调用栈规则决定：下一拍使能 `en[3:0]`、是否跳转 `jump_req` 与目标 `jump_addr`。
//!
//! 说明：收敛栈 / 调用栈的边界语义已按用户确认落实——路径 2 收敛点**不做地址比较**；
//! 弹栈时用栈顶 `mask4` 恢复使能子集；混合压栈的路径字段固定为 1；CALL/RET 与 r15
//! 数据栈完全独立；RET 下溢忽略、PC 照常 +2。栈溢出信号按用户确认**暂留**（预留，不实现）。

use crate::thread_core::CycleOut;

/// 每核心每周期送入 BRU 的输入。
#[derive(Debug, Clone, Copy, Default)]
pub struct BruCoreIn {
    pub br_op: u8,
    pub jump_addr: u16,
    pub cond: u8,
    pub uncond_attr: bool,
    pub func: bool,
    pub flags: u16,
    pub mask: bool,
    pub enabled: bool, // 当前 bru_en（本周期是否使能该核心）
}

impl BruCoreIn {
    /// 从核心周期输出 + 掩码 + 当前使能组装。
    pub fn from_out(o: &CycleOut, mask: bool, enabled: bool) -> Self {
        Self {
            br_op: o.branch.op,
            jump_addr: o.branch.jump_addr,
            cond: o.branch.cond,
            uncond_attr: o.branch.uncond_attr,
            func: o.branch.func,
            flags: o.flags,
            mask,
            enabled,
        }
    }
}

/// BRU 输出。
#[derive(Debug, Clone, Copy, Default)]
pub struct BruOut {
    pub en: [bool; 4],
    pub jump_req: bool,
    pub jump_addr: u16,
}

/// 收敛栈条目：`1bit 路径 + 4bit 掩码 + 16bit 返回地址`。
#[derive(Debug, Clone, Copy)]
pub struct ConvEntry {
    /// 分支路径（1 或 2）。
    pub path: u8,
    /// 收敛后待恢复的子集掩码（低 4bit = `~满足 & 使能`）。
    pub mask4: u8,
    /// 返回地址（当前指令下一条）。
    pub ret_addr: u16,
}

/// 调用栈深度（规格：16）。
pub const CALL_STACK_DEPTH: usize = 16;

/// 分支单元。
#[derive(Debug, Clone)]
pub struct BRU {
    /// 当前使能（4 个核心）。
    pub en: [bool; 4],
    /// BRU 状态：0 普通 / 1 分支路径 1 / 2 分支路径 2。
    pub state: u8,
    /// 收敛栈。
    pub conv_stack: Vec<ConvEntry>,
    /// 调用栈（CALL/RET）。
    pub call_stack: Vec<u16>,
    /// 当前 PC（用于 CALL 的返回地址）。
    pub pc: u16,
}

impl Default for BRU {
    fn default() -> Self {
        Self::new()
    }
}

impl BRU {
    pub fn new() -> Self {
        Self {
            en: [true; 4],
            state: 0,
            conv_stack: Vec::new(),
            call_stack: Vec::new(),
            pc: 0,
        }
    }

    pub fn reset(&mut self) {
        self.en = [true; 4];
        self.state = 0;
        self.conv_stack.clear();
        self.call_stack.clear();
        self.pc = 0;
    }

    /// 计算 `cond` 是否为真：标志端口低 10bit 为整数标志、高 6bit 为浮点标志，
    /// 整数/浮点 cond 编码的位索引恰好都是 `c`（0..=9 整数、10..=15 浮点）。
    fn cond_satisfied(cond: u8, flags: u16) -> bool {
        let c = (cond & 0xF) as usize;
        if c <= 15 {
            (flags >> c) & 1 != 0
        } else {
            false
        }
    }

    /// 单周期推进。`inv` 为 4 输入，`pc` 为当前取指地址。
    ///
    /// 返回下一周期应广播给 4 核心的使能、跳转请求与目标地址。
    pub fn step(&mut self, inv: &[BruCoreIn; 4], pc: u16) -> BruOut {
        self.pc = pc;
        let mut out = BruOut {
            en: self.en,
            jump_req: false,
            jump_addr: inv[0].jump_addr,
        };

        // 同一条取指指令 → 分支 op 通常一致，取第一个。
        let opc = inv[0].br_op;
        let jaddr = inv[0].jump_addr;
        let attr = inv[0].uncond_attr;
        let func = inv[0].func;

        match opc {
            0 => { /* 无分支：PC 顺延（由 PC 模块 +2） */ }

            // BRA：普通或收敛属性
            0x6 => {
                if !attr {
                    out.jump_req = true;
                    out.jump_addr = jaddr;
                } else {
                    // 收敛属性
                    match self.state {
                        1 => {
                            // 路径 1：状态→2，改栈顶路径为 2，按栈地址设 PC，写本条跳转地址替换地址字段
                            if let Some(top) = self.conv_stack.last_mut() {
                                let target = top.ret_addr;
                                top.path = 2;
                                top.ret_addr = jaddr;
                                out.jump_req = true;
                                out.jump_addr = target;
                            }
                            self.state = 2;
                        }
                        2 => {
                            // 路径 2：PC=栈地址，弹栈；用栈顶 mask4 恢复使能子集；不做地址比较（已确认）
                            if let Some(top) = self.conv_stack.pop() {
                                let target = top.ret_addr;
                                out.jump_req = true;
                                out.jump_addr = target;
                                for k in 0..4 {
                                    out.en[k] = ((top.mask4 >> k) & 1) != 0;
                                }
                            }
                            self.state = self
                                .conv_stack
                                .last()
                                .map(|e| e.path)
                                .unwrap_or(0); // 无栈块 → 普通
                        }
                        _ => {
                            // 普通状态下的收敛 BRA：视作普通跳转（待确认）
                            out.jump_req = true;
                            out.jump_addr = jaddr;
                        }
                    }
                }
            }

            // BRAF：三状态收敛
            0x7 => {
                let mut enabled_count = 0usize;
                let mut sat_mask = 0u8;
                let mut en_mask = 0u8;
                for k in 0..4 {
                    if self.en[k] {
                        enabled_count += 1;
                        en_mask |= 1 << k;
                        if Self::cond_satisfied(inv[k].cond, inv[k].flags) {
                            sat_mask |= 1 << k;
                        }
                    }
                }
                if enabled_count == 0 {
                    // 无使能线程：不跳转
                } else if sat_mask == en_mask {
                    // 仅满足
                    out.jump_req = true;
                    out.jump_addr = jaddr;
                } else if sat_mask == 0 {
                    // 仅不满足
                    out.jump_req = false;
                } else {
                    // 混合：压栈，只使能满足线程，转路径 1
                    // TODO(暂留)：收敛栈溢出信号 —— 预留，本期不实现（不做深度限制）。
                    self.conv_stack.push(ConvEntry {
                        path: 1, // 混合压栈时路径字段恒为 1（已确认）
                        mask4: (!sat_mask) & en_mask, // ~满足 & 使能
                        ret_addr: pc.wrapping_add(2),
                    });
                    let new_en_mask = sat_mask & en_mask;
                    for k in 0..4 {
                        out.en[k] = new_en_mask & (1 << k) != 0;
                    }
                    self.state = 1;
                    out.jump_req = true;
                    out.jump_addr = jaddr;
                }
                // 路径 1/2 状态：与普通状态同样处理（此处分类逻辑已对“仅使能核心”判定）
            }

            // BRAP / REA
            0x8 => {
                if !func {
                    // BRAP：掩码=1 关闭该核心使能，然后设 PC=入口
                    for k in 0..4 {
                        if inv[k].mask {
                            out.en[k] = false;
                        }
                    }
                    out.jump_req = true;
                    out.jump_addr = jaddr;
                } else {
                    // REA：使能全部打开
                    out.en = [true; 4];
                }
                self.state = 0;
            }

            // CALL / RET（对使能/收敛状态无影响 —— 已确认）
            0x9 => {
                if !func {
                    // CALL：压 PC+2，PC=目标地址
                    // TODO(暂留)：调用栈溢出信号 —— 预留，本期不实现（溢出时静默不压栈）。
                    if self.call_stack.len() < CALL_STACK_DEPTH {
                        self.call_stack.push(pc.wrapping_add(2));
                    }
                    out.jump_req = true;
                    out.jump_addr = jaddr;
                } else {
                    // RET：弹栈 → PC；下溢忽略、PC 照常 +2（已确认）
                    if let Some(ret) = self.call_stack.pop() {
                        out.jump_req = true;
                        out.jump_addr = ret;
                    } else {
                        // 下溢：不跳转，PC 顺延 +2。
                        out.jump_req = false;
                    }
                }
                // 不修改 self.state / out.en（CALL/RET 不影响使能与收敛状态）。
            }

            _ => { /* 其它 op（成为分支类型以外的）：不顺延只管 PC +2 */ }
        }

        self.en = out.en;
        out
    }
}
