# 分支端口（Branch Interface）规格书 —— GPU 模拟器

- 项目：GPU 模拟器
- 模块：线程核心 → 外部 **BRU（分支单元）**
- 状态：**端口结构与编码已确认；未写实现代码**
- 更新日期：2026-11-23
- 依据：用户 m00281 规定 + 指令集流控段（**BRA/BRAF/BRAP/REA/CALL/RET**，2026-11-23 更新版）
- 关联：`docs/thread_core_spec.md`、`docs/mem_interface_spec.md`、`docs/bru_interface_spec.md`

---

## 1. 端口清单（已确认）

| 端口名 | 方向 | 位宽 | 说明 |
|---|---|---|---|
| `br_jump_addr` | 输出 | 16bit | 跳转目标地址 |
| `br_op` | 输出 | 4bit | 执行控制流 op |
| `br_cond` | 输出 | 4bit | 分支条件 Cond 编码（0..15） |
| `br_uncond_attr` | 输出 | 1bit | 无条件跳转属性信号（BRA 的 0/1 收敛位） |
| `br_func` | 输出 | 1bit | 流控子类型 func（区分 CALL/RET、BRAP/REA） |
| `br_mask` | 输出 | 1bit | **掩码标志**：核心内 1bit 掩码寄存器的当前值（供 BRU 记录线程掩码） |

## 2. `br_op` 编码（已确认，随指令集更新）
| `br_op` | 含义 |
|---|---|
| `0000` | 无分支（本周期无流控指令，默认值） |
| `0110` | BRA（无条件跳转） |
| `0111` | BRAF（条件跳转） |
| `1000` | BRAP / REA（**由 func 区分**：func=0000→BRAP，func=0001→REA） |
| `1001` | CALL/RET（**由 func 区分**：func=0→CALL，func=1→RET） |

> **无请求有效信号**：BRU 通过 `br_op != 0000` 判断本周期是否存在分支请求，无需额外的 valid 端口。

## 2b. `br_func`（子类型，已确认）
- 1bit，传达流控指令的 `func/属性` 细分（取 H 字段的 0/1 值）：
  - **CALL/RET**（op=1001）：`0` = CALL（无条件跳转并压栈），`1` = RET（出栈并返回上次断点）。
  - **BRAP/REA**（op=1000）：`0` = BRAP，`1` = REA。
- BRA/BRAF 时置 `0`（BRA 的 0/1 收敛位单独走 `br_uncond_attr`）。

## 3. `br_cond`（已确认）
- 4bit 条件编码，**0..15** 全量可用，遵循指令集 Cond 定义：
  - 整数 `0000-1001`：EQ / NE / LT / GT / LE / GE / ULT / UGT / ULE / UGE
  - 浮点 `1010-1111`：FEQ / FNE / FLT / FGT / FLE / FGE
- **核心不自行判断**：核心只把 Cond 编码送出，由外部 BRU 结合核心计算标志（整数 10bit + 浮点 6bit，经 `flags_port`）自行判定是否跳转。

## 4. `br_uncond_attr`（已确认）
- 1bit，即 **BRA 的 0/1 属性位**：
  - `0` = 正常无条件跳转
  - `1` = 带分支收敛属性（branch convergence）的无条件跳转
- 该属性仅对无条件跳转（BRA）有意义；BRAF/BRAP/REA/CALL/RET 置 `0`。

## 5. `br_jump_addr`
- 16bit 跳转目标地址，由核心按流控指令的类型与指令字段计算后输出：
  - BRA / BRAF：目标地址来自指令字段（寄存器 / 立即数的具体折算在流控协议展开时定义）。
  - BRAP：内核入口（绝对）地址。
  - REA：返回地址（属性/来源待流控协议定）。
  - CALL：调用地址（caller 现场压栈）。
  - RET：出栈并返回上次断点地址。
- 精确的目标地址计算方案属于**执行流控**待展开项（见 §6）。

## 6. 悬置项（不猜测）
- 指令 `field` 到跳转地址的折算规则（BRA/BRAF/CALL/RET 的目标来源、是否相对/绝对、BP 偏移）—— 属流控协议，待用户展开。
- **CALL/RET 与栈**：CALL 压栈（保存返回点）、RET 出栈返回，涉及栈（r15）与访存交互 —— 访存/栈协议待展开。
- BRU 内部如何组合标志判 Cond（外部单元行为，暂不属本工程核心）。
- `br_op=0000` 时其余端口值的约束（建议去定义 / 置 0，具体由实现定）。

## 7. 变更记录
- 2026-11-23：确定分支端口 = `br_jump_addr`(16bit) + `br_op`(4bit) + `br_cond`(4bit) + `br_uncond_attr`(1bit)；无分支请求有效信号；`br_op` 编码 0000/0110/0111/1000/1001；`br_cond` 4bit 由 BRU 自判；`br_uncond_attr` 即 BRA 的 0/1 收敛位。实现代码待「写代码」口令。
- 2026-11-23（修订，随指令集更新）：**`1001` 由 RET 改为 CALL/RET（func 指定：0=CALL 无条件跳转并压栈、1=RET 出栈并返回上次断点）**；补充 op=1000 由 func 区分 BRAP(0000)/REA(0001)。
- 2026-11-23（修订 2）：**新增 1bit `br_func` 端口**，传达 CALL/RET 与 BRAP/REA 的 func 子类型（0/1）；BRA/BRAF 置 0。
- 2026-11-24（修订 3）：**新增 1bit `br_mask` 端口**——核心 **1bit 掩码寄存器**的当前值（掩码标志），输出给 BRU。

## 8. 参考：与内存接口 `branch_out` 的关系
- 早期 `mem_interface_spec.md` 中预留的 `branch_out`（给 BRU）字段，现以**本规格书的分支端口**为准；`mem_interface_spec.md` 中该行将标注引用本文件（见该文件修订记录）。