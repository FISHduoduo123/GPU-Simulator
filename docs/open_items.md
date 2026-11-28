# 架构待办总表（Open Items）—— GPU 模拟器

> 状态：进行中（随各 spec 的「待确认」演进）
> 日期：2026-11-28（本轮重写，反映最新进展）
> 用途：跨文档的架构未完成项总表。每项注明归属文档；**不猜测，待用户逐项确认**。
> 已定/已实现的项保留一行 struck 记录，便于追溯；真正未完成的集中在各节表格。

---

## A. 指令 / 编码（全部已定）

| # | 事项 | 结论 | 归属 | 状态 |
|---|---|---|---|---|
| A1 | 指令宽度/译码 | 24bit：`op=I[23:20]` + f1..f5；代码已重排（`src/thread_core.rs`/`src/system.rs`/`src/bru.rs`） | `docs/指令集表.md` | ✅ 已实现 |
| A2 | 分支目标地址宽度 | 16bit（`f1..f4=I[19:4]`，f5 作 attr/cond/func）；CALL 带目标地址 | `docs/指令集表.md` | ✅ 已定 2026-11-25 |
| A3 | 指令单元内字节序 | 大端：低地址单元放 `I[23:8]`、高地址单元放 `I[7:0]`（置于高 8 位）；取指交换上下 16 位 | `docs/memory/memory_spec.md` §3.3/§3.5 | ✅ 已定 2026-11-25 |
| A4 | PC 步长 / 地址口径 | PC 步长 **+2**；`pc[15:0]→g_raddr[15:0]`，`g_raddr[16]=0`；取指 4 字节/条（统一内存） | `docs/warp/pc_spec.md`；`docs/memory/memory_spec.md` §2.2/§3.6 | ✅ 已定 2026-11-25/27 |

## B. 流控 / BRU（全部已定）

B1–B7 均已确认（2026-11-26），摘要：收敛点不比较、弹栈用 `mask4` 恢复 `bru_en`、收敛/调用栈溢出信号暂留、RET 下溢忽略且 PC+2、CALL/RET 与 r15 独立且不影响使能/收敛、`bru_en[k]` 只控该核执行、同拍请求视为一个不仲裁、BRU 本周期组合输出（使能下拍锁存、PC 下拍置值）。详见 `docs/warp/bru_spec.md` §3。**无遗留**。

## C. 访存 / 内存（未完成）

| # | 事项 | 现状 | 归属 |
|---|---|---|---|
| C1 | 访存协议 | 地址 base+off 骨架已实现；栈（r15 PUSH/POP）方向与读响应时序仍悬 | `docs/core/thread_core_spec.md` §3.4；`docs/memory/memory_spec.md` §4 |
| C2 | 原子协议 | ATADD/ATSWAP/ATCAS 目前为单周期 RMW 临时语义；ATCAS 的 cmp/new 专用端口、P0/P1/P2 排队与同拍同地址仲裁未定 | `docs/memory/memory_spec.md`；`refs/GPU指令集.xlsx` P0/P1/P2 备注 |
| C3 | 内存模块对齐/介质 | 已按 16bit 单元 + 32bit 带字节使能实现（`BE_CORE=01`、`BE_FETCH=11`，`read32/write32/read32be`）；字节级 `Vec<u8>` 与非对齐行为未做 | `docs/memory/memory_spec.md` §3.2/§3.3 |

## D. Warp / 系统级

| # | 事项 | 现状 | 归属 |
|---|---|---|---|
| D1 | warp 对外接口：内存上移 GPU 全局级（分支 design/memory-decouple） | 设计中：warp 级内存控制接口 ↔ 全局内存控制器（`docs/memory/global_mem_controller_spec.md`）；端口/仲裁待细说 | `docs/warp/warp_spec.md` §4；`docs/memory/global_mem_controller_spec.md` |
| D2 | 共享取指通道接线 | 曾实现统一内存取指；**本分支将断开**，改经全局内存控制器（取指方式不变） | `docs/memory/memory_spec.md` §3.6；`docs/memory/global_mem_controller_spec.md` |
| D3 | 停机（HLT/DONE）后 PC 行为 | ✅ 已定（实现）：收尾拍 PC 保持、不再取指 | `docs/warp/pc_spec.md`；`src/system.rs` flush |
| D4 | SYNC 语义 | 当前为不阻塞桩；分支路径内栅栏可能死锁，阻塞语义待定 | `docs/core/thread_core_spec.md` §3.3 |
| D5 | **WCU 输出端口**列表/位宽/语义 | 已确认输入（4×2bit status）与聚合（全部核心 status≠00），输出待用户细说 | `docs/warp/wcu_spec.md` §2.2/§4 |
| D6 | **全局调度器**：block 列表内存格式、输出端口、槽位释放时机 | 已确认 block→4 槽位/FIFO/1 block=1 warp=4 核；上述三项待细说 | `docs/scheduler_spec.md` |
| D7 | warp 外部输入端口 `block_x/block_y/grid_x/grid_y`(16bit) 接入落码，MOV type 0011–0110 改接 | 规格已确认 2026-11-27，待「写代码」落码 | `docs/warp/warp_spec.md` §2；`docs/core/thread_core_spec.md` §3.3 |
| D8 | MOV type 0010（warp_id）来源 | 暂写死，之后再说 | `docs/core/thread_core_spec.md` §4 |

## E. 项目级（七维度仍悬置）

| # | 事项 | 归属 |
|---|---|---|
| E1 | 对外「对应接口」具体形态 | 各 spec 的「待确认」 |
| E2 | 运行方式 | 同上 |
| E3 | 验收标准 | 同上 |
| E4 | 汇编编译器（`assembler/` 目录，另起会话开发） | `docs/compiler_handoff.md` |

---

## 建议确认顺序（仅建议，由用户定）

1. **D5** WCU 输出端口（阻塞 warp 级停机落地）
2. **D6** 调度器 block 列表格式 / 输出端口 / 槽位释放（阻塞多 warp）
3. **C1 / C2** 访存协议与原子协议
4. **D4** SYNC 阻塞语义
5. **D8** warp_id 来源
6. **D7** warp 外部端口 + MOV 0011–0110 落码（需「写代码」口令）
7. **E1–E3** 项目级接口 / 运行方式 / 验收标准

## 变更记录

- 2026-11-25：创建，汇总 A–E 五类待办。
- 2026-11-26：A1 已实现；B1–B7 全部落实；同步各归属文档小节号。
- 2026-11-28：重写—— A 全部已定、B 全部已定、D2/D3 已实现，新增 D5（WCU 输出）/D6（调度器细节）/D7（warp 端口落码）/D8（warp_id）/E4（汇编器交接）；建议顺序按当前阻塞关系重排。
