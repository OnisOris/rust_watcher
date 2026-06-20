import { useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  ChevronRight,
  ChevronDown,
  Folder,
  File,
  Package,
  BookMarked,
  Flame,
  Clock,
  FocusIcon,
  EyeOff,
  ChevronsUpDown,
  AlertTriangle,
  Link2,
  FunctionSquare,
} from 'lucide-react'
import type { ProjectFile, GraphNode, SourceReachability } from '../types'

interface ProjectExplorerProps {
  files: ProjectFile[]
  nodes: GraphNode[]
  projectName?: string
  selectedNodeId: string | null
  onSelectNode: (id: string) => void
  onFocusFile: (path: string) => void
}

const complexityColor = { low: '#16A34A', medium: '#D97706', high: '#DC2626' }
const complexityBg = { low: 'rgba(22,163,74,0.10)', medium: 'rgba(217,119,6,0.10)', high: 'rgba(220,38,38,0.10)' }
const sourceColor: Record<SourceReachability | 'Unknown', string> = {
  Active: '#10B981',
  Detached: '#64748B',
  Generated: '#8B5CF6',
  External: '#94A3B8',
  Unknown: '#64748B',
}

const FALLBACK_TREE: TreeCrate[] = [
  {
    id: 'workspace',
    label: 'workspace',
    type: 'crate',
    children: [
      {
        id: 'src',
        label: 'src',
        type: 'module',
        children: [
          { id: 'file-main', label: 'main.rs', path: 'src/main.rs', type: 'file', nodeId: 'file-main', fns: 3, links: 12, diag: 0, complexity: 'medium', status: 'Active' },
          { id: 'file-example', label: 'example.rs', path: 'src/example.rs', type: 'file', nodeId: 'file-example', fns: 1, links: 0, diag: 0, complexity: 'low', status: 'Detached' },
        ],
      },
    ],
  },
]

export function ProjectExplorer({ files, nodes, projectName, selectedNodeId, onSelectNode, onFocusFile }: ProjectExplorerProps) {
  const [expandedCrates, setExpandedCrates] = useState<Set<string>>(new Set())
  const [expandedModules, setExpandedModules] = useState<Set<string>>(new Set())
  const [activeSection, setActiveSection] = useState<'workspace' | 'recent' | 'bookmarks' | 'hotspots'>('workspace')

  const tree = useMemo(() => files.length > 0 ? buildFileTree(files, nodes) : FALLBACK_TREE, [files, nodes])

  useEffect(() => {
    if (!tree.length) return
    setExpandedCrates(prev => prev.size ? prev : new Set(tree.map(crate => crate.id)))
    setExpandedModules(prev => prev.size ? prev : new Set(tree.flatMap(crate => crate.children.filter(isModule).map(module => module.id))))
  }, [tree])

  const fileNodeById = useMemo(() => new Map(nodes.filter(node => node.type === 'File').map(node => [node.id, node])), [nodes])
  const recent = files.slice(0, 8).map(file => ({ label: file.path, nodeId: resolveFileNodeId(file, nodes), ago: 'indexed' }))
  const bookmarks = nodes
    .filter(node => node.bookmarked || node.pinned)
    .slice(0, 10)
    .map(node => ({ label: node.label, path: node.file, nodeId: node.id, kind: node.type, status: node.reachability }))
  const hotspots = nodes
    .filter(node => node.type !== 'ExternalCrate')
    .map(node => ({
      node,
      links: node.connections ?? 0,
    }))
    .filter(item => item.links > 0)
    .sort((a, b) => b.links - a.links)
    .slice(0, 10)

  const activeFiles = tree.flatMap(crate => crate.children.flatMap(child => isModule(child) ? child.children : [child])).filter(file => file.status === 'Active').length
  const detachedFiles = tree.flatMap(crate => crate.children.flatMap(child => isModule(child) ? child.children : [child])).filter(file => file.status === 'Detached').length

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
        width: 300,
        background: 'var(--cc-panel)',
        borderRight: '1px solid var(--cc-border)',
        fontFamily: 'Inter, sans-serif',
      }}
    >
      <div className="px-3 py-2 shrink-0" style={{ borderBottom: '1px solid var(--cc-border)' }}>
        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <div style={{ color: 'var(--cc-text)', fontSize: 12, fontWeight: 700, letterSpacing: '0.02em', textTransform: 'uppercase' }}>
              {projectName ?? 'workspace'}
            </div>
            <div style={{ color: 'var(--cc-text-subtle)', fontSize: 10, marginTop: 2 }}>
              {activeFiles} active · {detachedFiles} detached · {files.length || 2} files
            </div>
          </div>
          <div className="flex items-center gap-1">
            <IconBtn icon={<FocusIcon size={13} />} title="Focus current file" />
            <IconBtn icon={<EyeOff size={13} />} title="Hide external crates" />
            <IconBtn icon={<ChevronsUpDown size={13} />} title="Collapse all" onClick={() => { setExpandedCrates(new Set()); setExpandedModules(new Set()) }} />
          </div>
        </div>
      </div>

      <div className="flex items-center shrink-0" style={{ borderBottom: '1px solid var(--cc-border)', padding: '0 6px' }}>
        {(['workspace', 'recent', 'bookmarks', 'hotspots'] as const).map(section => (
          <button
            key={section}
            onClick={() => setActiveSection(section)}
            style={{
              padding: '7px 7px',
              fontSize: 11,
              color: activeSection === section ? 'var(--cc-accent)' : 'var(--cc-text-subtle)',
              borderBottom: activeSection === section ? '2px solid var(--cc-accent)' : '2px solid transparent',
              background: 'none',
              cursor: 'pointer',
              transition: 'all 0.15s',
              fontWeight: activeSection === section ? 700 : 500,
              textTransform: 'capitalize',
            }}
          >
            {section}
          </button>
        ))}
      </div>

      <div className="overflow-y-auto flex-1 py-2" style={{ scrollbarWidth: 'thin', scrollbarColor: 'var(--cc-border) transparent' }}>
        {activeSection === 'workspace' && (
          <div>
            {tree.map(crate => (
              <div key={crate.id}>
                <button
                  className="flex items-center gap-2 w-full px-3 py-1.5 transition-colors"
                  style={{ background: 'none', cursor: 'pointer', color: 'var(--cc-text)' }}
                  onClick={() => toggleExpand(crate.id, 'crate')}
                >
                  {expandedCrates.has(crate.id) ? <ChevronDown size={12} color="var(--cc-text-subtle)" /> : <ChevronRight size={12} color="var(--cc-text-subtle)" />}
                  <Package size={13} color="var(--cc-crate)" />
                  <span style={{ fontSize: 12, fontWeight: 650, color: 'var(--cc-text)', minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{crate.label}</span>
                  <span style={{ marginLeft: 'auto', fontSize: 10, color: 'var(--cc-text-faint)', background: 'var(--cc-elevated)', padding: '1px 5px', borderRadius: 6 }}>crate</span>
                </button>

                {expandedCrates.has(crate.id) && (
                  <div>
                    {crate.children.map(child => (
                      <div key={child.id}>
                        {isModule(child) ? (
                          <div>
                            <button
                              className="flex items-center gap-2 w-full px-3 py-1 transition-colors"
                              style={{ paddingLeft: 28, background: 'none', cursor: 'pointer' }}
                              onClick={() => toggleExpand(child.id, 'module')}
                            >
                              {expandedModules.has(child.id) ? <ChevronDown size={11} color="var(--cc-text-subtle)" /> : <ChevronRight size={11} color="var(--cc-text-subtle)" />}
                              <Folder size={12} color="var(--cc-module)" />
                              <span style={{ fontSize: 11, color: 'var(--cc-text-muted)', minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}>{child.label}</span>
                              <span style={{ marginLeft: 'auto', fontSize: 9, color: 'var(--cc-text-faint)' }}>{child.children.length}</span>
                            </button>
                            {expandedModules.has(child.id) && child.children.map(file => (
                              <FileRow
                                key={file.id}
                                file={file}
                                depth={3}
                                selected={selectedNodeId === file.nodeId}
                                node={fileNodeById.get(file.nodeId)}
                                onClick={() => onSelectNode(file.nodeId)}
                                onDoubleClick={() => onFocusFile(file.path)}
                              />
                            ))}
                          </div>
                        ) : (
                          <FileRow
                            file={child}
                            depth={2}
                            selected={selectedNodeId === child.nodeId}
                            node={fileNodeById.get(child.nodeId)}
                            onClick={() => onSelectNode(child.nodeId)}
                            onDoubleClick={() => onFocusFile(child.path)}
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
            <SectionLabel label="Recently indexed" />
            {recent.map(item => (
              <button
                key={item.nodeId}
                onClick={() => onSelectNode(item.nodeId)}
                className="flex items-start gap-2 w-full rounded transition-colors py-1.5 px-2"
                style={{ background: selectedNodeId === item.nodeId ? 'var(--cc-selected-soft)' : 'none', cursor: 'pointer' }}
              >
                <Clock size={11} color="var(--cc-text-subtle)" style={{ marginTop: 2 }} />
                <div style={{ flex: 1, minWidth: 0, textAlign: 'left' }}>
                  <div style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontFamily: 'JetBrains Mono, monospace', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{item.label}</div>
                  <div style={{ fontSize: 10, color: 'var(--cc-text-faint)' }}>{item.ago}</div>
                </div>
              </button>
            ))}
            {!recent.length && <EmptyHint>No files indexed yet.</EmptyHint>}
          </div>
        )}

        {activeSection === 'bookmarks' && (
          <div className="px-3 py-1">
            <SectionLabel label="Pinned and bookmarked" />
            {bookmarks.map(item => (
              <button
                key={item.nodeId}
                onClick={() => onSelectNode(item.nodeId)}
                className="flex items-start gap-2 w-full rounded py-1.5 px-2 transition-colors"
                style={{ background: selectedNodeId === item.nodeId ? 'var(--cc-selected-soft)' : 'none', cursor: 'pointer' }}
              >
                <BookMarked size={11} color="var(--cc-accent)" style={{ marginTop: 2 }} />
                <div style={{ flex: 1, minWidth: 0, textAlign: 'left' }}>
                  <div style={{ fontSize: 11, color: 'var(--cc-text-muted)', fontFamily: 'JetBrains Mono, monospace' }}>{item.label}</div>
                  {item.path && <div style={{ fontSize: 10, color: 'var(--cc-text-faint)', overflow: 'hidden', textOverflow: 'ellipsis' }}>{item.path}</div>}
                </div>
                <KindBadge kind={item.kind} />
              </button>
            ))}
            {!bookmarks.length && <EmptyHint>No pinned or bookmarked nodes yet.</EmptyHint>}
          </div>
        )}

        {activeSection === 'hotspots' && (
          <div className="px-3 py-1">
            <SectionLabel label="Hotspots" />
            <p style={{ fontSize: 10, color: 'var(--cc-text-subtle)', marginBottom: 8 }}>Most connected nodes with source context.</p>
            {hotspots.map(({ node, links }) => (
              <button
                key={node.id}
                onClick={() => onSelectNode(node.id)}
                className="rounded-lg p-2 mb-2 w-full text-left transition-colors"
                style={{ background: selectedNodeId === node.id ? 'var(--cc-selected-soft)' : 'var(--cc-card)', border: '1px solid var(--cc-border)', cursor: 'pointer' }}
              >
                <div className="flex items-start gap-2">
                  <Flame size={11} color={links >= 6 ? '#DC2626' : links >= 3 ? '#D97706' : '#10B981'} style={{ marginTop: 2 }} />
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 12, color: 'var(--cc-text)', fontFamily: 'JetBrains Mono, monospace', overflowWrap: 'anywhere' }}>{node.label}</div>
                    <div style={{ fontSize: 10, color: 'var(--cc-text-subtle)' }}>{links} links · {node.type}{node.file ? ` · ${node.file}` : ''}</div>
                    {node.reachability && <SourceBadge status={node.reachability} />}
                  </div>
                </div>
              </button>
            ))}
            {!hotspots.length && <EmptyHint>No connected nodes in this view.</EmptyHint>}
          </div>
        )}
      </div>
    </div>
  )
}

type TreeFile = {
  id: string
  label: string
  path: string
  type: 'file'
  nodeId: string
  fns: number
  links: number
  diag: number
  complexity: 'low' | 'medium' | 'high'
  status: SourceReachability | 'Unknown'
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
  const fileNodeByPath = new Map(nodes.filter(node => node.type === 'File' && node.file).map(node => [node.file!, node]))
  const crates = new Map<string, TreeCrate>()
  const moduleMap = new Map<string, TreeModule>()

  for (const file of files) {
    const crateName = file.crate || 'workspace'
    let crateNode = crates.get(crateName)
    if (!crateNode) {
      crateNode = { id: crateName, label: crateName, type: 'crate', children: [] }
      crates.set(crateName, crateNode)
    }

    const graphNode = fileNodeByPath.get(file.path)
    const treeFile: TreeFile = {
      id: file.id,
      label: file.name,
      path: file.path,
      type: 'file',
      nodeId: graphNode?.id ?? file.id,
      fns: file.functionsCount,
      links: file.linksCount,
      diag: file.diagnosticsCount,
      complexity: file.complexity,
      status: graphNode?.reachability ?? 'Active',
    }

    const moduleName = file.module && file.module !== 'crate root' ? file.module : 'src'
    const moduleId = `${crateName}:${moduleName}`
    let moduleNode = moduleMap.get(moduleId)
    if (!moduleNode) {
      moduleNode = { id: moduleId, label: moduleName, type: 'module', children: [] }
      moduleMap.set(moduleId, moduleNode)
      crateNode.children.push(moduleNode)
    }
    moduleNode.children.push(treeFile)
  }

  return [...crates.values()]
}

function resolveFileNodeId(file: ProjectFile, nodes: GraphNode[]) {
  return nodes.find(node => node.type === 'File' && node.file === file.path)?.id ?? file.id
}

function isModule(node: TreeModule | TreeFile): node is TreeModule {
  return node.type === 'module'
}

function FileRow({ file, depth, selected, node, onClick, onDoubleClick }: {
  file: TreeFile
  depth: number
  selected: boolean
  node?: GraphNode
  onClick: () => void
  onDoubleClick: () => void
}) {
  const status = node?.reachability ?? file.status
  return (
    <button
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      className="flex items-center gap-1.5 w-full transition-all"
      title={`${file.path} · ${status}`}
      style={{
        paddingLeft: depth * 12 + 4,
        paddingRight: 8,
        paddingTop: 5,
        paddingBottom: 5,
        background: selected ? 'var(--cc-selected-soft)' : 'none',
        borderLeft: selected ? '2px solid var(--cc-accent)' : status === 'Detached' ? '2px dashed var(--cc-border-strong)' : '2px solid transparent',
        cursor: 'pointer',
        opacity: status === 'Detached' ? 0.82 : 1,
      }}
    >
      <File size={11} color={status === 'Detached' ? '#64748B' : 'var(--cc-file)'} />
      <div style={{ flex: 1, minWidth: 0, textAlign: 'left' }}>
        <div style={{ fontSize: 11, color: selected ? 'var(--cc-text)' : 'var(--cc-text-muted)', fontFamily: 'JetBrains Mono, monospace', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {file.label}
        </div>
        <div style={{ fontSize: 9, color: 'var(--cc-text-faint)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {file.path}
        </div>
      </div>
      <div className="flex items-center gap-1 shrink-0">
        {file.fns > 0 && <MiniCount title={`${file.fns} functions`} icon={<FunctionSquare size={9} />} value={file.fns} />}
        {file.links > 0 && <MiniCount title={`${file.links} links`} icon={<Link2 size={9} />} value={file.links} />}
        {file.diag > 0 && <MiniCount title={`${file.diag} diagnostics`} icon={<AlertTriangle size={9} />} value={file.diag} color="#DC2626" />}
        <SourceBadge status={status} compact />
      </div>
    </button>
  )
}

function MiniCount({ icon, value, title, color = 'var(--cc-text-faint)' }: { icon: ReactNode; value: number; title: string; color?: string }) {
  return (
    <span title={title} style={{ fontSize: 9, color, display: 'flex', alignItems: 'center', gap: 1 }}>
      {icon} {value}
    </span>
  )
}

function SourceBadge({ status, compact }: { status: SourceReachability | 'Unknown'; compact?: boolean }) {
  const color = sourceColor[status]
  return (
    <span style={{
      display: 'inline-flex',
      width: compact ? undefined : 'fit-content',
      marginTop: compact ? 0 : 4,
      fontSize: 9,
      lineHeight: 1,
      padding: compact ? '2px 4px' : '3px 6px',
      borderRadius: 999,
      background: `${color}18`,
      color,
      border: `1px solid ${color}30`,
      textTransform: 'uppercase',
      letterSpacing: '0.04em',
    }}>
      {compact ? status.slice(0, 3) : status}
    </span>
  )
}

function KindBadge({ kind }: { kind: string }) {
  const colors: Record<string, string> = {
    Function: '#EC4899', Struct: '#06B6D4', Class: '#0EA5E9', Object: '#38BDF8', Property: '#FACC15', Signal: '#FB7185', Handler: '#F472B6', Trait: '#10B981', Enum: '#F59E0B', Module: '#8B5CF6', File: '#3B82F6', Endpoint: '#E11D48',
  }
  const color = colors[kind] ?? '#64748B'
  return (
    <span style={{ fontSize: 9, padding: '2px 5px', borderRadius: 999, background: `${color}18`, color, border: `1px solid ${color}30` }}>
      {kind}
    </span>
  )
}

function SectionLabel({ label }: { label: string }) {
  return <p style={{ fontSize: 10, fontWeight: 700, color: 'var(--cc-text-faint)', letterSpacing: '0.07em', textTransform: 'uppercase', marginBottom: 6 }}>{label}</p>
}

function EmptyHint({ children }: { children: ReactNode }) {
  return <p style={{ fontSize: 11, color: 'var(--cc-text-faint)', padding: '8px 0', lineHeight: 1.45 }}>{children}</p>
}

function IconBtn({ icon, title, onClick }: { icon: ReactNode; title: string; onClick?: () => void }) {
  return (
    <button
      title={title}
      onClick={onClick}
      style={{ background: 'none', border: 'none', cursor: 'pointer', color: 'var(--cc-text-subtle)', padding: 4, borderRadius: 6, display: 'flex', alignItems: 'center', justifyContent: 'center' }}
    >
      {icon}
    </button>
  )
}
