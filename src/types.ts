export interface Project {
  dir: string
  cwd: string
  sessionCount: number
  lastActiveMs: number
  totalSizeBytes: number
}

export interface SessionMeta {
  sessionId: string
  projectDir: string
  cwd: string | null
  title: string
  startTime: string | null
  lastTime: string | null
  mtimeMs: number
  sizeBytes: number
  gitBranch: string | null
  version: string | null
  messageCount?: number
  /** recent 接口附带的所属项目路径 */
  projectCwd?: string
  /** search 接口附带：匹配片段与命中次数 */
  snippet?: string
  matchCount?: number
  /** archived 接口附带 */
  archived?: boolean
}

export interface ToolCall {
  name: string
  input: Record<string, unknown>
}

export interface ChatMessage {
  role: 'user' | 'assistant' | 'summary'
  text: string
  time?: string
  tools?: ToolCall[]
}
