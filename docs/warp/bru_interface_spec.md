# BRU（分支单元）接口规格书 —— GPU 模拟器

- 项目：GPU 模拟器
- 模块：外部 **BRU（Branch Unit）**
- 状态：**接口结构已确认；未写实现代码**
- 更新日期：2026-11-23
- 依据：用户规定（4 组核心输入接口 + 16bit 地址输出 + 1bit 跳转请求输出 + 4 个使能输出）
- 关联：`docs/warp/branch_interface_spec.md`（核心→BRU 分支端口）、`docs/core/thread_core_spec.md`

---

## 1. 总体结构
- BRU 负责收集各线程核心的分支请求，结合计算标志判定条件，仲裁后给出 **新 PC（跳转目标地址）**、**跳转请求**（传给 PC），并对各核心给出**使能**。

## 2. 输入端口（4 组，对 4 个线程核心）
每组 `bru_core_i[k]`（k = 0..3）即核心由 `branch_interface_spec.md` 送来的分支端口 + **一组标志端口**：

| 信号 | 位宽 | 说明 |
|---|---|---|
| `br_jump_addr[15:0]` | 16bit | 该核心的跳转目标地址 |
| `br_op[3:0]` | 4bit | 控制流 op：0000 无分支 / 0110 BRA / 0111 BRAF / 1000 BRAP·REA / 1001 CALL·RET |
| `br_cond[3:0]` | 4bit | 条件编码 0..15（整数/浮点判据） |
| `br_uncond_attr[0]` | 1bit | 无条件跳转收敛属性（BRA 的 0/1 位） |
| `br_func[0]` | 1bit | 流控子类型 func：CALL/RET(0/1)、BRAP/REA(0/1)；BRA/BRAF 置 0 |
| `br_flags[15:0]` | 16bit | **该核心的计算标志**：低 10bit 整数（EQ/NE/LT/GT/LE/GE/ULT/UGT/ULE/UGE）+ 高 6bit 浮点（FEQ/FNE/FLT/FGT/FLE/FGE），供 BRU 判定 `br_cond` |
| `br_mask[0]` | 1bit | 该核心的**掩码标志**（核心内 1bit 掩码寄存器的当前值），供 BRU 记录/恢复线程掩码 |

> 条件 `br_cond` 的判定由 BRU 内部结合该组 `br_flags` 完成。

## 3. 输出端口（已确认）
| 端口名 | 位宽 | 说明 |
|---|---|---|
| `bru_jump_addr[15:0]` | 16bit | 仲裁后的跳转目标地址（**新 PC**） |
| `bru_jump_req[0]` | 1bit | 跳转请求有效信号，**传给 PC** |
| `bru_en[3:0]` | 4bit | 4 个使能输出，**分别连接 4 个核心的使能** |

## 4. 端到端连接
```
core_0..3 ──(branch接口: br_jump_addr/br_op/br_cond/br_uncond_attr/br_func + br_flags + br_mask = 7信号/组)──► BRU
                                                                                                  │
          BRU ──bru_jump_addr(16bit)──► 指令/取指(新 PC)
              ├──bru_jump_req(1bit)────► PC
              └──bru_en[3:0]───────────► core_0..3 使能
```

## 5. 行为语义（要点，细节待展开）
- BRU 汇总 4 组请求（`br_op != 0000`），对条件 `br_cond` 依标志自判。
- 仲裁（同拍多请求优先级，见 §6）。
- 仲裁命中时：`bru_jump_req=1`，`bru_jump_addr=选中请求的目标地址`。
- `bru_en[k]`：对 4 个核心使能（是否允许该核心进入跳转、执行目标指令等，语义待用户明确）。

## 6. 待确认项（不猜测）
- **仲裁策略**：同拍多核同时跳转的优先级 / 并发策略。
- `bru_en[k]` 与 `bru_jump_req` 的时序关系（使能是否即对应进入目标指令、是否与跳转同拍）。
- **地址语义**：BRA/BRAF/CALL/RET 的目标地址折算、CALL 压栈与 RET 出栈的栈协议 —— 属流控协议，待用户展开。

## 7. 变更记录
- 2026-11-23：确定 BRU 接口 = 4 组核心分支输入（复用 `branch_interface_spec.md`）+ `bru_jump_addr`(16bit，仲裁后新 PC) + `bru_jump_req`(1bit，传给 PC) + `bru_en`(4bit，连接 4 核心使能)。
- 2026-11-23（修订 A）：**每组输入新增 16bit 标志端口 `br_flags`**（低10整数 + 高6浮点），供 BRU 判定 `br_cond`。
- 2026-11-23（修订 B，随指令集更新）：**`br_op=1001` 由 RET 改为 CALL/RET，`br_op=1000` 为 BRAP·REA**；**每组输入新增 1bit `br_func` 端口**区分这些子类型。实现代码待「写代码」口令。
- 2026-11-24（修订 C）：**每组输入新增 1bit `br_mask` 端口**——该核心**掩码寄存器**的当前值（掩码标志），供 BRU 记录线程掩码。