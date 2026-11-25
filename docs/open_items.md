# 架构待办总表（Open Items）—— GPU 模拟器

> 状态：进行中（随各 spec 的「待确认」演进）
> 日期：2026-11-25
> 用途：跨文档的架构未完成项总表。每项注明归属文档；**不猜测，待用户逐项确认**。
> 来源：2026-11-25 汇总自各 `docs/**` 的「待确认/悬置」小节。

## A. 指令 / 编码
| # | 事项 | 归属 | 备注 |
|---|---|---|---|
| A1 | 现有代码仍按 32bit 解码（`op[31:28]`、`fld` 位移 `28-4n`），需在「写代码」时重排为 24bit（`op[23:20]`+5 字段） | `docs/memory/memory_module_spec.md` §6；`src/thread_core.rs`、`src/bru.rs` | 规格已定 24bit，代码未改 |
| A2 | 分支目标地址宽度/折算：xlsx 中 BRA/BRAF/BRAP 的 `addr` 只占 `f1`(4bit)，但 `br_jump_addr` 为 16bit；相对/绝对、折算规则待定 | `docs/warp/branch_interface_spec.md` §6 | 与 A1 相关 |
| A3 | 指令 `+1` 单元内部字节序（暂按小端） | `docs/memory/memory_module_spec.md` §6 | 待确认 |
| A4 | PC 步长/地址口径：取指 4 字节/条 vs PC+2；PC 16bit 空间 vs 数据 17bit 地址空间 | `docs/warp/pc_spec.md` §4；`docs/memory/memory_module_spec.md` §6 | 待确认 |

## B. 流控 / BRU
| # | 事项 | 归属 |
|---|---|---|
| B1 | 收敛栈完整语义：路径2「比较栈顶地址与指令地址」结果预留用途、栈深度/溢出、LIFO 契约 | `docs/warp/bru_logic_spec.md` §7 |
| B2 | 调用栈溢出/下溢；CALL/RET 是否影响使能/收敛状态 | `docs/warp/bru_logic_spec.md` §7 |
| B3 | `bru_en[k]` 精确语义（允许跳转 vs 允许执行目标） | `docs/warp/bru_interface_spec.md` §5 |
| B4 | 分支目标地址折算（同 A2） | `docs/warp/branch_interface_spec.md` §6 |

## C. 访存 / 内存
| # | 事项 | 归属 |
|---|---|---|
| C1 | 访存协议：地址来源(base+off)、栈协议(r15 PUSH/POP 方向)、读响应时序/握手、寻址粒度 | `docs/core/thread_core_spec.md` §5；`docs/memory/mem_interface_spec.md` §6 |
| C2 | 原子协议：ATCAS 的 cmp/new 专用端口；ATADD/ATSWAP/ATCAS 语义；P0/P1/P2 排队与同拍同地址冲突仲裁 | `docs/memory/mem_interface_spec.md`；`refs/GPU指令集.xlsx` P0/P1/P2 备注 |
| C3 | 内存模块：4 字节对齐的非对齐行为；存储介质按字节 `Vec<u8>` 还是按字 | `docs/memory/memory_module_spec.md` §6 |

## D. Warp / 系统级
| # | 事项 | 归属 |
|---|---|---|
| D1 | Warp 对外接口：多 warp / 调度器 / 全局内存仲裁 | `docs/warp/warp_spec.md` §4 |
| D2 | 共享取指通道尚未接线（当前 `GpuSystem` 用内部 `imem[pc>>1]`） | `docs/memory/mem_interface_spec.md` §5 |
| D3 | 停机(HLT/DONE)后 PC 是否保持 | `docs/warp/pc_spec.md` §4 |
| D4 | SYNC 语义：当前为不阻塞桩；分支路径内栅栏可能死锁，阻塞语义待定 | `docs/core/thread_core_spec.md` §7；xlsx SYNC 备注 |

## E. 项目级（最初七维度仍悬置）
| # | 事项 | 归属 |
|---|---|---|
| E1 | 对外「对应接口」具体形态 | 各 spec 的「仍悬置的项目级定义」 |
| E2 | 运行方式 | 同上 |
| E3 | 验收标准 | 同上 |

## 建议确认顺序（仅建议，由用户定）
1. A1/A2（24bit 代码重排 + 分支地址折算）—— 影响译码与 BRU，尽早定。
2. A4/D3（PC 口径与停机行为）—— 影响取指与仿真循环。
3. B1–B4（BRU 收敛/调用栈/使能语义）—— 影响流控正确性。
4. C1–C3（访存/原子协议）—— 影响内存交互。
5. D1/D2（warp 对外与取指通道接线）—— 影响系统装配。
6. E1–E3（项目级接口/运行/验收）。

## 变更记录
- 2026-11-25：创建。汇总 A/B/C/D/E 五类待办，标注归属文档。
