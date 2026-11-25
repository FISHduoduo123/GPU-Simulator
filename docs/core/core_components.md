# 内核（线程核心）内部组件总览 —— GPU 模拟器

> 状态：进行中（各组件规格已确认，详见对应 spec；实现代码待用户说「写代码」）
> 日期：2026-11-25
> 范围：**线程核心（Thread Core）内部**的全部组件。对外接口见 `docs/core/thread_core_spec.md`；warp 级组成见 `docs/warp/warp_spec.md`。

## 0. 总图
```
                 24bit 指令端口（来自 g_rdata[31:8]）
                          │
                          ▼
        ┌──────────────────────────────┐
        │      指令译码控制模块          │  解析 op/func/字段，调度执行与回写
        │   (Decode & Control)         │  内部持有：寄存器组、标志寄存器、掩码寄存器
        └───────┬──────────┬───────────┘
                │读源      │回写(WB MUX)
                ▼          ▼
        ┌───────────┐  ┌────────────┐
        │ 寄存器组   │  │  回写 MUX   │◀── ALU.result / FPU.result / LDI imm / 访存返回
        │ RegFile   │  └─────┬──────┘
        │ 4读1写    │◀───────┘ waddr/wdata/wen
        └──┬──┬──┬─┘
           │  │  │  (读端口 A/B/C/D)
           ▼  ▼  ▼
        ┌────────┐ ┌────────┐
        │  ALU   │ │  FPU   │  纯组合单周期（op=0100 / op=0101）
        │ op=0100│ │ op=0101│
        └───┬────┘ └───┬────┘
            │flags10   │fflags6
            ▼          ▼
      整数标志寄存器  浮点标志寄存器（合并为 16bit 标志端口输出）
      1bit 掩码寄存器 ──▶ 掩码端口（给 BRU，MOV type=0111 写入 mask←reg[0]）
```

## 1. 组件清单

| 组件 | 要点 | 详细规格 | 源码 |
|---|---|---|---|
| **指令译码控制模块** | 解析 24bit 指令（`op[23:20]`+5×4bit 字段）、调度 ALU/FPU/访存、管理回写与标志 | `docs/core/thread_core_spec.md` §3/§4 | `src/thread_core.rs` |
| **寄存器组 RegFile** | 16×16bit（r0–r15），4 读 1 写；组合读 + 上升沿同步写；沿前读旧值/沿后读新值、无写旁路；异步复位全 0 | `docs/core/regfile_spec.md` | `src/regfile.rs` |
| **整数 ALU** | 仅 op=0100；2 源 a,b + 单独 dst；12 条（ADD/SUB·CMP/MUL/…/SHL/SHR）；MUL 取低 16bit；移位取 b 低 5bit；纯组合单周期 | `docs/core/alu_spec.md` | `src/alu.rs` |
| **浮点 FPU** | 仅 op=0101；FP16（IEEE half，RNE）；3 源 a,b,c + 单独 dst；HADD/HSUB·CMP/HMUL/HRECIP/HFMA/HMAX/HMIN/HCVT(0111 i2f, 1000 f2i)；纯组合单周期 | `docs/core/fpu_spec.md` | `src/fpu.rs` |
| **整数标志寄存器** | 10bit（EQ/NE/LT/GT/LE/GE/ULT/UGT/ULE/UGE），仅 ALU func=0001（SUB/CMP）更新；异步复位全 0 | `docs/core/alu_spec.md` §3 | `src/thread_core.rs` |
| **浮点标志寄存器** | 6bit（FEQ/FNE/FLT/FGT/FLE/FGE），仅 FPU func=0001（HSUB/CMP）更新；异步复位全 0；与整数标志合并为 16bit 标志端口 | `docs/core/fpu_spec.md` §5 | `src/thread_core.rs` |
| **掩码寄存器** | 1bit，核心内部状态；`MOV type=0111` 时 `mask ← reg[0]`；经掩码端口输出给 BRU | `docs/core/thread_core_spec.md` §1 | `src/thread_core.rs` |

## 2. 公共约定（内核内部一致）
- **指令**：24bit，高位对齐 `op=I[23:20]` + `f1..f5`（`I[19:16]`…`I[3:0]`），对应 xlsx 列 C..H。
- **时序**：纯组合执行 + `clk` 上升沿统一写回；软件模拟用「周期末 tick/cycle 提交 pending 写」等价。
- **复位**：异步复位，所有状态初值 0。
- **数据位宽**：寄存器/运算结果 16bit；标志端口 16bit（低 10 整数 + 高 6 浮点）。

## 3. 待确认（汇总自各 spec，不猜测）
- 项目级「接口形态 / 运行方式 / 验收标准」仍悬置。
- 流控（BRA/BRAF/BRAP/REA/CALL/RET）在核心侧仅出分支端口，由 BRU 裁决（`docs/warp/bru_logic_spec.md`）。
- 访存/原子协议（`docs/memory/mem_interface_spec.md`、`docs/memory/memory_module_spec.md`）。

## 4. 变更记录
- 2026-11-25：创建。汇总内核内部 7 个组件（译码控制、RegFile、ALU、FPU、整数标志、浮点标志、掩码寄存器），指向各详细 spec 与源码。
