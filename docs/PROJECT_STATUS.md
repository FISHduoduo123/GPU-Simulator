# PROJECT_STATUS —— GPU 模拟器（会话交接框架）

> **本文件只提供稳定的"框架 + 指针"。**
> 一切**具体设计**始终以对应文档为准（实时更新），本文件**不写死任何设计结论**，以免过期误导。
> 接手步骤：① 读本文件 ② 按 §2 各文档文件逐一读 ③ 需最新指令集时用 `dump_xlsx.py` 重读 `refs/GPU指令集.xlsx`。

- 工作区：`D:\Codes\GPU`
- 项目：GPU 模拟器 —— 用 Rust 代码模拟 GPU 硬件行为并提供对应接口
- 更新日期：2026-11-25

---

## 1. 项目硬性规则（务必遵守）
1. 未得到用户明确说「写代码」前，**禁止输出任何实现代码**；只做规格确认、文档、命令建议。
2. 用户说「编译」时**只给命令建议**，由用户执行并反馈。
3. **所有进度用中文汇报**。
4. 设计决策一律**先逐项与用户确认、不猜测**；确认结果才落入对应规格文档。
5. 用户说「提交」时，把**当前项目最新进度提交并推送**到 GitHub（仓库 `FISHduoduo123/GPU-Simulator`）。

## 2. 设计文档指针（实时为准，勿在本文件固化内容）
| 文档 | 侧重点 |
|---|---|
| `docs/core/regfile_spec.md` | 寄存器组 |
| `docs/core/alu_spec.md` | 整数 ALU |
| `docs/core/fpu_spec.md` | 浮点单元 |
| `docs/core/thread_core_spec.md` | 线程核心（译码+ALU+FPU、接口） |
| `docs/core/core_components.md` | **内核内部组件总览**（译码控制/RegFile/ALU/FPU/标志/掩码 一页汇总） |
| `docs/memory/mem_interface_spec.md` | 内存接口 |
| `docs/memory/memory_module_spec.md` | 测试阶段内存模块（64KB / 4 读写接口+1 取指读接口 / 字节使能） |
| `docs/warp/branch_interface_spec.md` | 分支端口（核心→BRU） |
| `docs/warp/bru_interface_spec.md` | BRU 接口 |
| `docs/warp/bru_logic_spec.md` | BRU 内部逻辑（进行中） |
| `docs/warp/pc_spec.md` | 程序计数器 PC（jump_req=1→跳转地址，否则+2，溢出置0） |
| `docs/warp/warp_spec.md` | Warp 组成：4 核心 + BRU + PC + 内存接口 构成一个 warp |
| `docs/coding_roadmap.md` | 编码就绪清单（模块↔源码↔规格、顺序、测试点、待确认项汇总） |
| `docs/open_items.md` | **架构待办总表**（跨文档未完成项 A–E 分类汇总） |
| `docs/diagrams/architecture_modules.mmd` | Mermaid 视图：模块连接架构 |
| `docs/diagrams/architecture_bru_flow.mmd` | Mermaid 视图：BRU 逻辑流程 |
> 每份文档自带「变更记录」，修订沿革以该文档为准。

## 3. 工程文件
- 源码：`src/lib.rs` `src/regfile.rs` `src/alu.rs` `src/fpu.rs` `src/thread_core.rs` `src/bru.rs` `src/memory.rs` `src/system.rs`（核心模块已实现；`cargo test` 本地已通过 **46/46**，但编译执行仍以用户口令为准则）。
- 指令集：`refs/GPU指令集.xlsx`（用户会频繁更新）。
- 重读脚本：`dump_xlsx.py`（**保留**，用于随时重读指令集）。
- 工程：`Cargo.toml`、`README.md`。
- 远程：`git remote` 指向 `ssh://git@ssh.github.com:443/FISHduoduo123/GPU-Simulator.git`（端口 443；`git push` 需 danger-full-access 权限，否则 msys sh 报 signal pipe 错）。

## 4. 交接待办（常见切入点，供新会话参考）
- 各规格文档当前的「**待确认项**」小节是接手必读的最新鲜状态。
- 典型下一步：收敛/多目标规则、BRAF 等 BRU 细分、BRA func=1 用途、访存/栈协议、原子协议、或依用户「写代码」实现各模块。

## 5. 变更记录
- 2026-11-24：创建本文件为**稳定交接框架**；具体设计一律指向 `docs/*.md`（实时更新），不在此固化。
- 2026-11-24：文档指针新增 `docs/coding_roadmap.md`（编码就绪清单）。
- 2026-11-24：新增硬性规则 5（用户说「提交」即提交并推送 GitHub）；§3 更新已实现源码清单（含 bru/memory/system）与远程说明。
- 2026-11-24：文档指针新增 `docs/warp/pc_spec.md`（程序计数器）；`docs/memory/mem_interface_spec.md` 升级为 4 核向量化接口并新增共享取指副读通道。
- 2026-11-24：文档指针新增 `docs/memory/memory_module_spec.md`（测试阶段内存模块：64KB、16bit 字节地址、32bit 数据、4 读写接口 + 1 取指读接口、字节使能小端）。
- 2026-11-25：文档指针新增 `docs/warp/warp_spec.md`——确认 **一个 warp = 4 线程核心 + 1 BRU + 1 PC + 内存接口**；指令重基线为 24bit（核心指令端口 24bit，取 `g_rdata[31:8]`），详见 `docs/core/thread_core_spec.md` §3、`docs/memory/mem_interface_spec.md` 修订 H、`docs/memory/memory_module_spec.md` 修订 D/E。
- 2026-11-25：新增 `docs/core/core_components.md`（内核内部组件总览，一页汇总 译码控制/RegFile/ALU/FPU/整数标志/浮点标志/掩码寄存器）；全体 md 做了一轮优化（统一头部 项目/状态/更新日期、README 目录与状态表同步、更新日期刷新）。
- 2026-11-25：docs 按层级重组为子目录 —— `docs/core/`（核心内部组件）、`docs/warp/`（BRU/PC/分支/warp）、`docs/memory/`（访存）、`docs/diagrams/`（Mermaid）；根层保留 `PROJECT_STATUS.md`、`coding_roadmap.md`；全部 md 内交叉引用已同步改写为新路径，README 目录树同步更新。
- 2026-11-25：新增 `docs/open_items.md`（架构待办总表：A 指令编码 / B 流控BRU / C 访存 / D warp系统级 / E 项目级，标注归属文档与建议确认顺序）。