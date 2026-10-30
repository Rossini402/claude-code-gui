<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import type { TerminalApp } from '../api'

defineProps<{
  label?: string
  disabled?: boolean
  /** 已打开成功的短暂反馈文案（替换 label 显示） */
  doneLabel?: string
  done?: boolean
}>()

const emit = defineEmits<{ select: [app: TerminalApp] }>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)

function onDocClick(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) open.value = false
}
onMounted(() => document.addEventListener('click', onDocClick))
onBeforeUnmount(() => document.removeEventListener('click', onDocClick))
</script>

<template>
  <div class="term-menu" ref="root">
    <button
      class="mini"
      :disabled="disabled"
      @click.stop="open = !open"
    >
      {{ done ? (doneLabel ?? '已打开 ✓') : (label ?? '终端') }}<span v-if="!done" class="arrow">▾</span>
    </button>
    <div v-if="open" class="menu" @click.stop>
      <button class="item" @click="open = false; emit('select', 'terminal')">Terminal.app</button>
      <button class="item" @click="open = false; emit('select', 'ghostty')">Ghostty</button>
    </div>
  </div>
</template>

<style scoped>
.term-menu {
  position: relative;
  flex-shrink: 0;
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

.arrow {
  margin-left: 3px;
  font-size: 9px;
}

.menu {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  background: var(--bg-active);
  border: 1px solid #3a4252;
  border-radius: 8px;
  padding: 4px;
  z-index: 20;
  min-width: 130px;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.45);
}

.item {
  display: block;
  width: 100%;
  background: none;
  border: none;
  border-radius: 5px;
  color: var(--text);
  font-size: 12.5px;
  padding: 6px 10px;
  text-align: left;
  white-space: nowrap;
}

.item:hover {
  background: var(--bg-hover);
}
</style>
