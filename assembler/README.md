# GPU 汇编编译器（assembler）

`D:\Codes\GPU` 主工程下的**独立 Rust 子项目（纯 std，无第三方依赖）**：把 GPU 汇编源文件（`.asm`）编译为 24bit 指令流 + 整段半字内存镜像，输出简易文本十六进制文件（`.hex`），并在编译成功时向 stdout 打印三段 ASCII 报告。

| 项 | 说明 |
|---|---|
| 交付形态 | 命令行：`assembler <输入.asm> <输出.hex>`（即 `cargo run -- <in.asm> <out.hex>`） |
| 源码 | 单文件 `src/main.rs`（~2950 行，含内联单元测试） |
| 状态 | 26 个单元测试全绿，clippy / fmt 干净 |

---

## 1. 构建与运行

```bash
cd assembler
cargo build --release
cargo test
cargo run -- examples/asm.asm target/asm.hex          # 最简示例
cargo run -- examples/macro_test.asm target/m.hex     # 宏系统示例
```

- 错误与警告走 **stderr**（带 `文件:行号` 定位）；成功退出码 0。
- 编译**成功**时向 **stdout** 打印 `=== 内存结构 ===` / `=== 宏列表 ===` / `=== 标签列表 ===` 三张 ASCII 表格（§6）。
- `#include "..."` 相对**输入 .asm 所在目录**解析；找不到只告警、不报错。

---

## 2. 语法规则（完整）

### 2.1 词法

- **行注释**：`//` 到行尾（预处理阶段剥除，但保留空行占位以维持行号）。
- **空白**：空格 / 制表符分隔 token；多余空白折叠。
- **预处理指令**：`#include "文件名"`，递归展开，带环检测；`.inc` 内容**自动视为 `.DEFINE` 定义**并入。
- **数字面量**：十进制、`0x` 十六进制、`0b` 二进制；可带前导 `-`。按目标位宽校验（LDI 立即数 16bit；H 字段 4bit；访存偏移 4bit；分支地址 16bit）。
- **表达式**：`+ - * /`、括号、一元负号、上述数字前缀（用于 `.half`/`.space`/立即数/分支地址）。
- **标识符**：字母 / 数字 / 下划线，首字符须字母或下划线，无长度上限。

### 2.2 顶层结构

```
#include "xxx.inc"        # 可选，内容并入 .DEFINE

.DEFINE                   # 可选段（只是分组标记）
.define _N_ 16            # 常量定义（见 2.4）
.macro _f_(x)             # 宏定义（见 2.5）
    PUSH \(x)

.DATA                     # 可选全局数据段
mat_A : .half 1, 2, 3, 4
mat_C : .space 4*2

.kernel mat_add :         # 一个或多个平铺 kernel 块
    LOOP:
    ... 指令 ...
    BRA LOOP 0
    DONE
```

- 段声明关键字：`.DATA` / `.DEFINE` / `.kernel`（`.data` 小写也认）；`.define` / `.macro` / `.half` / `.space` 前缀匹配。
- 没有 `.TEXT` 段；顶层 = 可选 `.DEFINE` + 可选 `.DATA` + 多个 `.kernel`。

### 2.3 数据段与标签（`.DATA` 及 kernel 内局部静态区）

- `名字 : .half v1, v2, …` —— 每值占 1 个半字，连续排布。值可用**逗号或空格**分隔，可为表达式。
- `名字 : .space N` —— 保留 N 个半字，初始化为 0（N 为表达式）。
- 也可裸写 `.half …` / `.space N`（无标签）。
- `.half` 值范围校验：`-0x8000 ..= 0xFFFF`。
- **标签**：`名字:` 或 `名字 :`（冒号前后空格随意）。数据区标签 → 该数据起始半字地址；指令标签 → 该标签下第一条指令的地址。
- `.kernel` 内出现的 `.half`/`.space` 落入该 kernel 的**局部静态区**（见 §3 布局）；其标签为该 kernel 的**局部静态符号**，仅在同一 kernel 内（含宏展开出的内容）可见、可引用，不与全局符号表冲突。

### 2.4 `.define` 常量

```
.define 名字 表达式
```

- 名字须为合法标识符（否则报错带行号，如 `.define -ULT- 6`）。
- VALUE 是**表达式文本**，按完整 token 在指令操作数 / 表达式 / 宏实参处替换后统一求值；**允许向后引用**（常量表先收集，宏展开后再求值）。
- 用 `.define` 定义的条件码 / 类型常量（如 `_EQ_ 0` … `_FGE_ 15`）可直接写在 H 字段位置，但 H 字段本质仍是整数。

### 2.5 `.macro` 宏

```
.macro 名字(p1, p2, ...)
    宏体行…
    下一行仍是宏体（注释/空行不算结束）
.下一个 .define / .macro / 段声明才结束宏体
```

- **宏调用**：`名字(实参1, 实参2)` 或无参宏裸写 `名字` / `名字 ()`。宏名**区分大小写**，且**不忽略首尾下划线**（`THREAD_start` ≠ `_THREAD_start_`）。
- 实参**逗号分隔**；`\(形参)` 做**原始文本替换**（可进表达式）。实参个数须与形参严格一致，否则报错带行号。
- 支持**嵌套调用**，深度上限 64，带环检测（`A→B→A` 报「宏循环调用」+ 行号）。
- **宏内标签**：指令标签按实例唯一化——每个宏维护独立计数器，外到内展开时每次实例化 +1，追加 `__<n>` 后缀（如第 2 次调用 `_m_` 的标签为 `_m_L___1`）并同步改写宏内对它的引用；**落在 `.half`/`.space` 上的静态分配标签不唯一化、保留原名**，作为该 kernel 的局部静态符号，在同一 kernel 内（含展开进来的宏）可直接引用。
- **未命中任何指令/宏的大写标识符行**（`.kernel` 内）→ 跳过并向 stderr 告警（不报错）。
- **未定义的标识符/常量/参数**（如 `\(rs)` 而形参无 rs）→ 报错并带 `文件:行号`。

### 2.6 指令书写规则

- **助记符大写**（`ADD`、`DONE`…）。寄存器 `r0–r15` **大小写皆可**。
- **操作数空格分隔、不用逗号**：`ADD r1 r2 r3`。
- **H 字段**（type / cond / func）用**整数常量**写，不写 `EQ` 等符号名。
- 仅「H 可变」的指令在**行尾显式写 H 整数**（如 `MOV r1 r2 2`、`BRA LOOP 0`、`HLTF 3`）；H 固定/未用的指令不写，编译器自动填 0。

### 2.7 指令操作数形式表

| 指令 | 操作数 | 说明 |
|---|---|---|
| `NOP` `SYNC` `HLT` `DONE` `RET` | 0 | RET 编码 f5=1 |
| `MOV` | 2 或 3 | `MOV d s type`；或 `MOV d s`（s 为寄存器→寄存器⇔寄存器，type=0）；或 `MOV d type`（s 非寄存器→读特殊寄存器，src=0） |
| `LDI d imm16` | 2 | 立即数 16bit，占 f2..f5 |
| `SEL d a b cond` | 4 | cond 为 4bit 整数 |
| `ADD SUB CMP MUL XOR XNOR AND NAND OR NOR NOT` | 3 | `d a b`，三者皆寄存器，func=H |
| `SHL SHR` | 3 | `d src 移位量寄存器`（移位量来自寄存器） |
| `HADD HMUL HRECIP HMAX HMIN` `HSUB` | 3 | `d a b` |
| `HFMA d a b c` | 4 | f4=c |
| `HCVT d s` | 2 | |
| `BRA target f5属性` | 2 | target=标签或16bit地址 |
| `BRAF target cond` | 2 | |
| `BRAP target` `REA target` `CALL target` | 1 | |
| `STR data base offset` `LDR d base offset` | 3 | offset 为 4bit 整数或寄存器 |
| `PUSH s` `POP d` | 1 | 栈基址 r15 |
| `ATADD ATSWAP` | 4 | `d value base offset` |
| `ATCAS d cmp base offset` | 4 | |
| `HLTF cond` `DONEF cond` | 1 | cond 4bit 整数 |

- **分支 / CALL 目标**：标签名（查符号表）或 16bit 数值表达式，编码进 f1..f4（`addr<<4`），f5 留属性/cond。
- 完整 40 条助记符见 `KNOWN_MNEMONICS`（`src/main.rs` 内）。

---

## 3. 内存布局（半字寻址）

**一个地址 = 一个 16bit 半字。** 布局顺序：

```
地址 0 ── 全局数据区 (.DATA) ──┐ 每 .half 值 +1，.space N 占 N 个半字
        ── kernel0 局部静态区 ─┼ 宏展开出的 .half/.space 落在此
        ── kernel0 指令区 ─────┤ 每条 24bit 指令占 2 个半字地址（PC 步长 +2）
        ── kernel1 局部静态区 ─┤
        ── kernel1 指令区 ─────┘
```

- 指令 24bit `I` 跨两半字存放（大端）：低地址半字 = `I[23:8]`，高地址半字 = `I[7:0]`。
- 数据与指令同处一段连续半字内存，同字节序（大端）。

## 4. 指令编码（24bit）

`I = op<<20 | f1<<16 | f2<<12 | f3<<8 | f4<<4 | f5`，`op=I[23:20]`，`f_n=I[20-4n ..]`。

| op | 含义 |
|---|---|
| 0x0 | NOP |
| 0x1 | MOV |
| 0x2 | LDI（imm16 占 f2..f5） |
| 0x3 | SEL |
| 0x4 | 整数 ALU（H=func） |
| 0x5 | FP16（H=func） |
| 0x6 / 0x7 | BRA / BRAF |
| 0x8 | BRAP(f5=0) / REA(f5=1) |
| 0x9 | CALL(f5=0) / RET(f5=1) |
| 0xA | 访存/原子（H=子操作） |
| 0xB..0xF | SYNC / HLT / HLTF / DONE / DONEF |

整数 ALU func：ADD=0, SUB/CMP=1, MUL=2, XOR=4, XNOR=5, AND=6, NAND=7, OR=8, NOR=9, NOT=A, SHL=B, SHR=C。
FP16 func：HADD=0, HSUB/CMP=1, HMUL=2, HRECIP=3, HFMA=4, HMAX=5, HMIN=6, HCVT=7。
访存 H：STR=0, LDR=1, PUSH=2, POP=3, ATADD=4, ATSWAP=5, ATCAS=6。
条件码整数：EQ=0 … UGE=9，FEQ=10 … FGE=15（通常用 `.define` 常量写）。

## 5. 输出 `.hex` 格式

简易文本，**每行 8 个半字**：

```
0000: 0001 0002 0003 0004 0001 0002 0003 0004
0008: 0000 0000 0000 0000 0000 0000 0000 0000
```

- 地址列为**半字地址**（十六进制 4 位），每行 8 项，下一行地址 **+8**。
- 每项为该半字的 16bit 值（4 位十六进制，大端直读）。
- 内容为整段连续内存：数据区（地址 0 起）→ 各 kernel 局部静态区 → 各 kernel 指令区。

## 6. stdout 三段报告（仅编译成功时）

ASCII 表格，列宽按终端显示宽度计算（CJK 计 2 列），边框严格对齐：

- **① 内存结构**：各区域 名 / 起始 / 结束 / 大小(半字) + 总计。
- **② 宏列表**：宏定义（名 / 参数 / `文件:行号`）+ 宏调用展开点（调用位置 → 展开自 / 行流段，含嵌套）。
- **③ 标签列表**：`标签 → 半字地址(十六进制)`，按地址升序。

## 7. 目录结构

```
assembler/
├── Cargo.toml / Cargo.lock
├── .gitignore
├── README.md
├── src/main.rs        # 全部源码（单文件 + 内联测试）
└── examples/          # asm.asm / macro_test.asm / gpu_core.inc
```

## 8. 备注

- 指令与数据统一按用户确认的**大端**处理；`docs/memory/memory_spec.md` 中「指令 +1 单元内字节序暂记小端」与本实现不一致，读侧需对齐。
- 真实 `gpu_core.inc` 内存在笔误时，编译器会**精确报错 + 行号**（如 `_get_warp_thread_id_` 拼写、小写 `mov`、`.define -ULT-`、未声明的 `\(rs)`），请在自己的 `.inc` 中修正。
