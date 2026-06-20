import { useState } from 'react'
import { Clock } from 'lucide-react'
import type { AnalyzerServiceStatus, AnalyzerStatus } from '../types'
import {
  analyzerCapabilityLabel,
  analyzerEngineLabel,
  analyzerStatusColor,
  sortAnalyzers,
  summarizeAnalyzers,
} from '../api/analyzerStatus'
import { formatUpdatedLabel } from '../utils/time'

interface AnalyzerStatusSummaryProps {
  analyzers?: AnalyzerServiceStatus[]
  overallStatus: AnalyzerStatus
  message?: string | null
  lastUpdated?: string | null
  filesCount?: number
}

export function AnalyzerStatusSummary({
  analyzers = [],
  overallStatus,
  message,
  lastUpdated,
  filesCount = 0,
}: AnalyzerStatusSummaryProps) {
  const [open, setOpen] = useState(false)
  const sorted = sortAnalyzers(analyzers)
  const summary = summarizeAnalyzers(sorted)
  const color = summary.error
    ? analyzerStatusColor('Error')
    : summary.indexing
      ? analyzerStatusColor('Indexing')
      : summary.fallback || summary.stale
        ? analyzerStatusColor('Fallback')
        : analyzerStatusColor(overallStatus)

  return (
    <div className="hidden lg:flex items-center gap-2 shrink-0 relative" title={message ?? undefined}>
      <button
        type="button"
        onClick={() => setOpen(value => !value)}
        className="flex items-center gap-2 rounded-lg transition-all"
        style={{
          height: 30,
          padding: '4px 8px',
          background: open ? 'var(--cc-selected-soft)' : 'transparent',
          border: open ? '1px solid rgba(14,165,233,0.35)' : '1px solid transparent',
          cursor: 'pointer',
        }}
      >
        <span className="relative" style={{ width: 9, height: 9 }}>
          <span style={{ display: 'block', width: 9, height: 9, borderRadius: 999, background: color }} />
          {(overallStatus === 'Starting' || overallStatus === 'Indexing') && (
            <span className="animate-ping" style={{ position: 'absolute', inset: 0, borderRadius: 999, background: color, opacity: 0.65 }} />
          )}
        </span>
        <span style={{ color, fontSize: 11, fontWeight: 700 }}>{summary.label}</span>
      </button>
      <span style={{ color: 'var(--cc-text-subtle)', fontSize: 11 }}>
        <Clock size={10} className="inline mr-1" />
        {formatUpdatedLabel(lastUpdated)}
      </span>
      <span style={{ color: 'var(--cc-text-subtle)', fontSize: 11 }}>· {filesCount} files</span>

      {open && (
        <div
          className="absolute top-9 left-0 z-50 rounded-xl p-2"
          style={{
            width: 360,
            background: 'var(--cc-panel)',
            border: '1px solid var(--cc-border)',
            boxShadow: 'var(--cc-shadow)',
          }}
        >
          <div style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 800, letterSpacing: '0.07em', textTransform: 'uppercase', padding: '4px 6px 8px' }}>
            Analyzer Status
          </div>
          {sorted.length === 0 ? (
            <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', padding: 8 }}>No analyzers reported yet.</div>
          ) : sorted.map(analyzer => (
            <AnalyzerRow key={analyzer.id} analyzer={analyzer} />
          ))}
        </div>
      )}
    </div>
  )
}

function AnalyzerRow({ analyzer }: { analyzer: AnalyzerServiceStatus }) {
  const color = analyzerStatusColor(analyzer.status)
  const capabilities = analyzer.capabilities.slice(0, 5)
  return (
    <div className="rounded-lg px-2 py-2" style={{ border: '1px solid var(--cc-border)', background: 'var(--cc-surface)', marginBottom: 6 }}>
      <div className="flex items-center gap-2">
        <span style={{ width: 7, height: 7, borderRadius: 999, background: color, flexShrink: 0 }} />
        <span style={{ fontSize: 11, color: 'var(--cc-text)', fontWeight: 750 }}>{analyzer.label}</span>
        <span style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>· {analyzerEngineLabel(analyzer.engine)}</span>
        {analyzer.mode && <span style={{ fontSize: 10, color: 'var(--cc-text-faint)' }}>· {analyzer.mode}</span>}
        <span style={{ marginLeft: 'auto', fontSize: 10, color }}>{analyzer.status}</span>
      </div>
      {analyzer.message && (
        <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginTop: 5, lineHeight: 1.35 }}>{analyzer.message}</div>
      )}
      <div className="flex flex-wrap gap-1" style={{ marginTop: 6 }}>
        {capabilities.map(capability => (
          <span key={capability} style={{ fontSize: 9, color: 'var(--cc-text-subtle)', background: 'var(--cc-elevated)', border: '1px solid var(--cc-border)', borderRadius: 6, padding: '1px 5px' }}>
            {analyzerCapabilityLabel(capability)}
          </span>
        ))}
        {analyzer.capabilities.length > capabilities.length && (
          <span style={{ fontSize: 9, color: 'var(--cc-text-faint)', padding: '1px 2px' }}>+{analyzer.capabilities.length - capabilities.length}</span>
        )}
      </div>
    </div>
  )
}
