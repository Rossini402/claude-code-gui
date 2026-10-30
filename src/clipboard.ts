/**
 * 剪贴板双路径：WKWebView 里 navigator.clipboard 不可靠，桌面端走 Tauri 插件。
 */
const isTauri =
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

export async function copyText(text: string): Promise<void> {
  if (isTauri) {
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
    await writeText(text)
  } else {
    await navigator.clipboard.writeText(text)
  }
}
