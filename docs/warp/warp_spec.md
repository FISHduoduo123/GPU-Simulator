# Warp（线程束）—— GPU 模拟器
> 一个 warp = 4 个线程核心 + 1 个 BRU + 1 个 PC + 内存接口。

- 模块：warp 级组成
- 状态：组成已确认；内部协议见各子规格
- 更新日期：2026-11-26
- 依据：用户确认“四个核心 + BRU + PC + 内存接口构成一个 warp”
- 关联：`docs/core/thread_core_spec.md`、`docs/warp/{bru,pc}_spec.md`、`docs/memory/memory_spec.md`

## 1. 概述
| 组成单元 | 数量 | 规格文档 |
|---|---|---|
| 线程核心 ThreadCore | 4 | `docs/core/thread_core_spec.md` |
| 分支/收敛单元 BRU | 1 | `docs/warp/bru_spec.md` |
| 程序计数器 PC | 1 | `docs/warp/pc_spec.md` |
| 内存接口 | 1 组 | `docs/memory/memory_spec.md` |

- 4 核共用一个 PC，同拍执行同一地址（SIMT 锁步）。
- “wrap”即 warp。

## 2. 端口与接口（内部连接拓扑）
- **warp 外部输入端口（新增，2026-11-27/28）**：`block_x`(16)、`block_y`(16)、`grid_x`(16)、`grid_y`(16) —— **4 核共享同一组值**（用户先说 32bit，后更正为 **16bit**）。另增 **`warp_id`(16)**—— warp 二维编号（x 高 8 位、y 低 8 位，与 thread id 同样打包），4 核共享，默认 (0,0)=0，由调度器分配。以上供 MOV type 0010/0011/0100/0101/0110 读取（见 `docs/core/thread_core_spec.md`、`docs/指令集表.md` MOV type 子表）。
- **取指通道**：`PC.pc`(16) → `addr_in` → `g_raddr`(17) → 内存取指 → `g_rdata`(32) → 取高 24 位广播到 4 核指令端口。
- **分支通道**：4 核各送 7 信号/组（`br_jump_addr/br_op/br_cond/br_uncond_attr/br_func/br_flags/br_mask`）→ BRU；BRU 输出 `bru_jump_addr`(16) + `bru_jump_req`(1) → PC，以及 `bru_en`(4) → 4 核使能端口。
- **访存通道**：4 组读写端口（⇔ 4 核）+ 1 个共享取指读接口（⇔ 取指通道）。
- **标志**：各核 16bit 计算标志端口 → BRU 的 `br_flags`。

## 3. 功能与行为
- 4 核锁步：同一 PC 取同一条指令，由各自 `en` 决定是否执行。
- BRU 汇总分支请求、判定条件、给出新 PC 与 4 核使能（详见 `docs/warp/bru_spec.md`）。

## 4. 待确认 / 悬置
- warp 与外部（多个 warp / 调度器 / 全局内存仲裁）的接口。
- warp 内访存冲突仲裁（同拍同地址）。
- 项目级：接口形态/运行方式/验收标准（`docs/open_items.md` E1–E3）。

## 5. 变更记录
- 2026-11-25：创建；确认 warp 组成。
- 2026-11-26：统一文档格式。
- 2026-11-27：新增 4 个 warp 外部输入端口 `block_x/block_y/grid_x/grid_y`（16bit，4 核共享；原误称 32bit 已更正），供 MOV type 0011–0110 读取。
- 2026-11-28：新增 `warp_id`(16) 外部输入端口——warp 二维编号（x 高8/y 低8，同 thread id 打包），4 核共享，默认 0，供 MOV type 0010 读取。**已落码**（`GpuSystem::set_warp_inputs`，cargo build 通过并实测）。
