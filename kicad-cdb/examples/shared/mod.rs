//! kicad-cdb 示例程序共用的小工具: 极简命令行参数解析、读板。
//!
//! 每个示例文件用 `#[path = "shared/mod.rs"] mod shared;` 引入; 各示例只用得到其中一部分,
//! 所以整个模块不报"未使用"。Cargo 不会把本目录当成示例 (没有 `main.rs`)。
#![allow(dead_code)]

use kicad_cdb::stitching::StitchConfig;
use kicad_json5::ir::board::Board;

/// 极简命令行参数. 约定: 不以 `--` 开头的是位置参数 (第一个是板文件); 声明过的开关 (`--all`) 不带值;
/// 其余 `--opt` 带一个值.
#[derive(Debug, Default)]
pub struct Args {
    /// 位置参数, 第一个通常是板文件
    pub positional: Vec<String>,
    switches: &'static [&'static str],
    on: Vec<String>,
    values: Vec<(String, String)>,
}

impl Args {
    /// `switches` 列出所有不带值的开关名.
    pub fn parse(switches: &'static [&'static str]) -> Self {
        let mut args = Args {
            switches,
            ..Default::default()
        };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < argv.len() {
            let a = &argv[i];
            if !a.starts_with("--") {
                args.positional.push(a.clone());
                i += 1;
            } else if switches.contains(&a.as_str()) {
                args.on.push(a.clone());
                i += 1;
            } else {
                let Some(value) = argv.get(i + 1) else {
                    fail(format!("{} 后面少了一个值", a));
                };
                args.values.push((a.clone(), value.clone()));
                i += 2;
            }
        }
        args
    }

    /// 板文件路径 (第一个位置参数); 没给就打印用法并以退出码 2 结束.
    pub fn board_path_or_exit(&self, usage: &str) -> String {
        match self.positional.first() {
            Some(p) => p.clone(),
            None => {
                eprintln!("usage: {usage}");
                std::process::exit(2);
            }
        }
    }

    /// 第 `n` 个位置参数 (从 0 数); 没给就打印用法并以退出码 2 结束.
    pub fn arg(&self, n: usize, what: &str, usage: &str) -> String {
        match self.positional.get(n) {
            Some(p) => p.clone(),
            None => {
                eprintln!("usage: {usage}\n缺少 {what}");
                std::process::exit(2);
            }
        }
    }

    pub fn has(&self, flag: &str) -> bool {
        self.on.iter().any(|s| s == flag)
    }

    pub fn value(&self, opt: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == opt)
            .map(|(_, v)| v.as_str())
    }

    /// 带值的选项; 没给用默认值, 给了但不是合法数字就报错退出.
    pub fn f64_or(&self, opt: &str, default: f64) -> f64 {
        match self.value(opt) {
            None => default,
            Some(v) => v
                .parse()
                .unwrap_or_else(|_| fail(format!("{} 要一个数字, 收到 {v:?}", opt))),
        }
    }

    /// 同 [`Args::f64_or`], 但没给时返回 `None`.
    pub fn f64_opt(&self, opt: &str) -> Option<f64> {
        self.value(opt).map(|v| {
            v.parse()
                .unwrap_or_else(|_| fail(format!("{} 要一个数字, 收到 {v:?}", opt)))
        })
    }

    /// 同 [`Args::f64_or`], 整数版.
    pub fn usize_or(&self, opt: &str, default: usize) -> usize {
        match self.value(opt) {
            None => default,
            Some(v) => v
                .parse()
                .unwrap_or_else(|_| fail(format!("{} 要一个非负整数, 收到 {v:?}", opt))),
        }
    }

    /// 这几个互斥的选项里**最后给出的**那个, 返回 (选项名, 值).
    pub fn last_of(&self, opts: &[&str]) -> Option<(&str, &str)> {
        self.values
            .iter()
            .rev()
            .find(|(k, _)| opts.contains(&k.as_str()))
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// 逗号分隔的列表 (`--nets CLK,USB`); 没给返回空表.
    pub fn list(&self, opt: &str) -> Vec<String> {
        self.value(opt)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 逗号分隔的 `名字=值` 列表 (`--net VCC=1.5,GND=2`); 没给返回空表.
    pub fn pairs(&self, opt: &str) -> Vec<(String, String)> {
        self.value(opt)
            .map(|v| {
                v.split(',')
                    .map(|kv| kv.trim())
                    .filter(|kv| !kv.is_empty())
                    .map(|kv| match kv.split_once('=') {
                        Some((k, v)) => (k.trim().to_string(), v.trim().to_string()),
                        None => fail(format!("{} 要 NAME=VALUE 的逗号列表, 收到 {kv:?}", opt)),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// 读文本文件; 读不了就报错退出 (退出码 1).
pub fn read_text(what: &str, path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| fail(format!("读不了{what} {path:?}: {e}")))
}

/// 读 `.kicad_pcb` 并解析成 IR; 读不了或解析不了就报错退出 (退出码 1).
pub fn load_board(path: &str) -> Board {
    let text = read_text("板文件", path);
    kicad_json5::parse_board(&text)
        .unwrap_or_else(|e| fail(format!("解析板文件 {path:?} 失败: {e}")))
}

/// `StitchConfig` 的公共开关: `--all` (全部网都当高速网) / `--nets A,B` (按名字挑).
pub fn stitch_config(args: &Args) -> StitchConfig {
    let mut cfg = StitchConfig {
        all_nets: args.has("--all"),
        ..Default::default()
    };
    let nets = args.list("--nets");
    if !nets.is_empty() {
        cfg.net_patterns = nets;
    }
    cfg
}

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1)
}
