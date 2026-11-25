# 浮点计算单元（FPU）规格书 —— 基础线程核心

- 项目：GPU 模拟器
- 模块：基础线程核心 / FPU
- 状态：**规格已确认；实现代码待用户明确说“写代码”后才编写**
- 更新日期：2026-11-23
- 依据：`refs/GPU指令集.xlsx` 中 `op=0101` FP16 计算段（按用户更新后的指令集确认）

---

## 1. FP16 编码（已确认）
- **IEEE 754 binary16（half）**：1 符号位 + 5 指数位（偏置 15）+ 10 尾数位，共 16bit。

## 2. 覆盖范围与 func 编码（已确认）
- 运算为 FP16：操作数 a/b/c 均按 FP16 解释，结果写回目的寄存器（仍为 16bit）。

| func | 指令 | 语义 |
|---|---|---|
| 0000 | HADD | dst = a + b |
| 0001 | HSUB / CMP | dst = a − b，且**隐式比较 a、b，更新浮点标志**。CMP 即 HSUB |
| 0010 | HMUL | dst = a × b |
| 0011 | HRECIP | dst = 1 / a（单目，忽略 b/c；a=0 → ±Inf） |
| 0100 | HFMA | dst = a × b + c（三源） |
| 0101 | HMAX | dst = max(a, b) |
| 0110 | HMIN | dst = min(a, b) |
| 0111 | **HCVT** | **整转浮点（i2f）**：a 按 16bit 有符号整数 → FP16，写 dst |
| 1000 | **HCVT** | **浮点转整（f2i）**：a 按 FP16 → 16bit 有符号整数（舍入 see §4），写 dst |

## 3. 数据通路接口（已确认）
- **输入**：`a[15:0]`、`b[15:0]`、`c[15:0]`（三源，来自寄存器组读端口 A/B/C）；`func[3:0]` 来自指令译码；目的地址 `dst[3:0]` 单独输出。
- **输出**：
  - `result[15:0]` → WB MUX → 寄存器组 `wdata`
  - `dst[3:0]` → 寄存器组 `waddr`
  - 回写使能 `wen`：全部 op=0101 指令均写回（wen=1）
  - `fflags[5:0]` → 浮点标志寄存器（仅 func=0001 时更新）

## 4. 舍入与特殊值（已确认，IEEE 标准语义）
- 舍入方式：**round-to-nearest-even（RNE）**。
- 溢出 → ±Inf；下溢 → subnormal（或 0）；NaN 传播；HRECIP 的 1/0 → ±Inf（随符号）。
- 运算内部以不低于 FP16 的精度计算后按 RNE 舍入回 FP16（软件模拟可用更宽中间量避免二次舍入）。
- HCVT·f2i：FP16 → 16bit 有符号整数，按 RNE 舍入；溢出/NaN 的整数结果按约定处理（饱和到 16bit 有符号范围；NaN 记 0，可后续细化）。

## 5. 浮点标志寄存器（已确认）
- **单独 6bit 浮点标志寄存器**（与整数 10bit 标志寄存器分开），作为线程核心状态部件，**异步复位全 0**。
- 比较基准：`a` 与 `b`（IEEE 语义，NaN 参与比较一律 false，除 FNE）。

| 位bit | 标志 | 含义 |
|---|---|---|
| 0 | FEQ | a == b |
| 1 | FNE | a != b |
| 2 | FLT | a < b |
| 3 | FGT | a > b |
| 4 | FLE | a <= b |
| 5 | FGE | a >= b |

- **更新时机**：仅 func=0001（HSUB/CMP）更新；其余 op=0101 指令不改变浮点标志；HCVT 不更新标志。
- 外部（SEL、BRAF、HLTF 等）按 Cond 编码（1010=FEQ … 1111=FGE）从 6bit 中选择所需标志。

## 6. 延迟模型（已确认）
- **纯组合逻辑、单周期出结果**（含 HRECIP/HFMA），周期末上升沿统一提交写回，与 ALU/寄存器组一致。

## 7. 和线程核心的连接构造
```
指令(op=0101) ──func[3:0]──▶ FPU.func
regfile.rdata_a ──a[15:0]──▶ FPU.a
regfile.rdata_b ──b[15:0]──▶ FPU.b
regfile.rdata_c ──c[15:0]──▶ FPU.c
指令 目的字段 ──dst[3:0]──▶ FPU.dst ──▶ regfile.waddr
FPU.result ──▶ WB MUX ──▶ regfile.wdata
WRITEBACK = op==0101  ──▶ WB MUX ──▶ regfile.wen
FPU.fflags ──▶ 浮点标志寄存器（仅 func=0001 更新）
周期末上升沿：regfile 与浮点标志寄存器同拍提交
```

## 8. 指令集变更备注
- 用户已更新 `refs/GPU指令集.xlsx`：HCVT 由原 func=0111 一处，改为 **func=0111（整转浮点）与 func=1000（浮点转整）** 两处。本规格按更新后版本确认。

## 9. 仍悬置的项目级定义（不猜测）
- 接口形态（目标中“提供对应的接口”）
- 运行方式
- 验收标准

## 10. 变更记录
- 2026-11-23：确认 FP16=IEEE 754 half；三源输入+单独目的地址；IEEE 标准舍入与特殊值（RNE/±Inf/subnormal/NaN 传播/1÷0→±Inf）；HCVT func=0111=整转浮点、func=1000=浮点转整；浮点标志=单独 6bit（FEQ/FNE/FLT/FGT/FLE/FGE）仅 HSUB/CMP 更新、异步复位全0；HRECIP=1/a；FPU 纯组合单周期。规格落盘于本文档，实现代码待“写代码”口令。