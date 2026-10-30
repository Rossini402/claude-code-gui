import type { ChatMessage, Project, SessionMeta } from './types'

/**
 * 双路径 API：
 * - Tauri 桌面端 → invoke 走 Rust 数据层（主路径）
 * - 浏览器调试 → fetch 走 Node server（server/ 目录已冻结，仅维护调试用）
 */
const isTauri =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

async function get<T>(url: string): Promise<T> {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`${res.status} ${await res.text()}`)
  return res.json()
}

async function post<T>(url: string, body?: unknown): Promise<T> {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  })
  if (!res.ok) throw new Error(`${res.status} ${await res.text()}`)
  return res.json()
}

type Invoker = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>
let invoke: Invoker | null = null
if (isTauri) {
  // 动态 import 避免浏览器模式打包进 Tauri 运行时
  import('@tauri-apps/api/core').then((m) => {
    invoke = m.invoke as Invoker
  })
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) throw new Error('invoke 仅在桌面端可用')
  // invoke 模块是异步加载的，等它就绪
  for (let i = 0; i < 100 && !invoke; i++) {
    await new Promise((r) => setTimeout(r, 20))
  }
  if (!invoke) throw new Error('Tauri IPC 未就绪')
  return invoke<T>(cmd, args)
}

export function fetchProjects(): Promise<Project[]> {
  return isTauri ? call('list_projects') : get<{ projects: Project[] }>('/api/projects').then((d) => d.projects)
}

export function fetchSessions(dir: string): Promise<SessionMeta[]> {
  return isTauri
    ? call('get_sessions', { dir })
    : get<{ sessions: SessionMeta[] }>(`/api/projects/${encodeURIComponent(dir)}/sessions`).then((d) => d.sessions)
}

export function fetchRecent(limit = 30): Promise<SessionMeta[]> {
  return isTauri
    ? call('get_recent_sessions', { limit })
    : get<{ sessions: SessionMeta[] }>(`/api/sessions/recent?limit=${limit}`).then((d) => d.sessions)
}

export function fetchMessages(dir: string, sessionId: string, archived = false): Promise<ChatMessage[]> {
  return isTauri
    ? call('get_messages', { dir, sessionId, archived })
    : get<{ messages: ChatMessage[] }>(
        `/api/sessions/${encodeURIComponent(dir)}/${encodeURIComponent(sessionId)}/messages${archived ? '?archived=1' : ''}`,
      ).then((d) => d.messages)
}

export function searchAll(q: string, limit = 30): Promise<SessionMeta[]> {
  return isTauri
    ? call('search_sessions', { q, limit })
    : get<{ sessions: SessionMeta[] }>(`/api/search?q=${encodeURIComponent(q)}&limit=${limit}`).then((d) => d.sessions)
}

export function fetchArchived(): Promise<SessionMeta[]> {
  return isTauri
    ? call('get_archived_sessions')
    : get<{ sessions: SessionMeta[] }>('/api/archived').then((d) => d.sessions)
}

export function archiveSession(dir: string, sessionId: string): Promise<unknown> {
  return isTauri
    ? call('archive_session', { dir, sessionId })
    : post(`/api/sessions/${encodeURIComponent(dir)}/${encodeURIComponent(sessionId)}/archive`)
}

export function restoreSession(dir: string, sessionId: string): Promise<unknown> {
  return isTauri
    ? call('restore_session', { dir, sessionId })
    : post(`/api/sessions/${encodeURIComponent(dir)}/${encodeURIComponent(sessionId)}/restore`)
}

export type TerminalApp = 'terminal' | 'ghostty'

export function openTerminal(cwd: string, sessionId: string, terminal: TerminalApp = 'terminal'): Promise<unknown> {
  return isTauri
    ? call('open_terminal', { cwd, sessionId, terminal })
    : post('/api/open-terminal', { cwd, sessionId, terminal })
}
