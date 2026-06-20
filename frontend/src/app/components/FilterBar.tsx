import { useState, type ReactNode } from 'react'
import { Filter, ChevronDown, Save, PinOff } from 'lucide-react'
import type { EdgeVisibilityLevel, GraphFilters, NodeType, EdgeType, LanguageFilter, SavedView } from '../types'

interface FilterBarProps {
  filters: GraphFilters
  onFiltersChange: (f: GraphFilters) => void
  savedViews?: SavedView[]
  onApplyView?: (view: SavedView) => void
  onSaveView?: () => void
  onUnpinAll?: () => void
}

const ALL_NODE_TYPES: NodeType[] = ['File', 'Module', 'Struct', 'Class', 'Object', 'Enum', 'Trait', 'Impl', 'Function', 'Method', 'Component', 'Hook', 'Interface', 'TypeAlias', 'Property', 'Signal', 'Handler', 'Endpoint', 'Macro', 'ExternalCrate']
const ALL_EDGE_TYPES: EdgeType[] = ['Contains', 'Imports', 'Uses', 'Calls', 'Renders', 'ApiCall', 'EndpointHandler', 'Implements', 'TypeReference', 'DataFlow', 'ModDeclaration', 'ExternalDependency']

const NODE_COLORS: Record<NodeType, string> = {
  File: '#3B82F6', Module: '#8B5CF6', Struct: '#06B6D4', Class: '#0EA5E9', Object: '#38BDF8', Enum: '#F59E0B',
  Trait: '#10B981', Impl: '#6366F1', Function: '#EC4899', Method: '#F97316',
  Component: '#14B8A6', Hook: '#A855F7', Interface: '#22C55E', TypeAlias: '#84CC16',
  Property: '#FACC15', Signal: '#FB7185', Handler: '#F472B6',
  Endpoint: '#E11D48', Macro: '#EF4444', ExternalCrate: '#7D8795',
}

const DEPTH_OPTIONS: Array<{ value: 1 | 2 | 3 | 'full'; label: string; title: string }> = [
  { value: 1, label: '1', title: 'Show one-hop neighborhood' },
  { value: 2, label: '2', title: 'Show two-hop neighborhood' },
  { value: 3, label: '3', title: 'Show three-hop neighborhood' },
  { value: 'full', label: 'Full', title: 'Show the full current graph' },
]
const EDGE_VISIBILITY_OPTIONS: Array<{ value: EdgeVisibilityLevel; label: string; title: string }> = [
  { value: 'Essential', label: 'Ess', title: 'Essential containment and high-confidence edges only' },
  { value: 'Semantic', label: 'Sem', title: 'Semantic edges: calls, uses, route handlers and type references' },
  { value: 'All', label: 'All', title: 'All detected edge types' },
]
const LANGUAGE_FILTERS: Array<{ id: LanguageFilter; label: string; color: string }> = [
  { id: 'rust', label: 'Rust', color: '#EC4899' },
  { id: 'typescript', label: 'TS/JS', color: '#14B8A6' },
  { id: 'python', label: 'Python', color: '#0EA5E9' },
  { id: 'qml', label: 'QML', color: '#38BDF8' },
  { id: 'endpoints', label: 'Endpoints', color: '#E11D48' },
  { id: 'external', label: 'External', color: '#7D8795' },
]

export function FilterBar({ filters, onFiltersChange, savedViews = [], onApplyView, onSaveView, onUnpinAll }: FilterBarProps) {
  const [expanded, setExpanded] = useState(false)

  const toggleNodeType = (t: NodeType) => {
    const next = new Set(filters.nodeTypes)
    next.has(t) ? next.delete(t) : next.add(t)
    onFiltersChange({ ...filters, nodeTypes: next })
  }

  const toggleEdgeType = (t: EdgeType) => {
    const next = new Set(filters.edgeTypes)
    next.has(t) ? next.delete(t) : next.add(t)
    onFiltersChange({ ...filters, edgeTypes: next })
  }

  const toggleLanguage = (language: LanguageFilter) => {
    const next = new Set(filters.languages)
    next.has(language) ? next.delete(language) : next.add(language)
    onFiltersChange({ ...filters, languages: next })
  }

  const hiddenFilterCount = (ALL_NODE_TYPES.length - filters.nodeTypes.size)
    + (ALL_EDGE_TYPES.length - filters.edgeTypes.size)
    + (LANGUAGE_FILTERS.length - filters.languages.size)
    + (!filters.showTests ? 1 : 0)
    + (!filters.showExternal ? 1 : 0)
    + (!filters.showDetached ? 1 : 0)
    + (filters.onlyPublicAPI ? 1 : 0)

  return (
    <div
      className="absolute top-3 left-1/2 -translate-x-1/2 z-10"
      style={{ fontFamily: 'Inter, sans-serif', width: 'min(980px, calc(100% - 48px))' }}
    >
      <div
        className="rounded-xl"
        style={{
          background: 'var(--cc-overlay)',
          border: '1px solid var(--cc-border)',
          boxShadow: 'var(--cc-shadow)',
          backdropFilter: 'blur(14px)',
          overflow: 'hidden',
        }}
      >
        <div
          className="flex items-center gap-2 px-3"
          style={{ minHeight: 36 }}
          onClick={() => setExpanded(!expanded)}
        >
          <Filter size={13} color="var(--cc-text-subtle)" />
          <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontWeight: 750 }}>Filters</span>
          {hiddenFilterCount > 0 && (
            <span style={{ fontSize: 10, padding: '2px 6px', borderRadius: 999, background: 'rgba(100,116,139,0.10)', color: 'var(--cc-text-subtle)', border: '1px solid var(--cc-border)' }}>
              {hiddenFilterCount} hidden
            </span>
          )}

          <div
            className="flex items-center gap-2 min-w-0 flex-1 overflow-x-auto"
            style={{ scrollbarWidth: 'none' }}
            onClick={e => e.stopPropagation()}
          >
            <SegmentGroup
              label="Depth"
              options={DEPTH_OPTIONS}
              value={filters.depth}
              onChange={value => onFiltersChange({ ...filters, depth: value })}
            />
            <SegmentGroup
              label="Edges"
              options={EDGE_VISIBILITY_OPTIONS}
              value={filters.edgeVisibility}
              onChange={value => onFiltersChange({ ...filters, edgeVisibility: value })}
            />
            <ToggleChip
              label="Tests"
              enabled={filters.showTests}
              title={filters.showTests ? 'Tests are visible' : 'Tests are hidden'}
              onToggle={() => onFiltersChange({ ...filters, showTests: !filters.showTests })}
            />
            <ToggleChip
              label="External"
              enabled={filters.showExternal}
              title={filters.showExternal ? 'External dependencies are visible' : 'External dependencies are hidden'}
              onToggle={() => onFiltersChange({ ...filters, showExternal: !filters.showExternal })}
            />
            <ToggleChip
              label="Detached"
              enabled={filters.showDetached}
              title={filters.showDetached ? 'Detached files are visible' : 'Detached files are hidden'}
              onToggle={() => onFiltersChange({ ...filters, showDetached: !filters.showDetached })}
            />
            <ToggleChip
              label="Public API"
              enabled={filters.onlyPublicAPI}
              title={filters.onlyPublicAPI ? 'Only public API nodes are visible' : 'All visibility levels are visible'}
              onToggle={() => onFiltersChange({ ...filters, onlyPublicAPI: !filters.onlyPublicAPI })}
              inverse
            />
          </div>

          <div className="flex items-center gap-1 shrink-0" onClick={e => e.stopPropagation()}>
            {onUnpinAll && <IconAction title="Unpin all nodes" onClick={onUnpinAll}><PinOff size={12} /></IconAction>}
            {onSaveView && <IconAction title="Save current view" onClick={onSaveView}><Save size={12} /></IconAction>}
            <button
              title={expanded ? 'Collapse advanced filters' : 'Expand advanced filters'}
              onClick={() => setExpanded(!expanded)}
              className="flex items-center justify-center rounded-lg"
              style={{ width: 26, height: 26, border: '1px solid var(--cc-border)', color: 'var(--cc-text-subtle)', background: 'var(--cc-surface)' }}
            >
              <ChevronDown size={13} style={{ transform: expanded ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s' }} />
            </button>
          </div>
        </div>

        {expanded && (
          <div style={{ borderTop: '1px solid var(--cc-border)', padding: '10px 12px', maxHeight: 360, overflow: 'auto' }}>
            <div className="grid gap-4" style={{ gridTemplateColumns: '180px minmax(220px, 1fr) minmax(220px, 1fr)' }}>
              <div>
                <GroupTitle>Languages</GroupTitle>
                <div className="flex flex-wrap gap-1.5">
                  {LANGUAGE_FILTERS.map(language => (
                    <FilterChip
                      key={language.id}
                      label={language.label}
                      active={filters.languages.has(language.id)}
                      color={language.color}
                      onToggle={() => toggleLanguage(language.id)}
                    />
                  ))}
                </div>
                {!!savedViews.length && onApplyView && (
                  <div style={{ marginTop: 10 }}>
                    <GroupTitle>Saved views</GroupTitle>
                    <div className="flex flex-wrap gap-1.5">
                      {savedViews.map(view => (
                        <FilterChip key={view.id} label={view.name} active color="#64748B" onToggle={() => onApplyView(view)} />
                      ))}
                    </div>
                  </div>
                )}
              </div>

              <div>
                <GroupTitle>Node types</GroupTitle>
                <div className="flex flex-wrap gap-1.5">
                  {ALL_NODE_TYPES.map(t => (
                    <FilterChip key={t} label={t} active={filters.nodeTypes.has(t)} color={NODE_COLORS[t]} onToggle={() => toggleNodeType(t)} />
                  ))}
                </div>
              </div>

              <div>
                <GroupTitle>Edge types</GroupTitle>
                <div className="flex flex-wrap gap-1.5">
                  {ALL_EDGE_TYPES.map(t => (
                    <FilterChip key={t} label={t} active={filters.edgeTypes.has(t)} color="#64748B" onToggle={() => toggleEdgeType(t)} />
                  ))}
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

function SegmentGroup<T extends string | number>({ label, options, value, onChange }: {
  label: string
  options: Array<{ value: T; label: string; title: string }>
  value: T
  onChange: (value: T) => void
}) {
  return (
    <div className="flex items-center shrink-0 rounded-lg" style={{ background: 'var(--cc-surface)', border: '1px solid var(--cc-border)', padding: 2 }}>
      <span style={{ fontSize: 10, color: 'var(--cc-text-faint)', padding: '0 6px', fontWeight: 700 }}>{label}</span>
      {options.map(option => {
        const active = option.value === value
        return (
          <button
            key={String(option.value)}
            title={option.title}
            onClick={() => onChange(option.value)}
            className="rounded-md"
            style={{
              height: 24,
              minWidth: option.label === 'Full' ? 38 : 25,
              padding: '0 7px',
              fontSize: 10,
              lineHeight: 1,
              background: active ? 'var(--cc-selected-soft)' : 'transparent',
              color: active ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
              border: active ? '1px solid rgba(14,165,233,0.35)' : '1px solid transparent',
              fontWeight: active ? 750 : 600,
              cursor: 'pointer',
            }}
          >
            {option.label}
          </button>
        )
      })}
    </div>
  )
}

function ToggleChip({ label, enabled, onToggle, title, inverse }: { label: string; enabled: boolean; onToggle: () => void; title: string; inverse?: boolean }) {
  const highlighted = inverse ? enabled : enabled
  return (
    <button
      title={title}
      onClick={onToggle}
      className="rounded-lg shrink-0"
      style={{
        height: 28,
        padding: '0 9px',
        fontSize: 10,
        background: highlighted ? 'var(--cc-selected-soft)' : 'var(--cc-surface)',
        color: highlighted ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
        border: highlighted ? '1px solid rgba(14,165,233,0.30)' : '1px solid var(--cc-border)',
        cursor: 'pointer',
        fontWeight: highlighted ? 700 : 600,
        opacity: enabled ? 1 : 0.72,
        whiteSpace: 'nowrap',
      }}
    >
      {label}
    </button>
  )
}

function IconAction({ children, title, onClick }: { children: ReactNode; title: string; onClick: () => void }) {
  return (
    <button
      title={title}
      onClick={onClick}
      className="flex items-center justify-center rounded-lg"
      style={{ width: 26, height: 26, border: '1px solid var(--cc-border)', color: 'var(--cc-text-subtle)', background: 'var(--cc-surface)', cursor: 'pointer' }}
    >
      {children}
    </button>
  )
}

function FilterChip({ label, active, color, onToggle }: { label: string; active: boolean; color: string; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      title={active ? `${label} is visible` : `${label} is hidden`}
      style={{
        padding: '3px 8px',
        fontSize: 10,
        borderRadius: 6,
        background: active ? `${color}16` : 'var(--cc-surface)',
        color: active ? color : 'var(--cc-text-faint)',
        border: `1px solid ${active ? color + '40' : 'var(--cc-border)'}`,
        cursor: 'pointer',
        transition: 'all 0.15s',
        opacity: active ? 1 : 0.58,
        whiteSpace: 'nowrap',
      }}
    >
      {label}
    </button>
  )
}

function GroupTitle({ children }: { children: ReactNode }) {
  return (
    <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 750, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>
      {children}
    </p>
  )
}
