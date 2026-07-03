import { useCallback, useEffect, useState } from 'react'
import { DownloadCloud, RefreshCw } from 'lucide-react'
import { cloudFetch } from '../api/cloudAuth'

interface CloudUpdateNoticeProps {
  sessionToken: string
}

interface CloudUpdateStatus {
  currentVersion: string
  latestVersion?: string
  updateAvailable: boolean
  updating: boolean
  releaseUrl?: string
  assetName?: string
  message?: string
}

export function CloudUpdateNotice({ sessionToken }: CloudUpdateNoticeProps) {
  const [status, setStatus] = useState<CloudUpdateStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const loadStatus = useCallback(async () => {
    const response = await cloudFetch('/api/cloud/update/status', {}, sessionToken)
    if (!response.ok) throw new Error(await response.text())
    setStatus(await response.json() as CloudUpdateStatus)
  }, [sessionToken])

  useEffect(() => {
    void loadStatus().catch(() => {})
    const interval = window.setInterval(() => {
      void loadStatus().catch(() => {})
    }, 5 * 60 * 1000)
    return () => window.clearInterval(interval)
  }, [loadStatus])

  async function applyUpdate() {
    setBusy(true)
    setError(null)
    try {
      const response = await cloudFetch('/api/cloud/update/apply', { method: 'POST' }, sessionToken)
      if (!response.ok) throw new Error(await response.text())
      setStatus(await response.json() as CloudUpdateStatus)
      window.setTimeout(() => window.location.reload(), 12_000)
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Update failed.')
      setBusy(false)
    }
  }

  if (!status?.updateAvailable && !status?.updating && !error) return null

  return (
    <div
      className="fixed z-50 flex items-center gap-3 rounded-xl px-3 py-2"
      style={{
        top: 12,
        left: '50%',
        transform: 'translateX(-50%)',
        background: 'var(--cc-panel)',
        border: '1px solid var(--cc-border-strong)',
        boxShadow: 'var(--cc-shadow)',
        color: 'var(--cc-text)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      <DownloadCloud size={16} color="#06B6D4" />
      <div style={{ minWidth: 0 }}>
        <div style={{ fontSize: 12, fontWeight: 760 }}>
          {status?.updating ? 'Updating Rust Watcher...' : 'New update available'}
        </div>
        <div style={{ fontSize: 11, color: error ? '#B91C1C' : 'var(--cc-text-muted)' }}>
          {error ?? status?.message ?? `${status?.currentVersion} -> ${status?.latestVersion}`}
        </div>
      </div>
      {status?.updateAvailable && !status.updating && (
        <button
          onClick={() => void applyUpdate()}
          disabled={busy}
          className="flex items-center gap-1.5 rounded-lg"
          style={{
            minHeight: 30,
            padding: '0 10px',
            border: 'none',
            background: '#06B6D4',
            color: '#fff',
            fontSize: 12,
            fontWeight: 760,
            cursor: busy ? 'wait' : 'pointer',
            whiteSpace: 'nowrap',
          }}
        >
          <RefreshCw size={13} />
          {busy ? 'Starting...' : 'Update'}
        </button>
      )}
    </div>
  )
}
