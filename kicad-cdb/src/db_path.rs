//! 库里自己开数据库连接的辅助函数 (模板加载 / 模板列表) 用的数据库路径.
//!
//! 显式拿 `&ComponentDb` / `&Connection` 的函数不走这里. 命令行程序 (kdesign) 启动时用
//! [`set_active_db_path`] 把 `--db` 登记进来, 这些辅助函数就和它指向同一个库.

use std::sync::RwLock;

/// 进程启动时由应用程序登记的路径; `None` = 没登记.
static ACTIVE_DB_PATH: RwLock<Option<String>> = RwLock::new(None);

/// 登记本进程"当前数据库"的路径. 之后 [`db_path`] 优先返回它.
pub fn set_active_db_path(path: &str) {
    *ACTIVE_DB_PATH.write().unwrap() = Some(path.to_string());
}

/// 数据库路径. 优先级: [`set_active_db_path`] 登记的 > 环境变量 `CDB_PATH` > 当前目录的 `components.db`.
pub fn db_path() -> String {
    ACTIVE_DB_PATH
        .read()
        .unwrap()
        .clone()
        .or_else(|| std::env::var("CDB_PATH").ok())
        .unwrap_or_else(|| "components.db".to_string())
}
