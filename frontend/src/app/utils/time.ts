export function formatUpdatedLabel(value?: string | null, prefix = 'Updated') {
  const raw = String(value ?? '').trim()
  if (!raw) return 'Waiting for backend'

  const parsedDate = parseBackendTimestamp(raw)
  if (!parsedDate) return `${prefix} ${raw}`

  const diffMs = Date.now() - parsedDate.getTime()
  const absMs = Math.abs(diffMs)
  const suffix = diffMs < 0 ? 'from now' : 'ago'

  if (absMs < 5_000) return `${prefix} just now`
  if (absMs < 60_000) return `${prefix} ${Math.round(absMs / 1_000)}s ${suffix}`
  if (absMs < 3_600_000) return `${prefix} ${Math.round(absMs / 60_000)}m ${suffix}`
  if (absMs < 86_400_000) return `${prefix} ${Math.round(absMs / 3_600_000)}h ${suffix}`
  if (absMs < 604_800_000) return `${prefix} ${Math.round(absMs / 86_400_000)}d ${suffix}`

  return `${prefix} ${parsedDate.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })}`
}

function parseBackendTimestamp(raw: string) {
  const numeric = Number(raw)
  if (Number.isFinite(numeric)) {
    const ms = numeric > 1_000_000_000_000
      ? numeric
      : numeric > 1_000_000_000
        ? numeric * 1000
        : null
    if (ms !== null) {
      const date = new Date(ms)
      return Number.isNaN(date.getTime()) ? null : date
    }
  }

  const parsed = Date.parse(raw)
  if (Number.isNaN(parsed)) return null
  return new Date(parsed)
}
