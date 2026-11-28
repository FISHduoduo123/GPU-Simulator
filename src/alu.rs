//! 整数算数逻辑单元（ALU）—— 参见 `docs/alu_spec.md`。
//!
//! 规格要点：
//! - 仅覆盖 `op=0100` 的整数算逻指令，由 4bit `func` 区分
//! - 两源输入 `a`/`b[15:0]` + 单独目的地址输出 `dst`
//! - MUL 取低 16bit；ADD/SUB 按 16bit 回绕
//! - 移位量取 `b` 低 5bit；SHR 逻辑右移（补 0）；移位量 ≥ 16 时结果为 0
//! - SUB/CMP 合一：写回 `a-b` 且隐式比较 a、b，更新 10bit 整数标志
//! - 纯组合逻辑、单周期出结果

/// `op=0100` 下的 func 编码。
pub mod func {
    pub const ADD: u8 = 0b0000;
    pub const SUB_CMP: u8 = 0b0001;
    pub const MUL: u8 = 0b0010;
    /// 0011 空缺，按规格视为 NOP（不写回、不更新标志）。
    pub const NOP: u8 = 0b0011;
    pub const XOR: u8 = 0b0100;
    pub const XNOR: u8 = 0b0101;
    pub const AND: u8 = 0b0110;
    pub const NAND: u8 = 0b0111;
    pub const OR: u8 = 0b1000;
    pub const NOR: u8 = 0b1001;
    pub const NOT: u8 = 0b1010;
    pub const SHL: u8 = 0b1011;
    pub const SHR: u8 = 0b1100;
}

/// 整数比较标志（10bit），对应指令集 Cond 整数段 0000=EQ .. 1001=UGE。
///
/// 作为线程核心的整数标志寄存器内容；异步复位全 0 由调用方（state）持有。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IntFlags {
    pub eq: bool,
    pub ne: bool,
    pub lt: bool,
    pub gt: bool,
    pub le: bool,
    pub ge: bool,
    pub ult: bool,
    pub ugt: bool,
    pub ule: bool,
    pub uge: bool,
}

impl IntFlags {
    /// 对 16bit 操作数 `a`、`b` 做整型比较，生成全部 10 个标志。
    /// 有符号比较按 i16 解释，无符号比较按 u16 解释。
    pub fn compare(a: u16, b: u16) -> Self {
        let sa = a as i16;
        let sb = b as i16;
        let eq = a == b;
        Self {
            eq,
            ne: !eq,
            lt: sa < sb,
            gt: sa > sb,
            le: sa <= sb,
            ge: sa >= sb,
            ult: a < b,
            ugt: a > b,
            ule: a <= b,
            uge: a >= b,
        }
    }

    /// 按 4bit 整数 Cond 编码（0..=9）取出对应标志是否成立。
    /// 不足/越界编码一律为 `false`。
    pub fn test(&self, cond: u8) -> bool {
        match cond & 0xF {
            0 => self.eq,
            1 => self.ne,
            2 => self.lt,
            3 => self.gt,
            4 => self.le,
            5 => self.ge,
            6 => self.ult,
            7 => self.ugt,
            8 => self.ule,
            9 => self.uge,
            _ => false,
        }
    }
}

/// ALU 单周期执行结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AluResult {
    /// 运算结果（写回数据）。
    pub value: u16,
    /// 是否为 SUB/CMP（func=0001）。仅此时标志寄存器应收纳 `flags`。
    pub sub_cmp: bool,
    /// 10bit 整数比较标志（仅在 `sub_cmp` 时有效）。
    pub flags: IntFlags,
}

/// 纯组合 ALU。单周期调用 [`ALU::exec`] 得到结果；写回由上层在时钟沿提交。
#[derive(Debug, Clone, Copy, Default)]
pub struct ALU;

impl ALU {
    /// 计算 `func` 指定的运算，返回结果与标志。
    ///
    /// 对 NOP（func=0011）及未定义 func：不改变任何内部状态，`value` 不作数，
    /// 是否写回由 [`ALU::writes_back`] 决定。
    pub fn exec(&self, a: u16, b: u16, f: u8) -> AluResult {
        let ff = f & 0xF;
        let shamt = (b & 0x1F) as u32; // 移位量取 b 低 5bit
        let value = match ff {
            func::ADD => a.wrapping_add(b),
            func::SUB_CMP => a.wrapping_sub(b),
            func::MUL => (a as u32).wrapping_mul(b as u32) as u16, // 取低 16bit
            func::NOP => a,                                        // 不写回，值不作数
            func::XOR => a ^ b,
            func::XNOR => !(a ^ b),
            func::AND => a & b,
            func::NAND => !(a & b),
            func::OR => a | b,
            func::NOR => !(a | b),
            func::NOT => !a,
            func::SHL => {
                if shamt >= 16 {
                    0
                } else {
                    a << shamt
                }
            }
            func::SHR => {
                if shamt >= 16 {
                    0
                } else {
                    a >> shamt // u16 逻辑右移，补 0
                }
            }
            // 未定义 func：不写回（稳患处理）
            _ => a,
        };

        let sub_cmp = ff == func::SUB_CMP;
        AluResult {
            value,
            sub_cmp,
            flags: IntFlags::compare(a, b),
        }
    }

    /// `func` 对应的运算是否需要写回目的寄存器。
    ///
    /// NOP（0011）与未定义 func 返回 `false`。
    pub fn writes_back(f: u8) -> bool {
        match f & 0xF {
            func::ADD | func::SUB_CMP | func::MUL | func::XOR | func::XNOR
            | func::AND | func::NAND | func::OR | func::NOR | func::NOT
            | func::SHL | func::SHR => true,
            _ => false,
        }
    }
}
