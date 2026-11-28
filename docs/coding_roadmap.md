# 编码就绪清单（Coding Roadmap）—— GPU 模拟器

> 本文件是「准备写代码」的导航与检查表：只描述**模块划分、状态、顺序、测试点、待确认项**，不复述设计结论。
> 一切端口/编码/语义以对应规格文档为准；指令集以 `refs/GPU指令集.xlsx` 为唯一事实来源。
> 硬性规则：未得到用户明确说「写代码」前，不输出实现代码；说「编译」只给命令。

- 工作区：`D:\Codes\GPU`
- 更新日期：2026-11-26

---

## 1. 模块 ↔ 规格 ↔ 源码 映射

| 模块 | 规格文档 | 源码 | 当前状态 |
|---|---|---|---|
| 寄存器组 RegFile | `docs/core/regfile_spec.md` | `src/regfile.rs` | 已实现 |
| 整数 ALU | `docs/core/alu_spec.md` | `src/alu.rs` | 已实现 |
| FP16 FPU | `docs/core/fpu_spec.md` | `src/fpu.rs` | 已实现 |
| 线程核心（译码控制 + 接口） | `docs/core/thread_core_spec.md` | `src/thread_core.rs` | 已实现主要指令 |
| 内核组件总览 | `docs/core/core_components.md` | — | 汇总页 |
| Warp 组成 | `docs/warp/warp_spec.md` | `src/system.rs` | 4 核+BRU+PC+内存，已装配 |
| PC | `docs/warp/pc_spec.md` | `src/system.rs` | 已实现（内联在 system） |
| 内存（接口+模块） | `docs/memory/memory_spec.md` | `src/memory.rs` | 简化 16bit 字模型；32bit/字节使能待接入 |
| BRU（端口+接口+逻辑） | `docs/warp/bru_spec.md` | `src/bru.rs` | 已实现 |
| 顶层 / CLI | — | `src/lib.rs`、`src/main.rs`、`Cargo.toml` | 已装配可运行 |

## 2. 全局约定（写代码时必须一致）

- **位宽**：数据/寄存器 16bit；指令 **24bit**（高位对齐 `op[23:20]` + 5×4bit 字段）；PC/跳转地址 16bit；访存地址 17bit（数据）或 16bit（指令取指）；寄存器号 4bit。
- **时序模型**：组合逻辑 + 上升沿统一写回；软件模拟用「周期末 `tick`/`cycle` 提交 pending 写」等价实现。
- **复位**：异步复位，所有状态初值 0。
- **译码字段**：`op=I[23:20]`，`f_k = bits[20-4k : 17-4k]`，`k=1..5`（`f1=I[19:16]`…`f5=I[3:0]`），对应 xlsx 列 C..H。
- **无第三方依赖**：纯 Rust `std`。
- **指令事实来源**：`refs/GPU指令集.xlsx`；重读用 `tools/dump_xlsx.py`（bundled python：`C:\Users\FISHduoduo\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe`）。

## 3. 线程核心实现状态（对照 `docs/core/thread_core_spec.md` §3.3）

- **已实现**：NOP、MOV（含 `type=0111` → `mask←reg[0]`）、LDI、SEL、整数 ALU、FP16 FPU、HLT/HLTF/DONE/DONEF；流控指令（BRA/BRAF/BRAP/REA/CALL/RET）译码并输出分支端口。
- **BRU 已实现**：BRA 双属性、BRAF 三状态收敛栈、CALL/RET 调用栈、BRAP/REA。
- **待展开（协议未定，先不写死）**：访存协议（STR/LDR/PUSH/POP 地址/栈/响应时序）；原子（ATADD/ATSWAP/ATCAS 及 ATCAS 的 cmp/new 端口）；SYNC 语义。

## 4. 建议实现顺序（每步都需用户「写代码」口令）

1. **访存协议**：定 17bit 寻址粒度、读响应时序，实现 STR/LDR/PUSH/POP。
2. **原子协议**：ATADD/ATSWAP/ATCAS（含 cmp/new 专用端口）。
3. **共享取指通道接线**：用 `g_raddr/g_rdata` 替换当前内部 `imem[pc>>1]`。
4. **顶层装配细化**：访存 32bit/字节使能与 4 组端口接入。
5. **BRU 收尾**：收敛后以栈顶 `mask4` 恢复使能子集（open_items B3）。

## 5. 测试/验证要点（建议）

| 模块 | 关键用例 |
|---|---|
| RegFile | 4 端口同拍读；同拍写读同地址沿前读旧值；异步复位全 0 |
| ALU | 12 条 func；回绕；MUL 低 16bit；移位低 5bit 且 ≥16 归 0；仅 SUB/CMP 更新标志 |
| FPU | RNE；溢出/下溢/NaN/1÷0；HCVT 双向；仅 HSUB/CMP 更新浮点标志 |
| 线程核心 | 24bit 字段偏移；WB MUX；标志端口 16bit 打包；en=0 当 NOP；掩码写入 |
| BRU | BRA 普通/收敛；BRAF 三状态与压栈/使能；CALL 压 PC+2 且跳目标、RET 弹栈；BRAP 关核、REA 全使能；CALL/RET 不影响使能/收敛 |
| 系统 | 取指/NOP；LDI 广播 4 核；STR→LDR；BRA 跳转；停机 |

## 6. 仍阻塞「完整实现」的待确认项（汇总，勿猜）

> 跨文档完整待办总表见 **`docs/open_items.md`**（A 指令编码 / B 流控BRU / C 访存 / D warp系统级 / E 项目级）。

- **访存协议**：17bit 寻址粒度、读响应时序（单拍 vs 握手）、栈（r15）语义、冲突仲裁。
- **原子协议**：ATADD/ATSWAP/ATCAS 语义与 ATCAS 的 cmp/new 端口、P0/P1/P2 排队。
- **地址空间**：取指（16bit PC）与数据（17bit）是否同一内存；PC+2 的指令容量影响。
- **取指通道**：当前用内部 `imem`，共享取指通道未接线。
- **BRU 收尾**：收敛后是否用栈顶 `mask4` 恢复使能子集。
- **项目级**：对外接口形态、运行方式、验收标准（仍悬置）。

## 7. 变更记录
- 2026-11-24：创建本文件。
- 2026-11-25：§6 增加指向 `docs/open_items.md` 的入口。
- 2026-11-26：更新状态（24bit 重排、BRU、CLI 已实现）；同步各归属文档小节号；格式统一。