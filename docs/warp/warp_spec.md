# Warp（线程束）组成规格

- 项目：GPU 模拟器
- 模块：Warp 级组成
> 状态：进行中（组成已确认，内部协议见各子规格）
- 更新日期：2026-11-25
- 依据：用户确认「四个核心 + BRU + PC + 内存接口 构成一个 warp」

## 1. 组成（已确认）
**一个 Warp = 4 个线程核心 + 1 个 BRU + 1 个 PC + 内存接口**

| 组成单元 | 数量 | 规格文档 |
|---|---|---|
| 线程核心 ThreadCore | 4 | `docs/core/thread_core_spec.md` |
| 分支/收敛单元 BRU | 1 | `docs/warp/bru_interface_spec.md`、`docs/warp/bru_logic_spec.md` |
| 程序计数器 PC | 1 | `docs/warp/pc_spec.md` |
| 内存接口 | 1 组 | `docs/memory/mem_interface_spec.md`、`docs/memory/memory_module_spec.md` |

## 2. 内部连接（拓扑）
- **共享取指通道**：PC(`pc` 16bit) → `addr_in` → `g_raddr`(17bit) → 内存取指 → `g_rdata`(32bit) → **取上 24 位**广播到 4 核指令端口（4 核同拍执行同一条指令，SIMT 锁步）。
- **分支通道**：4 核各自输出分支端口（7 信号/组：`br_jump_addr`、`br_op`、`br_cond`、`br_uncond_attr`、`br_func`、`br_mask`、`br_flags`）→ BRU；BRU 输出 `bru_jump_addr`(16) + `bru_jump_req`(1) → PC，以及 `bru_en`(4) → 4 核使能端口。
- **访存接口**：4 组读写端口（⇔ 4 核）+ 1 共享取指读接口（⇔ 取指通道）。
- **计算标志**：各核 16bit 标志端口 → BRU 的 `br_flags`（判 cond 用）。

## 3. 说明
- 4 核共用 1 个 PC，天然锁步同地址执行（见 `docs/warp/bru_logic_spec.md`）。
- 「wrap」即 warp：一个 warp 内 4 线程由同一 PC/BRU 协调。

## 4. 悬置项
- Warp 与外部（多个 warp / 调度器 / 全局内存仲裁）的接口 —— 待展开。
- Warp 内访存冲突仲裁（同拍同地址）协议 —— 见 `docs/memory/memory_module_spec.md` §6。

## 5. 变更记录
- 2026-11-25：创建。确认 warp 组成 = 4 核心 + BRU + PC + 内存接口。
