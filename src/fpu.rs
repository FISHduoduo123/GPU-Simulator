//! 浮点计算单元（FPU）—— 参见 `docs/fpu_spec.md`。
//!
//! 规格要点：
//! - FP16 = IEEE 754 binary16（1 符号 + 5 指数偏置 15 + 10 尾数）
//! - 覆盖 `op=0101`，由 4bit `func` 区分
//! - 三源输入 `a`/`b`/`c[15:0]` + 单独目的地址输出
//! - 舍入 round-to-nearest-even；溢出→±Inf；下溢→subnormal/0；NaN 传播；1/0→±Inf
//! - HCVT：func=0111 整转浮点（i2f），func=1000 浮点转整（f2i）
//! - 浮点标志为单独 6bit（FEQ/FNE/FLT/FGT/FLE/FGE），仅 HSUB/CMP 更新
//! - 纯组合逻辑、单周期出结果

/// `op=0101` 下的 func 编码。
pub mod ffunc {
    pub const HADD: u8 = 0b0000;
    pub const HSUB_CMP: u8 = 0b0001;
    pub const HMUL: u8 = 0b0010;
    pub const HRECIP: u8 = 0b0011;
    pub const HFMA: u8 = 0b0100;
    pub const HMAX: u8 = 0b0101;
    pub const HMIN: u8 = 0b0110;
    /// 整转浮点（16bit 有符号整数 → FP16）。
    pub const HCVT_I2F: u8 = 0b0111;
    /// 浮点转整（FP16 → 16bit 有符号整数）。
    pub const HCVT_F2I: u8 = 0b1000;
}

// ---------------------------------------------------------------------------
// IEEE 754 binary16 <-> f32 转换
// ---------------------------------------------------------------------------

/// 将 16bit 的 IEEE half 编码转换为等价 f32 值。
pub fn half_to_f32(h: u16) -> f32 {
    let sign: f32 = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1F;
    let man = (h & 0x03FF) as f32;

    if exp == 0 {
        // ±0 或 subnormal：值 = ±man * 2^-24
        return sign * man * (2f32.powi(-24));
    } else if exp == 0x1F {
        if (h & 0x03FF) == 0 {
            return sign * f32::INFINITY;
        } else {
            return f32::NAN;
        }
    }
    let e = (exp as i32) - 15;
    let mant = 1.0 + man / 1024.0;
    sign * mant * (2f32.powi(e))
}

/// 将 f32 值按 IEEE 语义舍入回 16bit 的 half 编码。
///
/// 采用 round-to-nearest-even；溢出 → ±Inf；下溢 → subnormal 或 0；NaN 传播。
pub fn f32_to_half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;

    if v.is_nan() {
        // 规范化 quiet NaN，保留符号
        return sign | 0x7C01;
    }
    if v.is_infinite() {
        return sign | 0x7C00;
    }

    let x = v.abs();
    if x == 0.0 {
        return sign; // ±0
    }

    let ib = x.to_bits();
    let exp = ((ib >> 23) & 0xFF) as i32;
    let manin = ib & 0x7FFFFF;

    // 溢出：实数指数 ≥ 16 → ±Inf
    if exp >= 143 {
        return sign | 0x7C00;
    }

    // 规范化分支：实数指数在 [-14, 15]
    if exp >= 113 {
        // exp 113..=142 => eh 1..=30
        let m2 = manin | 0x800000; // 24bit 有效数（含隐式位）
        let mut eh = exp - 112; // 1..=30
        let mut fr = ((m2 >> 13) & 0x3FF) as u16; // 10bit 小数
        let rem = m2 & 0x1FFF;
        let guard = (rem >> 12) & 1;
        let sticky = (rem & 0xFFF) != 0;
        let lsb = fr & 1;
        // RNE：guard=1 且（sticky 或 lsb=1）
        if guard == 1 && (sticky || lsb == 1) {
            fr += 1;
            if fr == 0x400 {
                fr = 0;
                eh += 1;
                if eh >= 31 {
                    return sign | 0x7C00;
                }
            }
        }
        return sign | ((eh as u16) << 10) | fr;
    }

    // subnormal / 下溢分支：值 = frac * 2^-24，对整数 frac 做 RNE
    let scaled = (x as f64) * 16777216.0; // * 2^24
    let fl = scaled.floor();
    let frac_part = scaled - fl;
    let mut r = fl as u32;
    if frac_part > 0.5 || (frac_part == 0.5 && (r & 1) == 1) {
        r += 1;
    }
    if r == 0 {
        return sign; // 下溢至 ±0
    }
    if r >= 0x400 {
        // 舍入抬升到最小规格化数 1.0 * 2^-14
        return sign | 0x0400;
    }
    sign | (r as u16)
}

/// 将 f32 舍入到最接近的整数（RNE），用于 f2i。
fn f32_round_even(v: f32) -> f32 {
    let fl = v.floor();
    let frac = v - fl;
    if frac == 0.5 {
        if (fl as i64) & 1 == 0 {
            fl
        } else {
            fl + 1.0
        }
    } else if frac > 0.5 {
        fl + 1.0
    } else {
        fl
    }
}

/// HCVT.f2i: 把 half 转为 16bit 有符号整数（RNE；溢出饱和到 i16 范围；NaN→0）。
fn half_to_int(h: u16) -> u16 {
    let v = half_to_f32(h);
    if v.is_nan() {
        return 0;
    }
    let r = f32_round_even(v);
    if r > 32767.0 {
        return 0x7FFF;
    }
    if r < -32768.0 {
        return 0x8000;
    }
    (r as i16) as u16
}

// ---------------------------------------------------------------------------
// 浮点标志
// ---------------------------------------------------------------------------

/// 浮点比较标志（6bit），对应指令集 Cond 浮点段 1010=FEQ .. 1111=FGE。
///
/// 作为线程核心的浮点标志寄存器内容；异步复位全 0 由调用方（state）持有。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FloatFlags {
    pub feq: bool,
    pub fne: bool,
    pub flt: bool,
    pub fgt: bool,
    pub fle: bool,
    pub fge: bool,
}

impl FloatFlags {
    /// 对两个 f32（由 half 解出）做 IEEE 比较。任一为 NaN 时除 FNE 外均为 false。
    pub fn compare(a: f32, b: f32) -> Self {
        let feq = a == b;
        Self {
            feq,
            fne: a != b,
            flt: a < b,
            fgt: a > b,
            fle: a <= b,
            fge: a >= b,
        }
    }

    /// 按 4bit 浮点 Cond 编码（10..=15，即 1010..1111）取标志。
    pub fn test(&self, cond: u8) -> bool {
        match cond & 0xF {
            10 => self.feq,
            11 => self.fne,
            12 => self.flt,
            13 => self.fgt,
            14 => self.fle,
            15 => self.fge,
            _ => false,
        }
    }
}

/// FPU 单周期执行结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FpuResult {
    /// 运算结果（写回数据，half 编码或整数）。
    pub value: u16,
    /// 是否为 HSUB/CMP（func=0001）。仅此时浮点标志寄存器应收纳 `flags`。
    pub sub_cmp: bool,
    /// 6bit 浮点比较标志（仅在 `sub_cmp` 时有效）。
    pub flags: FloatFlags,
}

/// 纯组合 FPU。单周期调用 [`FPU::exec`] 得到结果。
#[derive(Debug, Clone, Copy, Default)]
pub struct FPU;

impl FPU {
    /// 计算 `func` 指定的 FP16 运算。
    ///
    /// 注：HCVT_i2f 的入参 `a` 按 16bit 有符号整数读取；其余指令按 half 读取。
    pub fn exec(&self, a: u16, b: u16, c: u16, f: u8) -> FpuResult {
        let ff = f & 0xF;

        // HCVT 走整数/整数路径，参数解释不同。
        let value = if ff == ffunc::HCVT_I2F {
            f32_to_half((a as i16) as f32)
        } else if ff == ffunc::HCVT_F2I {
            half_to_int(a)
        } else {
            let af = half_to_f32(a);
            let bf = half_to_f32(b);
            let cf = half_to_f32(c);
            match ff {
                ffunc::HADD => f32_to_half(af + bf),
                ffunc::HSUB_CMP => f32_to_half(af - bf),
                ffunc::HMUL => f32_to_half(af * bf),
                ffunc::HRECIP => f32_to_half(1.0 / af),
                ffunc::HFMA => f32_to_half(af * bf + cf),
                ffunc::HMAX => f32_to_half(if af.is_nan() {
                    bf
                } else if bf.is_nan() {
                    af
                } else if af >= bf {
                    af
                } else {
                    bf
                }),
                ffunc::HMIN => f32_to_half(if af.is_nan() {
                    bf
                } else if bf.is_nan() {
                    af
                } else if af <= bf {
                    af
                } else {
                    bf
                }),
                _ => a, // 未定义 func，稳患：不写回（值不作数）
            }
        };

        let cmp = ff == ffunc::HSUB_CMP;
        // HSUB/CMP 隐式比较 a、b（half 解释），生成浮点标志。
        let flags = FloatFlags::compare(half_to_f32(a), half_to_f32(b));
        FpuResult {
            value,
            sub_cmp: cmp,
            flags,
        }
    }

    /// `func` 对应的运算是否需要写回目的寄存器。
    pub fn writes_back(f: u8) -> bool {
        (f & 0xF) <= ffunc::HCVT_F2I
    }
}
