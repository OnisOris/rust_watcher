import { useState } from 'react'
import { Filter, X, ChevronDown } from 'lucide-react'
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

const DEPTH_OPTIONS: Array<1 | 2 | 3 | 'full'> = [1, 2, 3, 'full']
const EDGE_VISIBILITY_OPTIONS: EdgeVisibilityLevel[] = ['Essential', 'Semantic', 'All']
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

  const activeFilterCount = (ALL_NODE_TYPES.length - filters.nodeTypes.size)
    + (ALL_EDGE_TYPES.length - filters.edgeTypes.size)
    + (LANGUAGE_FILTERS.length - filters.languages.size)

  return (
    <div
      className="absolute top-3 left-1/2 -translate-x-1/2 z-10"
      style={{ fontFamily: 'Inter, sans-serif' }}
    >
      <div
        className="rounded-xl shadow-2xl"
        style={{
          background: 'var(--cc-panel)',
          border: '1px solid var(--cc-border)',
          boxShadow: 'var(--cc-shadow)',
          minWidth: 480,
        }}
      >
        {/* collapsed bar */}
        <div
          className="flex items-center gap-2 px-3 cursor-pointer"
          style={{ height: 36 }}
          onClick={() => setExpanded(!expanded)}
        >
          <Filter size={13} color="var(--cc-text-subtle)" />
          <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontWeight: 500 }}>Filters</span>

          {activeFilterCount > 0 && (
            <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 10, background: 'rgba(6,182,212,0.15)', color: '#06B6D4', border: '1px solid rgba(6,182,212,0.25)' }}>
              {activeFilterCount} hidden
            </span>
          )}

          {/* quick depth pills */}
          <div className="flex items-center gap-0.5 ml-2" onClick={e => e.stopPropagation()}>
            {DEPTH_OPTIONS.map(d => (
              <button
                key={d}
                onClick={() => onFiltersChange({ ...filters, depth: d })}
                style={{
                  padding: '2px 7px',
                  fontSize: 10,
                  borderRadius: 4,
                  background: filters.depth === d ? '#06B6D4' : 'transparent',
                  color: filters.depth === d ? '#fff' : 'var(--cc-text-subtle)',
                  border: 'none',
                  cursor: 'pointer',
                  fontWeight: filters.depth === d ? 600 : 400,
                }}
              >
                {d === 'full' ? 'Full' : `D${d}`}
              </button>
            ))}
          </div>

          <div className="flex-1" />

          {/* quick toggles */}
          <div className="flex items-center gap-1" onClick={e => e.stopPropagation()}>
            {EDGE_VISIBILITY_OPTIONS.map(level => (
              <QuickToggle
                key={level}
                label={level === 'Essential' ? 'Ess' : level === 'Semantic' ? 'Sem' : 'All'}
                active={filters.edgeVisibility === level}
                onToggle={() => onFiltersChange({ ...filters, edgeVisibility: level })}
              />
            ))}
            <QuickToggle
              label="Tests"
              active={!filters.showTests}
              onToggle={() => onFiltersChange({ ...filters, showTests: !filters.showTests })}
            />
            <QuickToggle
              label="Ext"
              active={!filters.showExternal}
              onToggle={() => onFiltersChange({ ...filters, showExternal: !filters.showExternal })}
            />
            <QuickToggle
              label="Pub only"
              active={filters.onlyPublicAPI}
              onToggle={() => onFiltersChange({ ...filters, onlyPublicAPI: !filters.onlyPublicAPI })}
            />
            {onUnpinAll && (
              <QuickToggle
                label="Unpin"
                active={false}
                onToggle={onUnpinAll}
              />
            )}
            {onSaveView && (
              <QuickToggle
                label="Save"
                active={false}
                onToggle={onSaveView}
              />
            )}
          </div>

          <ChevronDown size={13} color="var(--cc-text-subtle)" style={{ transform: expanded ? 'rotate(180deg)' : 'none', transition: 'transform 0.2s' }} />
        </div>

        {/* expanded panel */}
        {expanded && (
          <div style={{ borderTop: '1px solid var(--cc-border)', padding: '10px 12px' }}>
            <div className="flex gap-6">
              {/* languages */}
              <div style={{ maxWidth: 170 }}>
                <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>Languages</p>
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
                    <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>Views</p>
                    <div className="flex flex-wrap gap-1.5">
                      {savedViews.map(view => (
                        <FilterChip
                          key={view.id}
                          label={view.name}
                          active
                          color="#64748B"
                          onToggle={() => onApplyView(view)}
                        />
                      ))}
                    </div>
                  </div>
                )}
                <div style={{ marginTop: 10 }}>
                  <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>Quick</p>
                  <div className="flex flex-wrap gap-1.5">
                    {LANGUAGE_FILTERS.filter(language => language.id !== 'external').map(language => (
                      <FilterChip
                        key={`quick-${language.id}`}
                        label={language.label}
                        active={filters.languages.size === 1 && filters.languages.has(language.id)}
                        color={language.color}
                        onToggle={() => onFiltersChange({ ...filters, languages: new Set([language.id]) })}
                      />
                    ))}
                    <FilterChip
                      label="Diagnostics"
                      active={filters.edgeVisibility === 'Essential'}
                      color="#F59E0B"
                      onToggle={() => onFiltersChange({ ...filters, edgeVisibility: 'Essential' })}
                    />
                  </div>
                </div>
              </div>

              {/* node types */}
              <div>
                <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>Node Types</p>
                <div className="flex flex-wrap gap-1.5">
                  {ALL_NODE_TYPES.map(t => (
                    <FilterChip
                      key={t}
                      label={t}
                      active={filters.nodeTypes.has(t)}
                      color={NODE_COLORS[t]}
                      onToggle={() => toggleNodeType(t)}
                    />
                  ))}
                </div>
              </div>

              {/* edge types */}
              <div>
                <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>Edge Types</p>
                <div className="flex flex-wrap gap-1.5">
                  {ALL_EDGE_TYPES.map(t => (
                    <FilterChip
                      key={t}
                      label={t}
                      active={filters.edgeTypes.has(t)}
                      color="#7D8795"
                      onToggle={() => toggleEdgeType(t)}
                    />
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

function FilterChip({ label, active, color, onToggle }: { label: string; active: boolean; color: string; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      style={{
        padding: '3px 8px',
        fontSize: 10,
        borderRadius: 4,
        background: active ? `${color}18` : 'var(--cc-surface)',
        color: active ? color : 'var(--cc-text-faint)',
        border: `1px solid ${active ? color + '40' : 'var(--cc-border)'}`,
        cursor: 'pointer',
        transition: 'all 0.15s',
        textDecoration: active ? 'none' : 'line-through',
        opacity: active ? 1 : 0.55,
      }}
    >
      {label}
    </button>
  )
}

function QuickToggle({ label, active, onToggle }: { label: string; active: boolean; onToggle: () => void }) {
  return (
    <button
      onClick={onToggle}
      style={{
        padding: '2px 7px',
        fontSize: 10,
        borderRadius: 4,
        background: active ? 'rgba(248,113,113,0.12)' : 'transparent',
        color: active ? '#F87171' : 'var(--cc-text-subtle)',
        border: `1px solid ${active ? 'rgba(248,113,113,0.25)' : 'transparent'}`,
        cursor: 'pointer',
      }}
    >
      {active ? <><X size={9} className="inline mr-0.5" />{label}</> : label}
    </button>
  )
}
