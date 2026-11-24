# GPU 模拟器

用 **Rust（纯标准库，无第三方依赖）** 以“代码模拟电路行为”的方式，搭建 GPU 基础线程核心。

## 当前已完成

基础线程核心的模块（规格 + 实现代码）：

| 模块 | 规格文档 | 实现源码 | 要点 |
|---|---|---|---|
| 寄存器组 RegFile | `docs/regfile_spec.md` | `src/regfile.rs` | 16×16bit，4 组合读端口 + 1 上升沿写端口，异步复位全 0，写请求在 `tick()`（上升沿）统一提交 |
| 整数 ALU | `docs/alu_spec.md` | `src/alu.rs` | `op=0100`，func 区分 12 条指令；MUL 取低16bit；移位取低5bit、SHR 逻辑右移；SUB 即 CMP 并隐式比较产 10bit 标志 |
| FP16 FPU | `docs/fpu_spec.md` | `src/fpu.rs` | `op=0101`，IEEE 754 half；三源输入；RNE 舍入、溢出→±Inf、NaN 传播；HCVT func=0111 整转浮点 / 1000 浮点转整；6bit 浮点标志仅 HSUB/CMP 更新 |
| 线程核心 | `docs/thread_core_spec.md` | `src/thread_core.rs` | 指令译码控制模块 + 四类接口；已实现寄存器类（MOV/LDI/SEL）与整数/浮点计算类；指令端口 32bit、使能端口、16bit 计算标志端口、访存接口骨架 |

所有部件均为**纯组合逻辑、单周期出结果**；写回由调用方在时钟上升沿统一提交（`RegFile::tick` / `ThreadCore::cycle`）。

## 目录结构

```
GPU/
├── Cargo.toml
├── README.md
├── dump_xlsx.py              # 重读指令集脚本（保留，用户会更新 refs/GPU指令集.xlsx）
├── refs/
│   └── GPU指令集.xlsx        # 指令集依据（原始材料，唯一事实来源）
├── docs/
│   ├── PROJECT_STATUS.md     # 会话交接稳定框架（文档指针，勿固化设计）
│   ├── regfile_spec.md       # 寄存器组
│   ├── alu_spec.md           # 整数 ALU
│   ├── fpu_spec.md           # 浮点单元
│   ├── thread_core_spec.md   # 线程核心（译码控制 + 接口 + 指令状态表）
│   ├── mem_interface_spec.md # 内存接口
│   ├── branch_interface_spec.md # 分支端口（核心→BRU）
│   ├── bru_interface_spec.md # BRU 接口
│   └── bru_logic_spec.md     # BRU 内部逻辑
├── docs/coding_roadmap.md    # 编码就绪清单（模块↔源码↔规格、顺序、测试点）
└── src/
    ├── lib.rs             # 聚合导出
    ├── regfile.rs
    ├── alu.rs
    ├── fpu.rs
    └── thread_core.rs     # 译码控制 + 核心接口
```

## 各文档状态（便于编码筹备）

| 模块 | 文档 | 源码 | 状态 |
|---|---|---|---|
| 寄存器组 | `docs/regfile_spec.md` | `src/regfile.rs` | 已实现 |
| 整数 ALU | `docs/alu_spec.md` | `src/alu.rs` | 已实现 |
| FP16 FPU | `docs/fpu_spec.md` | `src/fpu.rs` | 已实现 |
| 线程核心 | `docs/thread_core_spec.md` | `src/thread_core.rs` | 寄存器/计算类已实现；流控/访存/原子/SYNC 待展开 |
| 内存接口 | `docs/mem_interface_spec.md` | — | 端口已定；协议待展开 |
| 分支端口 | `docs/branch_interface_spec.md` | — | 端口已定 |
| BRU 接口 | `docs/bru_interface_spec.md` | — | 接口已定 |
| BRU 逻辑 | `docs/bru_logic_spec.md` | — | 逻辑进行中（收敛/调用栈/BRAP·REA 部分已确认） |

> 编代码前的完整模块/顺序/测试视图见 [`docs/coding_roadmap.md`](docs/coding_roadmap.md)。每个部件的对外端口/编码以对应规格文档为准，指令集以 `refs/GPU指令集.xlsx` 为唯一事实来源。

## 编译 / 测试（命令建议，由你执行并反馈结果）

```bash
cargo build
cargo test
cargo build --release
```

> 约定：你说“编译”我才给/执行编译；目前我只写了代码，尚未在本会话中运行过编译。

## 仍悬置的项目级定义

- 对外接口形态（“提供对应的接口”具体指什么）
- 运行方式
- 验收标准
