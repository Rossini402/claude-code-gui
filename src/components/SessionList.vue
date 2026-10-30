<script setup lang="ts">
import { ref } from 'vue'
import type { SessionMeta } from '../types'
import type { TerminalApp } from '../api'
import { copyText } from '../clipboard'
import { absTime, baseName, formatBytes, relTime, shortPath, splitHighlight } from '../format'
import TerminalMenu from './TerminalMenu.vue'

const props = defineProps<{
  sessions: SessionMeta[]
  loading: boolean
  showProject?: boolean
  /** 搜索态：摘要按该关键词高亮 */
  highlightQuery?: string
  /** normal=常规操作；archived=恢复操作 */
  mode?: 'normal' | 'archived'
  emptyText?: string
}>()

const emit = defineEmits<{
  open: [session: SessionMeta]
  archive: [session: SessionMeta]
  restore: [session: SessionMeta]
  openTerminal: [session: SessionMeta, terminal: TerminalApp]
}>()

/** 复制成功提示 */
const copiedId = ref<string | null>(null)

async function copyResume(s: SessionMeta) {
  await copyText(`claude --resume ${s.sessionId}`)
  copiedId.value = s.sessionId
  setTimeout(() => {
    if (copiedId.value === s.sessionId) copiedId.value = null
  }, 1200)
}

/** 归档两步确认：第一次点变成「确认归档」，3 秒内再点才生效 */
const confirmKey = ref<string | null>(null)
let confirmTimer: ReturnType<typeof setTimeout> | undefined

function keyOf(s: SessionMeta) {
  return `${s.projectDir}/${s.sessionId}`
}

function onArchiveClick(s: SessionMeta) {
  const key = keyOf(s)
  if (confirmKey.value === key) {
    clearTimeout(confirmTimer)
    confirmKey.value = null
    emit('archive', s)
  } else {
    confirmKey.value = key
    clearTimeout(confirmTimer)
    confirmTimer = setTimeout(() => {
      confirmKey.value = null
    }, 3000)
  }
}

</script>

<template>
  <div class="session-scroll">
    <div v-if="loading" class="hint">加载中…</div>
    <div v-else-if="!sessions.length" class="hint">{{ emptyText || '暂无会话' }}</div>

    <div class="session-list">
      <article
        v-for="s in sessions"
        :key="keyOf(s)"
        class="session-card"
        @click="emit('open', s)"
      >
        <div class="card-top">
          <h3 class="card-title">{{ s.title }}</h3>
          <div class="card-actions" v-if="mode !== 'archived'">
            <button
              class="mini"
              :class="{ done: copiedId === s.sessionId }"
              title="复制 claude --resume 命令"
              @click.stop="copyResume(s)"
            >
              {{ copiedId === s.sessionId ? '已复制 ✓' : 'resume' }}
            </button>
            <TerminalMenu
              :disabled="!(s.cwd || s.projectCwd)"
              @select="(app) => emit('openTerminal', s, app)"
            />
            <button
              class="mini danger"
              :class="{ confirming: confirmKey === keyOf(s) }"
              title="归档（移出 Claude Code，可随时恢复）"
              @click.stop="onArchiveClick(s)"
            >
              {{ confirmKey === keyOf(s) ? '确认归档' : '归档' }}
            </button>
          </div>
          <div class="card-actions" v-else>
            <button class="mini recover" title="恢复到 Claude Code" @click.stop="emit('restore', s)">
              恢复
            </button>
          </div>
        </div>

        <div v-if="showProject && (s.projectCwd || s.cwd)" class="card-project">
          <span class="proj-name">{{ baseName(s.projectCwd || s.cwd || '') }}</span>
          <span class="proj-path">{{ shortPath(s.projectCwd || s.cwd || '') }}</span>
        </div>

        <!-- 搜索摘要（命中片段，关键词高亮） -->
        <div v-if="s.snippet" class="card-snippet">
          <template v-for="(seg, i) in splitHighlight(s.snippet, highlightQuery || '')" :key="i">
            <mark v-if="seg.hit" class="hl">{{ seg.t }}</mark>
            <template v-else>{{ seg.t }}</template>
          </template>
        </div>

        <div class="card-meta">
          <span :title="s.lastTime || ''">{{ relTime(s.mtimeMs) }}</span>
          <span v-if="s.startTime" :title="s.startTime">始于 {{ absTime(new Date(s.startTime).getTime()) }}</span>
          <span v-if="s.messageCount != null">{{ s.messageCount }} 条消息</span>
          <span v-if="s.matchCount != null" class="hits">命中 {{ s.matchCount }} 处</span>
          <span>{{ formatBytes(s.sizeBytes) }}</span>
          <span v-if="s.gitBranch && s.gitBranch !== 'HEAD'" class="branch">⑂ {{ s.gitBranch }}</span>
          <span class="sid" :title="s.sessionId">#{{ s.sessionId.slice(0, 8) }}</span>
        </div>
      </article>
    </div>
  </div>
</template>

<style scoped>
.session-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 16px 20px 40px;
}

.hint {
  padding: 40px;
  text-align: center;
  color: var(--text-faint);
}

.session-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
  gap: 10px;
}

.session-card {
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
  cursor: pointer;
  transition: border-color 0.12s;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.session-card:hover {
  border-color: #3a4252;
}

.card-top {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.card-title {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-size: 13.5px;
  font-weight: 600;
  line-height: 1.45;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.card-actions {
  flex-shrink: 0;
  display: flex;
  gap: 4px;
}

.mini {
  background: var(--accent-soft);
  border: 1px solid transparent;
  border-radius: 5px;
  color: var(--accent);
  font-size: 11.5px;
  padding: 3px 8px;
  font-family: var(--mono);
  white-space: nowrap;
}

.mini:hover:not(:disabled) {
  border-color: var(--accent);
}

.mini:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.mini.done {
  color: #7bc47f;
  background: rgba(123, 196, 127, 0.12);
}

.mini.danger {
  color: #b46ead;
  background: rgba(180, 106, 173, 0.1);
}

.mini.danger.confirming {
  color: #fff;
  background: #a33d5e;
}

.mini.recover {
  color: #7bc47f;
  background: rgba(123, 196, 127, 0.12);
}

.card-project {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.proj-name {
  font-size: 12px;
  color: var(--accent);
  flex-shrink: 0;
  max-width: 40%;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.proj-path {
  font-family: var(--mono);
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.card-snippet {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-dim);
  border-left: 2px solid var(--border);
  padding-left: 8px;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.hl {
  background: rgba(224, 140, 63, 0.28);
  color: var(--accent);
  border-radius: 2px;
  padding: 0 1px;
}

.card-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 12px;
  font-size: 11.5px;
  color: var(--text-dim);
}

.hits {
  color: var(--accent);
}

.branch {
  color: #98c379;
}

.sid {
  margin-left: auto;
  font-family: var(--mono);
  color: var(--text-faint);
}
</style>
