//! 数据内存 —— 规格见 `docs/mem_interface_spec.md`。
//!
//! 当前为**同步、单周期、顺序访问**的软件模型：`read` 组合取值、`write` 覆盖写。
//! 寻址粒度 / 响应时序 / 原子排队（P0/P1/P2 同地址行为）已在规格中列为待确认项，
//! 此处按最简可行模型实现，后续按协议展开。
//!
//! 地址 17bit；`read`/`write` 对长度取模，避免越界。

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
}

#[cfg(test)]
mod tests {
    use super::Memory;

    #[test]
    fn write_then_read() {
        let mut m = Memory::new(64);
        m.write(0x100, 0xABCD);
        assert_eq!(m.read(0x100), 0xABCD);
        assert_eq!(m.peek(0x100 % 64), 0xABCD);
    }

    #[test]
    fn wrap_around_mask() {
        let mut m = Memory::new(64);
        // addr = 0x1FFFF + 1 → (0x20000) & 0x1FFFF = 0 → 取模落到 index 0
        m.write(0x1FFFF + 1, 0x55);
        assert_eq!(m.read(0x1FFFF + 1), 0x55);
        assert_eq!(m.peek(0), 0x55);
    }
}