import type { ReactNode } from 'react'
import { useState, useEffect, useRef } from 'react'
import { Search, File, Package, Layers, GitBranch, Zap, X, ArrowRight } from 'lucide-react'
import type { GraphNode, SearchResult } from '../types'

interface SearchCommandPaletteProps {
  nodes: GraphNode[]
  search?: (query: string) => Promise<SearchResult[]>
  open: boolean
  onClose: () => void
  onSelectNode: (id: string) => void
}

const KIND_ICONS: Record<string, ReactNode> = {
  File: <File size={13} color="#3B82F6" />,
  Module: <Package size={13} color="#8B5CF6" />,
  Struct: <Layers size={13} color="#06B6D4" />,
  Enum: <Layers size={13} color="#F59E0B" />,
  Trait: <GitBranch size={13} color="#10B981" />,
  Function: <Zap size={13} color="#EC4899" />,
  Method: <Zap size={13} color="#F97316" />,
  Macro: <Zap size={13} color="#EF4444" />,
  ExternalCrate: <Package size={13} color="#7D8795" />,
}

const KIND_COLORS: Record<string, string> = {
  File: '#3B82F6', Module: '#8B5CF6', Struct: '#06B6D4', Enum: '#F59E0B',
  Trait: '#10B981', Function: '#EC4899', Method: '#F97316', Macro: '#EF4444',
  ExternalCrate: '#7D8795', Impl: '#6366F1',
}

export function SearchCommandPalette({ nodes, search, open, onClose, onSelectNode }: SearchCommandPaletteProps) {
  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState(0)
  const [remoteResults, setRemoteResults] = useState<SearchResult[]>([])
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) {
      setQuery('')
      setCursor(0)
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open])

  const localResults = query.length < 1
    ? nodes.filter(n => n.bookmarked || n.type === 'Module' || n.type === 'Trait').slice(0, 8)
    : nodes.filter(n =>
        n.label.toLowerCase().includes(query.toLowerCase()) ||
        n.file?.toLowerCase().includes(query.toLowerCase()) ||
        n.module?.toLowerCase().includes(query.toLowerCase())
      ).slice(0, 10)

  useEffect(() => {
    let cancelled = false
    if (!search || query.length < 1) {
      setRemoteResults([])
      return
    }
    search(query).then(results => {
      if (!cancelled) setRemoteResults(results)
    }).catch(() => {
      if (!cancelled) setRemoteResults([])
    })
    return () => { cancelled = true }
  }, [query, search])

  const results = remoteResults.length > 0
    ? remoteResults.map(result => ({
        id: result.id,
        label: result.label,
        type: result.type,
        file: result.file ?? undefined,
        module: result.module ?? undefined,
        crate: result.crate ?? undefined,
        x: 0,
        y: 0,
        vx: 0,
        vy: 0,
      } satisfies GraphNode))
    : localResults

  useEffect(() => { setCursor(0) }, [query])

  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (e.key === 'ArrowDown') setCursor(c => Math.min(c + 1, results.length - 1))
      if (e.key === 'ArrowUp') setCursor(c => Math.max(c - 1, 0))
      if (e.key === 'Enter' && results[cursor]) {
        onSelectNode(results[cursor].id)
        onClose()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [open, results, cursor, onClose, onSelectNode])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center"
      style={{ paddingTop: 100, background: 'var(--cc-backdrop)', backdropFilter: 'blur(4px)' }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose() }}
    >
      <div
        className="rounded-2xl overflow-hidden shadow-2xl"
        style={{
          width: 560,
          background: 'var(--cc-panel)',
          border: '1px solid var(--cc-border)',
          boxShadow: 'var(--cc-shadow)',
          fontFamily: 'Inter, sans-serif',
        }}
      >
        {/* search input */}
        <div className="flex items-center gap-3 px-4" style={{ borderBottom: '1px solid var(--cc-border)', height: 52 }}>
          <Search size={16} color="var(--cc-text-subtle)" />
          <input
            ref={inputRef}
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Search symbol, file, trait, function…"
            style={{
              flex: 1,
              background: 'none',
              border: 'none',
              outline: 'none',
              color: 'var(--cc-text)',
              fontSize: 14,
              fontFamily: 'Inter, sans-serif',
            }}
          />
          {query && (
            <button onClick={() => setQuery('')} style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', display: 'flex' }}>
              <X size={14} />
            </button>
          )}
          <kbd style={{ fontSize: 10, padding: '2px 5px', borderRadius: 4, background: 'var(--cc-surface)', color: 'var(--cc-text-subtle)', border: '1px solid var(--cc-border)' }}>esc</kbd>
        </div>

        {/* section label */}
        <div className="px-4 py-2">
          <p style={{ fontSize: 10, color: 'var(--cc-text-faint)', fontWeight: 600, letterSpacing: '0.07em', textTransform: 'uppercase' }}>
            {query ? `Results for "${query}"` : 'Bookmarks & Modules'}
          </p>
        </div>

        {/* results */}
        <div style={{ maxHeight: 360, overflowY: 'auto', scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
          {results.length === 0 && (
            <div className="px-4 py-8 text-center">
              <p style={{ fontSize: 13, color: 'var(--cc-text-subtle)' }}>No results for "{query}"</p>
            </div>
          )}
          {results.map((node, i) => (
            <button
              key={node.id}
              onClick={() => { onSelectNode(node.id); onClose() }}
              className="flex items-center gap-3 w-full px-4 py-2.5 transition-colors"
              style={{
                background: i === cursor ? 'var(--cc-elevated)' : 'transparent',
                cursor: 'pointer',
                borderLeft: i === cursor ? '2px solid #06B6D4' : '2px solid transparent',
              }}
            >
              <div className="flex items-center justify-center rounded shrink-0" style={{ width: 28, height: 28, background: `${KIND_COLORS[node.type] ?? '#7D8795'}18` }}>
                {KIND_ICONS[node.type]}
              </div>
              <div className="flex-1 text-left min-w-0">
                <div style={{ fontSize: 13, color: 'var(--cc-text)', fontFamily: 'JetBrains Mono, monospace', fontWeight: 500 }}>{node.label}</div>
                <div style={{ fontSize: 11, color: 'var(--cc-text-subtle)', marginTop: 1 }}>
                  {node.file ?? node.module ?? node.crate ?? ''}
                </div>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <span style={{
                  fontSize: 10,
                  padding: '2px 7px',
                  borderRadius: 4,
                  background: `${KIND_COLORS[node.type] ?? '#7D8795'}18`,
                  color: KIND_COLORS[node.type] ?? '#7D8795',
                  border: `1px solid ${KIND_COLORS[node.type] ?? '#7D8795'}30`,
                }}>
                  {node.type}
                </span>
                {i === cursor && <ArrowRight size={12} color="#06B6D4" />}
              </div>
            </button>
          ))}
        </div>

        {/* footer hint */}
        <div className="flex items-center gap-4 px-4 py-2" style={{ borderTop: '1px solid var(--cc-border)' }}>
          <Hint keys={['↑', '↓']} label="navigate" />
          <Hint keys={['↵']} label="focus in graph" />
          <Hint keys={['esc']} label="close" />
        </div>
      </div>
    </div>
  )
}

function Hint({ keys, label }: { keys: string[]; label: string }) {
  return (
    <div className="flex items-center gap-1">
      {keys.map(k => (
        <kbd key={k} style={{ fontSize: 10, padding: '1px 5px', borderRadius: 3, background: 'var(--cc-surface)', color: 'var(--cc-text-muted)', border: '1px solid var(--cc-border)' }}>{k}</kbd>
      ))}
      <span style={{ fontSize: 10, color: 'var(--cc-text-faint)' }}>{label}</span>
    </div>
  )
}
