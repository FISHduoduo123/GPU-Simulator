//! 数据内存 —— 规格见 `docs/memory/memory_spec.md`。
//!
//! 当前为**同步、单周期、顺序访问**的软件模型：`read` 组合取值、`write` 覆盖写。
//! 寻址粒度 / 响应时序 / 原子排队（P0/P1/P2 同地址行为）已在规格中列为待确认项，
//! 此处按最简可行模型实现，后续按协议展开。
//!
//! 地址 17bit；`read`/`write` 对长度取模，避免越界。
//!
//! **字节使能（接线常量）**：核心侧 4 组 `be_k` 恒接 [`BE_CORE`]（`0b01`，16bit 只用低 16 位）；
//! 取指读接口恒接 [`BE_FETCH`]（`0b11`，读满 32bit）。见 `docs/memory/memory_spec.md` §3.2/§3.4。

/// 核心侧字节使能常量：`be=01`，只访问低 16 位（当前单元）。
pub const BE_CORE: u8 = 0b01;
/// 取指读接口字节使能常量：`be=11`，读满 32bit（两个半字）。
pub const BE_FETCH: u8 = 0b11;

/// 数据内存。
#[derive(Debug, Clone)]
pub struct Memory {
    data: Vec<u16>,
}

impl Memory {
    /// 构造 `words` 个字（×16bit）的内存，初值 0。
    pub fn new(words: usize) -> Self {
        Self {
            data: vec![0u16; words.max(1)],
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// 组合读：`addr`（低 17bit）对长度取模后返回。
    pub fn read(&self, addr: u32) -> u16 {
        let i = (addr as usize) & 0x1FFFF;
        self.data[i % self.data.len()]
    }

    /// 写：覆盖写。
    pub fn write(&mut self, addr: u32, v: u16) {
        let i = (addr as usize) & 0x1FFFF;
        let len = self.data.len();
        self.data[i % len] = v;
    }

    /// 直接读取某个字（用于测试 / 观察，`idx < len`）。
    pub fn peek(&self, idx: usize) -> u16 {
        self.data[idx % self.data.len()]
    }

    /// 以 `.hex` 镜像格式导出全部内存：`ADDR: w0 w1 …`，每行 8 个半字，大写 4 位十六进制。
    pub fn dump_hex(&self) -> String {
        let mut s = String::new();
        let n = self.data.len();
        let mut a = 0usize;
        while a < n {
            let end = (a + 8).min(n);
            s.push_str(&format!("{:04X}:", a));
            for i in a..end {
                s.push_str(&format!(" {:04X}", self.data[i]));
            }
            s.push('\n');
            a = end;
        }
        s
    }

    /// **32bit 读**，按字节使能门控（`memory_spec.md` §3.2 读表）：
    /// `be=11` 读满 32bit；`be=10` 低 16 位为 0；`be=01` 高 16 位为 0；`be=00` 全 0。
    /// 低 16 位来自 `unit[addr]`，高 16 位来自 `unit[addr+1]`。
    pub fn read32(&self, addr: u32, be: u8) -> u32 {
        let lo = self.read(addr) as u32;
        let hi = self.read(addr.wrapping_add(1)) as u32;
        let mut r = 0u32;
        if be & 0b01 != 0 {
            r |= lo;
        }
        if be & 0b10 != 0 {
            r |= hi << 16;
        }
        r
    }

    /// 取指读接口专用（大端，`memory_spec.md` §3.5）：`g_rdata = (unit[addr]<<16)|unit[addr+1]`，
    /// 低地址单元在高 16 位；上层取 `>>8` 得 24bit 指令。`be` 恒接 `BE_FETCH=11`。
    pub fn read32be(&self, addr: u32, be: u8) -> u32 {
        let a = self.read(addr) as u32;
        let b = self.read(addr.wrapping_add(1)) as u32;
        let mut r = 0u32;
        if be & 0b01 != 0 {
            r |= b; // addr+1 单元 → 低 16 位
        }
        if be & 0b10 != 0 {
            r |= a << 16; // addr 单元 → 高 16 位
        }
        r
    }

    /// **32bit 写**，按字节使能（`memory_spec.md` §3.2 写表）：
    /// `be[0]=1` 把 `wdata[15:0]` 写入 `unit[addr]`；`be[1]=1` 把 `wdata[31:16]` 写入 `unit[addr+1]`。
    pub fn write32(&mut self, addr: u32, be: u8, wdata: u32) {
        if be & 0b01 != 0 {
            self.write(addr, (wdata & 0xFFFF) as u16);
        }
        if be & 0b10 != 0 {
            self.write(addr.wrapping_add(1), ((wdata >> 16) & 0xFFFF) as u16);
        }
    }
}
