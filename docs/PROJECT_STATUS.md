# PROJECT_STATUS —— GPU 模拟器（会话交接框架）

> **本文件只提供稳定的"框架 + 指针"。**
> 一切**具体设计**始终以对应文档为准（实时更新），本文件**不写死任何设计结论**，以免过期误导。
> 接手步骤：① 读本文件 ② 按 §2 各文档文件逐一读 ③ 需最新指令集时用 `dump_xlsx.py` 重读 `refs/GPU指令集.xlsx`。

- 工作区：`D:\Codes\GPU`
- 项目：GPU 模拟器 —— 用 Rust 代码模拟 GPU 硬件行为并提供对应接口
- 更新日期：2026-11-24

---

## 1. 项目硬性规则（务必遵守）
1. 未得到用户明确说「写代码」前，**禁止输出任何实现代码**；只做规格确认、文档、命令建议。
2. 用户说「编译」时**只给命令建议**，由用户执行并反馈。
3. **所有进度用中文汇报**。
4. 设计决策一律**先逐项与用户确认、不猜测**；确认结果才落入对应规格文档。

## 2. 设计文档指针（实时为准，勿在本文件固化内容）
| 文档 | 侧重点 |
|---|---|
| `docs/regfile_spec.md` | 寄存器组 |
| `docs/alu_spec.md` | 整数 ALU |
| `docs/fpu_spec.md` | 浮点单元 |
| `docs/thread_core_spec.md` | 线程核心（译码+ALU+FPU、接口） |
| `docs/mem_interface_spec.md` | 内存接口 |
| `docs/branch_interface_spec.md` | 分支端口（核心→BRU） |
| `docs/bru_interface_spec.md` | BRU 接口 |
| `docs/bru_logic_spec.md` | BRU 内部逻辑（进行中） |
| `docs/coding_roadmap.md` | 编码就绪清单（模块↔源码↔规格、顺序、测试点、待确认项汇总） |
| `docs/architecture_modules.mmd` | Mermaid 视图：模块连接架构 |
| `docs/architecture_bru_flow.mmd` | Mermaid 视图：BRU 逻辑流程 |
> 每份文档自带「变更记录」，修订沿革以该文档为准。

## 3. 工程文件
- 源码：`src/lib.rs` `src/regfile.rs` `src/alu.rs` `src/fpu.rs` `src/thread_core.rs`（**已写、未编译**；编译/实现需用户口令）。
- 指令集：`refs/GPU指令集.xlsx`（用户会频繁更新）。
- 重读脚本：`dump_xlsx.py`（**保留**，用于随时重读指令集）。
- 工程：`Cargo.toml`、`README.md`。

## 4. 交接待办（常见切入点，供新会话参考）
- 各规格文档当前的「**待确认项**」小节是接手必读的最新鲜状态。
- 典型下一步：收敛/多目标规则、BRAF 等 BRU 细分、BRA func=1 用途、访存/栈协议、原子协议、或依用户「写代码」实现各模块。

## 5. 变更记录
- 2026-11-24：创建本文件为**稳定交接框架**；具体设计一律指向 `docs/*.md`（实时更新），不在此固化。
- 2026-11-24：文档指针新增 `docs/coding_roadmap.md`（编码就绪清单）。