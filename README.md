# GPU 模拟器

用 **Rust（纯标准库、零依赖）** 以「代码模拟电路行为」的方式搭建 GPU 基础线程核心，并组装成一个 **Warp**（4 核 + BRU + PC + 内存）运行观察。指令 24bit，权威依据 `refs/GPU指令集.xlsx`（逐条确认见 [`docs/指令集表.md`](docs/指令集表.md)）。

已实现：寄存器组、整数 ALU、FP16 FPU、线程核心、BRU、内存、顶层装配与命令行模拟器。

## 快速上手

```bash
cargo build                                        # 编译
cargo run --bin gpu -- demo                        # 内置示例（PC=0）
cargo run --bin gpu -- run <程序文件> [最大周期]    # 连续跑
cargo run --bin gpu -- step <程序文件> [周期数]    # 交互单步
```

## 命令行

- 启动时**交互式设置 PC**（`0x…`=十六进制，否则十进制，空=0，越界 0..65535 拒绝）。
- `step` 每周期提示：`s`/空行=下一周期，`m`=以 .hex 格式打印当前内存，`q`/EOF=退出；停机自动结束。
- 每周期打印：`pc/instr/en/jump`、BRU（state/收敛栈/调用栈）、每核 `En/stt/flags/mask/r0..r15`。
- 程序文件（首行含 `:` 判定为 .hex）：
  - **.hex 内存镜像** `ADDR: w0 w1 …`（半字地址）：载入统一内存，指令按大端 2 半字打包取指。
  - **汇编/24bit 十六进制**：每行一条指令，`#` 注释。

## 目录结构

```
GPU/
├── Cargo.toml / README.md
├── refs/GPU指令集.xlsx        # 指令集唯一事实来源
├── tools/dump_xlsx.py         # 重读指令集（生成物已 gitignore）
├── assembler/                 # 汇编编译器（另一会话）
├── docs/                      # 规格 / 清单 / 待办
└── src/  lib.rs main.rs regfile.rs alu.rs fpu.rs thread_core.rs bru.rs memory.rs system.rs
```

## 组件与状态

| 模块 | 规格 | 源码 | 状态 |
|---|---|---|---|
| 寄存器组 | `docs/core/regfile_spec.md` | `src/regfile.rs` | 已实现 |
| 整数 ALU | `docs/core/alu_spec.md` | `src/alu.rs` | 已实现 |
| FP16 FPU | `docs/core/fpu_spec.md` | `src/fpu.rs` | 已实现 |
| 线程核心 | `docs/core/thread_core_spec.md` | `src/thread_core.rs` | 已实现；访存/原子/SYNC 待展开 |
| BRU | `docs/warp/bru_spec.md` | `src/bru.rs` | 已实现；收敛栈细节待完善 |
| 内存 | `docs/memory/memory_spec.md` | `src/memory.rs` | 已实现；协议待展开 |
| PC / Warp | `docs/warp/pc_spec.md` / `warp_spec.md` | — | 已确认 |

## 文档

- [`docs/指令集表.md`](docs/指令集表.md) — 24bit 指令集权威字段表（每条指令的编码与字段含义）
- `docs/` 下另有各模块规格（core / warp / memory）与 Mermaid 架构图（`docs/diagrams/`）
