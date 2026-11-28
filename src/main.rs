//! GPU 模拟器命令行入口：把一段程序加载进 4 核 warp 逐周期运行，
//! 每周期打印 warp 级状态与每个核心的寄存器组。
//!
//! 用法：
//!   gpu demo                 —— 运行内置示例程序
//!   gpu run <程序文件> [最大周期]
//!   gpu step <程序文件> [周期数] —— 逐周期推进，每周期打印一次
//!
//! 程序文件支持两类：
//!   - **.hex 内存镜像**：`ADDR: w0 w1 …`（半字地址；指令按大端 2 半字打包）
//!   - 汇编/24bit 十六进制程序：每行一条指令，`#` 后为注释，如 `0xA210` 或助记符（见 [`asm`]）

use std::io::{self, Write};
use std::process::ExitCode;

use gpu_core::op;
use gpu_core::{Memory, enc, enc_ldi, GpuSystem};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        None | Some("-h") | Some("--help") => { print_help(); ExitCode::SUCCESS }
        Some("demo") => run_asm(&demo_program(), None, 256, 0),
        Some("run") => run_file(&args[1..], false),
        Some("step") => run_file(&args[1..], true),
        Some(other) => {
            eprintln!("未知命令：{other}（试试 `gpu --help`）");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "GPU 模拟器命令行\n\n\
         用法：\n  gpu demo\n  gpu run <程序文件> [最大周期]\n  gpu step <程序文件> [周期数]\n\n\
         run/step 启动时会交互式询问 PC（hex/dec、空=0、范围 0..65535）。程序文件可以是\n\
         .hex 内存镜像（`ADDR: w0 w1 …`，半字地址），或逐行指令（# 注释、24bit hex 或助记符）。\n\
         step 模式下每走一周期会提示：s/空行=下一周期，m=以 .hex 镜像格式打印当前内存，q/EOF=退出；\n\
         停机（HLT/DONE）后自动结束。[周期数] 为单步上限（缺省不限）。\n\n\
         助记符：LDI rX imm | MOV rX rY | MOV.M rX | ADD/SUB/MUL/XOR/AND/OR/NOT/SHL/SHR rD rA [rB]\n\
         HADD/HSUB/HMUL/HRECIP/HMAX/HMIN rD rA [rB] | STR rData rBase | LDR rDst rBase\n\
         PUSH rData | POP rDst | BRA addr | BRAF addr cond | BRAP addr | REA | CALL addr | RET\n\
         HLT | HLTF cond | DONE | DONEF cond | NOP | SYNC"
    );
}

fn run_file(args: &[String], interactive: bool) -> ExitCode {
    let path = args.first();
    let Some(path) = path else {
        eprintln!("缺少程序文件（用法见 gpu --help）");
        return ExitCode::FAILURE;
    };
    let body = match std::fs::read_to_string(path) {
        Ok(b) => b,
        Err(e) => { eprintln!("读取 {} 失败：{e}", path); return ExitCode::FAILURE; }
    };
    let arg1 = args.get(1).and_then(|s| s.parse::<u64>().ok());
    // step 模式：交互式单步（敲 s 走下一周期），[周期数] 为可选上限（缺省不限）；run 模式：上限=最大周期
    let step_each = if interactive { Some(arg1.unwrap_or(u64::MAX)) } else { None };
    let maxcycles = if interactive { u64::MAX } else { arg1.unwrap_or(1_000_000) };
    let pc = match prompt_pc() {
        Ok(p) => p,
        Err(e) => { eprintln!("{e}"); return ExitCode::FAILURE; }
    };
    // 第一行含 `:` 判定为 .hex 内存镜像，否则按汇编/十六进制程序
    let first = body.trim().lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.contains(':') {
        match parse_hex_image(&body) {
            Ok(pairs) => run_hex(&pairs, step_each, maxcycles, pc),
            Err(e) => { eprintln!("解析 .hex 失败：{e}"); ExitCode::FAILURE }
        }
    } else {
        match assemble(&body) {
            Ok(prog) => run_asm(&prog, step_each, maxcycles, pc),
            Err(l) => { eprintln!("第 {l} 行无法解析"); ExitCode::FAILURE }
        }
    }
}

/// 解析 `.hex` 内存镜像：每行 `ADDR: w0 w1 …`（地址与字均为十六进制半字）。
fn parse_hex_image(src: &str) -> Result<Vec<(u16, u16)>, String> {
    let mut out: Vec<(u16, u16)> = Vec::new();
    for (n, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (add, rest) = line.split_once(':').ok_or_else(|| format!("第 {} 行缺少冒号", n + 1))?;
        let base = u16::from_str_radix(add.trim(), 16)
            .map_err(|_| format!("第 {} 行地址不是十六进制", n + 1))?;
        for (i, w) in rest.split_whitespace().enumerate() {
            let w = u16::from_str_radix(w, 16)
                .map_err(|_| format!("第 {} 行第 {} 个字不是十六进制", n + 1, i + 1))?;
            out.push((base.wrapping_add(i as u16), w));
        }
    }
    if out.is_empty() {
        Err("空 .hex 镜像".to_string())
    } else {
        Ok(out)
    }
}

/// 交互式读取启动 PC：hex(`0x…`)/dec，空=0，越界 0..65535 报错。
fn prompt_pc() -> Result<u16, String> {
    print!("Enter startup PC (hex 0x…/dec, default 0): ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    let t = line.trim();
    parse_pc(t)
}

/// 解析 PC 输入：空=0；`0x…` 十六进制，否则十进制；超 0..65535 拒绝。
fn parse_pc(t: &str) -> Result<u16, String> {
    if t.is_empty() {
        return Ok(0);
    }
    let v = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u32::from_str_radix(h, 16)
    } else {
        t.parse::<u32>()
    }
    .map_err(|_| format!("无效的 PC 值：{t}"))?;
    if v > 0xFFFF {
        return Err(format!("PC 超出范围 0..65535：{t}"));
    }
    Ok(v as u16)
}

/// 运行汇编/24bit 指令程序（装入统一内存地址 0,2,4,…）。
fn run_asm(prog: &[u32], step_each: Option<u64>, maxcycles: u64, start_pc: u16) -> ExitCode {
    let mem_words = (prog.len() * 2 + 16).max(64);
    let mut s = GpuSystem::new(prog.to_vec(), mem_words);
    println!(
        "== 汇编程序 {} 条指令，内存 {} 字，起始 PC=0x{:04X} ==",
        prog.len(),
        mem_words,
        start_pc
    );
    drive(&mut s, start_pc, step_each, maxcycles, step_each.is_some())
}

/// 运行 `.hex` 内存镜像（统一内存：数据区 + 指令区同源取指）。
fn run_hex(pairs: &[(u16, u16)], step_each: Option<u64>, maxcycles: u64, start_pc: u16) -> ExitCode {
    let max_addr = pairs.iter().map(|&(a, _)| a as usize).max().unwrap_or(0);
    let mut s = GpuSystem::with_memory(Memory::new(max_addr + 2));
    s.load_hex(pairs);
    println!(
        "== .hex 镜像 {} 个字（地址 0x0000–0x{:04X}），起始 PC=0x{:04X} ==",
        pairs.len(),
        max_addr,
        start_pc
    );
    drive(&mut s, start_pc, step_each, maxcycles, step_each.is_some())
}

/// 逐周期推进并打印，直到停机/完成、达到单步上限或最大周期。
///
/// `interactive=true`（step 模式）时每走一周期就**等待用户输入**：输入 `s` 走下一周期；
/// 空行/回车也当作 `s`；输入 `q` 退出；其余输入视为无效并提示。停机判定优先于等待。
fn drive(
    s: &mut GpuSystem,
    start_pc: u16,
    step_each: Option<u64>,
    maxcycles: u64,
    interactive: bool,
) -> ExitCode {
    s.pc = start_pc;
    loop {
        let so = s.step();
        let c = so.cycle;
        print_state(s, &so);
        if (s.halted || s.done) && !s.flush {
            println!("[结束] halted={} done={}（共 {} 周期）", s.halted, s.done, c);
            break;
        }
        if let Some(e) = step_each {
            if c >= e {
                println!("[step 达到 {} 周期，停止]", e);
                break;
            }
            if interactive && !wait_next_step(&s.mem) {
                println!("[用户退出] 已运行 {} 周期", c);
                break;
            }
        }
        if c >= maxcycles {
            println!("[达到最大周期 {}，停止]", maxcycles);
            break;
        }
    }
    ExitCode::SUCCESS
}

/// 交互单步：`s`/空行=走下一周期；`m`=以 .hex 镜像格式打印当前内存（不推进周期）；`q`/EOF=退出。
/// 返回 `true` 表示继续下一周期。
fn wait_next_step(mem: &Memory) -> bool {
    loop {
        print!(">>> s=下一周期，m=打印内存(hex)，q=退出: ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => return false, // EOF
            Ok(_) => {}
            Err(_) => return false,
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "" | "s" => return true,
            "q" => return false,
            "m" => {
                println!("== 内存（hex 镜像格式，共 {} 字）==", mem.len());
                print!("{}", mem.dump_hex());
            }
            _ => println!("无效输入：请输入 s / m / q"),
        }
    }
}

/// BRU 三状态编号的可读名称（0 普通 / 1 收敛路径1 / 2 收敛路径2）。
fn bru_state_name(state: u8) -> &'static str {
    match state {
        0 => "普通",
        1 => "收敛·路径1",
        2 => "收敛·路径2",
        _ => "未知",
    }
}

/// 核心系统状态寄存器（2bit）的可读名称：00 正常 / 01 HLT / 11 DONE。
fn status_name(status: u8) -> &'static str {
    match status & 0x3 {
        0b00 => "RUN",
        0b01 => "HLT",
        0b10 => "RSV",
        0b11 => "DONE",
        _ => "?",
    }
}

/// 打印一个周期的 warp 级状态 + 4 核寄存器（ASCII 表格，避免输出过密）。
fn print_state(s: &GpuSystem, so: &gpu_core::StepOut) {
    // 1) 全局状态
    let en: String = (0..4)
        .map(|k| if so.en[k] { "ON" } else { "--" })
        .collect::<Vec<_>>()
        .join(" ");
    let g_header: Vec<String> =
        ["cycle", "pc", "instr", "en", "jump", "halted", "done"]
            .iter().map(|s| s.to_string()).collect();
    let g_rows: Vec<Vec<String>> = vec![vec![
        format!("{}", so.cycle),
        format!("0x{:04X}", so.pc),
        format!("0x{:06X}", so.instr & 0xFF_FFFF),
        en,
        format!("req={} @0x{:04X}", so.jump_req as u8, so.jump_addr),
        format!("{}", so.halted),
        format!("{}", so.done),
    ]];
    let g_right: Vec<bool> = vec![true, true, true, false, false, true, true];

    // 2) BRU 状态
    let b = &s.bru;
    let conv: Vec<String> = b
        .conv_stack
        .iter()
        .map(|e| format!("p{} m{:X} @0x{:04X}", e.path, e.mask4, e.ret_addr))
        .collect();
    let call: Vec<String> = b.call_stack.iter().map(|a| format!("0x{:04X}", a)).collect();
    let b_header: Vec<String> =
        ["state", "pc", "conv_stack", "call_stack"]
            .iter().map(|s| s.to_string()).collect();
    let b_rows: Vec<Vec<String>> = vec![vec![
        format!("{}({})", bru_state_name(b.state), b.state),
        format!("0x{:04X}", b.pc),
        if conv.is_empty() { "-".to_string() } else { conv.join(", ") },
        if call.is_empty() { "-".to_string() } else { call.join(", ") },
    ]];
    let b_right: Vec<bool> = vec![false, true, false, false];

    // 3) 每核心：En / flags / mask / r0..r15（前 2 列文字、后续列数值右对齐）
    let mut r_header: Vec<String> =
        ["core", "En", "stt", "flags", "mask"].iter().map(|s| s.to_string()).collect();
    r_header.extend((0..16).map(|i| format!("r{i}")));
    let mut r_rows: Vec<Vec<String>> = Vec::with_capacity(4);
    for k in 0..4 {
        let c = &s.cores[k];
        let regs = c.rf.snapshot();
        let mut r: Vec<String> = vec![
            format!("{k}"),
            if so.en[k] { "ON".to_string() } else { "OFF".to_string() },
            format!("{}", status_name(so.per_core[k].status)),
            format!("0x{:04X}", c.flags_port() & 0xFFFF),
            format!("{:X}", c.mask_port() as u8),
        ];
        r.extend(regs.iter().map(|v| format!("{v:04X}")));
        r_rows.push(r);
    }
    let r_right: Vec<bool> = std::iter::repeat(false)
        .take(3)
        .chain(std::iter::repeat(true).take(2 + 16))
        .collect();

    // 三张表统一总宽度：取最宽者作为目标，使左右框对齐。
    let target = table_total(&g_header, &g_rows)
        .max(table_total(&b_header, &b_rows))
        .max(table_total(&r_header, &r_rows));
    table_print(&g_header, &g_rows, &g_right, target);
    table_print(&b_header, &b_rows, &b_right, target);
    table_print(&r_header, &r_rows, &r_right, target);
    println!();
}

/// 计算出各列显示宽度后的表格总宽度（含左右边框）。
fn table_total(header: &[String], rows: &[Vec<String>]) -> usize {
    let mut w: Vec<usize> = header.iter().map(|h| cell_width(h)).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(cell_width(c));
        }
    }
    w.iter().sum::<usize>() + 3 * w.len() + 1
}

/// 打印一个 ASCII 表格：表头、分隔线、若干数据行。
/// `right[i]=true` 时第 i 列右对齐（数值），否则左对齐（文本）。
/// 列宽按「终端显示宽度」计算（中日韩全角字符记 2 列），保证含中文的表格对齐。
/// 指定 `min_total` 时，把内容列统一加宽到该总宽度，使多张表的左右框对齐。
fn table_print(header: &[String], rows: &[Vec<String>], right: &[bool], min_total: usize) {
    let mut w: Vec<usize> = header.iter().map(|h| cell_width(h)).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            w[i] = w[i].max(cell_width(c));
        }
    }
    // 加宽：轮流给每列 +1，直到总宽度达到 min_total
    let mut total: usize = w.iter().sum::<usize>() + 3 * w.len() + 1;
    while total < min_total {
        for x in w.iter_mut() {
            if total >= min_total {
                break;
            }
            *x += 1;
            total += 1;
        }
    }
    let border = format!(
        "+{}+",
        w.iter().map(|&x| "-".repeat(x + 2)).collect::<Vec<_>>().join("+")
    );
    let fmt = |cells: &[String], header_row: bool| -> String {
        let mut out = String::from("|");
        for (i, &x) in w.iter().enumerate() {
            let s = &cells[i];
            let pad = x - cell_width(s);
            out.push(' ');
            if !header_row && right[i] {
                for _ in 0..pad { out.push(' '); }
                out.push_str(s);
            } else {
                out.push_str(s);
                for _ in 0..pad { out.push(' '); }
            }
            out.push_str(" |");
        }
        out
    };
    println!("{border}");
    println!("{}", fmt(header, true));
    println!("{border}");
    for r in rows {
        println!("{}", fmt(r, false));
    }
    println!("{border}");
}

/// 字符串占用的终端显示宽度（中日韩全角字符记 2 列，其余 1 列）。
fn cell_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

/// 是否为全角（East Asian Wide）字符。
fn is_wide(c: char) -> bool {
    let u = c as u32;
    (0x1100..=0x115F).contains(&u)      // 谚文 Jamo
        || (0x2E80..=0xA4CF).contains(&u) // CJK 部首/汉字/假名等
        || (0xAC00..=0xD7A3).contains(&u) // 谚文音节
        || (0xF900..=0xFAFF).contains(&u) // CJK 兼容
        || (0xFE30..=0xFE4F).contains(&u) // 竖排标点
        || (0xFF00..=0xFF60).contains(&u) // 全角形式
        || (0xFFE0..=0xFFE6).contains(&u) // 全角符号
}

// ---------------------------------------------------------------------------
// 迷你汇编：助记符 → 24bit 指令（本会话足够演示用；完整编译器另会话开发）。
// ---------------------------------------------------------------------------

/// 把程序文本解析为 24bit 指令向量；出错返回所在行号（1 起）。
fn assemble(src: &str) -> Result<Vec<u32>, usize> {
    let mut out = Vec::new();
    for (i, raw) in src.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match asm(line).or_else(|| parse_hex(line)) {
            Some(instr) => out.push(instr),
            None => return Err(i + 1),
        }
    }
    Ok(out)
}

fn parse_hex(s: &str) -> Option<u32> {
    let t = s.trim();
    let n = u32::from_str_radix(t.trim_start_matches("0x"), 16).ok()?;
    Some(n & 0xFF_FFFF)
}

fn reg(t: &str) -> Option<u8> {
    let t = t.trim();
    let d = t.strip_prefix('r').or_else(|| t.strip_prefix('R'))?;
    d.parse::<u8>().ok().filter(|&r| r < 16)
}

fn num(t: &str) -> Option<u32> {
    let t = t.trim();
    if let Some(h) = t.strip_prefix("0x") {
        u32::from_str_radix(h, 16).ok()
    } else {
        t.parse::<u32>().ok()
    }
}

/// 16bit 跳转地址（f1..f4）与 attr/cond（f5）编码。
fn branch(op: u32, addr: u16, f5: u8) -> u32 {
    (op << 20) | (((addr as u32) & 0xFFFF) << 4) | ((f5 & 0xF) as u32)
}

fn alu_func(m: &str) -> Option<u8> {
    Some(match m {
        "ADD" => 0x0, "SUB" => 0x1, "MUL" => 0x2, "XOR" => 0x4, "XNOR" => 0x5,
        "AND" => 0x6, "NAND" => 0x7, "OR" => 0x8, "NOR" => 0x9, "NOT" => 0xA,
        "SHL" => 0xB, "SHR" => 0xC, _ => return None,
    })
}

fn fpu_func(m: &str) -> Option<u8> {
    Some(match m {
        "HADD" => 0x0, "HSUB" => 0x1, "HMUL" => 0x2, "HRECIP" => 0x3,
        "HFMA" => 0x4, "HMAX" => 0x5, "HMIN" => 0x6, _ => return None,
    })
}

/// 单条助记符 → 24bit 指令编码。
pub fn asm(line: &str) -> Option<u32> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let m = toks.first()?.to_uppercase();
    let t = &toks.as_slice()[1..];
    match m.as_str() {
        "LDI" => { let d = reg(t[0])?; let imm = num(t[1])? as u16; Some(enc_ldi(d, imm)) }
        "MOV" => { let d = reg(t[0])?; let s = reg(t[1])?; Some(enc(op::MOV, &[d, s, 0, 0, 0])) }
        "MOV.M" => { let s = reg(t[0])?; Some(enc(op::MOV, &[0, s, 0, 0, 0x7])) }
        "NOP" => Some(0),
        // ALU
        _ if alu_func(&m).is_some() => {
            let f = alu_func(&m).unwrap();
            let d = reg(t[0])?;
            let a = reg(t[1])?;
            let b = if f == 0xA { 0 } else { reg(t.get(2).copied().unwrap_or("r0"))? };
            Some(enc(op::ALU, &[d, a, b, 0, f]))
        }
        // FPU
        _ if fpu_func(&m).is_some() => {
            let f = fpu_func(&m).unwrap();
            let d = reg(t[0])?;
            let a = reg(t[1])?;
            match m.as_str() {
                "HRECIP" => Some(enc(op::FPU, &[d, a, 0, 0, f])),
                "HFMA" => { let c = reg(t[3])?; let b = reg(t[2])?; Some(enc(op::FPU, &[d, a, b, c, f])) }
                _ => { let b = reg(t[2])?; Some(enc(op::FPU, &[d, a, b, 0, f])) }
            }
        }
        // 访存
        "STR" => { let data = reg(t[0])?; let base = reg(t[1])?; Some(enc(op::MEM, &[0, data, base, 0, 0])) }
        "LDR" => { let dst = reg(t[0])?; let base = reg(t[1])?; Some(enc(op::MEM, &[dst, 0, base, 0, 1])) }
        "PUSH" => { let data = reg(t[0])?; Some(enc(op::MEM, &[0, data, 0, 0, 2])) }
        "POP" => { let dst = reg(t[0])?; Some(enc(op::MEM, &[dst, 0, 0, 0, 3])) }
        // 流控
        "BRA" => { let a = num(t[0])? as u16; Some(branch(op::BRA as u32, a, 0)) }
        "BRAV" => { let a = num(t[0])? as u16; Some(branch(op::BRA as u32, a, 1)) }
        "BRAF" => { let a = num(t[0])? as u16; let c = num(t[1])? as u8; Some(branch(op::BRAF as u32, a, c)) }
        "BRAP" => { let a = num(t[0])? as u16; Some(branch(op::BRAP_REA as u32, a, 0)) }
        "REA" => Some((op::BRAP_REA as u32) << 20 | 0x1),
        "CALL" => { let a = num(t[0])? as u16; Some(branch(op::RET as u32, a, 0)) }
        "RET" => Some((op::RET as u32) << 20 | 0x1),
        // 系统
        "HLT" => Some(0xCu32 << 20),
        "HLTF" => { let c = num(t[0])? as u8; Some((0xDu32 << 20) | (c as u32 & 0xF)) }
        "DONE" => Some(0xEu32 << 20),
        "DONEF" => { let c = num(t[0])? as u8; Some((0xFu32 << 20) | (c as u32 & 0xF)) }
        "SYNC" => Some(0xBu32 << 20),
        _ => None,
    }
}

/// 内置示例程序：加载/搬移/整数加/访存/跳转/停机（演示寄存器状态）。
fn demo_program() -> Vec<u32> {
    vec![
        enc_ldi(1, 8),                          // idx0: r1 = 8          （地址 0）
        enc_ldi(2, 5),                          // idx1: r2 = 5          （地址 2）
        enc(op::MOV, &[3, 2, 0, 0, 0]),         // idx2: r3 = r2         （地址 4）
        enc(op::ALU, &[4, 2, 3, 0, 0x0]),       // idx3: r4 = r2 + r3    （地址 6）
        enc(op::MEM, &[0, 2, 1, 0, 0b0000]),    // idx4: mem[r1] = r2    （地址 8）
        enc(op::MEM, &[5, 0, 1, 0, 0b0001]),    // idx5: r5 = mem[r1]    （地址 A）
        branch(op::BRA as u32, 0x10, 0),        // idx6: BRA 0x10        （地址 C）
        enc(op::NOP, &[]),                      // idx7: NOP             （地址 E）
        (op::HLT as u32) << 20,                 // idx8: HLT             （地址 0x10）
    ]
}
