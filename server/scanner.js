// 扫描 ~/.claude/projects，提取项目与会话元数据。
// 性能原则：列表视图只做 stat + 文件头/尾部分读取，不整体解析 JSONL。
import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import readline from 'node:readline'

export const CLAUDE_DIR = path.join(os.homedir(), '.claude')
export const PROJECTS_DIR = path.join(CLAUDE_DIR, 'projects')
const HISTORY_FILE = path.join(CLAUDE_DIR, 'history.jsonl')

/**
 * history.jsonl 兜底标题源：全局 prompt 历史，含 sessionId。
 * 每个 sessionId 取时间最早的一条（即会话首条输入）。
 * 懒加载一次常驻内存（约 2-3MB）。
 */
let historyTitles = null
export function historyTitle(sessionId) {
  if (historyTitles === null) {
    historyTitles = new Map()
    try {
      const lines = fs.readFileSync(HISTORY_FILE, 'utf8').split('\n')
      for (const line of lines) {
        const rec = safeParse(line)
        if (!rec?.sessionId || !rec.display) continue
        const prev = historyTitles.get(rec.sessionId)
        if (!prev || rec.timestamp < prev.ts) {
          historyTitles.set(rec.sessionId, { ts: rec.timestamp, text: rec.display })
        }
      }
    } catch {
      /* 文件不存在等情况直接用空表兜底 */
    }
  }
  const entry = historyTitles.get(sessionId)
  return entry ? toTitle(entry.text) : null
}

/** 读取文件前 maxBytes 字节，返回按行切开的完整行数组（丢弃可能被截断的首行） */
function readHeadLines(filePath, maxBytes = 64 * 1024) {
  const fd = fs.openSync(filePath, 'r')
  try {
    const buf = Buffer.alloc(maxBytes)
    const n = fs.readSync(fd, buf, 0, maxBytes, 0)
    const lines = buf.subarray(0, n).toString('utf8').split('\n').filter(Boolean)
    // 第一行可能被截断导致 JSON.parse 失败，逐行解析时容错即可
    return lines
  } finally {
    fs.closeSync(fd)
  }
}

/** 读取文件末尾 maxBytes 字节的完整行 */
function readTailLines(filePath, maxBytes = 16 * 1024) {
  const size = fs.statSync(filePath).size
  const fd = fs.openSync(filePath, 'r')
  try {
    const start = Math.max(0, size - maxBytes)
    const buf = Buffer.alloc(size - start)
    const n = fs.readSync(fd, buf, 0, buf.length, start)
    let text = buf.subarray(0, n).toString('utf8')
    if (start > 0) text = text.replace(/^[^\n]*\n/, '') // 丢弃可能被截断的首行
    return text.split('\n').filter(Boolean)
  } finally {
    fs.closeSync(fd)
  }
}

function safeParse(line) {
  try {
    return JSON.parse(line)
  } catch {
    return null
  }
}

/** 从消息 content 提取纯文本（兼容 string / blocks 数组）；只有图片时返回占位符 */
function extractText(content, placeholderIfOnlyImage = false) {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    const texts = content
      .filter((b) => b.type === 'text' && typeof b.text === 'string')
      .map((b) => b.text)
    if (texts.length) return texts.join('\n')
    if (placeholderIfOnlyImage && content.some((b) => b.type === 'image')) {
      return '[图片消息]'
    }
  }
  return ''
}

/** 清理系统注入的包裹标签，取首行/截断作为标题 */
function toTitle(text, maxLen = 80) {
  const cleaned = text
    .replace(/<user_message>\n?/g, '')
    .replace(/\n?<\/user_message>/g, '')
    .replace(/<command-name>[\s\S]*?<\/command-name>/g, '[命令]')
    .replace(/<local-command-stdout>[\s\S]*?<\/local-command-stdout>/g, '')
    .replace(/^Command: \S+\s*/m, '')
    .trim()
  const firstLine = cleaned.split('\n').find((l) => l.trim()) || '(空会话)'
  return firstLine.length > maxLen ? firstLine.slice(0, maxLen) + '…' : firstLine
}

/**
 * 单个会话文件元数据：stat + 头部若干行 + 尾部若干行。
 * 头部找 cwd / 首条用户消息；尾部找最后时间戳 / 分支。
 */
export function sessionMeta(projectDir, fileName) {
  const filePath = path.join(projectDir, fileName)
  const sessionId = fileName.replace(/\.jsonl$/, '')
  let stat
  try {
    stat = fs.statSync(filePath)
  } catch {
    return null
  }

  let cwd = null
  let title = null
  let startTime = null
  let gitBranch = null
  let version = null

  // 头部扫描：找到首条真实用户消息为止（跳过 queue-operation 等元记录）
  for (const line of readHeadLines(filePath, 256 * 1024)) {
    const rec = safeParse(line)
    if (!rec) continue
    if (!cwd && rec.cwd) cwd = rec.cwd
    if (!gitBranch && rec.gitBranch) gitBranch = rec.gitBranch
    if (!version && rec.version) version = rec.version
    if (!startTime && rec.timestamp) startTime = rec.timestamp
    if (title) break
    if (rec.type === 'user' && rec.message?.role === 'user') {
      const raw = extractText(rec.message.content, true)
      // 跳过命令回显、系统注入的纯标签消息，继续找下一条真实用户输入
      const isCommand =
        raw.startsWith('<command') || raw.startsWith('[命令]')
      const isMeta = raw.startsWith('<local-command') || raw.startsWith('<bash-')
      if (raw && !isCommand && !isMeta) {
        title = toTitle(raw)
      }
    }
  }

  // 尾部扫描：最后时间戳 / 分支（会话中途切分支时以尾部为准）
  const tail = readTailLines(filePath).map(safeParse).filter(Boolean)
  let lastTime = null
  for (let i = tail.length - 1; i >= 0; i--) {
    if (tail[i].timestamp) {
      lastTime = tail[i].timestamp
      break
    }
  }
  for (let i = tail.length - 1; i >= 0; i--) {
    if (tail[i].gitBranch) {
      gitBranch = tail[i].gitBranch
      break
    }
  }

  return {
    sessionId,
    projectDir: path.basename(projectDir),
    cwd,
    title: title || '(无用户消息)',
    startTime,
    lastTime: lastTime || startTime,
    mtimeMs: stat.mtimeMs,
    sizeBytes: stat.size,
    gitBranch: gitBranch || null,
    version: version || null,
  }
}

/** 列出某项目目录下全部会话，按 mtime 倒序 */
export function sessionsOfProject(projectDir) {
  let files
  try {
    files = fs.readdirSync(projectDir)
  } catch {
    return []
  }
  const metas = []
  for (const f of files) {
    if (!f.endsWith('.jsonl')) continue
    const meta = sessionMeta(projectDir, f)
    if (!meta) continue
    // jsonl 头部扫不到首条用户消息时（超长上下文记录挤到文件深处），
    // 用 history.jsonl 兜底取会话首条输入
    if (meta.title === '(无用户消息)') {
      meta.title = historyTitle(meta.sessionId) || meta.title
    }
    metas.push(meta)
  }
  metas.sort((a, b) => b.mtimeMs - a.mtimeMs)
  return metas
}

/**
 * 全部项目列表。此层只 stat 不读内容：
 * 会话数 = .jsonl 文件数，最近活跃 = 最新 mtime。
 * cwd 真实路径延迟到 sessionsOfProject 里从记录里取（目录名编码有歧义）。
 * cwdByDir 做一个进程级缓存，展开项目后回填。
 */
const cwdByDir = new Map()

export function listProjects() {
  let dirs
  try {
    dirs = fs.readdirSync(PROJECTS_DIR, { withFileTypes: true })
  } catch {
    return []
  }
  const projects = []
  for (const d of dirs) {
    if (!d.isDirectory()) continue
    const full = path.join(PROJECTS_DIR, d.name)
    let jsonlFiles
    try {
      jsonlFiles = fs.readdirSync(full).filter((f) => f.endsWith('.jsonl'))
    } catch {
      continue
    }
    if (jsonlFiles.length === 0) continue
    let lastMtime = 0
    let totalSize = 0
    let newestFile = null
    for (const f of jsonlFiles) {
      try {
        const st = fs.statSync(path.join(full, f))
        totalSize += st.size
        if (st.mtimeMs > lastMtime) {
          lastMtime = st.mtimeMs
          newestFile = f
        }
      } catch {
        /* 文件可能正被删掉，跳过 */
      }
    }
    // 目录名编码有歧义（'-' 既来自 '/' 也可能是字面 '-'），
    // 真实路径从最新会话文件的记录里取，读不到再用解码兜底
    if (!cwdByDir.has(d.name) && newestFile) {
      const cwd = readHeadCwd(path.join(full, newestFile))
      if (cwd) cwdByDir.set(d.name, cwd)
    }
    projects.push({
      dir: d.name,
      cwd: cwdByDir.get(d.name) || decodeDirName(d.name),
      sessionCount: jsonlFiles.length,
      lastActiveMs: lastMtime,
      totalSizeBytes: totalSize,
    })
  }
  projects.sort((a, b) => b.lastActiveMs - a.lastActiveMs)
  return projects
}

/** 从文件记录里找 cwd：先扫头部，找不到再扫尾部（首行可能是超长上下文记录） */
function readHeadCwd(filePath) {
  for (const line of readHeadLines(filePath, 128 * 1024)) {
    const rec = safeParse(line)
    if (rec?.cwd) return rec.cwd
  }
  for (const line of readTailLines(filePath, 64 * 1024)) {
    const rec = safeParse(line)
    if (rec?.cwd) return rec.cwd
  }
  return null
}

/** 目录名 -> 大致可读路径（无法区分字面 '-'，仅供兜底） */
function decodeDirName(dir) {
  const s = dir.replace(/^-/, '')
  // 盘符/根路径按 macOS 惯例还原
  return '/' + s.replaceAll('-', '/')
}

/** 展开项目时回填真实 cwd */
export function rememberCwd(dirName, cwd) {
  if (cwd) cwdByDir.set(dirName, cwd)
}

/**
 * 流式解析整个会话文件，输出消息流（会话详情页用）。
 * dirPath 为完整目录路径（项目目录或归档目录均可）。
 * 只保留 user / assistant 记录，工具调用折叠为一条摘要。
 */
export async function readSessionMessages(dirPath, sessionId) {
  const filePath = path.join(dirPath, `${sessionId}.jsonl`)
  if (!fs.existsSync(filePath)) return null

  const rl = readline.createInterface({
    input: fs.createReadStream(filePath, { encoding: 'utf8' }),
    crlfDelay: Infinity,
  })

  const messages = []
  // 合并连续 assistant 记录（一次回复可能分多条记录写入）
  let pendingAssistant = null
  const flush = () => {
    if (pendingAssistant) {
      messages.push(pendingAssistant)
      pendingAssistant = null
    }
  }

  for await (const line of rl) {
    const rec = safeParse(line)
    if (!rec) continue
    if (rec.isSidechain) continue // 子代理内部链路不展示

    if (rec.type === 'summary' && rec.summary) {
      flush()
      messages.push({ role: 'summary', text: rec.summary })
      continue
    }

    if (rec.type === 'user' && rec.message) {
      flush()
      const text = extractText(rec.message.content)
      const isMeta = text.startsWith('<local-command') || text.startsWith('<bash-')
      if (!text || isMeta) continue
      messages.push({
        role: 'user',
        text: text
          .replace(/<user_message>\n?/g, '')
          .replace(/\n?<\/user_message>/g, ''),
        time: rec.timestamp,
      })
      continue
    }

    if (rec.type === 'assistant' && rec.message) {
      const blocks = Array.isArray(rec.message.content) ? rec.message.content : []
      const text = blocks
        .filter((b) => b.type === 'text')
        .map((b) => b.text)
        .join('\n')
      const tools = blocks
        .filter((b) => b.type === 'tool_use')
        .map((b) => ({
          name: b.name,
          input: b.input,
        }))
      if (!text && tools.length === 0) continue
      if (!pendingAssistant) {
        pendingAssistant = { role: 'assistant', text: '', tools: [], time: rec.timestamp }
      }
      if (text) pendingAssistant.text += (pendingAssistant.text ? '\n' : '') + text
      pendingAssistant.tools.push(...tools)
    }
  }
  flush()
  return messages
}

/** 消息数统计（user+assistant 记录数），带缓存；列表页显示用。dirPath 为完整目录路径 */
const msgCountCache = new Map()
export function countMessages(dirPath, sessionId) {
  const filePath = path.join(dirPath, sessionId + '.jsonl')
  try {
    const st = fs.statSync(filePath)
    const cached = msgCountCache.get(filePath)
    if (cached && cached.mtimeMs === st.mtimeMs) return cached.count
    let count = 0
    const rl = readline.createInterface({
      input: fs.createReadStream(filePath, { encoding: 'utf8' }),
      crlfDelay: Infinity,
    })
    rl.on('line', (line) => {
      const t = line.slice(line.indexOf('"type":"') + 8, line.indexOf('"type":"') + 20)
      if (t.startsWith('user"') || t.startsWith('assistant"')) count++
    })
    return new Promise((resolve) => {
      rl.on('close', () => {
        msgCountCache.set(filePath, { mtimeMs: st.mtimeMs, count })
        resolve(count)
      })
    })
  } catch {
    return 0
  }
}
