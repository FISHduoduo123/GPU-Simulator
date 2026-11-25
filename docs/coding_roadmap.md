# 编码就绪清单（Coding Roadmap）—— GPU 模拟器

> 本文件是「**准备写代码**」的导航与检查表：只描述**模块划分、状态、顺序、测试点、待确认项**，**不复述设计结论**。
> 一切端口/编码/语义以对应规格文档为准；指令集以 `refs/GPU指令集.xlsx` 为**唯一事实来源**。
> **硬性规则**：未得到用户明确说「写代码」前，不输出实现代码；说「编译」只给命令。

- 工作区：`D:\Codes\GPU`
- 更新日期：2026-11-25

---

## 1. 模块 ↔ 规格 ↔ 源码 映射

| 模块 | 规格文档 | 源码 | 当前状态 |
|---|---|---|---|
| 寄存器组 RegFile | `docs/core/regfile_spec.md` | `src/regfile.rs` | 已实现 |
| 整数 ALU | `docs/core/alu_spec.md` | `src/alu.rs` | 已实现 |
| FP16 FPU | `docs/core/fpu_spec.md` | `src/fpu.rs` | 已实现 |
| 线程核心（译码控制 + 接口） | `docs/core/thread_core_spec.md` | `src/thread_core.rs` | 部分实现（见 §3） |
| 内核组件总览 | `docs/core/core_components.md` | — | 汇总页 |
| Warp 组成 | `docs/warp/warp_spec.md` | `src/system.rs` | 4 核+BRU+PC+内存接口，已确认 |
| PC | `docs/warp/pc_spec.md` | — | 接口已定 |
| 内存接口 | `docs/memory/mem_interface_spec.md` | （待建） | 端口已定，协议待展开 |
| 分支端口（核心→BRU） | `docs/warp/branch_interface_spec.md` | 线程核心输出侧 | 端口已定 |
| BRU 接口 | `docs/warp/bru_interface_spec.md` | （待建） | 接口已定 |
| BRU 内部逻辑 | `docs/warp/bru_logic_spec.md` | （待建） | 逻辑进行中 |
| 工程聚合 | `docs/PROJECT_STATUS.md` | `src/lib.rs`、`Cargo.toml` | 已有骨架 |

## 2. 全局约定（写代码时必须一致）

- **位宽**：数据/寄存器 16bit；指令 **24bit**（高位对齐，`op[23:20]` + 5×4bit 字段，经 dump_xlsx.py 核对，旧 32bit 假设已废弃）；PC/跳转地址 16bit；访存地址 17bit（数据）或 16bit（指令/P C）；寄存器号 4bit。
- **时序模型**：部件对外表现为「组合逻辑 + 上升沿统一写回」；软件模拟用「周期末 `tick`/`cycle` 提交 pending 写」等价实现。
- **复位**：异步复位，所有状态初值 0。
- **译码字段**：`op=I[23:20]`，`field_k = bits[20-4k : 17-4k]`，`k=1..5`（即 `op` 后 5 个连续 4bit 字段，`f1=I[19:16]`…`f5=I[3:0]`）。对应 xlsx 列 C..H。
- **无第三方依赖**：纯 Rust `std`。
- **指令事实来源**：`refs/GPU指令集.xlsx`；需重读时运行 `dump_xlsx.py`（bundled python：`C:\Users\FISHduoduo\.dsh\dsh-runtimes\dsh-primary-runtime\dependencies\python\python.exe`）。

## 3. 线程核心实现状态（对照 `docs/core/thread_core_spec.md` §7）

- **已实现**：NOP、MOV（type 0000–0110）、LDI、SEL、整数 ALU（op=0100）、FP16 FPU（op=0101）、HLT/HLTF/DONE/DONEF。
- **待实现**：
  - MOV `type=0111` → `mask ← reg[0]`（掩码寄存器写入）。
  - 流控译码（BRA/BRAF/BRAP/REA/CALL/RET）：产生 `br_op/br_jump_addr/br_cond/br_func/br_uncond_attr/br_mask` 分支端口信号。
  - SYNC。
- **待展开（协议未定，先不写死）**：访存 STR/LDR/PUSH/POP 的地址/栈/响应时序；原子 ATADD/ATSWAP/ATCAS 及 ATCAS 的 cmp/new 端口。

## 4. 建议实现顺序（每步都需用户「写代码」口令）

1. **补齐线程核心**：MOV type=0111 掩码写入 → 流控译码（仅生成端口信号，不做跳转）→ SYNC。
2. **BRU**：按 `docs/warp/bru_logic_spec.md` 实现 BRA 普通/收敛、BRAF 三状态收敛栈、CALL/RET 调用栈、BRAP/REA；先把「收敛栈 LIFO 契约 / 地址比较结果 / 路径2 重启掩码」等 §7 待确认项定稿。
3. **内存接口 + 访存/栈协议**：先定 17bit 寻址粒度、读响应时序，再实现 STR/LDR/PUSH/POP。
4. **原子协议**：ATADD/ATSWAP/ATCAS（含 cmp/new 专用端口）。
5. **顶层装配**：4× 线程核心 + BRU + PC + 内存，接 `bru_en`、`bru_jump_req/addr`、指令取指。

## 5. 测试/验证要点（建议）

| 模块 | 关键用例 |
|---|---|
| RegFile | 4 端口同拍读；同周期写读同地址读到旧值、沿后读到新值；异步复位全 0 |
| ALU | 12 条 func；ADD/SUB 回绕；MUL 低 16bit；移位 b 低 5bit 且 ≥16 归 0；仅 SUB/CMP 更新标志 |
| FPU | RNE 舍入；溢出 ±Inf、下溢 subnormal/0、NaN 传播、1/0→±Inf；HCVT 0111/1000 双向；仅 HSUB/CMP 更新浮点标志 |
| 线程核心 | 指令编码字段偏移 `fld`；WB MUX 选择；标志端口 16bit 打包（低 10 + 高 6）；en=0 当 NOP |
| BRU | BRA 普通直达 / func=1 收敛；BRAF 三状态（普通/路径1/路径2）与压栈/使能；CALL 压 PC+2 且跳目标、RET 弹栈；BRAP 掩码关核、REA 全使能 |

## 6. 仍阻塞「完整实现」的待确认项（汇总，勿猜）

> 跨文档的完整待办总表见 **`docs/open_items.md`**（A 指令编码 / B 流控BRU / C 访存 / D warp系统级 / E 项目级）。

- **BRU 收敛栈**：嵌套分支的 LIFO 契约、路径 2「地址比较」结果的具体作用、栈深/溢出。
- **掩码语义**：栈条目 4bit 的满足/使能语义、路径 2 重启时的掩码推导；掩码寄存器与 `bru_en` 的位对应。
- **CALL/RET**：栈溢出/下溢；在发散区内执行 CALL 的语义。
- **两栈交互**：收敛栈与调用栈的相互影响。
- **地址空间**：取指空间（16bit PC）与数据空间（17bit）是否同一内存；PC+2 的指令容量影响。
- **访存**：17bit 寻址粒度（字节 vs 16bit 字）、读响应时序（单拍 vs 握手）、栈（r15）语义。
- **原子**：ATADD/ATSWAP/ATCAS 语义与 ATCAS 的 cmp/new 端口。
- **项目级**：对外接口形态、运行方式、验收标准（仍悬置）。

## 7. 变更记录
- 2026-11-24：创建本文件，作为编码就绪的模块映射 / 顺序 / 测试 / 待确认项汇总（不固化设计结论）。
- 2026-11-25：§6 增加指向 `docs/open_items.md` 架构待办总表的入口。
