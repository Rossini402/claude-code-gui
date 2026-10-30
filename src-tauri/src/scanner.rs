// 扫描 ~/.claude/projects，提取项目与会话元数据。
// 从 Node 版 server/scanner.js 移植，行为保持一致：
// 列表视图只做 stat + 文件头/尾部分读取，不整体解析 JSONL。
use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// SystemTime -> epoch 毫秒（浮点，与 JS Date.now() 对齐） */
pub fn system_time_ms(m: SystemTime) -> f64 {
    m.duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

pub fn claude_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".claude")
}
pub fn projects_dir() -> PathBuf {
    claude_dir().join("projects")
}
pub fn archive_root() -> PathBuf {
    claude_dir().join("projects-archive")
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub dir: String,
    pub cwd: String,
    pub session_count: usize,
    pub last_active_ms: f64,
    pub total_size_bytes: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub session_id: String,
    pub project_dir: String,
    pub cwd: Option<String>,
    pub title: String,
    pub start_time: Option<String>,
    pub last_time: Option<String>,
    pub mtime_ms: f64,
    pub size_bytes: u64,
    pub git_branch: Option<String>,
    pub version: Option<String>,
    // 列表端点附加
    pub message_count: Option<usize>,
    pub project_cwd: Option<String>,
    // 搜索端点附加
    pub snippet: Option<String>,
    pub match_count: Option<usize>,
    // 归档端点附加
    pub archived: Option<bool>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub name: String,
    pub input: Value,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<ToolCall>,
}

fn read_head_lines(path: &Path, max_bytes: usize) -> Vec<String> {
    let Ok(mut f) = File::open(path) else {
        return vec![]
    };
    let mut buf = vec![0u8; max_bytes];
    let n = f.read(&mut buf).unwrap_or(0);
    String::from_utf8_lossy(&buf[..n])
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn read_tail_lines(path: &Path, max_bytes: usize) -> Vec<String> {
    let Ok(mut f) = File::open(path) else {
        return vec![]
    };
    let Ok(meta) = f.metadata() else {
        return vec![]
    };
    let size = meta.len();
    let start = size.saturating_sub(max_bytes as u64);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![]
    }
    let mut buf = vec![0u8; (size - start) as usize];
    let n = f.read(&mut buf).unwrap_or(0);
    let mut text = String::from_utf8_lossy(&buf[..n]).to_string();
    if start > 0 {
        // 丢弃可能被截断的首行
        if let Some(pos) = text.find('\n') {
            text = text[pos + 1..].to_string();
        } else {
            text.clear();
        }
    }
    text.split('\n').filter(|l| !l.is_empty()).map(|l| l.to_string()).collect()
}

fn parse_line(line: &str) -> Option<Value> {
    serde_json::from_str(line).ok()
}

fn str_field<'a>(rec: &'a Value, key: &str) -> Option<&'a str> {
    rec.get(key).and_then(|v| v.as_str())
}

/// 从消息 content 提取纯文本（兼容 string / blocks 数组）；只有图片时返回占位符
fn extract_text(content: Option<&Value>, placeholder_if_only_image: bool) -> String {
    let Some(c) = content else {
        return String::new()
    };
    if let Some(s) = c.as_str() {
        return s.to_string()
    }
    if let Some(arr) = c.as_array() {
        let texts: Vec<&str> = arr
            .iter()
            .filter(|b| str_field(b, "type") == Some("text"))
            .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
            .collect();
        if !texts.is_empty() {
            return texts.join("\n")
        }
        if placeholder_if_only_image && arr.iter().any(|b| str_field(b, "type") == Some("image")) {
            return "[图片消息]".to_string()
        }
    }
    String::new()
}

/// 清理系统注入的包裹标签，取首行/截断作为标题（与 Node 版 toTitle 一致） */
fn to_title(text: &str, max_len: usize) -> String {
    static RE_UM_OPEN: OnceLock<Regex> = OnceLock::new();
    static RE_UM_CLOSE: OnceLock<Regex> = OnceLock::new();
    static RE_CMD: OnceLock<Regex> = OnceLock::new();
    static RE_STDOUT: OnceLock<Regex> = OnceLock::new();
    static RE_CMD_PREFIX: OnceLock<Regex> = OnceLock::new();
    let re_um_open = RE_UM_OPEN.get_or_init(|| Regex::new(r"<user_message>\n?").unwrap());
    let re_um_close = RE_UM_CLOSE.get_or_init(|| Regex::new(r"\n?</user_message>").unwrap());
    let re_cmd = RE_CMD.get_or_init(|| Regex::new(r"(?s)<command-name>.*?</command-name>").unwrap());
    let re_stdout =
        RE_STDOUT.get_or_init(|| Regex::new(r"(?s)<local-command-stdout>.*?</local-command-stdout>").unwrap());
    let re_cmd_prefix = RE_CMD_PREFIX.get_or_init(|| Regex::new(r"(?m)^Command: \S+\s*").unwrap());

    let cleaned = re_um_open.replace_all(text, "");
    let cleaned = re_um_close.replace_all(&cleaned, "");
    let cleaned = re_cmd.replace_all(&cleaned, "[命令]");
    let cleaned = re_stdout.replace_all(&cleaned, "");
    let cleaned = re_cmd_prefix.replace_all(&cleaned, "");
    let cleaned = cleaned.trim();

    let first_line = cleaned.split('\n').find(|l| !l.trim().is_empty()).unwrap_or("(空会话)");
    let first_line = if first_line.is_empty() { "(空会话)" } else { first_line };
    if first_line.chars().count() > max_len {
        let truncated: String = first_line.chars().take(max_len).collect();
        format!("{}…", truncated)
    } else {
        first_line.to_string()
    }
}

/// 单个会话文件元数据：stat + 头部若干行 + 尾部若干行 */
pub fn session_meta(dir_path: &Path, file_name: &str) -> Option<SessionMeta> {
    let file_path = dir_path.join(file_name);
    let Ok(st) = fs::metadata(&file_path) else {
        return None
    };
    let session_id = file_name.trim_end_matches(".jsonl").to_string();
    let project_dir = dir_path.file_name()?.to_string_lossy().to_string();

    let mut cwd: Option<String> = None;
    let mut title: Option<String> = None;
    let mut start_time: Option<String> = None;
    let mut git_branch: Option<String> = None;
    let mut version: Option<String> = None;

    // 头部扫描：找到首条真实用户消息为止（跳过 queue-operation 等元记录）
    for line in read_head_lines(&file_path, 256 * 1024) {
        let Some(rec) = parse_line(&line) else { continue };
        if cwd.is_none() {
            if let Some(s) = str_field(&rec, "cwd") {
                cwd = Some(s.to_string());
            }
        }
        if git_branch.is_none() {
            if let Some(s) = str_field(&rec, "gitBranch") {
                git_branch = Some(s.to_string());
            }
        }
        if version.is_none() {
            if let Some(s) = str_field(&rec, "version") {
                version = Some(s.to_string());
            }
        }
        if start_time.is_none() {
            if let Some(s) = str_field(&rec, "timestamp") {
                start_time = Some(s.to_string());
            }
        }
        if title.is_some() {
            break
        }
        if str_field(&rec, "type") == Some("user") && rec.pointer("/message/role").and_then(|v| v.as_str()) == Some("user") {
            let raw = extract_text(rec.get("message").and_then(|m| m.get("content")), true);
            // 跳过命令回显、系统注入的纯标签消息，继续找下一条真实用户输入
            let is_command = raw.starts_with("<command") || raw.starts_with("[命令]");
            let is_meta = raw.starts_with("<local-command") || raw.starts_with("<bash-");
            if !raw.is_empty() && !is_command && !is_meta {
                title = Some(to_title(&raw, 80));
            }
        }
    }

    // 尾部扫描：最后时间戳 / 分支（会话中途切分支时以尾部为准）
    let tail: Vec<Value> = read_tail_lines(&file_path, 16 * 1024).iter().filter_map(|l| parse_line(l)).collect();
    let mut last_time: Option<String> = None;
    for rec in tail.iter().rev() {
        if let Some(s) = str_field(rec, "timestamp") {
            last_time = Some(s.to_string());
            break
        }
    }
    for rec in tail.iter().rev() {
        if let Some(s) = str_field(rec, "gitBranch") {
            git_branch = Some(s.to_string());
            break
        }
    }

    let mtime = st.modified().ok()?;
    let mtime_ms = system_time_ms(mtime);

    Some(SessionMeta {
        session_id,
        project_dir,
        cwd,
        title: title.unwrap_or_else(|| "(无用户消息)".to_string()),
        start_time: start_time.clone(),
        last_time: last_time.or(start_time),
        mtime_ms,
        size_bytes: st.len(),
        git_branch,
        version,
        ..Default::default()
    })
}

/// 目录名 -> 大致可读路径（无法区分字面 '-'，仅供兜底）
fn decode_dir_name(dir: &str) -> String {
    let s = dir.strip_prefix('-').unwrap_or(dir);
    format!("/{}", s.replace('-', "/"))
}

/// 从文件记录里找 cwd：先扫头部，找不到再扫尾部（首行可能是超长上下文记录） */
fn read_head_cwd(file_path: &Path) -> Option<String> {
    for line in read_head_lines(file_path, 128 * 1024) {
        if let Some(rec) = parse_line(&line) {
            if let Some(s) = str_field(&rec, "cwd") {
                return Some(s.to_string())
            }
        }
    }
    for line in read_tail_lines(file_path, 64 * 1024) {
        if let Some(rec) = parse_line(&line) {
            if let Some(s) = str_field(&rec, "cwd") {
                return Some(s.to_string())
            }
        }
    }
    None
}

fn cwd_cache() -> &'static Mutex<HashMap<String, String>> {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// history.jsonl 兜底标题源：全局 prompt 历史，含 sessionId。
/// 每个 sessionId 取时间最早的一条（即会话首条输入）。懒加载一次常驻内存。
fn history_title(session_id: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<Option<HashMap<String, (u64, String)>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap();
    if guard.is_none() {
        let mut map = HashMap::new();
        if let Ok(text) = fs::read_to_string(claude_dir().join("history.jsonl")) {
            for line in text.lines() {
                let Ok(rec) = serde_json::from_str::<Value>(line) else { continue };
                let Some(id) = str_field(&rec, "sessionId") else { continue };
                let Some(display) = str_field(&rec, "display") else { continue };
                let ts = rec.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0);
                match map.get(id) {
                    Some((prev_ts, _)) if ts >= *prev_ts => {}
                    _ => {
                        map.insert(id.to_string(), (ts, display.to_string()));
                    }
                }
            }
        }
        *guard = Some(map);
    }
    guard
        .as_ref()
        .unwrap()
        .get(session_id)
        .map(|(_, t)| to_title(t, 80))
}

/// 列出全部项目。此层只 stat 不读内容：会话数 = .jsonl 文件数，最近活跃 = 最新 mtime。
pub fn list_projects() -> Vec<Project> {
    let root = projects_dir();
    let Ok(dirs) = fs::read_dir(&root) else {
        return vec![]
    };
    let mut projects = Vec::new();
    for d in dirs.flatten() {
        if !d.path().is_dir() {
            continue
        }
        let dir_name = d.file_name().to_string_lossy().to_string();
        let Ok(files) = fs::read_dir(d.path()) else { continue };
        let mut count = 0usize;
        let mut total_size = 0u64;
        let mut last_mtime = 0.0f64;
        let mut newest_file: Option<PathBuf> = None;
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !name.ends_with(".jsonl") {
                continue
            }
            let Ok(st) = f.metadata() else { continue };
            count += 1;
            total_size += st.len();
            if let Ok(m) = st.modified() {
                let ms = system_time_ms(m);
                if ms > last_mtime {
                    last_mtime = ms;
                    newest_file = Some(f.path());
                }
            }
        }
        if count == 0 {
            continue
        }
        // 目录名编码有歧义（'-' 既来自 '/' 也可能是字面 '-'），
        // 真实路径从最新会话文件的记录里取，读不到再用解码兜底
        let cached = cwd_cache().lock().unwrap().get(&dir_name).cloned();
        let cwd = match cached {
            Some(c) => c,
            None => {
                let real = newest_file.and_then(|p| read_head_cwd(&p));
                let c = real.unwrap_or_else(|| decode_dir_name(&dir_name));
                cwd_cache().lock().unwrap().insert(dir_name.clone(), c.clone());
                c
            }
        };
        projects.push(Project {
            dir: dir_name,
            cwd,
            session_count: count,
            last_active_ms: last_mtime,
            total_size_bytes: total_size,
        });
    }
    projects.sort_by(|a, b| b.last_active_ms.partial_cmp(&a.last_active_ms).unwrap_or(std::cmp::Ordering::Equal));
    projects
}

/// 展开项目时回填真实 cwd */
pub fn remember_cwd(dir_name: &str, cwd: Option<String>) {
    if let Some(c) = cwd {
        cwd_cache().lock().unwrap().insert(dir_name.to_string(), c);
    }
}

/// 列出某目录下全部会话（含消息数），按 mtime 倒序。dir_path 为完整目录路径 */
pub fn sessions_of_project(dir_path: &Path) -> Vec<SessionMeta> {
    let Ok(files) = fs::read_dir(dir_path) else {
        return vec![]
    };
    let mut metas = Vec::new();
    for f in files.flatten() {
        let name = f.file_name().to_string_lossy().to_string();
        if !name.ends_with(".jsonl") {
            continue
        }
        if let Some(mut meta) = session_meta(dir_path, &name) {
            // jsonl 头部扫不到首条用户消息时（超长上下文记录挤到文件深处），
            // 用 history.jsonl 兜底取会话首条输入
            if meta.title == "(无用户消息)" {
                if let Some(t) = history_title(&meta.session_id) {
                    meta.title = t;
                }
            }
            meta.message_count = Some(count_messages(dir_path, &meta.session_id));
            metas.push(meta);
        }
    }
    metas.sort_by(|a, b| b.mtime_ms.partial_cmp(&a.mtime_ms).unwrap_or(std::cmp::Ordering::Equal));
    metas
}

/// 消息数统计（user+assistant 记录数），带缓存；列表页显示用 */
fn count_cache() -> &'static Mutex<HashMap<PathBuf, (f64, usize)>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (f64, usize)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn count_messages(dir_path: &Path, session_id: &str) -> usize {
    let file_path = dir_path.join(format!("{}.jsonl", session_id));
    let Ok(st) = fs::metadata(&file_path) else {
        return 0
    };
    let Ok(mtime) = st.modified() else {
        return 0
    };
    let mtime_ms = system_time_ms(mtime);
    if let Some((ts, count)) = count_cache().lock().unwrap().get(&file_path) {
        if *ts == mtime_ms {
            return *count
        }
    }
    let mut count = 0usize;
    if let Ok(f) = File::open(&file_path) {
        for line in BufReader::new(f).lines().map_while(Result::ok) {
            if line.contains("\"type\":\"user\"") || line.contains("\"type\":\"assistant\"") {
                count += 1;
            }
        }
    }
    count_cache().lock().unwrap().insert(file_path, (mtime_ms, count));
    count
}

/// 流式解析整个会话文件，输出消息流（会话详情页用）。
/// 只保留 user / assistant 记录，工具调用折叠为一条摘要。 */
pub fn read_session_messages(dir_path: &Path, session_id: &str) -> Option<Vec<ChatMessage>> {
    let file_path = dir_path.join(format!("{}.jsonl", session_id));
    let f = File::open(&file_path).ok()?;
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut pending: Option<ChatMessage> = None;

    for line in BufReader::new(f).lines().map_while(Result::ok) {
        let Some(rec) = parse_line(&line) else { continue };
        if rec.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
            continue // 子代理内部链路不展示
        }
        match str_field(&rec, "type") {
            Some("summary") => {
                if let Some(p) = pending.take() {
                    messages.push(p)
                }
                if let Some(s) = str_field(&rec, "summary") {
                    messages.push(ChatMessage {
                        role: "summary".into(),
                        text: s.to_string(),
                        time: str_field(&rec, "timestamp").map(|t| t.to_string()),
                        tools: vec![],
                    });
                }
            }
            Some("user") => {
                if let Some(p) = pending.take() {
                    messages.push(p)
                }
                let Some(msg) = rec.get("message") else { continue };
                let text = extract_text(msg.get("content"), true);
                let is_meta = text.starts_with("<local-command") || text.starts_with("<bash-");
                if text.is_empty() || is_meta {
                    continue
                }
                static RE_UM_OPEN: OnceLock<Regex> = OnceLock::new();
                static RE_UM_CLOSE: OnceLock<Regex> = OnceLock::new();
                let open = RE_UM_OPEN.get_or_init(|| Regex::new(r"<user_message>\n?").unwrap());
                let close = RE_UM_CLOSE.get_or_init(|| Regex::new(r"\n?</user_message>").unwrap());
                let text = open.replace_all(&text, "");
                let text = close.replace_all(&text, "").to_string();
                messages.push(ChatMessage {
                    role: "user".into(),
                    text,
                    time: str_field(&rec, "timestamp").map(|t| t.to_string()),
                    tools: vec![],
                });
            }
            Some("assistant") => {
                let Some(msg) = rec.get("message") else { continue };
                let blocks = msg.get("content").and_then(|v| v.as_array());
                let mut text_parts: Vec<String> = Vec::new();
                let mut tools: Vec<ToolCall> = Vec::new();
                if let Some(arr) = blocks {
                    for b in arr {
                        match str_field(b, "type") {
                            Some("text") => {
                                if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(t.to_string())
                                }
                            }
                            Some("tool_use") => {
                                tools.push(ToolCall {
                                    name: str_field(b, "name").unwrap_or("unknown").to_string(),
                                    input: b.get("input").cloned().unwrap_or(Value::Null),
                                });
                            }
                            _ => {}
                        }
                    }
                }
                let text = text_parts.join("\n");
                if text.is_empty() && tools.is_empty() {
                    continue
                }
                let entry = pending.get_or_insert(ChatMessage {
                    role: "assistant".into(),
                    text: String::new(),
                    time: str_field(&rec, "timestamp").map(|t| t.to_string()),
                    tools: vec![],
                });
                if !text.is_empty() {
                    if !entry.text.is_empty() {
                        entry.text.push('\n');
                    }
                    entry.text.push_str(&text);
                }
                entry.tools.extend(tools);
            }
            _ => {}
        }
    }
    if let Some(p) = pending.take() {
        messages.push(p)
    }
    Some(messages)
}
