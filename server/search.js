// 会话内容全文搜索：增量索引 + 内存查询。
// 索引内容：user/assistant 文本 + 工具调用关键输入（file_path/command/pattern/url 等），
// 以 JSONL 缓存在 .cache/search-index.jsonl，按 mtime+size 增量重建。
import fs from 'node:fs'
import path from 'node:path'
import readline from 'node:readline'
import { PROJECTS_DIR } from './scanner.js'

const CACHE_FILE = path.join(import.meta.dirname, '..', '.cache', 'search-index.jsonl')
const TOOL_KEYS = ['file_path', 'command', 'pattern', 'url', 'path', 'description']
const CAP_PER_SESSION = 2 * 1024 * 1024 // 单会话索引文本上限，防止异常大会话撑爆索引

let index = null // Map<key, {key, mtimeMs, size, text}>；null = 尚未加载
let building = null // 构建去重（并发请求共用一次构建）

function loadCache() {
  const map = new Map()
  try {
    for (const line of fs.readFileSync(CACHE_FILE, 'utf8').split('\n')) {
      if (!line) continue
      try {
        const e = JSON.parse(line)
        if (e?.key) map.set(e.key, e)
      } catch {
        /* 跳过坏行 */
      }
    }
  } catch {
    /* 无缓存文件，全量构建 */
  }
  return map
}

function saveCache(map) {
  fs.mkdirSync(path.dirname(CACHE_FILE), { recursive: true })
  const tmp = CACHE_FILE + '.tmp'
  const out = fs.createWriteStream(tmp)
  for (const e of map.values()) out.write(JSON.stringify(e) + '\n')
  out.end()
  return new Promise((resolve, reject) => {
    out.on('finish', () => fs.rename(tmp, CACHE_FILE, resolve))
    out.on('error', reject)
  })
}

/** 流式提取单个会话的可搜索文本 */
function extractText(filePath) {
  return new Promise((resolve) => {
    const rl = readline.createInterface({
      input: fs.createReadStream(filePath, { encoding: 'utf8' }),
      crlfDelay: Infinity,
    })
    const parts = []
    let total = 0
    rl.on('line', (line) => {
      if (total > CAP_PER_SESSION) {
        rl.close()
        return
      }
      let rec
      try {
        rec = JSON.parse(line)
      } catch {
        return
      }
      if (rec.isSidechain) return
      let t = ''
      if (rec.type === 'user' && rec.message) {
        const c = rec.message.content
        if (typeof c === 'string') t = c
        else if (Array.isArray(c)) {
          t = c
            .filter((b) => b.type === 'text' && typeof b.text === 'string')
            .map((b) => b.text)
            .join('\n')
        }
        if (t.startsWith('<local-command') || t.startsWith('<bash-')) t = ''
        t = t.replace(/<user_message>\n?/g, '').replace(/\n?<\/user_message>/g, '')
      } else if (rec.type === 'assistant' && rec.message && Array.isArray(rec.message.content)) {
        const segs = []
        for (const b of rec.message.content) {
          if (b.type === 'text' && b.text) {
            segs.push(b.text)
          } else if (b.type === 'tool_use' && b.input) {
            for (const k of TOOL_KEYS) {
              const v = b.input[k]
              if (typeof v === 'string' && v) segs.push(v.split('\n')[0].slice(0, 200))
            }
          }
        }
        t = segs.join('\n')
      }
      if (t) {
        parts.push(t)
        total += t.length
      }
    })
    rl.on('close', () => resolve(parts.join('\n')))
    rl.on('error', () => resolve(parts.join('\n')))
  })
}

/** 增量构建：stat 全部会话文件，变化的重新提取，已消失（归档/删除）的移除 */
async function ensureIndex() {
  if (building) return building
  building = (async () => {
    if (index === null) index = loadCache()
    const fresh = new Map()
    const tasks = []
    let dirs
    try {
      dirs = fs.readdirSync(PROJECTS_DIR, { withFileTypes: true })
    } catch {
      return index
    }
    for (const d of dirs) {
      if (!d.isDirectory()) continue
      const full = path.join(PROJECTS_DIR, d.name)
      let files
      try {
        files = fs.readdirSync(full)
      } catch {
        continue
      }
      for (const f of files) {
        if (!f.endsWith('.jsonl')) continue
        const filePath = path.join(full, f)
        let st
        try {
          st = fs.statSync(filePath)
        } catch {
          continue
        }
        const key = `${d.name}/${f}`
        const cached = index.get(key)
        if (cached && cached.mtimeMs === st.mtimeMs && cached.size === st.size) {
          fresh.set(key, cached)
        } else {
          tasks.push(
            extractText(filePath).then((text) =>
              fresh.set(key, { key, mtimeMs: st.mtimeMs, size: st.size, text }),
            ),
          )
        }
      }
    }
    // 提取并发分批，避免同时打开过多文件流
    for (let i = 0; i < tasks.length; i += 8) {
      await Promise.all(tasks.slice(i, i + 8))
    }
    index = fresh
    if (tasks.length) await saveCache(fresh)
    return index
  })().finally(() => {
    building = null
  })
  return building
}

function escapeRe(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

function makeSnippet(text, idx, len) {
  const start = Math.max(0, idx - 50)
  const end = Math.min(text.length, idx + len + 70)
  let s = text.slice(start, end).replace(/\s+/g, ' ').trim()
  if (start > 0) s = '…' + s
  if (end < text.length) s += '…'
  return s.length > 200 ? s.slice(0, 200) + '…' : s
}

/**
 * 全文检索，返回 [{key, matchCount, snippet, mtimeMs}]，
 * key = "<项目目录名>/<文件名>"，由调用方合并会话元数据。
 */
export async function searchSessions(q, limit = 30) {
  const idx = await ensureIndex()
  const query = q.trim()
  if (!query) return []
  const re = new RegExp(escapeRe(query), 'gi')
  const results = []
  for (const e of idx.values()) {
    re.lastIndex = 0
    const m = re.exec(e.text)
    if (!m) continue
    let count = 1
    while (count < 50 && re.exec(e.text)) count++
    results.push({
      key: e.key,
      matchCount: count,
      snippet: makeSnippet(e.text, m.index, query.length),
      mtimeMs: e.mtimeMs,
    })
  }
  results.sort((a, b) => b.matchCount - a.matchCount || b.mtimeMs - a.mtimeMs)
  return results.slice(0, limit)
}
