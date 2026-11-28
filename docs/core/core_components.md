# 内核内部组件总览 —— GPU 模拟器
> 线程核心（ThreadCore）内部全部组件的汇总页，细节指向各 spec。

- 模块：线程核心内部（总览）
- 状态：各组件规格已确认；多数已实现（见下表）
- 更新日期：2026-11-26
- 依据：各组件 spec（本页仅汇总）
- 关联：`docs/core/thread_core_spec.md`、`docs/warp/warp_spec.md`

## 1. 总图
```
                 24bit 指令端口（g_rdata[31:8]）
                          │
                          ▼
        ┌──────────────────────────────┐
        │      指令译码控制模块          │  解析 op/func/字段，调度执行与回写
        │   (Decode & Control)         │  内部持有：寄存器组、标志寄存器、掩码寄存器
        └───────┬──────────┬───────────┘
                │读源      │回写(WB MUX)
                ▼          ▼
        ┌───────────┐  ┌────────────┐
        │ 寄存器组   │  │  回写 MUX   │◀── ALU/FPU 结果、LDI imm、访存返回
        │ RegFile   │  └─────┬──────┘
        │ 4读1写    │◀───────┘ waddr/wdata/wen
        └──┬──┬──┬─┘
           │  │  │  (读端口 A/B/C/D)
           ▼  ▼  ▼
        ┌────────┐ ┌────────┐
        │  ALU   │ │  FPU   │  纯组合单周期（op=0100 / op=0101）
        └───┬────┘ └───┬────┘
            │flags10   │fflags6
            ▼          ▼
      整数标志寄存器  浮点标志寄存器（合并为 16bit 标志端口）
      1bit 掩码寄存器 ──▶ 掩码端口（给 BRU；MOV type=0111 写入 mask←reg[0]）
```

## 2. 组件清单
| 组件 | 要点 | 规格 | 源码 |
|---|---|---|---|
| 指令译码控制 | 解析 24bit 指令（op[23:20]+5×4bit）、调度 ALU/FPU/访存、管理回写与标志 | `docs/core/thread_core_spec.md` §3 | `src/thread_core.rs` |
| 寄存器组 RegFile | 16×16bit，4 读 1 写；组合读 + 上升沿写；无旁路；异步复位全 0 | `docs/core/regfile_spec.md` | `src/regfile.rs` |
| 整数 ALU | 仅 op=0100；2 源 + 单独 dst；12 条；MUL 低16bit；移位取 b 低5bit；单周期 | `docs/core/alu_spec.md` | `src/alu.rs` |
| 浮点 FPU | 仅 op=0101；FP16（IEEE half，RNE）；3 源；8 条；HCVT 0111 i2f/1000 f2i | `docs/core/fpu_spec.md` | `src/fpu.rs` |
| 整数标志寄存器 | 10bit（EQ…UGE），仅 ALU func=0001 更新；异步复位全 0 | `docs/core/alu_spec.md` §3.2 | `src/thread_core.rs` |
| 浮点标志寄存器 | 6bit（FEQ…FGE），仅 FPU func=0001 更新；与整数标志合并为 16bit 端口 | `docs/core/fpu_spec.md` §3.3 | `src/thread_core.rs` |
| 掩码寄存器 | 1bit；MOV type=0111 时 `mask ← reg[0]`；经掩码端口输出给 BRU | `docs/core/thread_core_spec.md` §1 | `src/thread_core.rs` |

## 3. 公共约定
- **指令**：24bit，`op=I[23:20]` + `f1..f5`（`I[19:16]`…`I[3:0]`）。
- **时序**：纯组合执行 + `clk` 上升沿统一写回（软件模拟为周期末提交 pending 写）。
- **复位**：异步，所有状态初值 0。
- **位宽**：寄存器/运算结果 16bit；标志端口 16bit（低 10 整数 + 高 6 浮点）。

## 4. 待确认 / 悬置
- 流控裁决在 BRU（`docs/warp/bru_spec.md`）；核心只出分支端口。
- 访存/原子协议（`docs/memory/memory_spec.md`）。
- 项目级：接口形态/运行方式/验收标准（`docs/open_items.md` E1–E3）。

## 5. 变更记录
- 2026-11-25：创建，汇总 7 个内部组件。
- 2026-11-26：统一格式；补源码列与状态。
