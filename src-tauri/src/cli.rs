// 无头 CLI 模式：不经 GUI 直接跑数据层，输出 JSON。
// 用途：调试 + 与 Node 版输出对照验证移植正确性。
// 例：app --list-projects | app --search "关键词" | app --messages <dir> <id> [--archived]
use crate::commands;
use crate::scanner;

pub fn maybe_handle_cli() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || !args[0].starts_with("--") {
        return false
    }

    let usage = || -> String {
        "未知命令。用法: \n  --list-projects\n  --recent [N]\n  --sessions <dir>\n  --search <关键词>\n  --messages <dir> <sessionId> [--archived]\n  --archived\n  --archive <dir> <sessionId>\n  --restore <dir> <sessionId>\n  --open-terminal <cwd> <sessionId>".to_string()
    };

    let result: Result<String, String> = (|| {
        let cmd = args[0].as_str();
        let arg = |i: usize| args.get(i).cloned().ok_or_else(|| format!("缺少参数 {}", i));
        match cmd {
            "--list-projects" => Ok(commands::cli_json(&scanner::list_projects())),
            "--recent" => {
                let n = args.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(20);
                commands::get_recent_sessions(Some(n)).map(|v| commands::cli_json(&v))
            }
            "--sessions" => commands::get_sessions(arg(1)?).map(|v| commands::cli_json(&v)),
            "--search" => commands::search_sessions(arg(1)?, Some(30)).map(|v| commands::cli_json(&v)),
            "--messages" => {
                let archived = args.iter().any(|a| a == "--archived");
                let dir = arg(1)?;
                let id = arg(2)?;
                commands::get_messages(dir, id, Some(archived)).map(|v| commands::cli_json(&v))
            }
            "--archived" => commands::get_archived_sessions().map(|v| commands::cli_json(&v)),
            "--archive" => commands::archive_session(arg(1)?, arg(2)?).map(|_| "ok".to_string()),
            "--restore" => commands::restore_session(arg(1)?, arg(2)?).map(|_| "ok".to_string()),
            "--open-terminal" => commands::open_terminal(arg(1)?, arg(2)?, args.get(3).cloned()).map(|_| "ok".to_string()),
            _ => Err(usage()),
        }
    })();

    match result {
        Ok(out) => {
            println!("{}", out);
            true
        }
        Err(e) => {
            eprintln!("错误: {}", e);
            std::process::exit(1);
        }
    }
}
