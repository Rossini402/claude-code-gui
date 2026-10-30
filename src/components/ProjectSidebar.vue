<script setup lang="ts">
import type { Project } from '../types'
import { baseName, formatBytes, relTime } from '../format'

defineProps<{
  projects: Project[]
  allCount: number
  loading: boolean
  selected: string | null
}>()

const search = defineModel<string>('search', { default: '' })
const emit = defineEmits<{ select: [dir: string]; refresh: [] }>()
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-head">
      <h1 class="logo">Claude Code</h1>
      <button class="btn-refresh" title="刷新" @click="emit('refresh')">⟳</button>
    </div>
    <div class="search-box">
      <input
        v-model="search"
        class="search-input"
        type="text"
        placeholder="搜索项目路径…"
        spellcheck="false"
      />
    </div>

    <div v-if="loading" class="hint">加载中…</div>
    <div v-else-if="!projects.length" class="hint">
      {{ allCount ? '没有匹配的项目' : '未找到 ~/.claude/projects' }}
    </div>

    <div class="project-list">
      <button
        v-for="p in projects"
        :key="p.dir"
        class="project-item"
        :class="{ active: selected === p.dir }"
        @click="emit('select', p.dir)"
      >
        <div class="project-name" :title="p.cwd">{{ baseName(p.cwd) }}</div>
        <div class="project-meta">
          <span class="path" :title="p.cwd">{{ p.cwd }}</span>
        </div>
        <div class="project-stats">
          <span>{{ p.sessionCount }} 会话</span>
          <span>{{ formatBytes(p.totalSizeBytes) }}</span>
          <span class="time">{{ relTime(p.lastActiveMs) }}</span>
        </div>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 300px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  background: var(--bg-panel);
  display: flex;
  flex-direction: column;
}

.sidebar-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px 8px;
}

.logo {
  font-size: 15px;
  margin: 0;
  font-weight: 600;
  letter-spacing: 0.3px;
}

.btn-refresh {
  background: none;
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-dim);
  width: 24px;
  height: 24px;
  line-height: 1;
}

.btn-refresh:hover {
  color: var(--text);
}

.search-box {
  padding: 4px 16px 10px;
}

.search-input {
  width: 100%;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 13px;
  padding: 6px 10px;
  outline: none;
}

.search-input:focus {
  border-color: var(--accent);
}

.hint {
  padding: 20px 16px;
  color: var(--text-faint);
  font-size: 13px;
}

.project-list {
  flex: 1;
  overflow-y: auto;
  padding: 0 8px 12px;
}

.project-item {
  display: block;
  width: 100%;
  text-align: left;
  background: none;
  border: 1px solid transparent;
  border-radius: 8px;
  padding: 8px 10px;
  color: var(--text);
  margin-bottom: 2px;
}

.project-item:hover {
  background: var(--bg-hover);
}

.project-item.active {
  background: var(--bg-active);
  border-color: var(--border);
}

.project-name {
  font-weight: 600;
  font-size: 13.5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.project-meta {
  margin-top: 2px;
}

.path {
  font-family: var(--mono);
  font-size: 11px;
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  display: block;
}

.project-stats {
  display: flex;
  gap: 10px;
  margin-top: 4px;
  font-size: 11.5px;
  color: var(--text-dim);
}

.project-stats .time {
  margin-left: auto;
  color: var(--text-faint);
}
</style>
