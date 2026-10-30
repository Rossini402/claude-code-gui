// 本地 HTTP 服务：API + 生产环境静态托管 dist。
// 零依赖，Node >= 20。仅绑定 127.0.0.1（含打开终端等敏感端点，不接受外部访问）。
import http from 'node:http'
import fs from 'node:fs'
import path from 'node:path'
import { execFile, spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import {
  listProjects,
  sessionsOfProject,
  sessionMeta,
  readSessionMessages,
  countMessages,
  rememberCwd,
  PROJECTS_DIR,
  CLAUDE_DIR,
} from './scanner.js'
import { searchSessions } from './search.js'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const DIST_DIR = path.join(__dirname, '..', 'dist')
const ARCHIVE_ROOT = path.join(CLAUDE_DIR, 'projects-archive')
const PORT = process.env.PORT || 3777

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
}

function sendJson(res, status, data) {
  const body = JSON.stringify(data)
  res.writeHead(status, {
    'Content-Type': 'application/json; charset=utf-8',
    'Cache-Control': 'no-store',
  })
  res.end(body)
}

function readBody(req) {
  return new Promise((resolve) => {
    let data = ''
    req.on('data', (c) => (data += c))
    req.on('end', () => {
      try {
        resolve(JSON.parse(data || '{}'))
      } catch {
        resolve({})
      }
    })
  })
}

/** 防目录穿越：只允许合法的项目目录名（原样 basename，无路径分隔符） */
function safeDirName(dir) {
  if (!/^[\w.\-]+$/.test(dir)) return null
  return dir
}

function projectPath(dir) {
  return path.join(PROJECTS_DIR, dir)
}

function archivePath(dir) {
  return path.join(ARCHIVE_ROOT, dir)
}

async function handleApi(req, res, url) {
  // GET /api/projects
  if (url.pathname === '/api/projects') {
    return sendJson(res, 200, { projects: listProjects() })
  }

  // GET /api/search?q=关键词 —— 会话全文搜索
  if (url.pathname === '/api/search') {
    const q = url.searchParams.get('q') || ''
    const limit = Math.min(parseInt(url.searchParams.get('limit') || '30', 10) || 30, 100)
    if (q.trim().length < 2) return sendJson(res, 200, { sessions: [] })
    const hits = await searchSessions(q, limit)
    // 合并会话元数据（标题/时间/分支等）
    const sessions = (
      await Promise.all(
        hits.map((h) => {
          const sep = h.key.lastIndexOf('/')
          const dirName = h.key.slice(0, sep)
          const fileName = h.key.slice(sep + 1)
          const meta = sessionMeta(projectPath(dirName), fileName)
          return meta ? { ...meta, ...h, projectDir: dirName } : null
        }),
      )
    ).filter(Boolean)
    return sendJson(res, 200, { sessions })
  }

  // GET /api/archived —— 已归档会话列表
  if (url.pathname === '/api/archived') {
    let dirs
    try {
      dirs = fs.readdirSync(ARCHIVE_ROOT, { withFileTypes: true })
    } catch {
      return sendJson(res, 200, { sessions: [] })
    }
    const all = []
    for (const d of dirs) {
      if (!d.isDirectory()) continue
      const metas = sessionsOfProject(archivePath(d.name))
      all.push(...metas.map((s) => ({ ...s, archived: true })))
    }
    all.sort((a, b) => b.mtimeMs - a.mtimeMs)
    return sendJson(res, 200, { sessions: all })
  }

  // GET /api/projects/:dir/sessions
  let m = url.pathname.match(/^\/api\/projects\/([^/]+)\/sessions$/)
  if (m) {
    const dir = safeDirName(decodeURIComponent(m[1]))
    if (!dir) return sendJson(res, 400, { error: 'bad dir' })
    const sessions = sessionsOfProject(projectPath(dir))
    if (sessions.length) rememberCwd(dir, sessions[0].cwd)
    // 消息数并行统计（每个文件流式行计数，结果有缓存）
    const counts = await Promise.all(
      sessions.map((s) => countMessages(projectPath(dir), s.sessionId)),
    )
    sessions.forEach((s, i) => (s.messageCount = counts[i]))
    return sendJson(res, 200, { sessions })
  }

  // GET /api/sessions/recent?limit=20 —— 跨项目最近会话
  if (url.pathname === '/api/sessions/recent') {
    const limit = Math.min(parseInt(url.searchParams.get('limit') || '20', 10) || 20, 100)
    // 对全部项目做一次轻量扫描（stat + head/tail），取最新 N 条
    const projects = listProjects()
    const all = []
    for (const p of projects) {
      // 只展开最近活跃的项目，凑够 limit*3 候选即提前停（按 mtime 排序后前面的一定更晚）
      const sessions = sessionsOfProject(projectPath(p.dir))
      all.push(...sessions.map((s) => ({ ...s, projectCwd: p.cwd })))
      if (all.length >= limit * 3) break
    }
    all.sort((a, b) => b.mtimeMs - a.mtimeMs)
    const top = all.slice(0, limit)
    const counts = await Promise.all(
      top.map((s) => countMessages(projectPath(s.projectDir), s.sessionId)),
    )
    top.forEach((s, i) => (s.messageCount = counts[i]))
    return sendJson(res, 200, { sessions: top })
  }

  // GET /api/sessions/:dir/:id/messages —— 会话消息流（?archived=1 读归档目录）
  m = url.pathname.match(/^\/api\/sessions\/([^/]+)\/([^/]+)\/messages$/)
  if (m) {
    const dir = safeDirName(decodeURIComponent(m[1]))
    const id = decodeURIComponent(m[2])
    if (!dir || !/^[\w-]+$/.test(id)) return sendJson(res, 400, { error: 'bad params' })
    const base = url.searchParams.get('archived') === '1' ? archivePath(dir) : projectPath(dir)
    const messages = await readSessionMessages(base, id)
    if (!messages) return sendJson(res, 404, { error: 'session not found' })
    return sendJson(res, 200, { messages })
  }

  // POST /api/open-terminal —— 新窗口执行 cd <cwd> && claude --resume <id>，terminal: terminal(默认) | ghostty
  if (url.pathname === '/api/open-terminal') {
    if (req.method !== 'POST') return sendJson(res, 405, { error: 'method not allowed' })
    const body = await readBody(req)
    const cwd = String(body.cwd || '')
    const sessionId = String(body.sessionId || '')
    const terminal = String(body.terminal || 'terminal')
    if (process.platform !== 'darwin') {
      return sendJson(res, 501, { error: '仅支持 macOS' })
    }
    if (!/^[\w-]+$/.test(sessionId)) return sendJson(res, 400, { error: 'bad sessionId' })
    if (!path.isAbsolute(cwd) || !fs.existsSync(cwd)) {
      return sendJson(res, 400, { error: 'bad cwd' })
    }
    const sq = (s) => "'" + s.replace(/'/g, "'\\''") + "'"
    const shellCmd = `cd ${sq(cwd)} && claude --resume ${sessionId}`

    if (terminal === 'ghostty') {
      const bin = '/Applications/Ghostty.app/Contents/MacOS/ghostty'
      if (!fs.existsSync(bin)) {
        return sendJson(res, 400, { error: '未找到 Ghostty（/Applications/Ghostty.app）' })
      }
      // -e 不经 shell：拆成独立 argv（zsh -l -c），否则 login 会把整串当可执行文件名
      const child = spawn(bin, ['-e', '/bin/zsh', '-l', '-c', shellCmd], { detached: true, stdio: 'ignore' })
      child.unref()
      return sendJson(res, 200, { ok: true })
    }

    const asEsc = shellCmd.replace(/\\/g, '\\\\').replace(/"/g, '\\"')
    const script = `tell application "Terminal"\nactivate\ndo script "${asEsc}"\nend tell`
    execFile('osascript', ['-e', script], (err) => {
      if (err) return sendJson(res, 500, { error: String(err.message || err) })
      sendJson(res, 200, { ok: true })
    })
    return
  }

  // POST /api/sessions/:dir/:id/archive —— 归档（移动到 projects-archive，可逆）
  m = url.pathname.match(/^\/api\/sessions\/([^/]+)\/([^/]+)\/archive$/)
  if (m) {
    const dir = safeDirName(decodeURIComponent(m[1]))
    const id = decodeURIComponent(m[2])
    if (!dir || !/^[\w-]+$/.test(id)) return sendJson(res, 400, { error: 'bad params' })
    const src = path.join(projectPath(dir), `${id}.jsonl`)
    if (!fs.existsSync(src)) return sendJson(res, 404, { error: 'session not found' })
    fs.mkdirSync(archivePath(dir), { recursive: true })
    fs.renameSync(src, path.join(archivePath(dir), `${id}.jsonl`))
    return sendJson(res, 200, { ok: true })
  }

  // POST /api/sessions/:dir/:id/restore —— 从归档恢复
  m = url.pathname.match(/^\/api\/sessions\/([^/]+)\/([^/]+)\/restore$/)
  if (m) {
    const dir = safeDirName(decodeURIComponent(m[1]))
    const id = decodeURIComponent(m[2])
    if (!dir || !/^[\w-]+$/.test(id)) return sendJson(res, 400, { error: 'bad params' })
    const src = path.join(archivePath(dir), `${id}.jsonl`)
    if (!fs.existsSync(src)) return sendJson(res, 404, { error: 'archived session not found' })
    const dst = path.join(projectPath(dir), `${id}.jsonl`)
    if (fs.existsSync(dst)) return sendJson(res, 409, { error: '目标已存在同名会话' })
    fs.mkdirSync(projectPath(dir), { recursive: true })
    fs.renameSync(src, dst)
    return sendJson(res, 200, { ok: true })
  }

  sendJson(res, 404, { error: 'not found' })
}

function serveStatic(req, res, url) {
  let filePath = path.join(DIST_DIR, decodeURIComponent(url.pathname))
  if (!filePath.startsWith(DIST_DIR)) {
    res.writeHead(403)
    return res.end('Forbidden')
  }
  if (!fs.existsSync(filePath) || fs.statSync(filePath).isDirectory()) {
    // SPA 回退
    filePath = path.join(DIST_DIR, 'index.html')
  }
  if (!fs.existsSync(filePath)) {
    res.writeHead(404)
    return res.end('dist not built, run `pnpm build` first')
  }
  const ext = path.extname(filePath)
  res.writeHead(200, {
    'Content-Type': MIME[ext] || 'application/octet-stream',
    'Cache-Control': ext === '.html' ? 'no-store' : 'public, max-age=3600',
  })
  fs.createReadStream(filePath).pipe(res)
}

const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`)
  try {
    if (url.pathname.startsWith('/api/')) {
      await handleApi(req, res, url)
    } else {
      serveStatic(req, res, url)
    }
  } catch (err) {
    console.error('[api error]', url.pathname, err)
    if (!res.headersSent) sendJson(res, 500, { error: String(err?.message || err) })
  }
})

server.listen(PORT, '127.0.0.1', () => {
  console.log(`[claude-code-gui] API  http://127.0.0.1:${PORT}/api/projects`)
  console.log(
    `[claude-code-gui] 面板 http://127.0.0.1:${PORT}${fs.existsSync(DIST_DIR) ? '' : ' (dev 模式请开 http://localhost:6001)'}`,
  )
})
