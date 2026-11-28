//! 寄存器组（RegFile）—— 参见 `docs/regfile_spec.md`。
//!
//! 规格要点：
//! - 16 个寄存器 × 16bit（r0..r15，4bit 地址）
//! - 4 个组合读端口（A/B/C/D），可同时读、允许同地址
//! - 1 个写端口，`clk` 上升沿同步写
//! - 时序语义：上升沿之后读新值、之前读旧值（故无需写旁路）
//! - 异步复位，初始全 0
//!
//! 软件模拟模型：写入先暂存为 pending，`tick()` 模拟时钟上升沿统一提交，
//! 从而严格等价于边沿触发寄存器 + 组合读。

/// 寄存器组：16 × 16bit。
///
/// 组合读通过 [`RegFile::read`] / [`RegFile::reads4`] 完成；写入与时钟推进通过
/// [`RegFile::write`] / [`RegFile::tick`] 完成。
#[derive(Debug, Clone)]
pub struct RegFile {
    regs: [u16; 16],
    // pending write：在中一个周期内暂存，tick（上升沿）时才提交。
    wen: bool,
    waddr: u8,
    wdata: u16,
    // 上一拍（上一次 tick 提交前）的快照，供调试 / 波形对比。
    prev: [u16; 16],
}

impl RegFile {
    /// 寄存器数量。
    pub const N: usize = 16;

    /// 构造一个全部清零的寄存器组。
    pub fn new() -> Self {
        Self {
            regs: [0u16; 16],
            wen: false,
            waddr: 0,
            wdata: 0,
            prev: [0u16; 16],
        }
    }

    /// 异步复位：所有寄存器立即清 0，并清空 pending 写请求。
    pub fn reset(&mut self) {
        self.regs = [0u16; 16];
        self.wen = false;
        self.waddr = 0;
        self.wdata = 0;
        self.prev = [0u16; 16];
    }

    /// 组合读（单个读端口）：返回 `addr` 指向寄存器的当前值。
    /// 地址按 4bit 截断（0..=15）。
    pub fn read(&self, addr: u8) -> u16 {
        self.regs[(addr as usize) & 0xF]
    }

    /// 4 个组合读端口同时读取，对应规格中端口 A/B/C/D。
    pub fn reads4(&self, addrs: [u8; 4]) -> [u16; 4] {
        [
            self.read(addrs[0]),
            self.read(addrs[1]),
            self.read(addrs[2]),
            self.read(addrs[3]),
        ]
    }

    /// 写端口：暂存写请求。本周期内 [`RegFile::read`] 仍读到旧值，
    /// 直到 [`RegFile::tick`]（上升沿）才真正提交写回。
    pub fn write(&mut self, wen: bool, waddr: u8, wdata: u16) {
        self.wen = wen;
        self.waddr = waddr;
        self.wdata = wdata;
    }

    /// 模拟时钟上升沿：先保存上一拍快照，再提交 pending 写请求，并清空写使能。
    pub fn tick(&mut self) {
        self.prev = self.regs;
        if self.wen {
            self.regs[(self.waddr as usize) & 0xF] = self.wdata;
        }
        self.wen = false;
    }

    /// 上一拍快照（tick 提交前的寄存器值）。
    pub fn prev(&self) -> &[u16; 16] {
        &self.prev
    }

    /// 当前寄存器全量快照。
    pub fn snapshot(&self) -> [u16; 16] {
        self.regs
    }
}

impl Default for RegFile {
    fn default() -> Self {
        Self::new()
    }
}
