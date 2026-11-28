//! GPU 汇编编译器（单文件版，Rust 纯 std）。
//!
//! 把 `.asm` 源文本编译为一段连续半字内存镜像（半字寻址：一个地址 = 16bit 半字；
//! 每条 24bit 指令占 2 个半字地址），输出简易文本 `.hex`（每行 `地址: 8×16bit`，下一行 +8），
//! 并在 stdout 打印内存结构 / 宏列表 / 标签列表三段 ASCII 表格报告。
//!
//! 用法: `assembler <输入.asm> <输出.hex>`

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fmt::Write;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// ==================== 模块: error ====================
// 汇编错误类型。

/// 汇编错误：带（可选）源文件、行号与消息。
#[derive(Debug, Clone)]
pub struct AsmError {
    /// 出错文件标识。`None` 表示主文件（不打印文件名，只打印行号）。
    pub file: Option<String>,
    /// 出错行号（1-based，该文件内的源码行），0 = 非行错误（如文件、编码校验）。
    pub line: usize,
    pub msg: String,
}

impl AsmError {
    /// 行错误：只带行号（主文件）。
    pub fn at(line: usize, msg: impl Into<String>) -> Self {
        Self {
            file: None,
            line,
            msg: msg.into(),
        }
    }

    /// 行错误：带文件名 + 行号。
    pub fn at_file(file: impl Into<String>, line: usize, msg: impl Into<String>) -> Self {
        Self {
            file: Some(file.into()),
            line,
            msg: msg.into(),
        }
    }

    /// 全局错误：不带位置。
    pub fn global(msg: impl Into<String>) -> Self {
        Self {
            file: None,
            line: 0,
            msg: msg.into(),
        }
    }

    /// 定位描述（`文件:行` / `行 N` / 空）。
    pub fn location(&self) -> String {
        match (&self.file, self.line) {
            (Some(f), l) if l > 0 => format!("{f}:{l}"),
            (Some(f), _) => f.clone(),
            (None, l) if l > 0 => format!("行 {l}"),
            (None, _) => String::new(),
        }
    }
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let loc = self.location();
        if loc.is_empty() {
            write!(f, "{}", self.msg)
        } else {
            write!(f, "{loc}: {}", self.msg)
        }
    }
}

/// 汇编结果类型。
pub type Result<T> = std::result::Result<T, AsmError>;
// ==================== 模块: source ====================
// 源码行：携带文件标识与行号，供宏系统展开与 stdout 报告做精确源码定位。

/// 一行源码（预处理/展开后保留），携带它来自哪个文件、第几行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    /// 文件标识：<主文件> 或 .inc 路径。
    pub file: String,
    /// 该文件内的 1 起始行号。
    pub line: usize,
    /// 行的文本（已去除行注释、已含 include 展开）。
    pub text: String,
}

impl SourceLine {
    /// 构造一行。
    pub fn new(file: impl Into<String>, line: usize, text: impl Into<String>) -> Self {
        SourceLine {
            file: file.into(),
            line,
            text: text.into(),
        }
    }
}
// ==================== 模块: ast ====================
// 抽象语法树（AST）：`parse` 的结果。
//
// 顶层结构：`.DATA` 段（一个）+ 多个平铺的 `.kernel name :` 块。
// 标签（数据标签/分支标签）在 `layout` 阶段解析为地址。

/// 整个含 `include` 展开后的程序。
#[derive(Debug, Clone)]
pub struct Program {
    /// 全局静态数据区（`.DATA` 段）。
    pub data: Vec<DataItem>,
    pub kernels: Vec<Kernel>,
}

/// `.DATA` 段中的一个数据声明（符号定义或 `.space`）。
#[derive(Debug, Clone)]
pub struct DataItem {
    pub label: Option<String>,
    pub kind: DataKind,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum DataKind {
    /// `.half v1, v2, …` 每个值 16bit 连续排布。
    Half(Vec<i64>),
    /// `.space N` — 保留 N 个 16bit 字，初始化为 0。
    Space(u32),
}

/// 一个 `.kernel name :` 块。
#[derive(Debug, Clone)]
pub struct Kernel {
    pub name: String,
    pub line: usize,
    /// kernel 内局部静态区（宏展开出的 `.space`/`.half`），地址在指令之前。
    pub data: Vec<DataItem>,
    pub instrs: Vec<Instr>,
}

/// 一条指令（助记符已解析，操作数为原始 token 串，待 encode 解析）。
#[derive(Debug, Clone)]
pub struct Instr {
    pub label: Option<String>,
    pub mnemonic: String,
    pub operands: Vec<String>,
    pub line: usize,
}

impl fmt::Display for Instr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.mnemonic)?;
        for op in &self.operands {
            write!(f, " {op}")?;
        }
        Ok(())
    }
}
// ==================== 模块: expr ====================
// 算术表达式求值：整数常量表达式。
//
// 支持：十进制 / `0x`十六进制 / `0b`二进制 字面量、一元 `-`、四则运算 `+ - * /`、
// 括号、空白。结果按 i64 计算，最后校验 16bit 范围（-32768..=65535 或按位宽截断由调用方决定）。

/// 词法/语法：递归下降表达式解析。
pub fn eval(src: &str, line: usize) -> Result<i64> {
    let mut p = Parser {
        chars: src.chars().collect(),
        pos: 0,
        line,
    };
    let v = p.parse_expr()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(AsmError::at(line, format!("表达式含多余字符: {}", src)));
    }
    Ok(v)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    line: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }
    fn parse_expr(&mut self) -> Result<i64> {
        let mut lhs = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    let rhs = self.parse_term()?;
                    lhs = lhs
                        .checked_add(rhs)
                        .ok_or_else(|| AsmError::at(self.line, "表达式上溢（i64）".to_string()))?;
                }
                Some('-') => {
                    self.pos += 1;
                    let rhs = self.parse_term()?;
                    lhs = lhs
                        .checked_sub(rhs)
                        .ok_or_else(|| AsmError::at(self.line, "表达式下溢（i64）".to_string()))?;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }
    fn parse_term(&mut self) -> Result<i64> {
        let mut lhs = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    lhs = lhs
                        .checked_mul(rhs)
                        .ok_or_else(|| AsmError::at(self.line, "表达式上溢（i64）".to_string()))?;
                }
                Some('/') => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    if rhs == 0 {
                        return Err(AsmError::at(self.line, "除以零".to_string()));
                    }
                    lhs /= rhs;
                }
                _ => break,
            }
        }
        Ok(lhs)
    }
    fn parse_factor(&mut self) -> Result<i64> {
        self.skip_ws();
        // 一元负号
        if self.peek() == Some('-') {
            self.pos += 1;
            let v = self.parse_factor()?;
            return v
                .checked_neg()
                .ok_or_else(|| AsmError::at(self.line, "取负溢出".to_string()));
        }
        if self.peek() == Some('(') {
            self.pos += 1;
            let v = self.parse_expr()?;
            self.skip_ws();
            if self.peek() != Some(')') {
                return Err(AsmError::at(self.line, "缺少右括号".to_string()));
            }
            self.pos += 1;
            return Ok(v);
        }
        self.parse_number()
    }
    fn parse_number(&mut self) -> Result<i64> {
        self.skip_ws();
        let start = self.pos;
        // 收集连续的 alphanumeric（用于识别 0x / 0b / 十进制）
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() {
                s.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if s.is_empty() {
            let c = self.peek().unwrap_or('\0');
            return Err(AsmError::at(
                self.line,
                format!("无法解析数字（遇到 '{c}'）"),
            ));
        }
        let (radix, digits): (u32, &str) =
            if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                (16, rest)
            } else if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
                (2, rest)
            } else {
                (10, s.as_str())
            };
        if digits.is_empty() {
            self.pos = start;
            return Err(AsmError::at(self.line, format!("数字字面量无效: {s}")));
        }
        let v = i64::from_str_radix(digits, radix)
            .map_err(|e| AsmError::at(self.line, format!("数字字面量无效: {s} ({e})")))?;
        Ok(v)
    }
}

#[cfg(test)]
mod tests_expr {
    use super::*;
    #[test]
    fn basics() {
        assert_eq!(eval("4*2", 1).unwrap(), 8);
        assert_eq!(eval("1 + 2*3", 1).unwrap(), 7);
        assert_eq!(eval("(1+2)*3", 1).unwrap(), 9);
        assert_eq!(eval("0x10", 1).unwrap(), 16);
        assert_eq!(eval("0b101", 1).unwrap(), 5);
        assert_eq!(eval("-5", 1).unwrap(), -5);
        assert_eq!(eval("10/4", 1).unwrap(), 2);
    }
}
// ==================== 模块: preprocess ====================
// 预处理：去行注释、`#include "..."` 递归展开。
//
// 返回带源码位置（文件标识 + 文件内行号）的行序列，供宏系统展开与
// stdout 报告做精确源码定位。被包含文件缺失时仅向 stderr 告警、不报错。

/// 主文件在行序列中的标识。
pub const MAIN_FILE: &str = "<主文件>";

/// 读取并预处理源文件内容，返回带位置的源码行序列。
pub fn preprocess(source: &str, base_dir: &Path) -> Result<Vec<SourceLine>> {
    let mut stack = HashSet::new();
    let mut out = Vec::new();
    let root_key = "<root>".to_string();
    expand_lines(source, base_dir, MAIN_FILE, &root_key, &mut stack, &mut out);
    Ok(out)
}

/// 展开一个文本块的行；`key` 用于递归 include 的环检测。
fn expand_lines(
    text: &str,
    base_dir: &Path,
    file: &str,
    key: &str,
    stack: &mut HashSet<String>,
    out: &mut Vec<SourceLine>,
) {
    if !stack.insert(key.to_string()) {
        eprintln!("[assembler] 警告：检测到 #include 递归，跳过 {file}");
        stack.remove(key);
        return;
    }

    for (idx, line) in text.lines().enumerate() {
        let lineno = idx + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // 保序保留空行。空行在宏体内不算结束标记。
            out.push(SourceLine::new(file, lineno, line));
            continue;
        }
        // 行注释 `//`
        if let Some(pos) = line.find("//") {
            out.push(SourceLine::new(file, lineno, &line[..pos]));
            continue;
        }
        // include
        if let Some(name) = parse_include(trimmed) {
            match load_include(&name, base_dir) {
                Ok(contents) => {
                    let sub_display = resolved_display(&name, base_dir);
                    let sub_key = resolved_key(&name, base_dir);
                    expand_lines(&contents, base_dir, &sub_display, &sub_key, stack, out);
                }
                Err(e) => {
                    eprintln!(
                        "[assembler] 警告：无法读取被包含文件 \"{name}\"（{e}），已跳过该 include 行"
                    );
                    // 占位空行，保持行序列对齐
                    out.push(SourceLine::new(file, lineno, ""));
                }
            }
            continue;
        }
        out.push(SourceLine::new(file, lineno, line));
    }

    stack.remove(key);
}

/// include 文件在报错/报告中的展示名（尽量用规范路径）。
fn resolved_display(name: &str, base_dir: &Path) -> String {
    let path: PathBuf = if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else {
        base_dir.join(name)
    };
    path.to_string_lossy().into_owned()
}

/// include 文件用于环检测的键。
fn resolved_key(name: &str, base_dir: &Path) -> String {
    resolved_display(name, base_dir)
}

/// 解析 `#include "file"`（引号内文件名）；返回 None 表示非 include 行。
fn parse_include(line: &str) -> Option<String> {
    let rest = line.strip_prefix("#include")?;
    let rest = rest.trim().trim_start_matches('"');
    let name = rest.split('"').next()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests_preprocess {
    use super::*;

    #[test]
    fn strips_line_comments_preserving_lines() {
        let src = ".DATA\n// 注释\nx : .half 1\n.kernel k :\n    DONE\n";
        let base = Path::new(".");
        let lines = preprocess(src, base).unwrap();
        assert_eq!(lines[0].text, ".DATA");
        assert_eq!(lines[0].line, 1);
        assert_eq!(lines[1].text, ""); // 注释剥离留空
        assert_eq!(lines[1].line, 2);
        assert_eq!(lines[2].text, "x : .half 1");
        assert_eq!(lines[3].text, ".kernel k :");
        assert_eq!(lines[4].text, "    DONE");
    }
}

/// 加载被包含文件（相对 base_dir）。
fn load_include(name: &str, base_dir: &Path) -> io::Result<String> {
    let path: PathBuf = if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else {
        base_dir.join(name)
    };
    let mut f = std::fs::File::open(&path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    Ok(buf)
}
// ==================== 模块: define ====================
// 宏定义收集：提取 `.define` 常量与 `.macro` 宏体。
//
// 从预处理后的源码行流中抽走定义段，返回（待展开的程序行流）。
//
// 规则（已确认）：
// - `.define NAME VALUE`：编译期常量，`VALUE` 可引用任意（含更晚）已定义常量。
// - `.macro name (a, b):` … 宏体写到下一个 `.define`/`.macro` 或段声明
//   （`.DATA`/`.DEFINE`/`.kernel`）；注释与空行**不算**宏体结束（保留在体内）。
// - `.DEFINE` 段只是分组标记，本身无数据；`#include` 内容自动视作定义并入。
// - 宏名匹配不区分大小写、不忽略首尾下划线；参数名用于 `\(param)` 替换。

/// `.define` 常量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantDef {
    /// 定义时名字（按原名保存，匹配时严格按 token）。
    pub name: String,
    /// 值表达式原文。
    pub value: String,
    pub file: String,
    pub line: usize,
}

/// `.macro` 宏定义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroDef {
    /// 定义时名字。
    pub name: String,
    /// 参数名（`\(param)` 引用）。
    pub params: Vec<String>,
    /// 宏体行（含注释/空行，自带文件与行号）。
    pub body: Vec<SourceLine>,
    /// 定义头所在文件与行号。
    pub file: String,
    pub line: usize,
}

/// 定义收集结果。
#[derive(Debug, Clone, Default)]
pub struct DefineTable {
    /// 收集到的常量（按出现顺序，允许向后引用）。
    pub constants: Vec<ConstantDef>,
    /// 收集到的宏（按出现顺序）。
    pub macros: Vec<MacroDef>,
}

/// 段声明关键字：出现即终结当前宏体。
const SECTION_KEYS: &[&str] = &[".DATA", ".DEFINE", ".kernel"];

/// 从源码行流中抽走定义，返回（待展开的程序行流）。
pub fn collect(lines: &[SourceLine]) -> Result<(DefineTable, Vec<SourceLine>)> {
    let mut table = DefineTable::default();
    let mut rest: Vec<SourceLine> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let sl = &lines[i];
        let text = sl.text.trim();
        if text.is_empty() {
            i += 1;
            continue;
        }
        if text == ".DEFINE" {
            // 分组标记：无数据，直接消耗
            i += 1;
            continue;
        }
        if let Some(rest_of) = text.strip_prefix(".define") {
            let rest_of = rest_of.trim();
            if !rest_of.is_empty() {
                let c = parse_constant(sl.file.clone(), sl.line, rest_of)?;
                table.constants.push(c);
                i += 1;
                continue;
            }
            // `.define` 仅此而已（无值）→ 视为坏常量名，交给报错
            return Err(AsmError::at_file(
                sl.file.clone(),
                sl.line,
                ".define 缺少名称",
            ));
        }
        if let Some(rest_of) = text.strip_prefix(".macro") {
            if !rest_of.trim().is_empty() {
                let m = parse_macro(lines, &mut i, sl)?;
                table.macros.push(m);
                continue;
            }
            return Err(AsmError::at_file(
                sl.file.clone(),
                sl.line,
                ".macro 缺少名称",
            ));
        }
        // 其余行：留作程序行流
        rest.push(sl.clone());
        i += 1;
    }
    Ok((table, rest))
}

/// 解析 `.define NAME VALUE`。
fn parse_constant(file: String, line: usize, rest: &str) -> Result<ConstantDef> {
    let mut it = rest.splitn(2, char::is_whitespace);
    let name = it.next().unwrap_or("");
    if name.is_empty() {
        return Err(AsmError::at_file(file, line, ".define 缺少名称"));
    }
    if let Some(bad) = bad_ident(name) {
        return Err(AsmError::at_file(
            file,
            line,
            format!(".define 名非法（{bad}）: {name}"),
        ));
    }
    let value = it.next().unwrap_or("").trim().to_string();
    Ok(ConstantDef {
        name: name.to_string(),
        value,
        file,
        line,
    })
}

/// 解析 `.macro name (a, b):`，并把宏体行读入（i 前进到宏体结束之后）。
fn parse_macro(lines: &[SourceLine], i: &mut usize, header: &SourceLine) -> Result<MacroDef> {
    let text = header.text.trim();
    let rest = text
        .strip_prefix(".macro")
        .unwrap()
        .trim()
        .trim_end_matches(':')
        .trim();
    // 名字 = 直到 '(' 或空白
    let name = {
        let mut end = rest.len();
        for (idx, ch) in rest.char_indices() {
            if ch == '(' || ch.is_whitespace() {
                end = idx;
                break;
            }
        }
        rest[..end].trim().to_string()
    };
    if name.is_empty() {
        return Err(AsmError::at_file(
            header.file.clone(),
            header.line,
            ".macro 缺少名称",
        ));
    }
    if let Some(bad) = bad_ident(&name) {
        return Err(AsmError::at_file(
            header.file.clone(),
            header.line,
            format!(".macro 名非法（{bad}）: {name}"),
        ));
    }
    // 参数表：第一对括号内，逗号分隔
    let params = {
        let mut ps = Vec::new();
        if let Some(lp) = rest.find('(') {
            if let Some(rp) = rest[lp..].find(')') {
                let inner = &rest[lp + 1..lp + rp];
                for p in inner.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    ps.push(p.to_string());
                }
            }
        }
        ps
    };

    // 宏体：从 i+1 起到下一个定义/段声明
    let mut body: Vec<SourceLine> = Vec::new();
    let mut j = *i + 1;
    while j < lines.len() {
        let sl = &lines[j];
        let t = sl.text.trim();
        if !t.is_empty() && (is_define_header(t) || is_macro_header(t) || is_section(t)) {
            break;
        }
        body.push(sl.clone());
        j += 1;
        if j >= lines.len() {
            break;
        }
    }
    *i = j; // 停在下一个定义/段声明头，由外层循环处理

    Ok(MacroDef {
        name,
        params,
        body,
        file: header.file.clone(),
        line: header.line,
    })
}

fn is_define_header(t: &str) -> bool {
    t == ".DEFINE" || t.starts_with(".define")
}
fn is_macro_header(t: &str) -> bool {
    t.starts_with(".macro")
}
fn is_section(t: &str) -> bool {
    SECTION_KEYS.iter().any(|k| t.starts_with(k))
}

/// 标识符合法性与返回非法原因：仅字母/数字/下划线，首字符须字母或下划线。
fn bad_ident(name: &str) -> Option<&'static str> {
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Some("首字符须字母或下划线");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Some("仅允许字母/数字/下划线");
    }
    None
}

#[cfg(test)]
mod tests_define {
    use super::*;

    fn lines_of(v: Vec<&str>) -> Vec<SourceLine> {
        v.into_iter()
            .enumerate()
            .map(|(i, s)| SourceLine::new("test.inc", i + 1, s))
            .collect()
    }

    #[test]
    fn collect_constants_and_macros() {
        let src = lines_of(vec![
            ".define _gpu_thread_num_ 16",
            ".define _EQ_ 0",
            ".macro _mem_push_ (rs):",
            "    PUSH \\(rs)",
            ".macro _mem_pop_ (rd):",
            "    POP \\(rd)",
            ".DATA",
            "x : .half 1",
            ".kernel k :",
            "    DONE",
        ]);
        let (table, rest) = collect(&src).unwrap();
        // 两个常量
        assert_eq!(table.constants.len(), 2);
        assert_eq!(table.constants[0].name, "_gpu_thread_num_");
        assert_eq!(table.constants[0].value, "16");
        // 两个宏
        assert_eq!(table.macros.len(), 2);
        assert_eq!(table.macros[0].name, "_mem_push_");
        assert_eq!(table.macros[0].params, vec!["rs".to_string()]);
        assert_eq!(table.macros[0].body.len(), 1);
        assert_eq!(table.macros[1].name, "_mem_pop_");
        // 程序行流去掉定义，保留 .DATA / .half / .kernel / DONE
        assert_eq!(rest.len(), 4);
        assert_eq!(rest[0].text, ".DATA");
        assert_eq!(rest[3].text, "    DONE");
    }

    #[test]
    fn constant_bad_name_reported() {
        let src = lines_of(vec![".define -ULT- 6"]);
        let r = collect(&src);
        assert!(r.is_err());
        let e = r.unwrap_err();
        assert!(e.msg.contains("-ULT-"), "msg 应提到非法名: {}", e.msg);
    }
}
// ==================== 模块: expand ====================
// 宏展开：把 `.define` 常量与 `.macro` 调用递归展开成程序行流。
//
// 输入：`DefineTable` +（`collect` 返回的）待展开程序行流。
// 输出：完全展开的行流 + 供 stdout 报告的宏调用记录。
//
// 规则（已确认）：
// - 宏调用：行首 token 命中某宏名（不区分大小写、保留下划线）。无参宏可裸调用也可 `名 ()`。
// - 实参逗号分隔、`\(param)` 文本替换、参数个数严格匹配；支持嵌套。
// - `.define` 常量按完整 token 文本替换；常量值可引用任意（含更晚）常量；处理时环检测。
// - 宏内标签：默认局部化 + 每实例唯一化；被其他宏/源码引用的标签视为导出、保留原名。
// - 深度上限 64；循环调用（宏在使用栈中出现）报错。

/// 宏展开结果。
#[derive(Debug, Clone, Default)]
pub struct Expansion {
    /// 完全展开后的程序行流。
    pub lines: Vec<SourceLine>,
    /// 每次宏调用的展开记录（用于 stdout 报告）。
    pub calls: Vec<MacroCallRecord>,
}

/// 一次宏调用的展开记录。
#[derive(Debug, Clone)]
pub struct MacroCallRecord {
    /// 被调用的宏名（原名）。
    pub name: String,
    /// 定义位置。
    pub def_file: String,
    pub def_line: usize,
    /// 调用位置。
    pub call_file: String,
    pub call_line: usize,
    /// 本次展开在最终行流中的区段 [start, end)。
    pub start: usize,
    pub end: usize,
}

/// 深度上限。
const MAX_DEPTH: usize = 64;

/// 展开器。
struct Expander<'a> {
    constants: Vec<&'a ConstantDef>,
    macros: Vec<&'a MacroDef>,
    out: Vec<SourceLine>,
    calls: Vec<MacroCallRecord>,
    /// (宏名, 局部标签) → 需要唯一化。键用于查是否局部。
    local_labels: HashSet<(String, String)>,
    /// 正在展开的宏名（精确名，环检测用；宏名区分大小写）。
    stack: Vec<String>,
    /// 所有已知标签名（宏内定义 + 源码定义），供未定义标识符校验。
    known_labels: HashSet<String>,
    /// 每个宏的实例计数器（外到内展开，每次实例化 +1，作为局部标签唯一化后缀）。
    instance_counters: HashMap<String, usize>,
}

/// 展开入口。
pub fn expand(table: &DefineTable, program: &[SourceLine]) -> Result<Expansion> {
    // 收集所有已知标签名（含宏体与源码）
    let mut known_labels: HashSet<String> = HashSet::new();
    for m in &table.macros {
        for sl in &m.body {
            for lab in extract_label_defs(sl.text.trim()) {
                known_labels.insert(lab);
            }
        }
    }
    for sl in program {
        for lab in extract_label_defs(sl.text.trim()) {
            known_labels.insert(lab);
        }
    }
    let mut e = Expander {
        constants: table.constants.iter().collect(),
        macros: table.macros.iter().collect(),
        out: Vec::new(),
        calls: Vec::new(),
        local_labels: HashSet::new(),
        stack: Vec::new(),
        known_labels,
        instance_counters: HashMap::new(),
    };
    e.analyze_labels(table, program);
    e.expand_lines(program, 0)?;
    Ok(Expansion {
        lines: e.out,
        calls: e.calls,
    })
}

/// 是否形如寄存器 token（r0–r15，大小写皆可）。
fn is_reg_tok(t: &str) -> bool {
    let lower = t.to_lowercase();
    match lower.strip_prefix('r').map(|n| n.parse::<u32>()) {
        Some(Ok(v)) => v <= 15,
        _ => false,
    }
}

/// 若形如 `name__<inst>`（局部标签唯一化产物），返回 `name`；否则 None。
fn strip_uniquify_suffix(t: &str) -> Option<String> {
    let (base, rest) = t.rsplit_once("__")?;
    if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
        Some(base.to_string())
    } else {
        None
    }
}

impl<'a> Expander<'a> {
    /// 按定义名精确匹配宏（宏名区分大小写）。
    fn find_macro(&self, name: &str) -> Option<&'a MacroDef> {
        self.macros.iter().find(|m| m.name == name).copied()
    }

    fn find_const(&self, name: &str) -> Option<&'a ConstantDef> {
        self.constants.iter().find(|c| c.name == name).copied()
    }

    /// 解析一行的宏调用头。返回 (宏名, 参数表)；非宏调用返回 None。
    fn parse_call(&self, text: &str) -> Option<(String, Vec<String>)> {
        let toks: Vec<&str> = text.split_whitespace().collect();
        if toks.is_empty() {
            return None;
        }
        // 候选名：取第一 token 中 '(' 之前部分，若无则整个第一 token
        let t0 = toks[0];
        let name_cand = {
            let cut = t0.find('(').unwrap_or(t0.len());
            t0[..cut].trim().to_string()
        };
        if name_cand.is_empty() || self.find_macro(&name_cand).is_none() {
            return None;
        }
        // 参数表来源：t0 内括号，否则 toks[1] 的括号，否则空
        let args = if let Some(a) = extract_args(t0) {
            a
        } else if toks.len() > 1 {
            extract_args(toks[1]).unwrap_or_default()
        } else {
            Vec::new()
        };
        Some((name_cand, args))
    }

    /// 展开一组程序行流。
    fn expand_lines(&mut self, lines: &[SourceLine], depth: usize) -> Result<()> {
        if depth > MAX_DEPTH {
            return Err(AsmError::global("宏展开深度超过上限(64)"));
        }
        for sl in lines {
            self.expand_line(sl, depth)?;
        }
        Ok(())
    }

    /// 展开单行；宏调用则递归展开，否则常量替换后入流。
    fn expand_line(&mut self, sl: &SourceLine, depth: usize) -> Result<()> {
        let text = sl.text.trim();
        if text.is_empty() {
            return Ok(());
        }
        if let Some((name, args)) = self.parse_call(text) {
            if let Some(m) = self.find_macro(&name) {
                if self.stack.iter().any(|s| s == &m.name) {
                    return Err(AsmError::at_file(
                        sl.file.clone(),
                        sl.line,
                        format!("宏循环调用: {}", m.name),
                    ));
                }
                return self.expand_macro_call(sl, m, args, depth);
            }
        }
        // 非宏调用：常量替换后入流（去掉空结果行）
        let resolved = self.resolve_constants(text, &sl.file, sl.line)?;
        if !resolved.trim().is_empty() {
            // 校验：操作数位置的裸标识符必须是 寄存器/标签/常量(已替换)，否则报未定义
            self.check_undefined_idents(&resolved, &sl.file, sl.line)?;
            self.out
                .push(SourceLine::new(sl.file.clone(), sl.line, resolved));
        }
        Ok(())
    }

    /// 检查行内（首 token 之后）是否有未定义的裸标识符（非寄存器、非已知标签、非常量残留）。
    fn check_undefined_idents(&self, text: &str, f: &str, line: usize) -> Result<()> {
        let toks: Vec<&str> = text.split_whitespace().collect();
        // 指令直译为 `.xxx` 伪指令/段声明：`.kernel name :`、`.half …` 等，
        // 名称是 kernel/符号名而非标识符语义，跳过校验。
        if toks[0].starts_with('.') {
            return Ok(());
        }
        for t in toks.iter().skip(1) {
            let bare = t.trim_end_matches(':');
            if bare.is_empty() || bare.starts_with('.') {
                continue;
            }
            let first = bare.chars().next().unwrap();
            if !(first.is_ascii_alphabetic() || first == '_') {
                continue; // 数字/表达式开头
            }
            if is_reg_tok(bare) || self.known_labels.contains(bare) {
                continue;
            }
            // 局部标签已唯一化（`name__<inst>`），按去唯一化后缀后的基础名校验
            if let Some(base) = strip_uniquify_suffix(bare) {
                if self.known_labels.contains(&base) {
                    continue;
                }
            }
            if is_known_mnemonic(bare) {
                continue;
            }
            return Err(AsmError::at_file(
                f.to_string(),
                line,
                format!("未定义的标识符/常量: {bare}"),
            ));
        }
        Ok(())
    }

    /// 常量在文本中的完整 token 替换（递归，带环检测）。
    fn resolve_constants(&self, text: &str, f: &str, line: usize) -> Result<String> {
        let toks: Vec<&str> = text.split_whitespace().collect();
        let mut out = Vec::new();
        for t in &toks {
            let leading_colon = t.ends_with(':'); // 形如 name: 的联动（罕见）
            let bare = t.trim_end_matches(':');
            if let Some(c) = self.find_const(bare) {
                let mut stack = Vec::new();
                let val = self.resolve_const_inner(&c.value, &c.name, &mut stack, f, line, 0)?;
                let tail = if leading_colon { ":" } else { "" };
                out.push(format!("{}{}", val, tail));
            } else {
                out.push(t.to_string());
            }
        }
        Ok(out.join(" "))
    }

    fn resolve_const_inner(
        &self,
        value: &str,
        name: &str,
        stack: &mut Vec<String>,
        f: &str,
        line: usize,
        depth: usize,
    ) -> Result<String> {
        if depth > MAX_DEPTH {
            return Err(AsmError::at_file(f, line, format!("常量循环引用: {name}")));
        }
        if stack.iter().any(|s| s == name) {
            return Err(AsmError::at_file(f, line, format!("常量循环引用: {name}")));
        }
        stack.push(name.to_string());
        let toks: Vec<&str> = value.split_whitespace().collect();
        let mut out = Vec::new();
        for t in &toks {
            let bare = t.trim_end_matches(':');
            if let Some(c) = self.find_const(bare) {
                let v = self.resolve_const_inner(&c.value, &c.name, stack, f, line, depth + 1)?;
                out.push(if t.ends_with(':') { format!("{v}:") } else { v });
            } else {
                out.push(t.to_string());
            }
        }
        stack.pop();
        Ok(out.join(" "))
    }

    /// 展开一次宏调用。
    fn expand_macro_call(
        &mut self,
        call: &SourceLine,
        m: &'a MacroDef,
        args: Vec<String>,
        depth: usize,
    ) -> Result<()> {
        if args.len() != m.params.len() {
            return Err(AsmError::at_file(
                call.file.clone(),
                call.line,
                format!(
                    "宏 {} 实参个数 {} 与形参 {} 不符",
                    m.name,
                    args.len(),
                    m.params.len()
                ),
            ));
        }
        // 每宏独立计数：外到内展开，每次实例化该宏则计数 +1，作为局部标签唯一化后缀
        let instance = if self.has_local_labels(&m.name) {
            let c = self.instance_counters.entry(m.name.clone()).or_insert(0);
            let v = *c;
            *c += 1;
            Some(v)
        } else {
            None
        };
        let start = self.out.len();
        self.stack.push(m.name.clone());
        self.expand_macro_body(&m.body, m, instance, &args, depth + 1)?;
        self.stack.pop();
        let end = self.out.len();
        self.calls.push(MacroCallRecord {
            name: m.name.clone(),
            def_file: m.file.clone(),
            def_line: m.line,
            call_file: call.file.clone(),
            call_line: call.line,
            start,
            end,
        });
        Ok(())
    }

    fn expand_macro_body(
        &mut self,
        body: &[SourceLine],
        m: &'a MacroDef,
        instance: Option<usize>,
        args: &[String],
        depth: usize,
    ) -> Result<()> {
        for sl in body {
            // 1) `\(param)` -> 实参
            let mut text = sl.text.clone();
            for (i, p) in m.params.iter().enumerate() {
                let pat = format!(r"\({})", p);
                text = text.replace(&pat, &args[i]);
            }
            // 2) 局部标签唯一化
            if let Some(inst) = instance {
                text = self.uniquify_labels(&text, &m.name, inst);
            }
            let new_sl = SourceLine::new(sl.file.clone(), sl.line, text);
            // 3) 递归展开
            self.expand_line(&new_sl, depth)?;
        }
        Ok(())
    }

    fn has_local_labels(&self, mname: &str) -> bool {
        self.local_labels.iter().any(|(m, _)| m == mname)
    }
    fn is_local_label(&self, mname: &str, label: &str) -> bool {
        self.local_labels
            .contains(&(mname.to_string(), label.to_string()))
    }

    /// 局部标签唯一化：对宏体行里每个 token，若（去尾冒号后）是本宏的局部标签，
    /// 则替换为 `name__inst`（含原结尾冒号）。
    fn uniquify_labels(&self, text: &str, mname: &str, inst: usize) -> String {
        let toks: Vec<&str> = text.split_whitespace().collect();
        let suffix = format!("__{inst}");
        let mut out = Vec::new();
        for t in &toks {
            let colon = t.ends_with(':');
            let bare = t.trim_end_matches(':');
            if self.is_local_label(mname, bare) {
                out.push(format!(
                    "{}{}{}",
                    bare,
                    suffix,
                    if colon { ":" } else { "" }
                ));
            } else {
                out.push(t.to_string());
            }
        }
        out.join(" ")
    }

    /// 预分析宏内标签：落在静态分配（`.half`/`.space`）上的标签**不唯一化**，
    /// 交由 layout 作 kernel 局部静态符号；其余（指令标签）作为局部标签，展开时按实例唯一化。
    /// （已去除"导出标签"机制：不再因被其他宏/源码引用而保留原名。）
    fn analyze_labels(&mut self, table: &DefineTable, _program: &[SourceLine]) {
        for m in &table.macros {
            for (i, sl) in m.body.iter().enumerate() {
                let text = sl.text.trim();
                for lab in extract_label_defs(text) {
                    if line_label_is_static(&m.body, i) {
                        // 静态分配标签：保留原名，不入 local_labels（不唯一化）
                        continue;
                    }
                    self.local_labels.insert((m.name.clone(), lab));
                }
            }
        }
    }
}

/// 判断宏体第 `i` 行的标签定义是否落在静态分配上：
/// - 同行冒号后是 `.half`/`.space`；或
/// - 纯标签行（冒号后为空），且下一非空行是 `.half`/`.space`。
fn line_label_is_static(body: &[SourceLine], i: usize) -> bool {
    let text = body[i].text.trim();
    if let Some(col) = text.find(':') {
        let rest = text[col + 1..].trim();
        if rest.starts_with(".half") || rest.starts_with(".space") {
            return true;
        }
        if !rest.is_empty() {
            return false; // 同行是「标签: 指令」→ 指令标签
        }
    } else {
        return false;
    }
    // 纯标签行：看下一非空行
    let mut j = i + 1;
    while j < body.len() && body[j].text.trim().is_empty() {
        j += 1;
    }
    if j < body.len() {
        let nx = body[j].text.trim();
        return nx.starts_with(".half") || nx.starts_with(".space");
    }
    false
}

/// 从一行提取参数表（第一对括号内的逗号分隔）；无括号返回 None。
fn extract_args(text: &str) -> Option<Vec<String>> {
    let lp = text.find('(')?;
    let rp_rel = text[lp..].find(')')?;
    let inner = &text[lp + 1..lp + rp_rel];
    let args: Vec<String> = inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Some(args)
}

/// 从宏体行提取标签定义（`name:` 或 `name :`）。
fn extract_label_defs(text: &str) -> Vec<String> {
    let mut labs = Vec::new();
    if let Some(col) = text.find(':') {
        let name = text[..col].trim();
        if !name.is_empty() && !name.starts_with('.') {
            labs.push(name.to_string());
        }
    }
    labs
}

#[cfg(test)]
mod tests_expand {
    use super::*;

    fn run(src: Vec<&str>) -> Result<Expansion> {
        let lines: Vec<SourceLine> = src
            .iter()
            .enumerate()
            .map(|(i, s)| SourceLine::new("<主文件>", i + 1, *s))
            .collect();
        let (table, rest) = collect(&lines)?;
        expand(&table, &rest)
    }

    #[test]
    fn simple_macro_expansion() {
        let e = run(vec![
            ".macro _mem_push_ (rs):",
            "    PUSH \\(rs)",
            ".kernel k :",
            "_mem_push_ (r3)",
        ])
        .unwrap();
        // out = [".kernel k :", "PUSH r3"]
        assert_eq!(e.lines.len(), 2);
        assert_eq!(e.lines[1].text.trim(), "PUSH r3");
        assert_eq!(e.calls.len(), 1);
        assert_eq!(e.calls[0].start, 1);
        assert_eq!(e.calls[0].end, 2);
    }

    #[test]
    fn constant_substitution() {
        let e = run(vec![
            ".define _gpu_thread_num_ 16",
            ".define _param_size_ _gpu_thread_num_ * 2",
            ".kernel k :",
            "    LDI r0 _param_size_",
        ])
        .unwrap();
        // out = [".kernel k :", LDI...]
        assert_eq!(e.lines.len(), 2);
        assert!(
            e.lines[1].text.contains("16 * 2"),
            "got: {}",
            e.lines[1].text
        );
    }

    #[test]
    fn arg_count_mismatch_errors() {
        let r = run(vec![
            ".macro _f_ (a):",
            "    NOP",
            ".kernel k :",
            "_f_ (1, 2)",
        ]);
        assert!(r.is_err());
        assert!(r.unwrap_err().msg.contains("实参个数"));
    }

    #[test]
    fn nested_recursion_depth() {
        let r = run(vec![
            ".macro _A_ ():",
            "    _B_ ()",
            ".macro _B_ ():",
            "    _A_ ()",
            ".kernel k :",
            "    _A_ ()",
        ]);
        assert!(r.is_err(), "应有循环调用错误");
    }

    #[test]
    fn local_label_uniquified_across_instances() {
        let e = run(vec![
            ".macro _m_ ():",
            "_m_L_:",
            "    BRA _m_L_ 0",
            ".kernel k :",
            "_m_ ()",
            "_m_ ()",
        ])
        .unwrap();
        // out = [kernel, L..., BRA.., L..., BRA..] 共 5 行
        assert_eq!(e.lines.len(), 5, "got {e:#?}");
        let first: Vec<&str> = e.lines[1].text.split_whitespace().collect();
        let third: Vec<&str> = e.lines[3].text.split_whitespace().collect();
        let a = first[0].trim_end_matches(':');
        let b = third[0].trim_end_matches(':');
        assert_ne!(a, b, "两实例局部标签应唯一化: {e:#?}");
        // 每宏独立计数器：两次调用 _m_ 的后缀应为 __0、__1
        assert!(a.ends_with("__0"), "首个实例应为 __0: {a}");
        assert!(b.ends_with("__1"), "第二个实例应为 __1: {b}");
        // 且 BRA 引用了同实例标签
        assert!(e.lines[2].text.contains(a));
        assert!(e.lines[4].text.contains(b));
    }

    #[test]
    fn static_label_not_uniquified() {
        // 静态分配标签（落在 .space/.half 上）不唯一化，保留原名（交由 layout 作 kernel 局部符号）
        let e = run(vec![
            ".macro _def_ ():",
            "_param_ : .space 4",
            ".macro _use_ ():",
            "    LDI r0 _param_",
            ".kernel k :",
            "_def_ ()",
            "_use_ ()",
        ])
        .unwrap();
        // out = [kernel, _param_ space, LDI] 共 3 行
        let texts: Vec<String> = e.lines.iter().map(|l| l.text.trim().to_string()).collect();
        assert_eq!(texts[1], "_param_ : .space 4");
        assert_eq!(texts[2], "LDI r0 _param_");
    }
}
// ==================== 模块: encode ====================
// 指令编码：将 `Instr`（助记符 + 操作数 token）编码为 24bit 指令。
//
// 位布局（权威 `docs/指令集表.md`）：
//   op=I[23:20], f1=I[19:16], f2=I[15:12], f3=I[11:8], f4=I[7:4], f5=I[3:0]
//   分支/CALL 16bit 地址占 f1..f4 = (I>>4)&0xFFFF。
//
// 操作数字典（来自 `refs/GPU指令集.xlsx` 转储 `tools/dump_xlsx.txt`，需与文档核对实现）。

/// 已知助记符集合（MVP 已实现的 40 条指令）。
pub const KNOWN_MNEMONICS: &[&str] = &[
    "NOP", "MOV", "LDI", "SEL", "ADD", "SUB", "CMP", "MUL", "XOR", "XNOR", "AND", "NAND", "OR",
    "NOR", "NOT", "SHL", "SHR", "HADD", "HSUB", "HMUL", "HRECIP", "HFMA", "HMAX", "HMIN", "HCVT",
    "BRA", "BRAF", "BRAP", "REA", "CALL", "RET", "STR", "LDR", "PUSH", "POP", "ATADD", "ATSWAP",
    "ATCAS", "SYNC", "HLT", "HLTF", "DONE", "DONEF",
];

/// 判断助记符是否为已知指令。
pub fn is_known_mnemonic(m: &str) -> bool {
    KNOWN_MNEMONICS.contains(&m.to_uppercase().as_str())
}

/// 编码单条指令为 24bit 值。
pub fn encode_instr(instr: &Instr, symbols: &HashMap<String, u16>) -> Result<u32> {
    let m = instr.mnemonic.as_str();
    let op = &instr.operands;
    let line = instr.line;

    // 通用操作数取值辅助
    let reg = |tok: &str| parse_reg(tok, line);
    let val = |tok: &str| resolve_value(tok, symbols, line);

    match m {
        "NOP" => Ok(build(0x0, 0, 0, 0, 0, 0)),

        // MOV dest [src|type]
        // - 3 操作数: dest src type（type!=0 时 src 忽略，数据源来自特殊寄存器）
        // - 2 操作数: 若第二个是寄存器 rN → 视为 src，type=0（寄存器⇔寄存器）
        //             否则 → 视为 type 常量，src=0（读特殊寄存器）
        "MOV" => match op.len() {
            3 => Ok(build(
                0x1,
                reg(&op[0])?,
                reg(&op[1])?,
                0,
                0,
                field4(op[2].as_str(), line, "type")?,
            )),
            2 => {
                if is_reg(&op[1]) {
                    Ok(build(0x1, reg(&op[0])?, reg(&op[1])?, 0, 0, 0))
                } else {
                    Ok(build(
                        0x1,
                        reg(&op[0])?,
                        0,
                        0,
                        0,
                        field4(op[1].as_str(), line, "type")?,
                    ))
                }
            }
            _ => Err(AsmError::at(
                line,
                "MOV 需要 2 或 3 个操作数: 目的 [源] type",
            )),
        },

        // LDI dest imm16
        "LDI" => {
            require(op, 2, "LDI 需要 2 个操作数: 目的 立即数", line)?;
            let imm = val(&op[1])?;
            if imm > 0xFFFF {
                return Err(AsmError::at(
                    line,
                    format!("LDI 立即数超出 16bit: {}", op[1]),
                ));
            }
            Ok(build(
                0x2,
                reg(&op[0])?,
                (imm >> 12) & 0xF,
                (imm >> 8) & 0xF,
                (imm >> 4) & 0xF,
                imm & 0xF,
            ))
        }

        // SEL dest a b cond
        "SEL" => {
            require(op, 4, "SEL 需要 4 个操作数: 目的 a b cond", line)?;
            Ok(build(
                0x3,
                reg(&op[0])?,
                reg(&op[1])?,
                reg(&op[2])?,
                0,
                field4(op[3].as_str(), line, "cond")?,
            ))
        }

        // 整数 ALU (op=0100)
        "ADD" => int_alu(op, 0x0, line),
        "SUB" | "CMP" => int_alu(op, 0x1, line),
        "MUL" => int_alu(op, 0x2, line),
        "XOR" => int_alu(op, 0x4, line),
        "XNOR" => int_alu(op, 0x5, line),
        "AND" => int_alu(op, 0x6, line),
        "NAND" => int_alu(op, 0x7, line),
        "OR" => int_alu(op, 0x8, line),
        "NOR" => int_alu(op, 0x9, line),
        "NOT" => int_alu(op, 0xA, line),
        "SHL" => int_alu_sh(op, 0xB, line),
        "SHR" => int_alu_sh(op, 0xC, line),

        // FP16 ALU (op=0101)
        "HADD" => fp_alu(op, 0x0, 3, line),
        "HSUB" => fp_cmp(op, 0x1, line),
        "HMUL" => fp_alu(op, 0x2, 3, line),
        "HRECIP" => fp_alu(op, 0x3, 3, line),
        "HFMA" => fp_hfma(op, line),
        "HMAX" => fp_alu(op, 0x5, 3, line),
        "HMIN" => fp_alu(op, 0x6, 3, line),
        "HCVT" => fp_cvt(op, line),

        // 分支
        "BRA" => {
            require(op, 2, "BRA 需要 2 个操作数: 目标 f5属性", line)?;
            let addr = branch_addr(&op[0], symbols, line)?;
            build_branch(0x6, addr, field4(op[1].as_str(), line, "f5属性")?)
        }
        "BRAF" => {
            require(op, 2, "BRAF 需要 2 个操作数: 目标 cond", line)?;
            let addr = branch_addr(&op[0], symbols, line)?;
            build_branch(0x7, addr, field4(op[1].as_str(), line, "cond")?)
        }
        "BRAP" => {
            require(op, 1, "BRAP 需要 1 个操作数: 目标", line)?;
            let addr = branch_addr(&op[0], symbols, line)?;
            build_branch(0x8, addr, 0x0)
        }
        "REA" => {
            require(op, 1, "REA 需要 1 个操作数: 目标", line)?;
            let addr = branch_addr(&op[0], symbols, line)?;
            build_branch(0x8, addr, 0x1)
        }
        "CALL" => {
            require(op, 1, "CALL 需要 1 个操作数: 目标", line)?;
            let addr = branch_addr(&op[0], symbols, line)?;
            build_branch(0x9, addr, 0x0)
        }
        "RET" => Ok(build(0x9, 0, 0, 0, 0, 0x1)),

        // 访存 (op=1010)
        "STR" => mem_str(op, line, symbols),
        "LDR" => mem_ldr(op, line, symbols),
        "PUSH" => mem_push(op, line),
        "POP" => mem_pop(op, line),
        "ATADD" => mem_at(op, 0x4, line, symbols),
        "ATSWAP" => mem_at(op, 0x5, line, symbols),
        "ATCAS" => mem_atcas(op, line, symbols),

        // 系统
        "SYNC" => Ok(build(0xB, 0, 0, 0, 0, 0)),
        "HLT" => Ok(build(0xC, 0, 0, 0, 0, 0)),
        "HLTF" => {
            require(op, 1, "HLTF 需要 1 个操作数: cond", line)?;
            Ok(build(
                0xD,
                0,
                0,
                0,
                0,
                field4(op[0].as_str(), line, "cond")?,
            ))
        }
        "DONE" => Ok(build(0xE, 0, 0, 0, 0, 0)),
        "DONEF" => {
            require(op, 1, "DONEF 需要 1 个操作数: cond", line)?;
            Ok(build(
                0xF,
                0,
                0,
                0,
                0,
                field4(op[0].as_str(), line, "cond")?,
            ))
        }

        _ => Err(AsmError::at(line, format!("未知助记符: {m}"))),
    }
}

fn build(op: u32, f1: u32, f2: u32, f3: u32, f4: u32, f5: u32) -> u32 {
    (op << 20) | (f1 << 16) | (f2 << 12) | (f3 << 8) | (f4 << 4) | f5
}
fn build_branch(op: u32, addr: u16, f5: u32) -> Result<u32> {
    Ok((op << 20) | ((addr as u32) << 4) | f5)
}

fn require(op: &[String], n: usize, msg: &str, line: usize) -> Result<()> {
    if op.len() < n {
        Err(AsmError::at(line, msg))
    } else if op.len() > n {
        Err(AsmError::at(line, format!("{msg}（操作数过多）")))
    } else {
        Ok(())
    }
}

/// 解析寄存器 token（r0–r15，大小写皆可）。
fn parse_reg(tok: &str, line: usize) -> Result<u32> {
    let t = tok.trim();
    let lower = t.to_lowercase();
    let n = lower
        .strip_prefix('r')
        .ok_or_else(|| AsmError::at(line, format!("需要寄存器 r0-r15，得到: {tok}")))?;
    let v: u32 = n
        .parse()
        .map_err(|_| AsmError::at(line, format!("非法寄存器: {tok}")))?;
    if v > 15 {
        return Err(AsmError::at(line, format!("寄存器超出 r15: {tok}")));
    }
    Ok(v)
}

/// 判断 token 是否为寄存器名（r0–r15，大小写皆可）。
fn is_reg(tok: &str) -> bool {
    let lower = tok.trim().to_lowercase();
    match lower.strip_prefix('r').map(|n| n.parse::<u32>()) {
        Some(Ok(v)) => v <= 15,
        _ => false,
    }
}

/// 4bit 字段取值（用于 type/cond/func 等）。
fn field4(tok: &str, line: usize, what: &str) -> Result<u32> {
    let v = resolve_value(tok, &HashMap::new(), line)?;
    if v > 0xF {
        return Err(AsmError::at(line, format!("{what} 超出 4bit 范围: {tok}")));
    }
    Ok(v as u32)
}

/// 解析一个可为裸符号或数值表达式的操作数到 32bit 值。
fn resolve_value(tok: &str, symbols: &HashMap<String, u16>, line: usize) -> Result<u32> {
    let t = tok.trim();
    // 若匹配符号名，取符号值
    if let Some(v) = symbols.get(t) {
        return Ok(*v as u32);
    }
    // 否则当数值/表达式
    let v = eval(t, line)?;
    if v < 0 {
        return Err(AsmError::at(line, format!("操作数不能为负: {tok}")));
    }
    Ok(v as u32)
}

/// 分支/CALL 16bit 地址：可为目标标签名（符号表）或数值表达式。
fn branch_addr(tok: &str, symbols: &HashMap<String, u16>, line: usize) -> Result<u16> {
    if let Some(v) = symbols.get(tok.trim()) {
        return Ok(*v);
    }
    let v = eval(tok.trim(), line)?;
    if !(0..=0xFFFF).contains(&v) {
        return Err(AsmError::at(line, format!("分支地址超出 16bit: {tok}")));
    }
    Ok(v as u16)
}

// ---- 具体指令族编码 ----

/// 整数 ALU：dest a b（f1,f2,f3），func=H。
fn int_alu(op: &[String], func: u32, line: usize) -> Result<u32> {
    require(op, 3, "整数运算需要 3 个操作数: 目的 a b", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    let f3 = parse_reg(&op[2], line)?;
    Ok(build(0x4, f1, f2, f3, 0, func))
}
/// 移位：SHL/SHR 目的 源 移位量寄存器（移位量来自寄存器，f3=该寄存器号）。
fn int_alu_sh(op: &[String], func: u32, line: usize) -> Result<u32> {
    require(op, 3, "移位需要 3 个操作数: 目的 源 移位量寄存器", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    let f3 = parse_reg(&op[2], line)?;
    Ok(build(0x4, f1, f2, f3, 0, func))
}

/// FP16 三操作数：dest a b。
fn fp_alu(op: &[String], func: u32, n: usize, line: usize) -> Result<u32> {
    require(op, n, "浮点运算操作数数量错误", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    let f3 = parse_reg(&op[2], line)?;
    Ok(build(0x5, f1, f2, f3, 0, func))
}
/// HSUB/CMP（同编码，别名）。
fn fp_cmp(op: &[String], func: u32, line: usize) -> Result<u32> {
    fp_alu(op, func, 3, line)
}
/// HFMA dest a b c（f4=c）。
fn fp_hfma(op: &[String], line: usize) -> Result<u32> {
    require(op, 4, "HFMA 需要 4 个操作数: 目的 a b c", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    let f3 = parse_reg(&op[2], line)?;
    let f4 = parse_reg(&op[3], line)?;
    Ok(build(0x5, f1, f2, f3, f4, 0x4))
}
/// HCVT dest src（两种 func 变体均接受两操作数）。
fn fp_cvt(op: &[String], line: usize) -> Result<u32> {
    require(op, 2, "HCVT 需要 2 个操作数: 目的 源", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    Ok(build(0x5, f1, f2, 0, 0, 0x7))
}

// 访存
fn mem_str(op: &[String], line: usize, symbols: &HashMap<String, u16>) -> Result<u32> {
    require(op, 3, "STR 需要 3 个操作数: 数据 基址 偏移", line)?;
    let f2 = parse_reg(&op[0], line)?;
    let f3 = parse_reg(&op[1], line)?;
    let f4 = mem_off(&op[2], symbols, line)?;
    Ok(build(0xA, 0, f2, f3, f4, 0x0))
}
fn mem_ldr(op: &[String], line: usize, symbols: &HashMap<String, u16>) -> Result<u32> {
    require(op, 3, "LDR 需要 3 个操作数: 目的 基址 偏移", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f3 = parse_reg(&op[1], line)?;
    let f4 = mem_off(&op[2], symbols, line)?;
    Ok(build(0xA, f1, 0, f3, f4, 0x1))
}
fn mem_push(op: &[String], line: usize) -> Result<u32> {
    require(op, 1, "PUSH 需要 1 个操作数: 待压值", line)?;
    let f2 = parse_reg(&op[0], line)?;
    Ok(build(0xA, 0, f2, 0, 0, 0x2))
}
fn mem_pop(op: &[String], line: usize) -> Result<u32> {
    require(op, 1, "POP 需要 1 个操作数: 目的", line)?;
    let f1 = parse_reg(&op[0], line)?;
    Ok(build(0xA, f1, 0, 0, 0, 0x3))
}
fn mem_at(op: &[String], func: u32, line: usize, symbols: &HashMap<String, u16>) -> Result<u32> {
    require(op, 4, "原子操作需要 4 个操作数: 目的 值 基址 偏移", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    let f3 = parse_reg(&op[2], line)?;
    let f4 = mem_off(&op[3], symbols, line)?;
    Ok(build(0xA, f1, f2, f3, f4, func))
}
fn mem_atcas(op: &[String], line: usize, symbols: &HashMap<String, u16>) -> Result<u32> {
    require(op, 4, "ATCAS 需要 4 个操作数: 目的 比较值 基址 新值", line)?;
    let f1 = parse_reg(&op[0], line)?;
    let f2 = parse_reg(&op[1], line)?;
    let f3 = parse_reg(&op[2], line)?;
    let f4 = mem_off(&op[3], symbols, line)?;
    Ok(build(0xA, f1, f2, f3, f4, 0x6))
}

/// 访存偏移：4bit（寄存器或整数）。
fn mem_off(tok: &str, symbols: &HashMap<String, u16>, line: usize) -> Result<u32> {
    // 若为寄存器则取 ; 否则取 4bit 整数
    if let Ok(v) = parse_reg(tok, line) {
        return Ok(v);
    }
    let v = resolve_value(tok, symbols, line)?;
    if v > 0xF {
        return Err(AsmError::at(line, format!("访存偏移超出 4bit: {tok}")));
    }
    Ok(v)
}

#[cfg(test)]
mod tests_encode {
    use super::*;
    fn syms() -> HashMap<String, u16> {
        let mut m = HashMap::new();
        m.insert("loop".to_string(), 24);
        m
    }
    fn ins(mnemonic: &str, operands: &[&str]) -> Instr {
        Instr {
            label: None,
            mnemonic: mnemonic.to_string(),
            operands: operands.iter().map(|s| s.to_string()).collect(),
            line: 1,
        }
    }
    #[test]
    fn enc_basic() {
        assert_eq!(encode_instr(&ins("NOP", &[]), &syms()).unwrap(), 0);
        // MOV r1 r2 2
        assert_eq!(
            encode_instr(&ins("MOV", &["r1", "r2", "2"]), &syms()).unwrap(),
            0x1_1_2_0_0_2
        );
        // LDI r1 0x1234
        assert_eq!(
            encode_instr(&ins("LDI", &["r1", "0x1234"]), &syms()).unwrap(),
            0x0021_1234
        );
        // DONE
        assert_eq!(encode_instr(&ins("DONE", &[]), &syms()).unwrap(), 0xE0_0000);
        // BRA loop 0 -> op=0x6, addr=24(0x18)<<4=0x180
        assert_eq!(
            encode_instr(&ins("BRA", &["loop", "0"]), &syms()).unwrap(),
            0x600180
        );
    }
    #[test]
    fn enc_alu() {
        // ADD  dest=a r1, a=r2, b=r3
        assert_eq!(
            encode_instr(&ins("ADD", &["r1", "r2", "r3"]), &syms()).unwrap(),
            0x412300
        );
        // SHL dest r1 src r2 移位量寄存器 r3, func=0xB
        assert_eq!(
            encode_instr(&ins("SHL", &["r1", "r2", "r3"]), &syms()).unwrap(),
            0x412300 | 0xB
        );
    }
    #[test]
    fn enc_mem() {
        assert_eq!(
            encode_instr(&ins("POP", &["r1"]), &syms()).unwrap(),
            0xA_1_0_0_0_3
        );
        assert_eq!(
            encode_instr(&ins("PUSH", &["r2"]), &syms()).unwrap(),
            0xA_0_2_0_0_2
        );
    }
}
// ==================== 模块: parse ====================
// 解析器：将预处理后的行序列解析为 AST（`Program`）。
//
// 语法要点（已确认）：
// - 段声明/标签冒号无需紧贴：`name:` 或 `name :` 均可。
// - 顶层 = `.DATA` 段 + 多个平铺 `.kernel name :` 块。
// - 指令助记符操作数空格分隔（无逗号）；行尾可选 H 整数字段。
// - 操作数为原始 token 串，具体编码在 `encode` 阶段逐条解析。

/// 解析预处理后的行序列（带源码位置）为 AST。
pub fn parse(lines: &[SourceLine]) -> Result<Program> {
    let mut data: Vec<DataItem> = Vec::new();
    let mut kernels: Vec<Kernel> = Vec::new();
    let mut in_data = false;
    let mut cur: Option<Kernel> = None;
    let mut pending_label: Option<String> = None;

    let flush_kernel = |cur: &mut Option<Kernel>, kernels: &mut Vec<Kernel>| {
        if let Some(k) = cur.take() {
            kernels.push(k);
        }
    };

    for raw in lines.iter() {
        let src = raw;
        let line = src.line;
        let text = src.text.trim();
        if text.is_empty() {
            continue; // 保序但忽略空行（空行不计物理数据）
        }
        // 防御性：剥除行注释（正常由 preprocess 处理）
        let text = match text.find("//") {
            Some(pos) => text[..pos].trim(),
            None => text,
        };
        if text.is_empty() {
            continue;
        }
        let toks: Vec<&str> = text.split_whitespace().collect();
        let first = toks[0];

        // 段/标签声明：检测“冒号结尾”或“冒号独立 token”。
        // 形式 A: `name :`（k=toks, symbol 在前）
        // 形式 B: `name:`（toks[0] 以 : 结尾）
        if let Some(label) = scan_label(&toks, first) {
            match toks.len() {
                1 => {
                    // 纯标签行（如 `name:`），挂到当前上下文
                    pending_label = Some(label);
                    continue;
                }
                2 if toks[1] == ":" => {
                    // `name :` 形式且仅声明
                    pending_label = Some(label);
                    continue;
                }
                _ => {
                    // 形式 A（冒号独立 token）: `name : .half …` / `name : .kernel …`
                    // 形式 B（冒号紧贴）   : `name: .half …` / `name: .kernel …`
                    // 决定指令 token 从哪个下标开始：A 时 toks[1]==":"、directive=toks[2]；
                    // B 时 directive=toks[1]（标签名以 ':' 结尾）。
                    let (directive, rest) = if toks[1] == ":" {
                        (toks[2], &toks[3..])
                    } else {
                        (toks[1], &toks[2..])
                    };
                    if directive == ".half" {
                        let vals = parse_half_values(&rest.join(" "), line)?;
                        let item = DataItem {
                            label: pending_label.take().or(Some(label.clone())),
                            kind: DataKind::Half(vals),
                            line,
                        };
                        if let Some(k) = cur.as_mut() {
                            k.data.push(item);
                        } else {
                            data.push(item);
                            in_data = true;
                        }
                        continue;
                    } else if directive == ".space" {
                        let n = parse_space_count(&rest.join(" "), line)?;
                        let item = DataItem {
                            label: pending_label.take().or(Some(label.clone())),
                            kind: DataKind::Space(n),
                            line,
                        };
                        if let Some(k) = cur.as_mut() {
                            k.data.push(item);
                        } else {
                            data.push(item);
                            in_data = true;
                        }
                        continue;
                    } else if directive == ".kernel" {
                        flush_kernel(&mut cur, &mut kernels);
                        let name = rest
                            .iter()
                            .next()
                            .map(|s| s.to_string())
                            .ok_or_else(|| AsmError::at(line, ".kernel 缺少名称"))?;
                        cur = Some(Kernel {
                            name,
                            line,
                            data: Vec::new(),
                            instrs: Vec::new(),
                        });
                        in_data = false;
                        pending_label = None;
                        continue;
                    }
                    return Err(AsmError::at(line, format!("无法识别的声明: {text}")));
                }
            }
        }

        // `.half` / `.space`（全局数据区，或 kernel 内局部静态区）
        if first == ".half" {
            let vals = parse_half_values(text[6..].trim(), line)?;
            let item = DataItem {
                label: pending_label.take(),
                kind: DataKind::Half(vals),
                line,
            };
            if let Some(k) = cur.as_mut() {
                k.data.push(item); // kernel 内局部静态区
            } else {
                data.push(item);
                in_data = true;
            }
            continue;
        }
        if first == ".space" {
            let n = parse_space_count(text[7..].trim(), line)?;
            let item = DataItem {
                label: pending_label.take(),
                kind: DataKind::Space(n),
                line,
            };
            if let Some(k) = cur.as_mut() {
                k.data.push(item); // kernel 内局部静态区
            } else {
                data.push(item);
                in_data = true;
            }
            continue;
        }
        if first == ".DATA" || first == ".data" {
            flush_kernel(&mut cur, &mut kernels);
            in_data = true;
            pending_label = None;
            continue;
        }
        if first == ".kernel" {
            flush_kernel(&mut cur, &mut kernels);
            let name = toks
                .get(1)
                .map(|s| s.to_string())
                .ok_or_else(|| AsmError::at(line, ".kernel 缺少名称"))?;
            cur = Some(Kernel {
                name,
                line,
                data: Vec::new(),
                instrs: Vec::new(),
            });
            in_data = false;
            pending_label = None;
            continue;
        }

        // 指令：首 token 应为助记符。若位于 .kernel 内但助记符未识别
        // （大写标识符裸行，如宏调用 THREAD_start），跳过并告警（MVP 决策）。
        if cur.is_some() && !in_data && !is_known_mnemonic(first) {
            eprintln!("[assembler] 警告: 行 {line}: 忽略未识别的助记符/宏调用: {first}");
            continue;
        }

        // 指令：首 token 为助记符（可能前有标签，标签已在上面处理）。
        // 若存在 pending_label，把它作为该指令的标签。
        let (label_from, rest_tokens) = if let Some(l) = pending_label.take() {
            (Some(l), toks)
        } else {
            (None, toks)
        };

        // 指令必须出现在 .kernel 内
        if cur.is_none() {
            return Err(AsmError::at(
                line,
                format!("指令 {first} 出现在 .kernel 之外"),
            ));
        }
        if in_data {
            return Err(AsmError::at(
                line,
                format!("指令 {first} 出现在 .DATA 段内"),
            ));
        }
        let mnemonic = first.to_string();
        let operands: Vec<String> = rest_tokens[1..].iter().map(|s| s.to_string()).collect();
        cur.as_mut().unwrap().instrs.push(Instr {
            label: label_from,
            mnemonic,
            operands,
            line,
        });
    }

    // 收尾
    flush_kernel(&mut cur, &mut kernels);
    // 末尾残留标签没有宿主（如文件末尾裸标签且无后续指令/数据）
    if pending_label.is_some() {
        // 允许：挂到第一节或忽略。此处忽略静默即可（不加指令的数据标签无意义）。
    }

    Ok(Program { data, kernels })
}

/// 尝试从行 token 中检测“标签：”形式。
/// 返回标签名；非标签行返回 None。
fn scan_label(toks: &[&str], first: &str) -> Option<String> {
    // 形式 B: first 以 ':' 结尾且去掉冒号非空
    if first.ends_with(':') {
        let name = first.trim_end_matches(':');
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    // 形式 A: `name :` — toks[0]=name, toks[1]=':'
    if toks.len() >= 2 && toks[1] == ":" {
        return Some(first.to_string());
    }
    None
}

/// 解析 `.half v1, v2, …` — 值之间用逗号**或**空格分隔均可，
/// 每个为表达式。
fn parse_half_values(s: &str, line: usize) -> Result<Vec<i64>> {
    if s.trim().is_empty() {
        return Err(AsmError::at(line, ".half 缺少数值"));
    }
    s.split(',')
        .flat_map(|part| part.split_whitespace())
        .map(|tok| eval(tok, line))
        .collect()
}

/// 解析 `.space N` — N 为表达式，取非负整数值。
fn parse_space_count(s: &str, line: usize) -> Result<u32> {
    let v = eval(s.trim(), line)?;
    if v < 0 || v > u32::MAX as i64 {
        return Err(AsmError::at(line, format!(".space 数量无效: {v}")));
    }
    Ok(v as u32)
}

#[cfg(test)]
mod tests_parse {
    use super::*;

    fn lines_of(v: Vec<&str>) -> Vec<SourceLine> {
        v.into_iter()
            .enumerate()
            .map(|(i, s)| SourceLine::new("<主文件>", i + 1, s))
            .collect()
    }

    #[test]
    fn parse_basic() {
        let lines = lines_of(vec![
            ".DATA",
            "mat_A : .half 1, 2, 3, 4",
            "mat_C : .space 4*2",
            "//注释",
            ".kernel mat_add :",
            "THREAD_start:",
            "BRA THREAD_start 0",
            "DONE",
        ]);
        let p = parse(&lines).unwrap();
        assert_eq!(p.data.len(), 2);
        assert_eq!(p.data[0].label.as_deref(), Some("mat_A"));
        if let DataKind::Half(v) = &p.data[0].kind {
            assert_eq!(v.len(), 4);
        } else {
            panic!("expected Half");
        }
        if let DataKind::Space(n) = &p.data[1].kind {
            assert_eq!(*n, 8);
        } else {
            panic!("expected Space");
        }
        assert_eq!(p.kernels.len(), 1);
        let k = &p.kernels[0];
        assert_eq!(k.name, "mat_add");
        assert_eq!(k.instrs.len(), 2);
        assert_eq!(k.instrs[0].label.as_deref(), Some("THREAD_start"));
        assert_eq!(k.instrs[0].mnemonic, "BRA");
    }

    #[test]
    fn skip_unknown_macro_call_in_kernel() {
        let lines = lines_of(vec![
            ".kernel mat_add :",
            "THREAD_start", // 宏调用，应跳过并告警
            "DONE",
        ]);
        let p = parse(&lines).unwrap();
        assert_eq!(p.kernels.len(), 1);
        // 宏调用行不应进入指令列表
        assert_eq!(p.kernels[0].instrs.len(), 1);
        assert_eq!(p.kernels[0].instrs[0].mnemonic, "DONE");
    }

    #[test]
    fn kernel_local_data_in_kernel() {
        let lines = lines_of(vec![
            ".DATA",
            "mat_A : .half 1, 2",
            ".kernel k :",
            "_param_ : .space 4",
            "_p2_ : .half 5, 6",
            "DONE",
        ]);
        let p = parse(&lines).unwrap();
        // 全局数据区不被 kernel 内数据污染
        assert_eq!(p.data.len(), 1);
        let k = &p.kernels[0];
        assert_eq!(k.data.len(), 2, "kernel 应有 2 个局部静态项: {k:?}");
        assert_eq!(k.data[0].label.as_deref(), Some("_param_"));
        if let DataKind::Space(n) = &k.data[0].kind {
            assert_eq!(*n, 4);
        } else {
            panic!("expected Space");
        }
        assert_eq!(k.instrs.len(), 1);
    }

    #[test]
    fn labels_with_optional_space_before_colon() {
        // 冒号与名称之间：允许多个或没有空格（紧贴 或 独立 token 均可）
        let lines = lines_of(vec![
            ".DATA",
            "V_a :  .half 1, 2, 3, 4", // 多个空格 + 独立冒号 token
            "V_b: .half 5, 6",         // 紧贴冒号 + 空格后跟指令
            "V_c :   .half 7",         // 独立冒号 + 多空格
            ".kernel k :",
            "tgt:",
            "BRA tgt 0",
            "DONE",
        ]);
        let p = parse(&lines).unwrap();
        assert_eq!(p.data.len(), 3, "三种冒号写法都应是数据项");
        assert_eq!(p.data[0].label.as_deref(), Some("V_a"));
        assert_eq!(p.data[1].label.as_deref(), Some("V_b"));
        assert_eq!(p.data[2].label.as_deref(), Some("V_c"));
        if let DataKind::Half(v) = &p.data[0].kind {
            assert_eq!(v.len(), 4);
        } else {
            panic!("expected Half");
        }
        let k = &p.kernels[0];
        assert_eq!(k.instrs.len(), 2);
        assert_eq!(k.instrs[0].label.as_deref(), Some("tgt"));
    }

    #[test]
    fn half_values_comma_or_space_separated() {
        // 逗号分隔 与 空格分隔 都应被接受
        let lines = lines_of(vec![
            ".DATA",
            "a: .half 1, 2, 3, 4", // 逗号分隔
            "b : .half 5 6 7",     // 空格分隔
            "c: .half 8,  9, 10",  // 逗号+多余空格
            ".kernel k :",
            "DONE",
        ]);
        let p = parse(&lines).unwrap();
        assert_eq!(p.data.len(), 3);
        if let DataKind::Half(v) = &p.data[0].kind {
            assert_eq!(v.len(), 4);
        } else {
            panic!("expected Half");
        }
        if let DataKind::Half(v) = &p.data[1].kind {
            assert_eq!(v.len(), 3, "空格分隔的 3 个值");
        } else {
            panic!("expected Half");
        }
        if let DataKind::Half(v) = &p.data[2].kind {
            assert_eq!(*v, vec![8, 9, 10]);
        } else {
            panic!("expected Half");
        }
    }
}
// ==================== 模块: layout ====================
// 内存布局与符号解析。
//
// 已确认的内存映像（半字寻址：一个地址 = 一个 16bit 半字，大端）：
// - 全局静态数据区从地址 0 起，`.half/.space` 连续；每个 16bit 半字占 1 个地址。
// - 每个 `.kernel` 内部：先局部静态区，再指令区；每条指令占 2 个半字地址（PC 步长 +2）。
// - 数据区标签 → 该数据起始地址；指令区标签 → 该标签下第一条指令地址。

/// 内存布局结果：整段连续 16bit 字 + 符号表（标签 → 字节地址）。
#[derive(Debug)]
pub struct Layout {
    /// 数据区字。
    pub data_words: Vec<u16>,
    /// 指令区字（每条指令 1 个 24bit 已按字打包见 encode；此处为 16bit 表达前原始值待 encode）。
    /// 实际：指令编码为 24bit，最终映像按跨两单元展开在 `bin/buf` 层处理。
    pub instr_words: Vec<u16>,
    /// 符号表：标签 → 字节地址。
    pub symbols: HashMap<String, u16>,
}

/// 逐条指令的地址分配结果（供 encode 收集分支目标）。
#[derive(Debug)]
pub struct InstrAddr {
    pub label: Option<String>,
    pub addr: u16, // 该指令字节地址
    pub line: usize,
    /// 指令在 Program 中的内核索引与内核内序号（供写入返回给 encode）。
    pub kernel_idx: usize,
    pub seq_in_kernel: usize,
}

/// 布局：扫描 `.DATA` 全局数据区 + 各 `.kernel`（局部静态区 在前、指令区 在后），
/// 分配地址并建立符号表。
///
/// 返回三元组：
/// - `全局符号表`（`.DATA` 全局标签 + 各 kernel 指令标签）
/// - `每 kernel 的局部静态符号表`（`kernel.data` 里 `.half`/`.space` 的标签，仅在该 kernel 内可见）
/// - 逐条指令的地址/标签元数据
///
/// 内存映像顺序（半字寻址，一个地址 = 一个 16bit 半字）：
/// 全局 `.DATA` 区 → kernel0 局部静态区 → kernel0 指令 → kernel1 局部静态区 → …
/// 每条指令占 2 个半字地址（PC 步长 +2）。
#[allow(clippy::type_complexity)]
pub fn layout(
    prog: &Program,
) -> Result<(
    HashMap<String, u16>,
    Vec<HashMap<String, u16>>,
    Vec<InstrAddr>,
)> {
    let mut symbols: HashMap<String, u16> = HashMap::new();
    let mut addr: u32 = 0;

    // 1) 全局数据区（`prog.data`）→ 全局符号表
    for item in &prog.data {
        insert_data(&mut symbols, &mut addr, item)?;
    }

    // 2) 各 kernel：先局部静态区（标签入该 kernel 局部表），再指令区（标签入全局表）
    let mut instr_addrs: Vec<InstrAddr> = Vec::new();
    let mut kernel_locals: Vec<HashMap<String, u16>> = Vec::new();
    for (ki, kernel) in prog.kernels.iter().enumerate() {
        // 2a) kernel 局部静态区 → 局部符号表
        let mut local: HashMap<String, u16> = HashMap::new();
        for item in &kernel.data {
            insert_data(&mut local, &mut addr, item)?;
        }
        // 2b) kernel 指令区（每条指令占 2 个半字地址），指令标签入全局表
        for (si, instr) in kernel.instrs.iter().enumerate() {
            if let Some(label) = &instr.label {
                if addr > 0xFFFF {
                    return Err(AsmError::at(
                        instr.line,
                        "指令区地址超过 16bit 半字地址上限",
                    ));
                }
                insert_symbol(&mut symbols, label, addr as u16, instr.line)?;
            }
            instr_addrs.push(InstrAddr {
                label: instr.label.clone(),
                addr: addr as u16,
                line: instr.line,
                kernel_idx: ki,
                seq_in_kernel: si,
            });
            addr = addr
                .checked_add(2)
                .ok_or_else(|| AsmError::at(instr.line, "指令区地址溢出"))?;
            if addr > 0x10000 {
                return Err(AsmError::at(instr.line, "指令区超出半字地址空间"));
            }
        }
        kernel_locals.push(local);
    }

    Ok((symbols, kernel_locals, instr_addrs))
}

/// 为一个数据项（`.half`/`.space`，可能带标签）分配地址。
fn insert_data(symbols: &mut HashMap<String, u16>, addr: &mut u32, item: &DataItem) -> Result<()> {
    if let Some(label) = &item.label {
        if *addr > 0xFFFF {
            return Err(AsmError::at(
                item.line,
                "数据区地址超过 16bit 半字地址上限 (65535)",
            ));
        }
        insert_symbol(symbols, label, *addr as u16, item.line)?;
    }
    let words = match &item.kind {
        DataKind::Half(vals) => vals.len() as u32,
        DataKind::Space(n) => *n,
    };
    *addr = addr
        .checked_add(words)
        .ok_or_else(|| AsmError::at(item.line, "数据区地址溢出"))?;
    if *addr > 0x10000 {
        return Err(AsmError::at(item.line, "数据区超出半字地址空间 (65536)"));
    }
    Ok(())
}

fn insert_symbol(
    symbols: &mut HashMap<String, u16>,
    label: &str,
    addr: u16,
    line: usize,
) -> Result<()> {
    if symbols.contains_key(label) {
        return Err(AsmError::at(line, format!("重复符号: {label}")));
    }
    symbols.insert(label.to_string(), addr);
    Ok(())
}

#[cfg(test)]
mod tests_layout {
    use super::*;
    fn half_prog() -> Program {
        Program {
            data: vec![
                DataItem {
                    label: Some("mat_A".into()),
                    kind: DataKind::Half(vec![1, 2, 3, 4]),
                    line: 2,
                },
                DataItem {
                    label: Some("mat_C".into()),
                    kind: DataKind::Space(8),
                    line: 4,
                },
            ],
            kernels: vec![Kernel {
                name: "k".into(),
                line: 6,
                data: vec![],
                instrs: vec![
                    Instr {
                        label: Some("loop".into()),
                        mnemonic: "DONE".into(),
                        operands: vec![],
                        line: 7,
                    },
                    Instr {
                        label: None,
                        mnemonic: "NOP".into(),
                        operands: vec![],
                        line: 8,
                    },
                ],
            }],
        }
    }
    #[test]
    fn addr_calc_halfword() {
        let p = half_prog();
        let (sym, _ks, ia) = layout(&p).unwrap();
        // 半字寻址：.half 4 值占 4 个半字地址(0..3)；.space 8 占 8 个(4..11)；指令区 @12
        assert_eq!(sym["mat_A"], 0);
        assert_eq!(sym["mat_C"], 4);
        assert_eq!(sym["loop"], 12);
        assert_eq!(ia[0].addr, 12);
        assert_eq!(ia[1].addr, 14); // 每条指令占 2 个半字地址
    }

    #[test]
    fn kernel_local_data_before_instr() {
        let p = Program {
            data: vec![DataItem {
                label: Some("g".into()),
                kind: DataKind::Half(vec![1, 2]),
                line: 1,
            }],
            kernels: vec![Kernel {
                name: "k".into(),
                line: 3,
                data: vec![
                    DataItem {
                        label: Some("loc".into()),
                        kind: DataKind::Space(4),
                        line: 4,
                    },
                    DataItem {
                        label: Some("loc2".into()),
                        kind: DataKind::Half(vec![9, 8]),
                        line: 5,
                    },
                ],
                instrs: vec![Instr {
                    label: Some("start".into()),
                    mnemonic: "DONE".into(),
                    operands: vec![],
                    line: 6,
                }],
            }],
        };
        let (sym, ks, ia) = layout(&p).unwrap();
        // 全局数据区 g 占地址 0..1；kernel 局部静态区 loc@2 (4 半字) loc2@6 (2 半字) 入局部表；指令 @8
        assert_eq!(sym["g"], 0);
        assert_eq!(ks[0]["loc"], 2);
        assert_eq!(ks[0]["loc2"], 6);
        assert_eq!(sym["start"], 8);
        assert_eq!(ia[0].addr, 8);
    }
}
// ==================== 模块: output ====================
// 输出：构建整段连续内存的字映像，并渲染为简易文本十六进制文件。
//
// 内存映像（权威 `docs/memory/memory_spec.md` §B.3/§B.5.2）：
// - 数据区：每个 `.half` 值 = 1 个半字（16bit）；`.space N` = N 个 0 半字。
// - 指令区：每条 24bit 指令占 **2 个半字**：
//   - 低地址单元（半字 `2k`）= `I[23:8]`（高 16 位）
//   - 高地址单元（半字 `2k+1`）= `I[7:0] << 8`（低 8 位置于高 8 位、低 8 位置 0）
// - 文本按半字（16bit）列出（每项 4 位 hex，文本即半字值），每行 8 项；
//   地址列为**半字地址**，每项半字 = 1 个地址，每行 +8。

/// 把一条 24bit 指令展开为 2 个半字（低地址半字 = I[23:8]，高地址半字 = I[7:0]<<8）。
pub fn instr_words(instr: u32) -> [u16; 2] {
    let hi = ((instr >> 8) & 0xFFFF) as u16; // I[23:8]
    let lo = ((instr & 0xFF) as u16) << 8; // I[7:0] 放高 8 位
    [hi, lo]
}

/// 构建整段内存字映像：数据区半字 + 指令区半字。
///
/// - `data_words`：数据区半字序列（已展开 `.half`/`.space`）。
/// - `instrs`：指令区每条的 24bit 编码（按程序顺序）。
pub fn build_image(data_words: &[u16], instrs: &[u32]) -> Vec<u16> {
    let mut words = Vec::with_capacity(data_words.len() + instrs.len() * 2);
    words.extend_from_slice(data_words);
    for &instr in instrs {
        let [hi, lo] = instr_words(instr);
        words.push(hi);
        words.push(lo);
    }
    words
}

/// 将字映像渲染为简易文本十六进制。
///
/// 每行 8 个半字（16bit）项即 8 个地址，下一行地址 +8：
/// ```text
/// 0000: 1234 5678 0000 0000 0000 0000 0000 0000
/// 0008: ...
/// ```
pub fn render(words: &[u16]) -> Result<String> {
    let mut out = String::new();
    for (i, chunk) in words.chunks(8).enumerate() {
        let addr = i * 8; // 半字寻址：每行 8 个半字 = 8 个地址
        write!(&mut out, "{addr:04X}:").unwrap();
        for w in chunk {
            write!(&mut out, " {w:04X}").unwrap();
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests_output {
    use super::*;

    #[test]
    fn image_layout() {
        // 数据 2 半字 + 1 条指令(0x0060_1800)
        let img = build_image(&[0x1111, 0x2222], &[0x0060_1800]);
        assert_eq!(img.len(), 4);
        assert_eq!(img[0], 0x1111);
        assert_eq!(img[1], 0x2222);
        // I=0x601800: I[23:8]=0x6018, I[7:0]=0x00
        assert_eq!(img[2], 0x6018);
        assert_eq!(img[3], 0x0000);
    }

    #[test]
    fn render_lines_halfword() {
        let words: Vec<u16> = (0..10).map(|i| i as u16).collect();
        let s = render(&words).unwrap();
        let lines: Vec<&str> = s.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("0000:")); // 8 项 = 8 个半字地址
        assert!(lines[1].starts_with("0008:")); // 下一行 +8
    }
}
// ==================== 模块: report ====================
// 编译成功时向 stdout 打印的诊断报告（用户 m00880 需求 + ASCII 表格优化）。
//
// 三段内容（不新建文件、不并入 `.hex`，仅成功时打印）：
// 1. **内存结构**：区域表格（区域 / 起始 / 结束 / 大小）。
// 2. **宏列表**：宏定义表 + 宏调用展开点表。
// 3. **标签列表**：标签 / 地址表（半字地址十六进制）
//
// 地址口径：**半字寻址**，一个地址 = 一个 16bit 半字；每条指令占 2 个地址。

/// 一个内存区域（用于 ① 内存结构报告）。
#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    /// 起始半字地址（含）。
    pub start: u32,
    /// 结束半字地址（不含）。
    pub end: u32,
}

impl Region {
    pub fn new(name: impl Into<String>, start: u32, end: u32) -> Self {
        Region {
            name: name.into(),
            start,
            end,
        }
    }

    /// 区域大小（半字数）。
    pub fn size(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

/// 报告所需的全部数据。
pub struct ReportInput<'a> {
    pub regions: &'a [Region],
    pub table: &'a DefineTable,
    pub expansion: &'a Expansion,
    /// 最终符号表：标签 → 半字地址。
    pub symbols: &'a HashMap<String, u16>,
    /// 程序总半字数（内存大小）。
    pub total: usize,
}

/// 生成完整报告文本（三段）。
pub fn render_report(input: &ReportInput<'_>) -> String {
    let mut s = String::new();
    s.push_str(&render_memory(input.regions, input.total));
    s.push('\n');
    s.push_str(&render_macros(input.table, input.expansion));
    s.push('\n');
    s.push_str(&render_labels(input.symbols));
    s
}

// ------------------------- ASCII 表格渲染 -------------------------

/// 依对齐渲染一个 ASCII 表格。
/// `align`: 每列对齐，`L`=左对齐、`R`=右对齐。
///
/// 列宽按**显示宽度**计算：CJK 全角字符在等宽终端占 2 列，其余占 1 列。
/// 否则中文单元格会溢出，导致右侧边框行行错位。
fn render_ascii_table(header: &[&str], rows: &[Vec<String>], align: &[char]) -> String {
    assert_eq!(header.len(), align.len());
    if rows.iter().any(|r| r.len() != header.len()) {
        panic!("表格列数与表头不一致");
    }
    let cols = header.len();
    let widths: Vec<usize> = (0..cols)
        .map(|c| {
            let hw = display_width(header[c]);
            rows.iter()
                .map(|r| display_width(&r[c]))
                .fold(hw, usize::max)
        })
        .collect();

    let draw_border = |s: &mut String| {
        s.push('+');
        for w in &widths {
            s.push_str(&"-".repeat(w + 2));
            s.push('+');
        }
        s.push('\n');
    };

    let draw_row = |s: &mut String, cells: &[String]| {
        s.push('|');
        for (c, cell) in cells.iter().enumerate() {
            let pad = widths[c].saturating_sub(display_width(cell));
            if align[c] == 'L' {
                let _ = write!(s, " {cell} {}|", " ".repeat(pad));
            } else {
                let _ = write!(s, " {} {cell}|", " ".repeat(pad));
            }
        }
        s.push('\n');
    };

    let mut s = String::new();
    draw_border(&mut s);
    draw_row(
        &mut s,
        &header.iter().map(|h| h.to_string()).collect::<Vec<_>>(),
    );
    draw_border(&mut s);
    for r in rows {
        draw_row(&mut s, r);
    }
    draw_border(&mut s);
    s
}

/// 终端**显示宽度**（列数）：
/// - 零宽字符（组合附加符、零宽空格、变体选择符）占 0；
/// - 宽字符（CJK 统一表意文字、假名、谚文、全角标点/符号、表情符号等）占 2；
/// - 其余占 1。
pub fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if (0x0300..=0x036F).contains(&u)   // 组合用重音附加符
                || (0x200B..=0x200F).contains(&u) // 零宽空格/控制
                || (0xFE00..=0xFE0F).contains(&u) // 变体选择符
                || u == 0
            {
                0
            } else if is_wide(u) {
                2
            } else {
                1
            }
        })
        .sum()
}

/// 是否为占 2 列的宽字符。
fn is_wide(u: u32) -> bool {
    matches!(u,
        0x1100..=0x115F       // 谚文音节字母
        | 0x2E80..=0x303E    // CJK 部首补充 .. CJK 符号和标点
        | 0x3041..=0x33FF    // 平假名、片假名、注音、CJK 兼容
        | 0x3400..=0x4DBF    // CJK 扩展 A
        | 0x4E00..=0x9FFF    // CJK 统一表意文字
        | 0xA000..=0xA4CF    // 彝文
        | 0xAC00..=0xD7A3    // 谚文音节
        | 0xF900..=0xFAFF    // CJK 兼容表意文字
        | 0xFE10..=0xFE19    // 竖排标点
        | 0xFE30..=0xFE6F    // CJK 兼容形式、小写变体
        | 0xFF00..=0xFF60    // 全角 ASCII
        | 0xFFE0..=0xFFE6    // 全角符号
        | 0x1F300..=0x1F64F  // 表情符号
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x2FFFD  // CJK 扩展 B 及以后
        | 0x30000..=0x3FFFD
    )
}

// ------------------------- ① 内存结构 -------------------------

/// ① 内存结构：区域 ASCII 表格。
pub fn render_memory(regions: &[Region], total: usize) -> String {
    let mut s = String::new();
    s.push_str("=== 内存结构 ===\n");
    let header = ["区域", "起始", "结束", "大小(半字)"];
    let rows: Vec<Vec<String>> = regions
        .iter()
        .map(|r| {
            if r.size() == 0 {
                vec![r.name.clone(), "—".into(), "—".into(), "0".into()]
            } else {
                vec![
                    r.name.clone(),
                    format!("{:04X}", r.start),
                    format!("{:04X}", r.end),
                    format!("{}", r.size()),
                ]
            }
        })
        .collect();
    let mut all = rows.clone();
    all.push(vec![
        "总计".into(),
        String::new(),
        String::new(),
        format!("{total}"),
    ]);
    s.push_str(&render_ascii_table(&header, &all, &['L', 'R', 'R', 'R']));
    s
}

// ------------------------- ② 宏列表 -------------------------

/// ② 宏列表：定义 + 调用展开点（ASCII 表格）。
pub fn render_macros(table: &DefineTable, expansion: &Expansion) -> String {
    let mut s = String::new();
    s.push_str("=== 宏列表 ===\n");
    if table.macros.is_empty() && expansion.calls.is_empty() {
        s.push_str("(无宏定义/调用)\n");
        return s;
    }

    if !table.macros.is_empty() {
        s.push_str("--- 宏定义 ---\n");
        let header = ["宏", "参数", "定义位置"];
        let rows: Vec<Vec<String>> = table
            .macros
            .iter()
            .map(|m| {
                let params = if m.params.is_empty() {
                    String::new()
                } else {
                    m.params.join(", ")
                };
                vec![
                    format!(".macro {}", m.name),
                    params,
                    format!("{}:{}", m.file, m.line),
                ]
            })
            .collect();
        s.push_str(&render_ascii_table(&header, &rows, &['L', 'L', 'L']));
    }

    if !expansion.calls.is_empty() {
        s.push_str("--- 宏调用展开点 ---\n");
        let header = ["宏", "调用位置", "展开自", "行流段"];
        let rows: Vec<Vec<String>> = expansion
            .calls
            .iter()
            .map(|c| {
                vec![
                    c.name.clone(),
                    format!("{}:{}", c.call_file, c.call_line),
                    format!("{}:{}", c.def_file, c.def_line),
                    format!("{}..{}", c.start, c.end),
                ]
            })
            .collect();
        s.push_str(&render_ascii_table(&header, &rows, &['L', 'L', 'L', 'R']));
    }
    s
}

// ------------------------- ③ 标签列表 -------------------------

/// ③ 标签列表：标签 / 地址（ASCII 表格），按地址升序。
pub fn render_labels(symbols: &HashMap<String, u16>) -> String {
    let mut rows: Vec<(&String, u16)> = symbols.iter().map(|(k, v)| (k, *v)).collect();
    rows.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(b.0)));
    let mut s = String::new();
    s.push_str("=== 标签列表 ===\n");
    if rows.is_empty() {
        s.push_str("(无标签)\n");
        return s;
    }
    let header = ["标签", "地址"];
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|(n, a)| vec![(*n).clone(), format!("{a:04X}")])
        .collect();
    s.push_str(&render_ascii_table(&header, &data, &['L', 'R']));
    s
}

#[cfg(test)]
mod tests_report {
    use super::*;

    fn src(text: &str) -> SourceLine {
        SourceLine::new("<主文件>", 1, text)
    }

    #[test]
    fn memory_report_two_regions() {
        let regions = vec![
            Region::new("全局数据区 (.DATA)", 0, 4),
            Region::new("kernel0 指令区:mat_add", 4, 8),
        ];
        let out = render_memory(&regions, 8);
        assert!(out.contains("=== 内存结构 ==="), "got:\n{out}");
        // ASCII 表格表头
        assert!(out.contains("| 区域"), "got:\n{out}");
        // 两行数据 + 总计行都在表格内
        assert!(out.contains("全局数据区 (.DATA)"), "got:\n{out}");
        assert!(out.contains("0004"), "got:\n{out}");
        assert!(out.contains("kernel0 指令区:mat_add"), "got:\n{out}");
        assert!(out.contains("总计"), "got:\n{out}");
        assert!(out.contains('+') && out.contains('-'), "got:\n{out}");
    }

    #[test]
    fn memory_report_empty_region_shows_dash() {
        let regions = vec![Region::new("kernel0 局部静态区:k", 6, 6)];
        let out = render_memory(&regions, 8);
        assert!(out.contains('—'), "got:\n{out}"); // 空区域用 — 占位
    }

    #[test]
    fn macro_report_def_and_call() {
        let table = DefineTable {
            constants: vec![ConstantDef {
                name: "_gpu_thread_num_".into(),
                value: "16".into(),
                file: "gpu_core.inc".into(),
                line: 5,
            }],
            macros: vec![MacroDef {
                name: "_mem_push_".into(),
                params: vec!["rs".into()],
                body: vec![src("PUSH \\(rs)")],
                file: "gpu_core.inc".into(),
                line: 116,
            }],
        };
        let expansion = Expansion {
            lines: vec![src("PUSH r3")],
            calls: vec![MacroCallRecord {
                name: "_mem_push_".into(),
                def_file: "gpu_core.inc".into(),
                def_line: 116,
                call_file: "<主文件>".into(),
                call_line: 9,
                start: 0,
                end: 1,
            }],
        };
        let out = render_macros(&table, &expansion);
        assert!(out.contains("--- 宏定义 ---"), "got:\n{out}");
        assert!(out.contains(".macro _mem_push_"), "got:\n{out}");
        assert!(out.contains("gpu_core.inc:116"), "got:\n{out}");
        assert!(out.contains("--- 宏调用展开点 ---"), "got:\n{out}");
        assert!(out.contains("_mem_push_"), "got:\n{out}");
        assert!(out.contains("<主文件>:9"), "got:\n{out}");
        assert!(out.contains("0..1"), "got:\n{out}");
    }

    #[test]
    fn labels_sorted_by_addr() {
        let mut syms: HashMap<String, u16> = HashMap::new();
        syms.insert("loop".into(), 6);
        syms.insert("mat_A".into(), 0);
        syms.insert("done".into(), 12);
        let out = render_labels(&syms);
        let idx = |n: &str| out.find(n).unwrap();
        assert!(idx("mat_A") < idx("loop"));
        assert!(idx("loop") < idx("done"));
        assert!(out.contains("mat_A") && out.contains("0000"), "got:\n{out}");
        assert!(out.contains("loop") && out.contains("0006"), "got:\n{out}");
        assert!(out.contains("done") && out.contains("000C"), "got:\n{out}");
        assert!(out.contains("| 标签"), "got:\n{out}");
    }
}
// ==================== 模块: lib（Artifact/compile） ====================
// GPU 汇编编译器 MVP（Rust 纯 std）。
//
// 功能：把 `.asm` 汇编源文本译成一段连续半字内存（半字寻址：一个地址 = 16bit 半字；
// 每条 24bit 指令占 2 个半字地址），并以简易文本格式输出
// （每行 `aaaa: wwwww ...`，每行 8 个半字，下一行地址 +8）。
//
// 模块划分：
// - [`ast`]：抽象语法树类型。
// - [`source`]：带源码位置（文件 + 行号）的源码行。
// - [`preprocess`]：去注释、`#include` 展开。
// - [`expr`]：算术表达式求值（四则 + 括号 + 前缀）。
// - [`parse`]：分词与建 AST（数据段 / kernel / 标签 / 指令）。
// - [`layout`]：两遍汇编第一遍 —— 布局数据与代码、建立符号表（标签→半字地址）。
// - [`encode`]：单条指令 → 24bit 编码（引用符号表解析分支/立即数）。
// - [`output`]：半字镜像 → 简易文本 hex。
// - [`error`]：汇编错误类型。

/// 一次汇编的完整产物（供 stdout 报告使用）。
pub struct Artifact {
    /// 整段连续内存的半字镜像。
    pub words: Vec<u16>,
    /// 最终符号表：标签 → 半字地址。
    pub symbols: HashMap<String, u16>,
    /// 内存区域（① 报告用）。
    pub regions: Vec<Region>,
    /// `.define`/`.macro` 定义表（② 报告用）。
    pub table: DefineTable,
    /// 宏展开结果（② 报告用：调用展开点）。
    pub expansion: Expansion,
}

/// 完整汇编（含报告数据）。`assemble` 是其薄封装。
pub fn compile(source: &str, base_dir: &Path) -> Result<Artifact> {
    let lines = preprocess(source, base_dir)?;

    // 宏系统：收集 .define/.macro 定义表，把 .DEFINE 段剥离出待展开程序行流
    let (table, program_lines) = collect(&lines)?;
    let expansion = expand(&table, &program_lines)?;
    let expanded = &expansion.lines;

    let program = parse(expanded)?;

    // 布局：数据 + 指令地址，建立符号表（全局 + 各 kernel 局部静态符号表）
    let (symbols, kernel_locals, _instr_addrs) = layout(&program)?;

    // 逐区域构建字镜像 + 记录内存区域（半字寻址）
    let mut words: Vec<u16> = Vec::new();
    let mut regions: Vec<Region> = Vec::new();
    let mut addr: usize = 0;

    // 全局数据区
    let mut start = addr;
    for item in &program.data {
        push_data(&mut words, item)?;
    }
    addr = words.len();
    regions.push(Region::new("全局数据区 (.DATA)", start as u32, addr as u32));

    // 逐 kernel
    for (ki, kernel) in program.kernels.iter().enumerate() {
        // kernel 局部静态区
        start = addr;
        for item in &kernel.data {
            push_data(&mut words, item)?;
        }
        addr = words.len();
        regions.push(Region::new(
            format!("kernel{} 局部静态区:{}", ki, kernel.name),
            start as u32,
            addr as u32,
        ));
        // kernel 指令区：编码时用「全局符号表 + 本 kernel 局部静态符号表」合并
        let mut syms = symbols.clone();
        if let Some(local) = kernel_locals.get(ki) {
            for (k, v) in local {
                syms.insert(k.clone(), *v);
            }
        }
        start = addr;
        for inst in &kernel.instrs {
            let [hi, lo] = instr_words(encode_instr(inst, &syms)?);
            words.push(hi);
            words.push(lo);
        }
        addr = words.len();
        regions.push(Region::new(
            format!("kernel{} 指令区:{}", ki, kernel.name),
            start as u32,
            addr as u32,
        ));
    }

    // 报告用符号表 = 全局 + 所有 kernel 局部静态符号合并
    let mut all_symbols = symbols.clone();
    for local in &kernel_locals {
        for (k, v) in local {
            all_symbols.entry(k.clone()).or_insert(*v);
        }
    }

    Ok(Artifact {
        words,
        symbols: all_symbols,
        regions,
        table,
        expansion,
    })
}

/// 把一个数据项（`.half`/`.space`）展开为半字序列。
fn push_data(words: &mut Vec<u16>, item: &DataItem) -> Result<()> {
    match &item.kind {
        DataKind::Half(vals) => {
            for v in vals {
                words.push(to_u16(*v, item.line)?);
            }
        }
        DataKind::Space(n) => {
            let n = *n as usize;
            if words.len().checked_add(n).is_none() {
                return Err(AsmError::global("数据区字数量溢出"));
            }
            words.resize(words.len() + n, 0);
        }
    }
    Ok(())
}

/// 把 i64 值截断/校验为 16bit 无符号字。
fn to_u16(v: i64, line: usize) -> Result<u16> {
    if !(-0x8000..=0xFFFF).contains(&v) {
        return Err(AsmError::at(line, format!("值超出 16bit: {v}")));
    }
    Ok(v as u16)
}
// ==================== 入口: main ====================
// 命令行入口：`assembler <输入.asm> <输出.hex>`。
//
// 编译产物为简易文本十六进制（每行 `地址: 8×16bit 字`）。
// 错误与告警输出到 stderr；成功退出码 0，失败非 0。

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!(
            "用法: {} <输入.asm> <输出.hex>",
            args.first().map(|s| s.as_str()).unwrap_or("assembler")
        );
        eprintln!("例如: assembler asm.asm out.hex");
        return ExitCode::from(2);
    }
    let in_path = PathBuf::from(&args[1]);
    let out_path = PathBuf::from(&args[2]);

    let source = match std::fs::read_to_string(&in_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("无法读取输入文件 {}: {e}", in_path.display());
            return ExitCode::from(1);
        }
    };
    let base_dir = in_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    let artifact = match compile(&source, &base_dir) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("汇编失败: {e}");
            return ExitCode::from(1);
        }
    };

    let text = match render(&artifact.words) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("输出渲染失败: {e}");
            return ExitCode::from(1);
        }
    };

    if let Err(e) = std::fs::write(&out_path, text) {
        eprintln!("无法写入输出文件 {}: {e}", out_path.display());
        return ExitCode::from(1);
    }

    // 编译成功：向 stdout 打印三段诊断报告（m00880 需求）
    let rep = render_report(&ReportInput {
        regions: &artifact.regions,
        table: &artifact.table,
        expansion: &artifact.expansion,
        symbols: &artifact.symbols,
        total: artifact.words.len(),
    });
    print!("{rep}");

    eprintln!(
        "汇编完成: {} 个 16bit 字, 输出 {}",
        artifact.words.len(),
        out_path.display()
    );
    ExitCode::SUCCESS
}
