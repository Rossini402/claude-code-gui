/** 相对时间（中文），如「3 分钟前」 */
export function relTime(ms: number): string {
  const diff = Date.now() - ms
  const m = Math.floor(diff / 60_000)
  if (m < 1) return '刚刚'
  if (m < 60) return `${m} 分钟前`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h} 小时前`
  const d = Math.floor(h / 24)
  if (d < 30) return `${d} 天前`
  const mo = Math.floor(d / 30)
  if (mo < 12) return `${mo} 个月前`
  return `${Math.floor(mo / 12)} 年前`
}

/** 绝对时间：今天只显示时分，否则含日期 */
export function absTime(ms: number): string {
  const d = new Date(ms)
  const now = new Date()
  const sameDay = d.toDateString() === now.toDateString()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  if (sameDay) return hm
  const y = d.getFullYear() === now.getFullYear() ? '' : `${d.getFullYear()}/`
  return `${y}${d.getMonth() + 1}/${d.getDate()} ${hm}`
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(0)} KB`
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`
  return `${(n / 1024 ** 3).toFixed(2)} GB`
}

/** 路径取最后一段作为项目名 */
export function baseName(p: string): string {
  const s = p.replace(/\/+$/, '')
  return s.slice(s.lastIndexOf('/') + 1) || '/'
}

/** 从路径里截出主目录段的紧凑显示：/Users/yichen/Desktop/04_Product/x → …/04_Product/x */
export function shortPath(p: string): string {
  const HOME = '/Users/'
  if (p.startsWith(HOME)) {
    const rest = p.slice(HOME.length)
    const segs = rest.split('/')
    if (segs.length <= 1) return p
    return '…/' + segs.slice(1).join('/')
  }
  return p
}

/** 把文本按关键词切开（大小写不敏感），供高亮渲染：[{t, hit}] */
export function splitHighlight(text: string, q: string): { t: string; hit: boolean }[] {
  if (!q || !text) return [{ t: text, hit: false }]
  const lowerText = text.toLowerCase()
  const lowerQ = q.toLowerCase()
  const out: { t: string; hit: boolean }[] = []
  let i = 0
  while (i <= text.length - lowerQ.length) {
    const idx = lowerText.indexOf(lowerQ, i)
    if (idx === -1) break
    if (idx > i) out.push({ t: text.slice(i, idx), hit: false })
    out.push({ t: text.slice(idx, idx + lowerQ.length), hit: true })
    i = idx + lowerQ.length
  }
  if (i < text.length) out.push({ t: text.slice(i), hit: false })
  return out
}
