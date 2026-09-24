// 32k 行布线/布局引擎的内部构造器形态: 领域性结构 lint 放行(注释注明),
// 其余 lint 全部对齐 CI 门禁。
#![allow(clippy::type_complexity)] // 布局/布线内部构建器承载多维坐标元组
#![allow(clippy::too_many_arguments)]
// 布线求解器上下文按显式参数传递
// 以下为存量风格债的定向放行(逐类清点, 收敛后逐项摘除):
#![allow(clippy::needless_range_loop)] // 位标/网格索引循环按坐标语义书写
#![allow(clippy::collapsible_match)] // 解析器分支树保持展开形态
#![allow(clippy::if_same_then_else)] // 占位分支(两臂暂同), 语义待分化
#![allow(clippy::manual_strip)] // 手工切片解析外部报告行(erc/drc 文本)
#![allow(clippy::doc_overindented_list_items)] // 文档列表沿用旧缩进

pub mod audit;
pub mod bom;
pub mod composition;
pub mod config;
pub mod csv_import;
pub mod db;
pub mod design;
pub mod design_iteration;
pub mod design_review;
pub mod drc;
pub mod erc;
pub mod erc_vis;
pub mod explore;
pub mod footprint;
pub mod footprint_check;
pub mod footprint_lib;
pub mod gpu_router;
pub mod hqapi;
pub mod ic_knowledge;
pub mod ic_template;
pub mod import;
pub mod layer_config;
pub mod layout_directives;
pub mod layout_engine;
pub mod loop_audit;
pub mod models;
pub mod netlist;
pub mod pipeline;
pub mod power_tree;
pub mod query;
pub mod requirement;
pub mod router;
pub mod rules;
pub mod schema;
pub mod service;
pub mod silk;
pub mod skill_comp;
pub mod skill_match;
pub mod skills;
pub mod symgen;
pub mod topology;

pub use db::ComponentDb;
pub use models::*;
