// 会话内容全文搜索：增量索引 + 内存查询（从 Node 版 server/search.js 移植）。
// 索引内容：user/assistant 文本 + 工具调用关键输入，
// 以 JSONL 缓存在 ~/.claude/ccg-search-index.jsonl，按 mtime+size 增量重建。
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::scanner::projects_dir;

const TOOL_KEYS: [&str; 6] = ["file_path", "command", "pattern", "url", "path", "description"];
const CAP_PER_SESSION: usize = 2 * 1024 * 1024; // 单会话索引文本上限，防止异常大会话撑爆索引

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub key: String,
    pub mtime_ms: f64,
    pub size: u64,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub key: String,
    pub match_count: usize,
    pub snippet: String,
    pub mtime_ms: f64,
}

fn cache_file() -> PathBuf {
    crate::scanner::claude_dir().join("ccg-search-index.jsonl")
}

fn index_state() -> &'static Mutex<Option<HashMap<String, Entry>>> {
    static S: OnceLock<Mutex<Option<HashMap<String, Entry>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

fn build_lock() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

fn load_cache() -> HashMap<String, Entry> {
    let mut map = HashMap::new();
    let Ok(text) = fs::read_to_string(cache_file()) else {
        return map
    };
    for line in text.lines() {
        if line.is_empty() {
            continue
        }
        if let Ok(e) = serde_json::from_str::<Entry>(line) {
            map.insert(e.key.clone(), e);
        }
    }
    map
}

fn save_cache(map: &HashMap<String, Entry>) {
    let path = cache_file();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("jsonl.tmp");
    let Ok(mut out) = File::create(&tmp) else { return };
    for e in map.values() {
        let _ = writeln!(out, "{}", serde_json::to_string(e).unwrap_or_default());
    }
    drop(out);
    let _ = fs::rename(&tmp, &path);
}

/// 流式提取单个会话的可搜索文本 */
fn extract_text_of_file(file_path: &Path) -> String {
    let Ok(f) = File::open(file_path) else {
        return String::new()
    };
    let mut parts: Vec<String> = Vec::new();
    let mut total = 0usize;
    for line in BufReader::new(f).lines().map_while(Result::ok) {
        if total > CAP_PER_SESSION {
            break
        }
        let Ok(rec) = serde_json::from_str::<Value>(&line) else { continue };
        if rec.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
            continue
        }
        let mut t = String::new();
        match rec.get("type").and_then(|v| v.as_str()) {
            Some("user") => {
                if let Some(msg) = rec.get("message") {
                    if let Some(c) = msg.get("content") {
                        if let Some(s) = c.as_str() {
                            t = s.to_string();
                        } else if let Some(arr) = c.as_array() {
                            let texts: Vec<&str> = arr
                                .iter()
                                .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
                                .filter_map(|b| b.get("text").and_then(|v| v.as_str()))
                                .collect();
                            t = texts.join("\n");
                        }
                    }
                }
                if t.starts_with("<local-command") || t.starts_with("<bash-") {
                    t.clear();
                }
                t = t.replace("<user_message>\n", "").replace("<user_message>", "");
                t = t.replace("\n</user_message>", "").replace("</user_message>", "");
            }
            Some("assistant") => {
                if let Some(arr) = rec.get("message").and_then(|m| m.get("content")).and_then(|v| v.as_array()) {
                    let mut segs: Vec<String> = Vec::new();
                    for b in arr {
                        match b.get("type").and_then(|v| v.as_str()) {
                            Some("text") => {
                                if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                    segs.push(t.to_string())
                                }
                            }
                            Some("tool_use") => {
                                if let Some(input) = b.get("input") {
                                    for k in TOOL_KEYS {
                                        if let Some(v) = input.get(k).and_then(|v| v.as_str()) {
                                            if !v.is_empty() {
                                                segs.push(v.lines().next().unwrap_or("").chars().take(200).collect());
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    t = segs.join("\n");
                }
            }
            _ => {}
        }
        if !t.is_empty() {
            total += t.len();
            parts.push(t);
        }
    }
    parts.join("\n")
}

/// 增量构建：stat 全部会话文件，变化的重新提取，已消失（归档/删除）的移除。
/// 构建期间持锁串行化，完成后整体换入。
fn ensure_index() -> HashMap<String, Entry> {
    let _guard = build_lock().lock().unwrap();
    {
        let mut state = index_state().lock().unwrap();
        if state.is_none() {
            *state = Some(load_cache());
        }
    }
    let cached = index_state().lock().unwrap().take().unwrap_or_default();

    let mut fresh: HashMap<String, Entry> = HashMap::new();
    let mut changed = false;
    let root = projects_dir();
    if let Ok(dirs) = fs::read_dir(&root) {
        for d in dirs.flatten() {
            if !d.path().is_dir() {
                continue
            }
            let dir_name = d.file_name().to_string_lossy().to_string();
            let Ok(files) = fs::read_dir(d.path()) else { continue };
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().to_string();
                if !name.ends_with(".jsonl") {
                    continue
                }
                let Ok(st) = f.metadata() else { continue };
                let Ok(mtime) = st.modified() else { continue };
                let mtime_ms = crate::scanner::system_time_ms(mtime);
                let key = format!("{}/{}", dir_name, name);
                let hit = cached.get(&key).filter(|e| e.mtime_ms == mtime_ms && e.size == st.len());
                match hit {
                    Some(e) => {
                        fresh.insert(key, e.clone());
                    }
                    None => {
                        changed = true;
                        let text = extract_text_of_file(&f.path());
                        fresh.insert(key.clone(), Entry { key, mtime_ms, size: st.len(), text });
                    }
                }
            }
        }
    }

    if changed {
        save_cache(&fresh);
    }
    *index_state().lock().unwrap() = Some(fresh.clone());
    fresh
}

fn make_snippet(text: &str, idx: usize, len: usize) -> String {
    let char_idx = byte_to_char_idx(text, idx);
    let total_chars = text.chars().count();
    let start = char_idx.saturating_sub(50);
    let end = (char_idx + len + 70).min(total_chars);
    let slice: String = text.chars().skip(start).take(end - start).collect();
    let mut s: String = slice.split_whitespace().collect::<Vec<&str>>().join(" ");
    if start > 0 {
        s = format!("…{}", s);
    }
    if end < total_chars {
        s.push('…');
    }
    if s.chars().count() > 200 {
        s = s.chars().take(200).collect::<String>() + "…";
    }
    s
}

/// 字节偏移 -> 字符偏移（正则给出的是字节位置） */
fn byte_to_char_idx(text: &str, byte_idx: usize) -> usize {
    text.char_indices().take_while(|(i, _)| *i < byte_idx).count()
}

/// 全文检索，返回命中列表（按命中次数降序、时间次之），key = "<项目目录名>/<文件名>" */
pub fn search_sessions(q: &str, limit: usize) -> Vec<SearchHit> {
    let query = q.trim();
    if query.is_empty() {
        return vec![]
    }
    let index = ensure_index();
    let Ok(re) = RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build() else {
        return vec![]
    };
    let mut results: Vec<SearchHit> = Vec::new();
    for e in index.values() {
        let mut count = 0usize;
        let mut first: Option<usize> = None;
        for m in re.find_iter(&e.text) {
            if count == 0 {
                first = Some(m.start());
            }
            count += 1;
            if count >= 50 {
                break
            }
        }
        if count == 0 {
            continue
        }
        let idx = first.unwrap_or(0);
        results.push(SearchHit {
            key: e.key.clone(),
            match_count: count,
            snippet: make_snippet(&e.text, idx, query.len()),
            mtime_ms: e.mtime_ms,
        });
    }
    results.sort_by(|a, b| b.match_count.cmp(&a.match_count).then(b.mtime_ms.partial_cmp(&a.mtime_ms).unwrap_or(std::cmp::Ordering::Equal)));
    results.truncate(limit);
    results
}
