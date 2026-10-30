<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import {
  archiveSession,
  fetchArchived,
  fetchProjects,
  fetchRecent,
  fetchSessions,
  openTerminal,
  restoreSession,
  searchAll,
  type TerminalApp,
} from './api'
import type { Project, SessionMeta } from './types'
import { copyText } from './clipboard'
import ProjectSidebar from './components/ProjectSidebar.vue'
import SessionList from './components/SessionList.vue'
import SessionDetail from './components/SessionDetail.vue'

const projects = ref<Project[]>([])
const projectsLoading = ref(true)
const search = ref('')
const selectedDir = ref<string | null>(null)

type Tab = 'recent' | 'project' | 'archived'
const tab = ref<Tab>('recent')
const sessions = ref<SessionMeta[]>([])
const sessionsLoading = ref(false)
const recent = ref<SessionMeta[]>([])
const archivedSessions = ref<SessionMeta[] | null>(null)

/** 全局搜索（会话内容全文检索） */
const searchQ = ref('')
const searching = ref(false)
const searchResults = ref<SessionMeta[]>([])
const inSearch = computed(() => searchQ.value.trim().length >= 2)

/** 会话详情浮层 */
const viewing = ref<{
  dir: string
  sessionId: string
  title: string
  cwd: string | null
  archived: boolean
} | null>(null)

const filteredProjects = computed(() => {
  const q = search.value.trim().toLowerCase()
  if (!q) return projects.value
  return projects.value.filter(
    (p) => p.cwd.toLowerCase().includes(q) || p.dir.toLowerCase().includes(q),
  )
})

const selectedProject = computed(
  () => projects.value.find((p) => p.dir === selectedDir.value) ?? null,
)

/** 当前展示的会话列表：搜索态优先，否则按 tab */
const currentSessions = computed(() => {
  if (inSearch.value) return searchResults.value
  if (tab.value === 'project') return sessions.value
  if (tab.value === 'archived') return archivedSessions.value ?? []
  return recent.value
})

const currentLoading = computed(() => {
  if (inSearch.value) return searching.value
  return tab.value === 'project' ? sessionsLoading.value : false
})

const emptyText = computed(() => {
  if (inSearch.value) return `没有匹配「${searchQ.value.trim()}」的会话`
  if (tab.value === 'project') return '该项目暂无会话'
  if (tab.value === 'archived') return '暂无归档会话'
  return '暂无最近会话'
})

async function loadProjects() {
  projectsLoading.value = true
  try {
    projects.value = await fetchProjects()
  } finally {
    projectsLoading.value = false
  }
}

async function loadRecent() {
  recent.value = await fetchRecent(30)
}

async function loadArchived() {
  archivedSessions.value = await fetchArchived()
}

watch(selectedDir, async (dir) => {
  if (!dir) return
  tab.value = 'project'
  searchQ.value = ''
  sessionsLoading.value = true
  sessions.value = []
  try {
    sessions.value = await fetchSessions(dir)
  } finally {
    sessionsLoading.value = false
  }
})

watch(tab, (t) => {
  // 切 tab 退出搜索态；归档 tab 懒加载
  if (inSearch.value) searchQ.value = ''
  if (t === 'archived' && archivedSessions.value === null) loadArchived()
})

/** 搜索防抖 */
let searchTimer: ReturnType<typeof setTimeout> | undefined
watch(searchQ, (q) => {
  clearTimeout(searchTimer)
  if (q.trim().length < 2) {
    searchResults.value = []
    searching.value = false
    return
  }
  searchTimer = setTimeout(async () => {
    searching.value = true
    try {
      searchResults.value = await searchAll(q.trim())
    } finally {
      searching.value = false
    }
  }, 350)
})

function selectProject(dir: string) {
  // 点击已选中项目时切回项目 tab（可能正在看最近会话）
  if (selectedDir.value === dir) {
    tab.value = 'project'
  } else {
    selectedDir.value = dir
  }
}

async function refresh() {
  await Promise.all([loadProjects(), loadRecent()])
  if (tab.value === 'archived') loadArchived()
  if (selectedDir.value) {
    sessions.value = await fetchSessions(selectedDir.value)
  }
}

async function copyPath(p: string) {
  await copyText(p)
}

function openSession(s: SessionMeta) {
  viewing.value = {
    dir: s.projectDir,
    sessionId: s.sessionId,
    title: s.title,
    cwd: s.cwd || s.projectCwd || null,
    archived: !inSearch.value && tab.value === 'archived',
  }
}

async function onArchive(s: SessionMeta) {
  await archiveSession(s.projectDir, s.sessionId)
  // 从各列表移除
  for (const list of [sessions.value, recent.value, searchResults.value]) {
    const i = list.findIndex(
      (x) => x.sessionId === s.sessionId && x.projectDir === s.projectDir,
    )
    if (i !== -1) list.splice(i, 1)
  }
  if (viewing.value?.sessionId === s.sessionId) viewing.value = null
  archivedSessions.value = null // 失效，进归档 tab 时重拉
  loadProjects() // 会话数/大小变化
}

async function onRestore(s: SessionMeta) {
  await restoreSession(s.projectDir, s.sessionId)
  archivedSessions.value = null
  if (viewing.value?.sessionId === s.sessionId) viewing.value = null
  loadProjects()
  loadRecent()
  if (selectedDir.value === s.projectDir) {
    sessions.value = await fetchSessions(selectedDir.value)
  }
}

async function onOpenTerminal(s: SessionMeta, app: TerminalApp) {
  const cwd = s.cwd || s.projectCwd
  if (!cwd) return
  try {
    await openTerminal(cwd, s.sessionId, app)
  } catch (e) {
    alert(`打开终端失败：${(e as Error).message}`)
  }
}

onMounted(async () => {
  await Promise.all([loadProjects(), loadRecent()])
})
</script>

<template>
  <div class="layout">
    <ProjectSidebar
      :projects="filteredProjects"
      :all-count="projects.length"
      :loading="projectsLoading"
      :selected="selectedDir"
      v-model:search="search"
      @select="selectProject"
      @refresh="refresh"
    />

    <main class="main">
      <header class="main-header">
        <nav class="tabs">
          <button
            class="tab"
            :class="{ active: !inSearch && tab === 'recent' }"
            @click="tab = 'recent'"
          >
            最近会话
          </button>
          <button
            class="tab"
            :class="{ active: !inSearch && tab === 'project', disabled: !selectedProject }"
            :disabled="!selectedProject"
            @click="tab = 'project'"
          >
            项目会话
            <span v-if="selectedProject" class="tab-count">{{ sessions.length }}</span>
          </button>
          <button
            class="tab"
            :class="{ active: !inSearch && tab === 'archived' }"
            @click="tab = 'archived'"
          >
            归档
            <span v-if="archivedSessions" class="tab-count">{{
              archivedSessions.length
            }}</span>
          </button>
        </nav>

        <div class="global-search">
          <span v-if="inSearch" class="search-tag">搜索结果</span>
          <input
            v-model="searchQ"
            class="search-input"
            type="text"
            placeholder="搜索全部会话内容…"
            spellcheck="false"
          />
          <button
            v-if="searchQ"
            class="search-clear"
            title="清除"
            @click="searchQ = ''"
          >
            ×
          </button>
        </div>

        <div v-if="!inSearch && tab === 'project' && selectedProject" class="project-banner">
          <span class="project-path" :title="selectedProject.cwd">{{
            selectedProject.cwd
          }}</span>
          <button
            class="btn-copy"
            title="复制项目路径"
            @click="copyPath(selectedProject.cwd)"
          >
            复制路径
          </button>
        </div>
      </header>

      <SessionList
        :sessions="currentSessions"
        :loading="currentLoading"
        :show-project="inSearch || tab !== 'project'"
        :highlight-query="inSearch ? searchQ.trim() : ''"
        :mode="!inSearch && tab === 'archived' ? 'archived' : 'normal'"
        :empty-text="emptyText"
        @open="openSession"
        @archive="onArchive"
        @restore="onRestore"
        @open-terminal="onOpenTerminal"
      />
    </main>

    <SessionDetail
      v-if="viewing"
      :dir="viewing.dir"
      :session-id="viewing.sessionId"
      :title="viewing.title"
      :cwd="viewing.cwd"
      :archived="viewing.archived"
      @close="viewing = null"
    />
  </div>
</template>

<style scoped>
.layout {
  display: flex;
  height: 100%;
}

.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.main-header {
  padding: 14px 20px 0;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}

.tabs {
  display: flex;
  gap: 4px;
}

.tab {
  background: none;
  border: none;
  color: var(--text-dim);
  font-size: 14px;
  padding: 8px 14px 10px;
  border-bottom: 2px solid transparent;
}

.tab.active {
  color: var(--text);
  border-bottom-color: var(--accent);
}

.tab.disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.tab-count {
  margin-left: 6px;
  font-size: 12px;
  color: var(--text-faint);
}

.global-search {
  position: relative;
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 8px;
}

.search-tag {
  font-size: 12px;
  color: var(--accent);
  white-space: nowrap;
}

.search-input {
  width: 260px;
  background: var(--bg-panel);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 13px;
  padding: 6px 26px 6px 10px;
  outline: none;
}

.search-input:focus {
  border-color: var(--accent);
}

.search-clear {
  position: absolute;
  right: 6px;
  background: none;
  border: none;
  color: var(--text-faint);
  font-size: 15px;
  padding: 2px 6px;
}

.search-clear:hover {
  color: var(--text);
}

.project-banner {
  display: flex;
  align-items: center;
  gap: 8px;
  padding-bottom: 8px;
  min-width: 0;
}

.project-path {
  font-family: var(--mono);
  font-size: 12px;
  color: var(--text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 480px;
}

.btn-copy {
  flex-shrink: 0;
  background: none;
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--text-dim);
  font-size: 12px;
  padding: 2px 8px;
}

.btn-copy:hover {
  color: var(--text);
  border-color: var(--text-faint);
}
</style>
