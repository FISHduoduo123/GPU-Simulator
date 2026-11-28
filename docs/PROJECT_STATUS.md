# PROJECT_STATUS —— GPU 模拟器（会话交接框架）

> **本文件只提供稳定的"框架 + 指针"。**
> 一切**具体设计**始终以对应文档为准（实时更新），本文件**不写死任何设计结论**，以免过期误导。
> 接手步骤：① 读本文件 ② 按 §2 各文档文件逐一读 ③ 需最新指令集时用 `tools/dump_xlsx.py` 重读 `refs/GPU指令集.xlsx`。

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
| `docs/memory/memory_spec.md` | 内存（4 核一对一接口 + 共享取指通道 + 测试内存模块：64KB / 字节使能 / 大端 / 24bit 指令存放） |
| `docs/memory/global_mem_controller_spec.md` | **全局内存控制器**（GPU 顶层；分支 design/memory-decouple 设计中，端口/仲裁待细说） |
| `docs/warp/bru_spec.md` | BRU（分支端口 + 外部接口 + 内部逻辑：BRA/BRAF 收敛栈/CALL·RET/BRAP·REA） |
| `docs/warp/pc_spec.md` | 程序计数器 PC（jump_req=1→跳转地址，否则+2，溢出置0） |
| `docs/warp/warp_spec.md` | Warp 组成：4 核心 + BRU + PC + 内存接口 构成一个 warp |
| `docs/warp/wcu_spec.md` | **Warp 控制单元 WCU**（输入=4×2bit 核心 status；聚合=全部核心进入结束状态；输出端口待细说） |
| `docs/scheduler_spec.md` | **GPU 全局调度器**（block→4 个 warp 槽位，FIFO；输入=内存 block 列表+命令行 grid；1 block=1 warp=4 核；格式/输出/释放待细说） |
| `docs/coding_roadmap.md` | 编码就绪清单（模块↔源码↔规格、顺序、测试点、待确认项汇总） |
| `docs/open_items.md` | **架构待办总表**（跨文档未完成项 A–E 分类汇总） |
| `docs/指令集表.md` | **指令集表**（24bit 编码权威字段表：位布局 op=I[23:20] + f1..f5、各指令字段语义、取指/PC 对齐；已由用户逐条确认） |
| `docs/diagrams/architecture_modules.mmd` | Mermaid 视图：模块连接架构 |
| `docs/diagrams/architecture_bru_flow.mmd` | Mermaid 视图：BRU 逻辑流程 |
> 每份文档自带「变更记录」，修订沿革以该文档为准。

## 3. 工程文件
- 源码：`src/lib.rs` `src/regfile.rs` `src/alu.rs` `src/fpu.rs` `src/thread_core.rs` `src/bru.rs` `src/memory.rs` `src/system.rs`（核心模块已实现；已按用户要求移除单元测试，本地 `cargo build` 通过；编译执行仍以用户口令为准则）。
- 指令集：`refs/GPU指令集.xlsx`（用户会频繁更新）。
- 重读脚本：`tools/dump_xlsx.py`（**保留**，用于随时重读指令集）。
- 工程：`Cargo.toml`、`README.md`。
- 远程：`git remote` 指向 `ssh://git@ssh.github.com:443/FISHduoduo123/GPU-Simulator.git`（端口 443；`git push` 需 danger-full-access 权限，否则 msys sh 报 signal pipe 错）。

## 4. 交接待办（常见切入点，供新会话参考）
- 各规格文档当前的「**待确认项**」小节是接手必读的最新鲜状态。
- 典型下一步：收敛/多目标规则、BRAF 等 BRU 细分、BRA func=1 用途、访存/栈协议、原子协议、或依用户「写代码」实现各模块。

## 5. 变更记录
- 2026-11-24：创建本文件为**稳定交接框架**；具体设计一律指向 `docs/*.md`（实时更新），不在此固化。
- 2026-11-24：文档指针新增 `docs/coding_roadmap.md`（编码就绪清单）。
- 2026-11-24：新增硬性规则 5（用户说「提交」即提交并推送 GitHub）；§3 更新已实现源码清单（含 bru/memory/system）与远程说明。
- 2026-11-24：文档指针新增 `docs/warp/pc_spec.md`（程序计数器）；`docs/memory/memory_spec.md` 升级为 4 核向量化接口并新增共享取指副读通道。
- 2026-11-24：文档指针新增 `docs/memory/memory_spec.md`（测试阶段内存模块：64KB、16bit 字节地址、32bit 数据、4 读写接口 + 1 取指读接口、字节使能小端）。
- 2026-11-25：文档指针新增 `docs/warp/warp_spec.md`——确认 **一个 warp = 4 线程核心 + 1 BRU + 1 PC + 内存接口**；指令重基线为 24bit（核心指令端口 24bit，取 `g_rdata[31:8]`），详见 `docs/core/thread_core_spec.md` §3、`docs/memory/memory_spec.md` 修订 H、`docs/memory/memory_spec.md` 修订 D/E。
- 2026-11-25：新增 `docs/core/core_components.md`（内核内部组件总览，一页汇总 译码控制/RegFile/ALU/FPU/整数标志/浮点标志/掩码寄存器）；全体 md 做了一轮优化（统一头部 项目/状态/更新日期、README 目录与状态表同步、更新日期刷新）。
- 2026-11-25：docs 按层级重组为子目录 —— `docs/core/`（核心内部组件）、`docs/warp/`（BRU/PC/分支/warp）、`docs/memory/`（访存）、`docs/diagrams/`（Mermaid）；根层保留 `PROJECT_STATUS.md`、`coding_roadmap.md`；全部 md 内交叉引用已同步改写为新路径，README 目录树同步更新。
- 2026-11-25：新增 `docs/open_items.md`（架构待办总表：A 指令编码 / B 流控BRU / C 访存 / D warp系统级 / E 项目级，标注归属文档与建议确认顺序）。
- 2026-11-25：按用户要求把「端口/接口定义 md」与「功能逻辑 md」合并为每组件一个 md —— `docs/warp/bru_spec.md`（原 branch_interface_spec + bru_interface_spec + bru_logic_spec）、`docs/memory/memory_spec.md`（原 mem_interface_spec + memory_module_spec）；旧文件删除，全部交叉引用、README 目录树、src 注释同步更新；§2 指针表合并为各一行。
- 2026-11-26：核心新增 2bit 系统状态寄存器与状态输出端口（HLT=01 / DONE=11，上升沿提交；顶层停机多一拍收尾显示状态、不再取指），代码已实现（src/thread_core.rs / src/system.rs / src/main.rs），详见 `docs/core/thread_core_spec.md` §3.6。
- 2026-11-26：`.hex` 内存镜像加载已实现——统一内存（数据+指令同源取指），指令按大端 2 半字打包（`unit[a]=I[23:8]`、`unit[a+1]=低8位置高字节`），`fetch=unit[a]<<8|unit[a+1]>>8`；CLI 自动识别 .hex（第一行含 `:`），见 `docs/memory/memory_spec.md` §3.5 与 `README.md` 命令行。
- 2026-11-25：BRU 收敛/调用栈语义收尾——收敛点不做比较；收敛栈/调用栈**溢出信号暂留**；**CALL/RET 对使能/收敛状态无影响**（`docs/warp/bru_spec.md` §3.2.1/§3.4/§3.7，`docs/open_items.md` B1/B2 已标已定）。
- 2026-11-25：新增 `docs/compiler_handoff.md`（汇编编译器会话交接：编译器输入/输出契约、权威指令集、对接要点、编译器特异待确认项），用于在**同一工作区新开会话**开发 GPU 汇编编译器。
- 2026-11-25：新增 `docs/指令集表.md`（指令集表：24bit 编码权威字段表、位布局、各指令字段语义、取指/PC 对齐、代码改动清单；已由用户逐条确认）。
- 2026-11-25：**内存改为大端存储**（低地址=高有效字节）；指令布局=低地址单元放高 16 位 `I[23:8]`、高地址单元放低 8 位 `I[7:0]`；取指读端口交换上下 16 位重组后取高 24 位。详见 `docs/memory/memory_spec.md` §3.3/§3.5；open_items A3 标已定。
- 2026-11-25：PC 步长维持 **+2**（用户澄清「pc+1 不用管、不采信该错误信息」，本规格不变；见 `docs/warp/pc_spec.md` §5）。
- 2026-11-25：明确取指地址接线——**PC 地址输出 `[15:0]` 直连内存 17bit 读地址端口 `[15:0]`，最高位 `[16]` 接 0**；见 `docs/warp/pc_spec.md` §1/§3、`docs/memory/memory_spec.md` §2.2/§3.6（修订 J）。
- 2026-11-26：BRU 待确认项全部解决并落盘（同拍 4 核锁步=单一请求不做仲裁；`bru_en` 只控执行；弹栈用 `mask4` 恢复子集使能；CALL/RET 与 r15 独立；RET 下溢忽略 PC+2；BRU 组合输出、核心使能/PC 下周期锁存）——open_items B1–B7 标已定。
- 2026-11-26：确认 `run`/`step` 启动**交互式设置 PC**（hex/dec 自动识别、空=0、越界 0..65535 拒绝退出；demo 恒为 0）。
- 2026-11-26：用户 m02610「确认开始写代码」→ 实现更正：`src/bru.rs`（混合压栈 path 恒 1、路径2 弹栈用 mask4 恢复使能、RET 下溢忽略不跳）+ `src/main.rs`（交互式 PC、`parse_pc`）；`cargo test` 本地 **52/52 通过**（含新测试：ret 下溢、mask4 恢复、PC 解析）。
- 2026-11-26：汇编编译器交接已完成，`docs/compiler_handoff.md` 删除；README 顶部交接提示与目录树、PROJECT_STATUS §2 指针表同步移除（§5 历史记录保留）。
- 2026-11-26：按用户要求去除测试代码，`src/*.rs` 中全部 `#[cfg(test)]` 单元测试模块已删除（`cargo test` 现 0 测试）；保留 `cargo build` 编译与 CLI demo 手动验证；README/各组件 spec 状态行同步更新。
- 2026-11-27：内存字节使能按**常量接入**——核心 `be=01`、取指 `be=11`（`BE_CORE`/`BE_FETCH`）；`src/memory.rs` 增 `read32/write32`（小端数据路径）与 `read32be`（取指大端）；`src/system.rs` 数据/原子/取指均走 32bit 带 be 接口。`cargo build` 通过，demo 与 .hex 运行正常。
- 2026-11-27：修复 `StepOut.pc` 显示——此前每拍显示的是**更新后的下一拍 PC**（首拍显示 start+2），现改为显示**本拍实际取指/执行的 PC**（`pc_this` 在 PC 更新前捕获）。`src/system.rs` step()。
- 2026-11-27：`step` 单步新增 **`m` 指令**——以 `.hex` 镜像格式（`ADDR: w0 w1 …`，每行 8 半字）打印当前内存，不推进周期。`src/memory.rs` 新增 `Memory::dump_hex()`，`src/main.rs` `wait_next_step(&mem)` 支持 s/m/q。`cargo build` 通过并实测（STR 写入后 m 可见）。
- 2026-11-27：精简 README（113→69 行）：合并「介绍/快速上手/命令行」冗余描述，保留命令、CLI 行为、目录、组件状态、文档导航、项目约定、悬置项。
- 2026-11-27：新增 `docs/warp/wcu_spec.md`（Warp 控制单元）—— 输入 4×2bit 核心 status、纯组合、聚合=「全部核心进入结束状态（status≠00）」已确认；输出端口待细说。未写代码。
- 2026-11-27：新增 `docs/scheduler_spec.md`（GPU 全局调度器）—— block→4 个 warp 槽位、FIFO、输入=内存 block 列表+命令行 grid 参数、1 block=1 warp=4 核固定、上层关系，均已确认；block 列表格式/输出端口/槽位释放待细说。未写代码。
- 2026-11-27：warp 新增 4 个外部输入端口 `block_x/block_y/grid_x/grid_y`（**16bit**，4 核共享；原误称 32bit 已更正），MOV type 0011–0110 改从该端口取值（`docs/warp/warp_spec.md` §2、`docs/core/thread_core_spec.md`）；warp_id(type 0010) 暂写死待定。未写代码。
- 2026-11-27：新增 `docs/warp/wcu_spec.md`（Warp 控制单元）—— 输入 4×2bit 核心 status、纯组合、聚合=「全部核心进入结束状态（status≠00）」已确认；输出端口待用户细说。未写代码。
- 2026-11-27：`step` 改为**交互式单步**——每周期提示，输入 `s`（或空行）走下一周期，`q`/EOF 退出，停机自动结束；可选 `[周期数]` 为单步上限。`src/main.rs` `drive`/`wait_next_step`，`cargo build` 通过并实测。
- 2026-11-28：重写 `docs/open_items.md`—— A/B 全部已定、D2/D3 已实现，新增 D5（WCU 输出端口）、D6（调度器 block 列表/输出/槽位释放）、D7（warp 外部端口落码）、D8（warp_id 来源）、E4（汇编器交接）；建议确认顺序按阻塞关系重排。
- 2026-11-28：完善 MOV——type 0001=**warp_thread_id**（2D 编号 x 高8/y 低8，核 k→(k>>1,k&1)）、type 0010=**warp_id**（新增 warp 级输入端口，2D 编号同打包，4 核共享，默认 0）、0011–0110=block/grid 端口；均 16bit 写入 rd=f1。落盘 `docs/指令集表.md` MOV type 子表、`docs/warp/warp_spec.md` §2、`docs/core/thread_core_spec.md` §3.3/§4。未写代码。
- 2026-11-28：用户说「可以写代码了，把新增都实现」→ 已落码：`thread_core::warp_thread_id(k)`（2D 打包）、`GpuSystem` 新增 `warp_id/block_x/block_y/grid_x/grid_y` 字段 + `set_warp_inputs()` 写入 4 核 `ThreadContext`；MOV type 0001/0010/0011–0110 经 `ctx.by_mov_type` 生效。`cargo build` 通过；实测 core0..3 的 warp_thread_id = 0x0000/0x0001/0x0100/0x0101，warp_id/block_x 全核一致；demo 回归正常（9 周期停机）。
- 2026-11-28：README 改为面向用户的自述——删除「项目约定」「仍悬置的项目级定义」「口令」备注与开发向文档链接，仅保留介绍/快速上手/命令行/目录/组件状态/文档（指令集表与 docs 概览）。
- 2026-11-28：新建分支 `design/memory-decouple`——决定把**整个统一内存与 warp 的连接断开**（取指+访存），内存上移 GPU 全局级；新增 `docs/memory/global_mem_controller_spec.md`（全局内存控制器骨架：warp 级内存控制接口 ↔ 全局内存控制器，取指按原方式，调度器不管内存；端口/仲裁待细说）。未写代码。
- 2026-11-28：PUSH/POP 栈机制已定—— 核心新增栈请求输出端口（`stack_op`2bit[00无/01PUSH/10POP] + `stack_addr`17bit=r15 + `stack_wdata`16bit），接内存控制器；控制器内 16bit 计数器（初0、回绕），有效地址=r15+计数器；PUSH 下拍上升沿 counter+1，POP 本拍组合 counter-1、下拍上升沿更新。落盘 `docs/core/thread_core_spec.md` §3.4 与 `docs/memory/global_mem_controller_spec.md` §3.1。未写代码。