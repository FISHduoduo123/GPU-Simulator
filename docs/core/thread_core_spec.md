# 线程核心（ThreadCore）—— GPU 模拟器
> 一条线程的执行主体：指令译码控制 + RegFile + ALU + FPU + 标志/掩码寄存器。

- 模块：线程核心（4 个组成一个 warp）
- 状态：主要指令已实现（`src/thread_core.rs`）；访存/原子协议待展开；SYNC 仅译码不阻塞
- 更新日期：2026-11-26
- 依据：`refs/GPU指令集.xlsx`、`docs/core/{regfile,alu,fpu}_spec.md`、`docs/warp/bru_spec.md`、`docs/memory/memory_spec.md`
- 关联：`docs/core/core_components.md`、`docs/warp/warp_spec.md`

## 1. 概述
核心内部包含 5 类状态/部件：
- **指令译码控制模块**：解析指令、调度执行、管理回写；内部持有寄存器组。
- **寄存器组 RegFile**：16×16bit，4 读 1 写（见 `docs/core/regfile_spec.md`）。
- **整数 ALU**：`op=0100`（见 `docs/core/alu_spec.md`）。
- **浮点 FPU**：`op=0101`（见 `docs/core/fpu_spec.md`）。
- **掩码寄存器**：**1bit**；写入来源 = `MOV type=0111` 时 `mask ← reg[0]`；当前值经掩码端口送 BRU。
- 整数标志寄存器（10bit）、浮点标志寄存器（6bit）为内部状态，合并成 16bit 计算标志端口输出。

## 2. 端口与接口
### 2.1 输入
| 端口 | 位宽 | 说明 |
|---|---|---|
| 指令端口 | **24bit** | 取自共享取指通道 `g_rdata[31:8]`（`instr = g_rdata >> 8`） |
| 使能端口 `en` | 1bit | `en=1` 执行；`en=0` 当 NOP |
| 访存接口 | 见 §3.4 | 读/写请求与读返回 |

### 2.2 输出
| 端口 | 位宽 | 说明 |
|---|---|---|
| 计算标志端口 | 16bit | 低 10bit 整数标志 + 高 6bit 浮点标志 |
| 掩码端口 | 1bit | 掩码寄存器当前值，送 BRU |
| 系统状态端口 `status` | 2bit | 状态寄存器当前值：`00 正常 / 01 HLT / 11 DONE`（`10` 保留，正常不发生）；上升沿提交，本拍输出旧值 |
| 分支端口 | 7 信号 | 见 §2.3 |

### 2.3 分支端口（输出到 BRU）
= `docs/warp/bru_spec.md` §2.1 的 7 信号：`br_jump_addr`(16)/`br_op`(4)/`br_cond`(4)/`br_uncond_attr`(1)/`br_func`(1)/`br_flags`(16)/`br_mask`(1)。
- `br_flags` 即 §2.2 的计算标志端口。
- **无请求有效信号**：BRU 以 `br_op != 0000` 判断本拍是否有分支请求。
- 核心只产生分支信息，**是否跳转/如何收敛由 BRU 裁决**。

> 时钟 `clk`（上升沿写回）与复位（异步、全 0）为内部公共信号。

## 3. 功能与行为
### 3.1 指令编码（24bit，高位对齐）
`op = I[23:20]`，随后 5 个 4bit 字段：

| 位域 | bits | 含义（多数指令） |
|---|---|---|
| op | [23:20] | 操作码 |
| f1 | [19:16] | 目的寄存器 / 分支地址高 4 位 等 |
| f2 | [15:12] | 源 1 |
| f3 | [11:8] | 源 2 / 基址 |
| f4 | [7:4] | 源 3 / 偏移 / 分支地址扩展 |
| f5 | [3:0] | func / type / cond |

- 对应 xlsx 列 C..H（各 4bit）。各指令的字段语义见 `docs/指令集表.md`。
- 取指：32bit `g_rdata` 上 24 位 → 24bit 指令端口。

### 3.2 单周期流程
1. **取指**：`en=1` 读指令端口；`en=0` 视为 NOP。
2. **译码**：解析 op/func、目的/源寄存器、立即数、访存字段。
3. **读源**：组合读寄存器组，送 ALU/FPU。
4. **执行**：纯组合、单周期。
5. **访存**：`op=1010` 时经访存接口发请求（§3.4）。
6. **标志**：比较结果写计算标志端口。
7. **写回**：结果经 WB MUX → 寄存器组写端口，`clk` 上升沿统一提交。

### 3.3 指令执行状态
| op | 指令 | 状态 |
|---|---|---|
| 0000 | NOP | 已实现 |
| 0001 | MOV（type 0000 reg⇔reg；**0001 warp_thread_id**（2D 编号 x 高8/y 低8，核 k→(k>>1,k&1)，每核不同）；**0010 warp_id**（warp 端口，2D 编号同打包，4 核同值，默认 0）；0011–0110 ← warp 端口 block_x/block_y/grid_x/grid_y；0111 mask←reg[0]）。均 16bit 写入 rd=f1 | ✅ 已实现 2026-11-28 |
| 0010 | LDI（rx ← imm16） | 已实现 |
| 0011 | SEL（rx ← cond ? a : b） | 已实现 |
| 0100 | 整数 ALU（12 条） | 已实现 |
| 0101 | FP16 FPU（8 条） | 已实现 |
| 0110 | BRA（普通/收敛） | 译码并输出分支端口；裁决在 BRU |
| 0111 | BRAF（条件分支） | 同上 |
| 1000 | BRAP / REA | 同上 |
| 1001 | CALL / RET | 同上 |
| 1010 | 访存 STR/LDR/PUSH/POP/ATADD/ATSWAP/ATCAS | 单周期模型已实现；协议/原子待展开 |
| 1011 | SYNC | 仅译码、不阻塞（语义待展开） |
| 1100–1111 | HLT / HLTF / DONE / DONEF | 已实现 |

> “已实现”指 `src/thread_core.rs` 有对应处理；流控最终由 BRU 决定跳转。

### 3.4 访存接口（骨架）
- **输出**：`mem_req_valid`（有请求）、`mem_req_cmd`（读/写/原子）、`mem_req_addr`、`mem_req_wdata`（及原子所需的第二数据）。
- **输入**：`mem_rd_valid` / `mem_rd_data`（读返回）。
- 覆盖 STR/LDR/ATADD/ATSWAP/ATCAS；详细端口与协议见 `docs/memory/memory_spec.md`。
- **PUSH/POP 不走普通访存口**，改为专用**栈请求输出端口**（2026-11-28 已定，接内存控制器）：

  | 端口 | 位宽 | 说明 |
  |---|---|---|
  | `stack_op` | 2bit | 00 无 / 01 PUSH / 10 POP |
  | `stack_addr` | 17bit | = `r15`（栈基址 / 内核地址） |
  | `stack_wdata` | 16bit | 仅 PUSH 有效（= f2 待压值） |

  - PUSH：f2 为待压值；POP：f1 为目的寄存器（读返回经普通 `mem_rd_data` 写回）。
  - 有效访存地址与计数器时序由**内存控制器**完成，见 `docs/memory/global_mem_controller_spec.md` §3.1。

### 3.5 计算标志端口（16bit）
- `flags[9:0]` = 10bit 整数标志；`flags[15:10]` = 6bit 浮点标志。
- 整数比较（ALU func=0001）写整数段；浮点比较（FPU func=0001）写浮点段。
- 非比较指令：对应段保持/清无效（valid 语义待确认）。

### 3.6 系统状态寄存器（2bit）
- 内部状态寄存器 `status`：`00 正常` / `01 HLT` / `11 DONE`；`10` 保留、正常情况下不发生。
- **时序**：执行 `HLT`/`DONE`（或 `HLTF`/`DONEF` 且条件满足）当拍只写入待提交值，**下个上升沿**才与其它寄存器一起提交；本拍输出的 `status` 仍是旧值，下一拍才见新值（无旁路）。
- `halted = status[0]`、`done = status[1]`；对外同时给出 2bit 状态端口与派生的 halted/done 布尔。
- 顶层停机时额外走一拍“收尾拍”以显示已锁存的状态，该拍不再取指，避免执行 HLT/DONE 之后的下一条指令。

## 4. 待确认 / 悬置
- 访存协议（地址/栈/读响应时序）与原子协议（见 `docs/memory/memory_spec.md`）。
- SYNC 的阻塞/栅栏语义。
- 标志端口 valid 位的精确语义。
- MOV type=0010（warp_id）来源：**warp 级输入端口 `warp_id`(16)**，2D 编号（x 高8/y 低8），4 核共享，调度器分配、默认 0（2026-11-28 已定，替代此前「写死」）。
- 对外接口形态、运行方式、验收标准（项目级，见 `docs/open_items.md` E1–E3）。

## 5. 变更记录
- 2026-11-23：确认内部结构、四类接口、24bit 编码、`en` 语义、单周期组合执行 + 上升沿写回、16bit 合并标志端口、访存骨架。
- 2026-11-24：新增 1bit 掩码寄存器与掩码端口；写入来源 = MOV type=0111。
- 2026-11-25：指令端口 32bit → 24bit（`g_rdata[31:8]`）。
- 2026-11-26：统一文档格式；状态更新为“主要指令已实现”；补全指令执行状态表。
- 2026-11-26：新增 2bit 系统状态寄存器与状态输出端口（HLT=01 / DONE=11，上升沿提交），新增 §3.6。
- 2026-11-28：PUSH/POP 改走专用栈请求输出端口（`stack_op`2/`stack_addr`17=r15/`stack_wdata`16），不再用普通访存口直发 r15 地址；有效地址与计数器由内存控制器完成（见 `docs/memory/global_mem_controller_spec.md` §3.1）。待落码。
- 2026-11-27：MOV type 0011/0100/0101/0110 的取值来源改为 **warp 外部输入端口** `block_x/block_y/grid_x/grid_y`（16bit，4 核共享同值；原误称 32bit 已更正为 16bit），见 `docs/warp/warp_spec.md` §2；warp_id(type 0010) 暂写死待定。待落码。
- 2026-11-28：MOV type 0001=**warp_thread_id**（2D 编号，x 高 8 位、y 低 8 位；4 线程 warp 核 k→x=k>>1、y=k&1，值 `(x<<8)|y`）；type 0010=**warp_id**（warp 级输入端口，2D 编号同打包，4 核共享，默认 0）。均 16bit 写入 rd=f1。详见 `docs/指令集表.md` MOV type 子表。**已落码实现**（`warp_thread_id()` + `GpuSystem::set_warp_inputs`，cargo build 通过并实测）。
