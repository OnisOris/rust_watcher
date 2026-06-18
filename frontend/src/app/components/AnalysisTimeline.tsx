import type { ReactNode } from 'react'
import { useState } from 'react'
import { ChevronUp, ChevronDown, AlertTriangle, AlertCircle, Info, Activity, GitBranch, X } from 'lucide-react'
import type { AnalysisEvent } from '../types'

interface AnalysisTimelineProps {
  events: AnalysisEvent[]
  collapsed: boolean
  onToggle: () => void
}

type Filter = 'all' | 'errors' | 'warnings' | 'analyzer' | 'graph'

const EVENT_ICONS: Record<AnalysisEvent['type'], ReactNode> = {
  info: <Info size={11} color="var(--cc-text-muted)" />,
  warning: <AlertTriangle size={11} color="#F59E0B" />,
  error: <AlertCircle size={11} color="#F87171" />,
  analyzer: <Activity size={11} color="#06B6D4" />,
  graph: <GitBranch size={11} color="#8B5CF6" />,
}

const EVENT_COLORS: Record<AnalysisEvent['type'], string> = {
  info: 'var(--cc-text-muted)',
  warning: '#F59E0B',
  error: '#F87171',
  analyzer: '#06B6D4',
  graph: '#8B5CF6',
}

export function AnalysisTimeline({ events, collapsed, onToggle }: AnalysisTimelineProps) {
  const [filter, setFilter] = useState<Filter>('all')

  const filtered = filter === 'all'
    ? events
    : events.filter(e => e.type === filter || (filter === 'errors' && e.type === 'error') || (filter === 'warnings' && e.type === 'warning'))

  const errorCount = events.filter(e => e.type === 'error').length
  const warnCount = events.filter(e => e.type === 'warning').length

  return (
    <div
      className="flex flex-col shrink-0 transition-all"
      style={{
        height: collapsed ? 36 : 130,
        background: 'var(--cc-surface)',
        borderTop: '1px solid var(--cc-border)',
        fontFamily: 'Inter, sans-serif',
        overflow: 'hidden',
      }}
    >
      {/* header bar */}
      <div
        className="flex items-center gap-3 px-3 cursor-pointer shrink-0"
        style={{ height: 36, borderBottom: collapsed ? 'none' : '1px solid var(--cc-border)' }}
        onClick={onToggle}
      >
        <Activity size={12} color="var(--cc-text-subtle)" />
        <span style={{ fontSize: 11, color: 'var(--cc-text-subtle)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase' }}>
          Analysis Timeline
        </span>

        {/* quick badges */}
        {errorCount > 0 && (
          <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 4, background: 'rgba(248,113,113,0.12)', color: '#F87171', border: '1px solid rgba(248,113,113,0.2)' }}>
            {errorCount} error{errorCount > 1 ? 's' : ''}
          </span>
        )}
        {warnCount > 0 && (
          <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 4, background: 'rgba(245,158,11,0.12)', color: '#F59E0B', border: '1px solid rgba(245,158,11,0.2)' }}>
            {warnCount} warn{warnCount > 1 ? 's' : ''}
          </span>
        )}

        <div className="flex-1" />

        {/* filter pills */}
        {!collapsed && (
          <div className="flex items-center gap-1" onClick={e => e.stopPropagation()}>
            {(['all', 'errors', 'warnings', 'analyzer', 'graph'] as Filter[]).map(f => (
              <button
                key={f}
                onClick={() => setFilter(f)}
                style={{
                  padding: '2px 7px',
                  fontSize: 10,
                  borderRadius: 4,
                  background: filter === f ? 'var(--cc-elevated)' : 'transparent',
                  color: filter === f ? 'var(--cc-text)' : 'var(--cc-text-subtle)',
                  border: filter === f ? '1px solid var(--cc-border-strong)' : '1px solid transparent',
                  cursor: 'pointer',
                  textTransform: 'capitalize',
                }}
              >
                {f}
              </button>
            ))}
          </div>
        )}

        <button
          onClick={e => { e.stopPropagation(); onToggle() }}
          style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', display: 'flex', alignItems: 'center' }}
        >
          {collapsed ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
        </button>
      </div>

      {/* events list */}
      {!collapsed && (
        <div className="overflow-y-auto flex-1 px-3 py-1" style={{ scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
          <div className="flex flex-col gap-0.5">
            {filtered.map(event => (
              <EventRow key={event.id} event={event} />
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function EventRow({ event }: { event: AnalysisEvent }) {
  return (
    <div className="flex items-center gap-2 py-0.5 rounded px-1" style={{ minHeight: 20 }}>
      <span className="shrink-0">{EVENT_ICONS[event.type]}</span>
      <span style={{ fontSize: 10, color: EVENT_COLORS[event.type], fontFamily: 'JetBrains Mono, monospace', whiteSpace: 'nowrap' }}>
        {event.file ? `[${event.file}]` : `[${event.type}]`}
      </span>
      <span style={{ fontSize: 10, color: 'var(--cc-text-muted)', flex: 1 }}>{event.message}</span>
      <span style={{ fontSize: 10, color: 'var(--cc-text-faint)', whiteSpace: 'nowrap', shrink: 0 }}>{event.timestamp}</span>
    </div>
  )
}
