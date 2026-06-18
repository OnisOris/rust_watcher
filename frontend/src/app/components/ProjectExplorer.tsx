import { useState } from 'react'
import type { ReactNode } from 'react'
import {
  ChevronRight, ChevronDown, Folder, File, Package,
  BookMarked, Flame, Clock, FocusIcon, EyeOff, ChevronsUpDown,
  AlertTriangle, Link2, FunctionSquare
} from 'lucide-react'
import type { ProjectFile, GraphNode } from '../types'
import { HOTSPOTS } from '../mockData'

interface ProjectExplorerProps {
  files: ProjectFile[]
  nodes: GraphNode[]
  projectName?: string
  selectedNodeId: string | null
  onSelectNode: (id: string) => void
  onFocusFile: (path: string) => void
}

const CRATE_TREE = [
  {
    id: 'server', label: 'server', type: 'crate',
    children: [
      { id: 'mod-handlers', label: 'handlers', type: 'module', children: [
        { id: 'file-users', label: 'users.rs', type: 'file', nodeId: 'file-users', fns: 6, links: 18, diag: 1, complexity: 'high' as const },
        { id: 'file-posts', label: 'posts.rs', type: 'file', nodeId: 'file-posts', fns: 4, links: 11, diag: 0, complexity: 'medium' as const },
      ]},
      { id: 'mod-middleware', label: 'middleware', type: 'module', children: [
        { id: 'file-middleware', label: 'mod.rs', type: 'file', nodeId: 'mod-middleware', fns: 3, links: 6, diag: 0, complexity: 'low' as const },
      ]},
      { id: 'file-main', label: 'main.rs', type: 'file', nodeId: 'file-main', fns: 3, links: 12, diag: 0, complexity: 'medium' as const },
    ]
  },
  {
    id: 'auth', label: 'auth', type: 'crate',
    children: [
      { id: 'mod-jwt-p', label: 'jwt', type: 'module', children: [
        { id: 'file-jwt', label: 'jwt.rs', type: 'file', nodeId: 'mod-jwt', fns: 3, links: 7, diag: 0, complexity: 'medium' as const },
      ]},
      { id: 'file-auth', label: 'auth.rs', type: 'file', nodeId: 'file-auth', fns: 5, links: 14, diag: 0, complexity: 'high' as const },
    ]
  },
  {
    id: 'db', label: 'db', type: 'crate',
    children: [
      { id: 'mod-queries-p', label: 'queries', type: 'module', children: [
        { id: 'file-queries', label: 'queries.rs', type: 'file', nodeId: 'mod-queries', fns: 12, links: 30, diag: 0, complexity: 'high' as const },
      ]},
      { id: 'file-db', label: 'mod.rs', type: 'file', nodeId: 'crate-db', fns: 8, links: 22, diag: 2, complexity: 'high' as const },
    ]
  },
  {
    id: 'models', label: 'models', type: 'crate',
    children: [
      { id: 'file-user-m', label: 'user.rs', type: 'file', nodeId: 'struct-user', fns: 4, links: 9, diag: 0, complexity: 'low' as const },
      { id: 'file-post-m', label: 'post.rs', type: 'file', nodeId: 'struct-post', fns: 2, links: 5, diag: 0, complexity: 'low' as const },
    ]
  },
]

const RECENT = [
  { label: 'handlers/users.rs', nodeId: 'file-users', ago: '2m ago' },
  { label: 'db/queries.rs', nodeId: 'mod-queries', ago: '8m ago' },
  { label: 'auth/jwt.rs', nodeId: 'mod-jwt', ago: '15m ago' },
  { label: 'create_user', nodeId: 'fn-create-user', ago: '22m ago' },
]

const BOOKMARKS = [
  { label: 'fn create_user', nodeId: 'fn-create-user', kind: 'Function' },
  { label: 'struct User', nodeId: 'struct-user', kind: 'Struct' },
]

const complexityColor = { low: '#34D399', medium: '#F59E0B', high: '#F87171' }
const complexityBg = { low: 'rgba(52,211,153,0.12)', medium: 'rgba(245,158,11,0.12)', high: 'rgba(248,113,113,0.12)' }

export function ProjectExplorer({ files, nodes, projectName, selectedNodeId, onSelectNode, onFocusFile }: ProjectExplorerProps) {
  const [expandedCrates, setExpandedCrates] = useState<Set<string>>(new Set(['server', 'auth']))
  const [expandedModules, setExpandedModules] = useState<Set<string>>(new Set(['mod-handlers']))
  const [activeSection, setActiveSection] = useState<'workspace' | 'recent' | 'bookmarks' | 'hotspots'>('workspace')
  const tree = files.length > 0 ? buildFileTree(files, nodes) : CRATE_TREE
  const recent = files.slice(0, 6).map(file => ({ label: file.path, nodeId: file.id, ago: 'indexed' }))
  const bookmarks = nodes
    .filter(node => node.bookmarked)
    .slice(0, 8)
    .map(node => ({ label: node.label, nodeId: node.id, kind: node.type }))
  const hotspots = files
    .filter(file => file.linksCount > 0 || file.functionsCount > 0 || file.diagnosticsCount > 0)
    .sort((a, b) => (b.linksCount + b.functionsCount + b.diagnosticsCount) - (a.linksCount + a.functionsCount + a.diagnosticsCount))
    .slice(0, 8)
    .map(file => ({
      id: file.id,
      label: file.path,
      reason: `${file.functionsCount} functions, ${file.linksCount} links${file.diagnosticsCount ? `, ${file.diagnosticsCount} diagnostics` : ''}`,
      severity: file.complexity === 'high' ? 'high' as const : file.complexity === 'medium' ? 'medium' as const : 'low' as const,
    }))

  const toggleExpand = (id: string, type: 'crate' | 'module') => {
    const setter = type === 'crate' ? setExpandedCrates : setExpandedModules
    setter(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }

  return (
    <div
      className="flex flex-col overflow-hidden shrink-0"
      style={{
        width: 280,
        background: 'var(--cc-panel)',
        borderRight: '1px solid var(--cc-border)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      {/* header */}
      <div className="flex items-center justify-between px-3 shrink-0" style={{ height: 40, borderBottom: '1px solid var(--cc-border)' }}>
        <span style={{ color: 'var(--cc-text-muted)', fontSize: 11, fontWeight: 600, letterSpacing: '0.08em', textTransform: 'uppercase' }}>
          {projectName ?? 'workspace'}
        </span>
        <div className="flex items-center gap-1">
          <IconBtn icon={<FocusIcon size={13} />} title="Focus current file" />
          <IconBtn icon={<EyeOff size={13} />} title="Hide external crates" />
          <IconBtn icon={<ChevronsUpDown size={13} />} title="Collapse all" />
        </div>
      </div>

      {/* section tabs */}
      <div className="flex items-center shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', padding: '0 6px' }}>
        {(['workspace', 'recent', 'bookmarks', 'hotspots'] as const).map(s => (
          <button
            key={s}
            onClick={() => setActiveSection(s)}
            style={{
              padding: '6px 8px',
              fontSize: 11,
              color: activeSection === s ? 'var(--cc-text)' : 'var(--cc-text-subtle)',
              borderBottom: activeSection === s ? '2px solid #06B6D4' : '2px solid transparent',
              background: 'none',
              cursor: 'pointer',
              transition: 'all 0.15s',
              fontWeight: activeSection === s ? 500 : 400,
              textTransform: 'capitalize',
            }}
          >
            {s}
          </button>
        ))}
      </div>

      {/* content */}
      <div className="overflow-y-auto flex-1 py-2" style={{ scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
        {activeSection === 'workspace' && (
          <div>
            {tree.map(crate => (
              <div key={crate.id}>
                {/* crate row */}
                <button
                  className="flex items-center gap-2 w-full px-3 py-1.5 transition-colors"
                  style={{ background: 'none', cursor: 'pointer', color: 'var(--cc-text)' }}
                  onClick={() => toggleExpand(crate.id, 'crate')}
                >
                  {expandedCrates.has(crate.id) ? <ChevronDown size={12} color="var(--cc-text-subtle)" /> : <ChevronRight size={12} color="var(--cc-text-subtle)" />}
                  <Package size={13} color="#8B5CF6" />
                  <span style={{ fontSize: 12, fontWeight: 500, color: 'var(--cc-text)' }}>{crate.label}</span>
                  <span style={{ marginLeft: 'auto', fontSize: 10, color: 'var(--cc-text-faint)', background: 'var(--cc-elevated)', padding: '1px 5px', borderRadius: 4 }}>crate</span>
                </button>

                {expandedCrates.has(crate.id) && (
                  <div>
                    {crate.children.map(child => (
                      <div key={child.id}>
                        {child.type === 'module' ? (
                          <div>
                            <button
                              className="flex items-center gap-2 w-full px-3 py-1 transition-colors"
                              style={{ paddingLeft: 28, background: 'none', cursor: 'pointer' }}
                              onClick={() => toggleExpand(child.id, 'module')}
                            >
                              {expandedModules.has(child.id) ? <ChevronDown size={11} color="var(--cc-text-subtle)" /> : <ChevronRight size={11} color="var(--cc-text-subtle)" />}
                              <Folder size={12} color="#6366F1" />
                              <span style={{ fontSize: 11, color: 'var(--cc-text-muted)' }}>{child.label}</span>
                            </button>
                            {expandedModules.has(child.id) && child.children?.map(file => (
                              <FileRow
                                key={file.id}
                                file={file as any}
                                depth={3}
                                selected={selectedNodeId === file.nodeId}
                                onClick={() => onSelectNode(file.nodeId!)}
                              />
                            ))}
                          </div>
                        ) : (
                          <FileRow
                            file={child as any}
                            depth={2}
                            selected={selectedNodeId === (child as any).nodeId}
                            onClick={() => onSelectNode((child as any).nodeId!)}
                          />
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        {activeSection === 'recent' && (
          <div className="px-3 py-1">
            <SectionLabel label="Recently Touched" />
            {(recent.length > 0 ? recent : RECENT).map(r => (
              <button
                key={r.nodeId}
                onClick={() => onSelectNode(r.nodeId)}
                className="flex items-center gap-2 w-full rounded transition-colors py-1.5 px-2"
                style={{
                  background: selectedNodeId === r.nodeId ? 'rgba(6,182,212,0.08)' : 'none',
                  cursor: 'pointer',
                }}
              >
                <Clock size={11} color="var(--cc-text-subtle)" />
                <span style={{ fontSize: 12, color: 'var(--cc-text-muted)', flex: 1, textAlign: 'left', fontFamily: 'JetBrains Mono, monospace' }}>{r.label}</span>
                <span style={{ fontSize: 10, color: 'var(--cc-text-faint)' }}>{r.ago}</span>
              </button>
            ))}
          </div>
        )}

        {activeSection === 'bookmarks' && (
          <div className="px-3 py-1">
            <SectionLabel label="Bookmarked Symbols" />
            {(bookmarks.length > 0 ? bookmarks : BOOKMARKS).map(b => (
              <button
                key={b.nodeId}
                onClick={() => onSelectNode(b.nodeId)}
                className="flex items-center gap-2 w-full rounded py-1.5 px-2 transition-colors"
                style={{
                  background: selectedNodeId === b.nodeId ? 'rgba(6,182,212,0.08)' : 'none',
                  cursor: 'pointer',
                }}
              >
                <BookMarked size={11} color="#06B6D4" />
                <span style={{ fontSize: 12, color: 'var(--cc-text-muted)', flex: 1, textAlign: 'left', fontFamily: 'JetBrains Mono, monospace' }}>{b.label}</span>
                <KindBadge kind={b.kind} />
              </button>
            ))}
            {bookmarks.length === 0 && nodes.length > 0 && (
              <p style={{ fontSize: 11, color: 'var(--cc-text-faint)', padding: '8px 0' }}>No bookmarks yet</p>
            )}
          </div>
        )}

        {activeSection === 'hotspots' && (
          <div className="px-3 py-1">
            <SectionLabel label="Hotspots" />
            <p style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginBottom: 8 }}>High connectivity or complexity areas</p>
            {(hotspots.length > 0 ? hotspots : HOTSPOTS).map(h => (
              <div
                key={h.id}
                className="rounded-lg p-2 mb-2"
                style={{ background: 'var(--cc-card)', border: '1px solid var(--cc-border)' }}
              >
                <div className="flex items-center gap-2 mb-1">
                  <Flame size={11} color={h.severity === 'high' ? '#F87171' : h.severity === 'medium' ? '#F59E0B' : '#34D399'} />
                  <span style={{ fontSize: 12, color: 'var(--cc-text)', fontFamily: 'JetBrains Mono, monospace' }}>{h.label}</span>
                </div>
                <p style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginLeft: 19 }}>{h.reason}</p>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

type TreeFile = {
  id: string
  label: string
  type: 'file'
  nodeId: string
  fns: number
  links: number
  diag: number
  complexity: 'low' | 'medium' | 'high'
}

type TreeModule = {
  id: string
  label: string
  type: 'module'
  children: TreeFile[]
}

type TreeCrate = {
  id: string
  label: string
  type: 'crate'
  children: Array<TreeModule | TreeFile>
}

function buildFileTree(files: ProjectFile[], nodes: GraphNode[]): TreeCrate[] {
  const fileNodeByPath = new Map(
    nodes
      .filter(node => node.type === 'File' && node.file)
      .map(node => [node.file!, node.id]),
  )
  const crates = new Map<string, TreeCrate>()
  const moduleMap = new Map<string, TreeModule>()

  for (const file of files) {
    const crateName = file.crate || 'workspace'
    const crateNode = crates.get(crateName) ?? {
      id: crateName,
      label: crateName,
      type: 'crate' as const,
      children: [],
    }
    crates.set(crateName, crateNode)

    const treeFile: TreeFile = {
      id: file.id,
      label: file.name,
      type: 'file',
      nodeId: fileNodeByPath.get(file.path) ?? file.id,
      fns: file.functionsCount,
      links: file.linksCount,
      diag: file.diagnosticsCount,
      complexity: file.complexity,
    }

    if (file.module && file.module !== 'crate root') {
      const moduleId = `${crateName}:${file.module}`
      let moduleNode = moduleMap.get(moduleId)
      if (!moduleNode) {
        moduleNode = {
          id: moduleId,
          label: file.module,
          type: 'module',
          children: [],
        }
        moduleMap.set(moduleId, moduleNode)
        crateNode.children.push(moduleNode)
      }
      moduleNode.children.push(treeFile)
    } else {
      crateNode.children.push(treeFile)
    }
  }

  return [...crates.values()]
}

function FileRow({ file, depth, selected, onClick }: {
  file: { label: string; nodeId?: string; fns: number; links: number; diag: number; complexity: 'low' | 'medium' | 'high' }
  depth: number
  selected: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      className="flex items-center gap-1.5 w-full transition-all"
      style={{
        paddingLeft: depth * 12 + 4,
        paddingRight: 8,
        paddingTop: 4,
        paddingBottom: 4,
        background: selected ? 'rgba(6,182,212,0.08)' : 'none',
        borderLeft: selected ? '2px solid #06B6D4' : '2px solid transparent',
        cursor: 'pointer',
      }}
    >
      <File size={11} color="#3B82F6" />
      <span style={{ fontSize: 11, color: selected ? 'var(--cc-text)' : 'var(--cc-text-muted)', flex: 1, textAlign: 'left', fontFamily: 'JetBrains Mono, monospace' }}>
        {file.label}
      </span>
      <div className="flex items-center gap-1 shrink-0">
        {file.fns > 0 && (
          <span title={`${file.fns} functions`} style={{ fontSize: 9, color: 'var(--cc-text-faint)', display: 'flex', alignItems: 'center', gap: 1 }}>
            <FunctionSquare size={9} /> {file.fns}
          </span>
        )}
        {file.links > 0 && (
          <span title={`${file.links} links`} style={{ fontSize: 9, color: 'var(--cc-text-faint)', display: 'flex', alignItems: 'center', gap: 1 }}>
            <Link2 size={9} /> {file.links}
          </span>
        )}
        {file.diag > 0 && (
          <span title={`${file.diag} diagnostics`} style={{ fontSize: 9, color: '#F87171', display: 'flex', alignItems: 'center', gap: 1 }}>
            <AlertTriangle size={9} /> {file.diag}
          </span>
        )}
        <span style={{
          fontSize: 9,
          padding: '1px 4px',
          borderRadius: 3,
          background: complexityBg[file.complexity],
          color: complexityColor[file.complexity],
        }}>
          {file.complexity}
        </span>
      </div>
    </button>
  )
}

function KindBadge({ kind }: { kind: string }) {
  const colors: Record<string, string> = {
    Function: '#EC4899', Struct: '#06B6D4', Trait: '#10B981', Enum: '#F59E0B', Module: '#8B5CF6',
  }
  const color = colors[kind] ?? '#7D8795'
  return (
    <span style={{
      fontSize: 9,
      padding: '1px 5px',
      borderRadius: 3,
      background: `${color}20`,
      color,
      border: `1px solid ${color}30`,
    }}>
      {kind}
    </span>
  )
}

function SectionLabel({ label }: { label: string }) {
  return <p style={{ fontSize: 10, fontWeight: 600, color: 'var(--cc-text-faint)', letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>{label}</p>
}

function IconBtn({ icon, title }: { icon: ReactNode; title: string }) {
  return (
    <button
      title={title}
      style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', padding: 3, borderRadius: 4, display: 'flex', alignItems: 'center', justifyContent: 'center' }}
    >
      {icon}
    </button>
  )
}
