<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { fetchMessages, openTerminal, type TerminalApp } from '../api'
import type { ChatMessage, ToolCall } from '../types'
import { copyText } from '../clipboard'
import { absTime } from '../format'
import TerminalMenu from './TerminalMenu.vue'

const props = defineProps<{
  dir: string
  sessionId: string
  title: string
  cwd?: string | null
  archived?: boolean
}>()

const emit = defineEmits<{ close: [] }>()

const messages = ref<ChatMessage[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

/** 在终端打开状态：idle | opening | done */
const terminalState = ref<'idle' | 'opening' | 'done'>('idle')

async function openInTerminal(app: TerminalApp) {
  if (!props.cwd) return
  terminalState.value = 'opening'
  try {
    await openTerminal(props.cwd, props.sessionId, app)
    terminalState.value = 'done'
    setTimeout(() => (terminalState.value = 'idle'), 1500)
  } catch (e) {
    terminalState.value = 'idle'
    alert(`打开终端失败：${(e as Error).message}`)
  }
}

/** 长会话分批渲染，避免一次挂载几千个节点 */
const renderCount = ref(150)
const rendered = computed(() => messages.value.slice(0, renderCount.value))

/** 工具调用是否展开（按消息内索引） */
const expandedTools = ref(new Set<string>())
function toggleTool(key: string) {
  const s = expandedTools.value
  if (s.has(key)) s.delete(key)
  else s.add(key)
  expandedTools.value = new Set(s)
}

/** 工具输入的一行摘要 */
function toolSummary(t: ToolCall): string {
  const i = t.input || {}
  const pick =
    (i.command as string) ||
    (i.file_path as string) ||
    (i.pattern as string) ||
    (i.url as string) ||
    (i.prompt as string) ||
    (i.description as string) ||
    (i.path as string) ||
    ''
  const oneLine = String(pick).split('\n')[0]
  return oneLine.length > 90 ? oneLine.slice(0, 90) + '…' : oneLine
}

const copied = ref(false)
async function copyResume() {
  await copyText(`claude --resume ${props.sessionId}`)
  copied.value = true
  setTimeout(() => (copied.value = false), 1200)
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(async () => {
  document.addEventListener('keydown', onKey)
  try {
    const msgs = await fetchMessages(props.dir, props.sessionId, !!props.archived)
    messages.value = msgs
  } catch (e) {
    error.value = String((e as Error).message || e)
  } finally {
    loading.value = false
  }
})

onUnmounted(() => document.removeEventListener('keydown', onKey))
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="panel">
      <header class="detail-header">
        <div class="detail-title-wrap">
          <h2 class="detail-title">{{ title }}</h2>
          <span class="detail-sid">{{ sessionId }}</span>
        </div>
        <div class="detail-actions">
          <TerminalMenu
            v-if="cwd && !archived"
            label="在终端打开"
            :done="terminalState === 'done'"
            :disabled="terminalState === 'opening'"
            @select="openInTerminal"
          />
          <button class="btn" :class="{ done: copied }" @click="copyResume">
            {{ copied ? '已复制 ✓' : '复制 resume 命令' }}
          </button>
          <button class="btn" @click="emit('close')">关闭 (Esc)</button>
        </div>
      </header>

      <div class="detail-body">
        <div v-if="loading" class="hint">加载会话内容…</div>
        <div v-else-if="error" class="hint error">{{ error }}</div>
        <div v-else-if="!messages.length" class="hint">会话内没有可展示的消息</div>

        <template v-else>
          <div
            v-for="(m, i) in rendered"
            :key="i"
            class="msg"
            :class="m.role"
          >
            <div class="msg-role">
              <template v-if="m.role === 'user'">你</template>
              <template v-else-if="m.role === 'assistant'">Claude</template>
              <template v-else>摘要</template>
              <span v-if="m.time" class="msg-time">{{
                absTime(new Date(m.time).getTime())
              }}</span>
            </div>

            <div v-if="m.role === 'summary'" class="msg-bubble summary">{{ m.text }}</div>
            <div v-else class="msg-bubble">
              <pre class="msg-text">{{ m.text }}</pre>

              <!-- 工具调用折叠区 -->
              <div v-if="m.tools?.length" class="tools">
                <div
                  v-for="(t, ti) in m.tools"
                  :key="ti"
                  class="tool-item"
                >
                  <button class="tool-head" @click="toggleTool(`${i}-${ti}`)">
                    <span class="tool-name">{{ t.name }}</span>
                    <span class="tool-desc">{{ toolSummary(t) }}</span>
                    <span class="tool-arrow">{{
                      expandedTools.has(`${i}-${ti}`) ? '▾' : '▸'
                    }}</span>
                  </button>
                  <pre
                    v-if="expandedTools.has(`${i}-${ti}`)"
                    class="tool-input">{{ JSON.stringify(t.input, null, 2) }}</pre>
                </div>
              </div>
            </div>
          </div>

          <div v-if="renderCount < messages.length" class="load-more">
            <button class="btn" @click="renderCount += 150">
              加载更多（{{ renderCount }}/{{ messages.length }}）
            </button>
          </div>
          <div v-else class="list-end">— 共 {{ messages.length }} 条 —</div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(6, 8, 12, 0.72);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  z-index: 10;
}

.panel {
  width: min(920px, 100%);
  height: 100%;
  max-height: 900px;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 12px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.detail-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 18px;
  border-bottom: 1px solid var(--border);
}

.detail-title-wrap {
  min-width: 0;
}

.detail-title {
  margin: 0;
  font-size: 15px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.detail-sid {
  font-family: var(--mono);
  font-size: 11px;
  color: var(--text-faint);
}

.detail-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.btn {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12.5px;
  padding: 5px 12px;
}

.btn:hover {
  color: var(--text);
  border-color: var(--text-faint);
}

.btn.done {
  color: #7bc47f;
  border-color: rgba(123, 196, 127, 0.4);
}

.detail-body {
  flex: 1;
  overflow-y: auto;
  padding: 18px;
}

.hint {
  padding: 60px;
  text-align: center;
  color: var(--text-faint);
}

.hint.error {
  color: #e06c75;
}

.msg {
  margin-bottom: 14px;
}

.msg-role {
  font-size: 11.5px;
  color: var(--text-faint);
  margin-bottom: 4px;
  display: flex;
  align-items: baseline;
  gap: 8px;
}

.msg.user .msg-role {
  color: var(--accent);
}

.msg-time {
  font-family: var(--mono);
  font-weight: 400;
}

.msg-bubble {
  border-radius: 10px;
  padding: 10px 14px;
  background: var(--assistant-bg);
  border: 1px solid var(--border);
}

.msg.user .msg-bubble {
  background: var(--user-bg);
}

.msg-bubble.summary {
  background: var(--summary-bg);
  font-size: 12.5px;
  color: #8fbf9f;
}

.msg-text {
  margin: 0;
  font-family: inherit;
  font-size: 13.5px;
  line-height: 1.65;
  white-space: pre-wrap;
  word-break: break-word;
}

.tools {
  margin-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tool-item {
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
}

.tool-head {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  background: var(--bg-panel);
  border: none;
  padding: 5px 10px;
  text-align: left;
  font-size: 12px;
}

.tool-head:hover {
  background: var(--bg-hover);
}

.tool-name {
  font-family: var(--mono);
  color: #61afef;
  flex-shrink: 0;
}

.tool-desc {
  color: var(--text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
  flex: 1;
}

.tool-arrow {
  color: var(--text-faint);
  flex-shrink: 0;
}

.tool-input {
  margin: 0;
  padding: 8px 10px;
  background: var(--bg);
  font-family: var(--mono);
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--text-dim);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 300px;
  overflow-y: auto;
}

.load-more {
  text-align: center;
  padding: 12px 0;
}

.list-end {
  text-align: center;
  color: var(--text-faint);
  font-size: 12px;
  padding: 16px 0 4px;
}
</style>
