# FP16 浮点单元（FPU）—— GPU 模拟器
> `op=0101` 的 IEEE 754 binary16 运算单元，3 源输入 + 单独目的地址，纯组合单周期。

- 模块：线程核心内部 / 浮点 FPU
- 状态：已实现（`src/fpu.rs`）
- 更新日期：2026-11-26
- 依据：`refs/GPU指令集.xlsx` 中 `op=0101` FP16 段
- 关联：`docs/core/regfile_spec.md`、`docs/core/thread_core_spec.md`

## 1. 概述
- 数据格式：**IEEE 754 binary16（half）** = 1 符号 + 5 指数（偏置 15）+ 10 尾数，共 16bit。
- 操作数 a/b/c 均按 FP16 解释，结果写回目的寄存器（16bit）。
- 仅处理 **`op=0101`**；纯组合、单周期。

## 2. 端口与接口
| 端口 | 方向 | 位宽 | 说明 |
|---|---|---|---|
| `a` / `b` / `c` | 输入 | 16bit ×3 | 三个源操作数（读端口 A/B/C） |
| `func` | 输入 | 4bit | 运算选择（来自译码） |
| `dst` | 输入 | 4bit | 目的寄存器地址 |
| `result` | 输出 | 16bit | 结果 → 回写 MUX → `wdata` |
| `wen` | 输出 | 1bit | 回写使能（op=0101 全部为 1） |
| `fflags` | 输出 | 6bit | 浮点比较标志（仅 func=0001 更新） |

## 3. 功能与行为
### 3.1 func 编码
| func | 指令 | 语义 |
|---|---|---|
| 0000 | HADD | `a + b` |
| 0001 | HSUB / CMP | `a − b`，并隐式比较 a、b，更新浮点标志；CMP 即 HSUB |
| 0010 | HMUL | `a × b` |
| 0011 | HRECIP | `1 / a`（单目；`a=0` → ±Inf） |
| 0100 | HFMA | `a × b + c`（三源） |
| 0101 | HMAX | `max(a, b)` |
| 0110 | HMIN | `min(a, b)` |
| 0111 | HCVT | **整转浮点（i2f）**：a 按 16bit 有符号整数 → FP16 |
| 1000 | HCVT | **浮点转整（f2i）**：a 按 FP16 → 16bit 有符号整数（按 RNE 舍入） |

### 3.2 舍入与特殊值（IEEE 标准）
- 舍入：**round-to-nearest-even（RNE）**。
- 溢出 → ±Inf；下溢 → subnormal 或 0；NaN 传播；`1/0` → ±Inf（随符号）。
- 内部以不低于 FP16 的精度计算后按 RNE 舍回；软件模拟可用更宽中间量避免二次舍入。
- f2i：按 RNE 舍入；溢出饱和到 16bit 有符号范围；NaN 暂记 0。

### 3.3 浮点标志（6bit，比较基准 a、b）
| bit | 标志 | 含义 |
|---|---|---|
| 0 | FEQ | `a == b` |
| 1 | FNE | `a != b` |
| 2 | FLT | `a < b` |
| 3 | FGT | `a > b` |
| 4 | FLE | `a <= b` |
| 5 | FGE | `a >= b` |

- 单独 6bit 浮点标志寄存器（与整数 10bit 分开），异步复位全 0。
- **仅 `func=0001`（HSUB/CMP）更新**；HCVT 不更新。
- NaN 参与比较：除 FNE 外一律 false。

## 4. 待确认 / 悬置
- 对外接口形态、运行方式、验收标准（项目级，见 `docs/open_items.md` E1–E3）。

## 5. 变更记录
- 2026-11-23：确认 FP16=IEEE half；三源 + 单独 dst；RNE/±Inf/subnormal/NaN/1÷0；HCVT func=0111 整转浮点、func=1000 浮点转整；6bit 浮点标志仅 HSUB/CMP 更新、异步复位全0；HRECIP=1/a；纯组合单周期。
- 2026-11-26：统一文档格式，状态更新为“已实现”。
