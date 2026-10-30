// Tauri commands：与 Node 版 HTTP API 一一对应（含 open-terminal / 归档恢复）。
// 同步命令跑在线程池上，重扫描不会卡 UI。dev 模式打入口日志，方便确认 IPC 链路。
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::scanner::{self, archive_root, projects_dir, ChatMessage, Project, SessionMeta};
use crate::search;

fn project_path(dir: &str) -> PathBuf {
    projects_dir().join(dir)
}

fn archive_path(dir: &str) -> PathBuf {
    archive_root().join(dir)
}

/// 目录名白名单，防路径穿越
fn safe_dir_name(dir: &str) -> Option<String> {
    let ok = !dir.is_empty()
        && dir
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-');
    if ok {
        Some(dir.to_string())
    } else {
        None
    }
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

macro_rules! cmd_log {
    ($name:expr) => {
        #[cfg(debug_assertions)]
        eprintln!("[cmd] {}", $name)
    };
}

#[tauri::command]
pub fn list_projects() -> Result<Vec<Project>, String> {
    cmd_log!("list_projects");
    Ok(scanner::list_projects())
}

#[tauri::command]
pub fn get_sessions(dir: String) -> Result<Vec<SessionMeta>, String> {
    cmd_log!("get_sessions");
    let dir = safe_dir_name(&dir).ok_or("bad dir")?;
    let path = project_path(&dir);
    let sessions = scanner::sessions_of_project(&path);
    if let Some(first) = sessions.first() {
        scanner::remember_cwd(&dir, first.cwd.clone());
    }
    Ok(sessions)
}

#[tauri::command]
pub fn get_recent_sessions(limit: Option<usize>) -> Result<Vec<SessionMeta>, String> {
    cmd_log!("get_recent_sessions");
    let limit = limit.unwrap_or(30).clamp(1, 100);
    let projects = scanner::list_projects();
    let mut all: Vec<SessionMeta> = Vec::new();
    for p in &projects {
        // 只展开最近活跃的项目，凑够 limit*3 候选即提前停（按 mtime 排序后前面的一定更晚）
        let mut sessions = scanner::sessions_of_project(&project_path(&p.dir));
        for s in &mut sessions {
            s.project_cwd = Some(p.cwd.clone());
        }
        all.extend(sessions);
        if all.len() >= limit * 3 {
            break
        }
    }
    all.sort_by(|a, b| b.mtime_ms.partial_cmp(&a.mtime_ms).unwrap_or(std::cmp::Ordering::Equal));
    all.truncate(limit);
    Ok(all)
}

#[tauri::command]
pub fn search_sessions(q: String, limit: Option<usize>) -> Result<Vec<SessionMeta>, String> {
    cmd_log!("search_sessions");
    let limit = limit.unwrap_or(30).clamp(1, 100);
    if q.trim().len() < 2 {
        return Ok(vec![])
    }
    let hits = search::search_sessions(&q, limit);
    // 合并会话元数据（标题/时间/分支等）
    let mut sessions = Vec::with_capacity(hits.len());
    for h in hits {
        let Some((dir_name, file_name)) = h.key.rsplit_once('/') else { continue };
        if let Some(mut meta) = scanner::session_meta(&projects_dir().join(dir_name), file_name) {
            meta.project_dir = dir_name.to_string();
            meta.snippet = Some(h.snippet.clone());
            meta.match_count = Some(h.match_count);
            meta.mtime_ms = h.mtime_ms;
            sessions.push(meta);
        }
    }
    Ok(sessions)
}

#[tauri::command]
pub fn get_messages(
    dir: String,
    session_id: String,
    archived: Option<bool>,
) -> Result<Vec<ChatMessage>, String> {
    cmd_log!("get_messages");
    let dir = safe_dir_name(&dir).ok_or("bad dir")?;
    if !valid_session_id(&session_id) {
        return Err("bad sessionId".into())
    }
    let base = if archived.unwrap_or(false) { archive_path(&dir) } else { project_path(&dir) };
    scanner::read_session_messages(&base, &session_id).ok_or_else(|| "session not found".to_string())
}

#[tauri::command]
pub fn get_archived_sessions() -> Result<Vec<SessionMeta>, String> {
    cmd_log!("get_archived_sessions");
    let root = archive_root();
    let Ok(dirs) = fs::read_dir(&root) else {
        return Ok(vec![])
    };
    let mut all = Vec::new();
    for d in dirs.flatten() {
        if !d.path().is_dir() {
            continue
        }
        let mut metas = scanner::sessions_of_project(&d.path());
        for m in &mut metas {
            m.archived = Some(true);
        }
        all.extend(metas);
    }
    all.sort_by(|a, b| b.mtime_ms.partial_cmp(&a.mtime_ms).unwrap_or(std::cmp::Ordering::Equal));
    Ok(all)
}

/// 归档：移动到 projects-archive（可逆）
#[tauri::command]
pub fn archive_session(dir: String, session_id: String) -> Result<(), String> {
    cmd_log!("archive_session");
    let dir = safe_dir_name(&dir).ok_or("bad dir")?;
    if !valid_session_id(&session_id) {
        return Err("bad sessionId".into())
    }
    let src = project_path(&dir).join(format!("{}.jsonl", session_id));
    if !src.exists() {
        return Err("session not found".into())
    }
    fs::create_dir_all(archive_path(&dir)).map_err(|e| e.to_string())?;
    fs::rename(&src, archive_path(&dir).join(format!("{}.jsonl", session_id))).map_err(|e| e.to_string())
}

/// 从归档恢复
#[tauri::command]
pub fn restore_session(dir: String, session_id: String) -> Result<(), String> {
    cmd_log!("restore_session");
    let dir = safe_dir_name(&dir).ok_or("bad dir")?;
    if !valid_session_id(&session_id) {
        return Err("bad sessionId".into())
    }
    let src = archive_path(&dir).join(format!("{}.jsonl", session_id));
    if !src.exists() {
        return Err("archived session not found".into())
    }
    let dst = project_path(&dir).join(format!("{}.jsonl", session_id));
    if dst.exists() {
        return Err("目标已存在同名会话".into())
    }
    fs::create_dir_all(project_path(&dir)).map_err(|e| e.to_string())?;
    fs::rename(&src, &dst).map_err(|e| e.to_string())
}

/// 用指定终端（Terminal.app / Ghostty）新窗口执行 cd <cwd> && claude --resume <id>（仅 macOS） */
#[tauri::command]
pub fn open_terminal(cwd: String, session_id: String, terminal: Option<String>) -> Result<(), String> {
    cmd_log!("open_terminal");
    if !cfg!(target_os = "macos") {
        return Err("仅支持 macOS".into())
    }
    if !valid_session_id(&session_id) {
        return Err("bad sessionId".into())
    }
    let p = PathBuf::from(&cwd);
    if !p.is_absolute() || !p.exists() {
        return Err("bad cwd".into())
    }
    // shell 单引号包裹（两边终端都需要）
    let sq = format!("'{}'", cwd.replace('\'', "'\\''"));
    let shell_cmd = format!("cd {} && claude --resume {}", sq, session_id);

    match terminal.as_deref() {
        Some("ghostty") => {
            // Ghostty：spawn 绝对路径二进制（GUI app 的 PATH 里没有用户 shell 的路径）。
            // -e 不经 shell：整串命令必须拆成独立 argv（zsh -l -c "<cmd>"），
            // 否则会被 /usr/bin/login 当作单个可执行文件名 → ENOENT。
            // -l 让 zsh 读 .zprofile（fnm 等 PATH 初始化），claude 才能解析到。
            // spawn 不等待（ghostty -e 的 CLI 进程会随窗口存活）
            let bin = PathBuf::from("/Applications/Ghostty.app/Contents/MacOS/ghostty");
            if !bin.exists() {
                return Err("未找到 Ghostty（/Applications/Ghostty.app）".into())
            }
            Command::new(&bin)
                .args(["-e", "/bin/zsh", "-l", "-c"])
                .arg(&shell_cmd)
                .spawn()
                .map_err(|e| format!("启动 Ghostty 失败: {}", e))?;
            Ok(())
        }
        _ => {
            // 默认 Terminal.app：osascript（AppleScript 字符串再做一层转义）
            let as_esc = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!("tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell", as_esc);
            let out = Command::new("osascript").arg("-e").arg(&script).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).to_string())
            }
            Ok(())
        }
    }
}

/// 供 CLI 模式复用：跑一次数据层并输出 JSON 字符串
pub fn cli_json(v: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| "[]".to_string())
}
