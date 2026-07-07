import type { EdgeType, GraphEdge, GraphNode, SourceReachability } from '../types'
import type { AggregatedProjectEdge, ProjectGroup, ProjectGroupKind, ProjectMapGrouping, ProjectMapModel } from '../views/architecture/architectureTypes'

interface ProjectMapOptions {
  grouping?: ProjectMapGrouping
  groupBy?: ProjectMapGrouping | 'top-level'
  includeTests?: boolean
  includeExternal?: boolean
  includeGenerated?: boolean
}

const TOP_LEVEL_ORDER: ProjectGroupKind[] = ['frontend', 'backend', 'shared', 'tests', 'external', 'generated', 'unknown']

const GROUP_LABELS: Partial<Record<ProjectGroupKind, string>> = {
  frontend: 'Frontend / UI',
  backend: 'Backend / Services',
  shared: 'Shared / Protocol',
  tests: 'Tests',
  external: 'External Dependencies',
  generated: 'Generated / Mocks',
  unknown: 'Other',
}

const FILE_NODE_TYPES = new Set(['File'])
const KEY_SYMBOL_TYPES = new Set(['Component', 'Object', 'Hook', 'Endpoint', 'Function', 'Method', 'Struct', 'Class', 'Interface', 'Enum', 'Trait'])

export function buildProjectMapModel(
  nodes: GraphNode[],
  edges: GraphEdge[],
  options: ProjectMapOptions = {},
): ProjectMapModel {
  const grouping = normalizeGrouping(options.grouping ?? options.groupBy ?? 'architecture')
  const includeTests = options.includeTests ?? true
  const includeExternal = options.includeExternal ?? true
  const includeGenerated = options.includeGenerated ?? true
  const nodeToGroup = new Map<string, string>()
  const groupsById = new Map<string, ProjectGroup>()
  const childGroupsById = new Map<string, ProjectGroup>()
  const byId = nodeById(nodes)

  for (const node of nodes) {
    const kind = classifyNode(node)
    if (!includeTests && kind === 'tests') continue
    if (!includeExternal && kind === 'external') continue
    if (!includeGenerated && kind === 'generated') continue

    const target = groupTargetFor(node, kind, grouping)
    const group = ensureGroup(groupsById, target.id, target.kind, target.label, target.pathPrefix, node.language)
    addNodeStats(group, node)
    nodeToGroup.set(node.id, group.id)

    const childId = subgroupIdFor(node, kind)
    if (childId && childId !== group.id) {
      const child = ensureGroup(childGroupsById, childId, 'directory', childId.split('/').slice(-1)[0], childId, node.language)
      addNodeStats(child, node)
      const existingChildren = group.children ?? []
      if (!existingChildren.some(candidate => candidate.id === child.id)) {
        group.children = [...existingChildren, child]
      }
    }
  }

  const edgeBuckets = new Map<string, AggregatedProjectEdge>()
  for (const edge of edges) {
    if (edge.type === 'Contains' || edge.type === 'ModDeclaration') continue
    const sourceGroupId = nodeToGroup.get(edge.source)
    const targetGroupId = nodeToGroup.get(edge.target)
    if (!sourceGroupId || !targetGroupId || sourceGroupId === targetGroupId) continue
    const key = `${sourceGroupId}->${targetGroupId}`
    const bucket = edgeBuckets.get(key) ?? {
      id: key,
      sourceGroupId,
      targetGroupId,
      count: 0,
      edgeTypes: {},
      underlyingEdgeIds: [],
      examples: [],
    }
    const underlyingEdgeIds = edge.bundledEdgeIds?.length ? edge.bundledEdgeIds : [edge.id]
    const count = edge.bundledCount ?? underlyingEdgeIds.length
    bucket.count += Math.max(1, count)
    bucket.underlyingEdgeIds.push(...underlyingEdgeIds)
    addEdgeTypeCounts(bucket.edgeTypes, edge)
    addEdgeExample(bucket, edge, byId)
    edgeBuckets.set(key, bucket)
  }

  for (const edge of edgeBuckets.values()) {
    groupsById.get(edge.sourceGroupId)!.outgoingCount += edge.count
    groupsById.get(edge.targetGroupId)!.incomingCount += edge.count
  }

  const autoExpanded = grouping === 'architecture' && groupsById.size <= 2
  const groups = [...groupsById.values()]
    .map(group => ({
      ...finalizeGroupMetadata(group, nodes),
      children: group.children?.sort((a, b) => b.nodeIds.length - a.nodeIds.length || a.label.localeCompare(b.label)),
    }))
    .sort(compareGroups)

  return {
    groups,
    edges: [...edgeBuckets.values()].sort((a, b) => b.count - a.count || a.id.localeCompare(b.id)),
    nodeToGroup,
    grouping,
    autoExpanded,
  }
}

export function classifyNode(node: GraphNode): ProjectGroupKind {
  const file = normalizePath(node.file)
  const moduleName = (node.module ?? '').toLowerCase()
  const label = node.label.toLowerCase()
  const reachability: SourceReachability | undefined = node.reachability

  if (node.type === 'ExternalCrate' || reachability === 'External') return 'external'
  if (
    file.includes('/test/')
    || file.includes('/tests/')
    || file.endsWith('.test.ts')
    || file.endsWith('.spec.ts')
    || file.endsWith('_test.rs')
    || label.includes('test')
  ) return 'tests'
  if (
    file.includes('/generated/')
    || file.includes('/mocks/')
    || file.includes('/mock')
    || reachability === 'Generated'
  ) return 'generated'
  if (
    file.includes('/shared/')
    || file.startsWith('shared/')
    || file.includes('/common/')
    || file.startsWith('common/')
    || file.includes('/protocol/')
    || file.startsWith('protocol/')
    || file.includes('/types/')
    || moduleName.includes('shared')
    || moduleName.includes('protocol')
  ) return 'shared'
  if (
    file.startsWith('frontend/')
    || file.includes('/frontend/')
    || file.includes('/src/app/')
    || file.endsWith('.tsx')
    || file.endsWith('.ts')
    || node.language === 'typescript'
    || node.language === 'qml'
  ) return 'frontend'
  if (
    file.startsWith('backend/')
    || file.includes('/backend/')
    || file.startsWith('crates/')
    || file.endsWith('.rs')
    || file.endsWith('.py')
    || node.language === 'rust'
    || node.language === 'python'
  ) return 'backend'
  return 'unknown'
}

function groupTargetFor(node: GraphNode, kind: ProjectGroupKind, grouping: ProjectMapGrouping) {
  if (grouping === 'language') {
    const language = node.language ?? languageFromPath(node.file) ?? 'unknown'
    return { id: `language:${language}`, kind: kindForGroupedMode(kind), label: languageLabel(language), pathPrefix: language }
  }
  if (grouping === 'module') {
    const moduleName = moduleGroupLabel(node, kind)
    return { id: `module:${sanitizeGroupId(moduleName)}`, kind: 'module' as const, label: moduleName, pathPrefix: moduleName }
  }
  if (grouping === 'directory') {
    const directory = directoryGroupId(node, kind)
    return { id: `directory:${directory}`, kind: 'directory' as const, label: directory, pathPrefix: directory }
  }
  if (grouping === 'runtime') {
    const runtime = runtimeGroup(node, kind)
    return { id: `runtime:${sanitizeGroupId(runtime.label)}`, kind: runtime.kind, label: runtime.label, pathPrefix: runtime.label }
  }
  return { id: kind, kind, label: topLevelLabel(kind, kind), pathPrefix: pathPrefixFor(node, kind) }
}

function directoryGroupId(node: GraphNode, kind: ProjectGroupKind) {
  const file = normalizePath(node.file)
  if (!file) return kind
  const parts = file.split('/').filter(Boolean)
  if (parts.length <= 1) return kind
  if (parts[0] === 'frontend' && parts[1] === 'src') return parts.slice(0, Math.min(3, parts.length - 1)).join('/')
  if (parts[0] === 'backend') return parts.slice(0, Math.min(2, parts.length - 1)).join('/')
  if (parts[0] === 'crates') return parts.slice(0, Math.min(2, parts.length - 1)).join('/')
  return parts.slice(0, Math.min(2, parts.length - 1)).join('/') || `${kind}/${parts[0]}`
}

function subgroupIdFor(node: GraphNode, kind: ProjectGroupKind) {
  const file = normalizePath(node.file)
  if (!file) return null
  const parts = file.split('/').filter(Boolean)
  if (!parts.length) return null
  if (kind === 'frontend') {
    const index = Math.max(parts.indexOf('src'), parts.indexOf('app'))
    const section = parts[index + 1] ?? parts[1]
    return section ? `frontend/${section}` : null
  }
  if (kind === 'backend') {
    if (parts[0] === 'crates' && parts[1]) return `backend/${parts[1]}`
    const section = parts.includes('src') ? parts[parts.indexOf('src') + 1] : parts[1]
    return section ? `backend/${section}` : null
  }
  if (kind === 'shared') return `shared/${parts[parts.length > 1 ? parts.length - 2 : 0]}`
  if (kind === 'tests') return file.includes('/integration') ? 'tests/integration' : 'tests/unit'
  if (kind === 'external') return `external/${node.crate ?? node.label.split('::')[0] ?? 'packages'}`
  return null
}

function ensureGroup(
  groupsById: Map<string, ProjectGroup>,
  id: string,
  kind: ProjectGroupKind,
  label: string,
  pathPrefix?: string,
  language?: string,
) {
  const existing = groupsById.get(id)
  if (existing) return existing
  const group: ProjectGroup = {
    id,
    label,
    kind,
    pathPrefix,
    language,
    nodeIds: [],
    fileCount: 0,
    symbolCount: 0,
    incomingCount: 0,
    outgoingCount: 0,
  }
  groupsById.set(id, group)
  return group
}

function addNodeStats(group: ProjectGroup, node: GraphNode) {
  group.nodeIds.push(node.id)
  if (node.type === 'File') group.fileCount += 1
  else group.symbolCount += 1
}

function addEdgeTypeCounts(edgeTypes: Partial<Record<EdgeType, number>>, edge: GraphEdge) {
  if (edge.bundledTypes?.length) {
    for (const type of edge.bundledTypes) {
      edgeTypes[type] = (edgeTypes[type] ?? 0) + 1
    }
    return
  }
  edgeTypes[edge.type] = (edgeTypes[edge.type] ?? 0) + Math.max(1, edge.bundledCount ?? 1)
}

function topLevelLabel(groupId: string, kind: ProjectGroupKind) {
  return GROUP_LABELS[kind] ?? groupId.split('/').slice(-1)[0] ?? groupId
}

function pathPrefixFor(node: GraphNode, fallback: string) {
  const file = normalizePath(node.file)
  if (!file) return fallback
  const parts = file.split('/').filter(Boolean)
  return parts.slice(0, Math.min(2, parts.length)).join('/')
}

function compareGroups(a: ProjectGroup, b: ProjectGroup) {
  const ai = TOP_LEVEL_ORDER.indexOf(a.kind)
  const bi = TOP_LEVEL_ORDER.indexOf(b.kind)
  const orderA = ai === -1 ? TOP_LEVEL_ORDER.length : ai
  const orderB = bi === -1 ? TOP_LEVEL_ORDER.length : bi
  return orderA - orderB || b.nodeIds.length - a.nodeIds.length || a.label.localeCompare(b.label)
}

function normalizePath(path?: string | null) {
  return (path ?? '').replaceAll('\\', '/').replace(/^\/+/, '')
}

function sanitizeGroupId(value: string) {
  return value.replaceAll('::', '/').replaceAll('.', '/').replace(/[^a-zA-Z0-9_/-]/g, '-')
}

function normalizeGrouping(value: ProjectMapOptions['groupBy']): ProjectMapGrouping {
  if (!value || value === 'top-level') return 'architecture'
  return value
}

function kindForGroupedMode(kind: ProjectGroupKind): ProjectGroupKind {
  if (kind === 'external' || kind === 'tests' || kind === 'generated') return kind
  return 'module'
}

function languageLabel(language: string) {
  const normalized = language.toLowerCase()
  if (normalized === 'typescript') return 'TypeScript'
  if (normalized === 'rust') return 'Rust'
  if (normalized === 'python') return 'Python'
  if (normalized === 'qml') return 'QML'
  return language || 'Unknown'
}

function languageFromPath(file?: string | null) {
  const normalized = normalizePath(file).toLowerCase()
  if (normalized.endsWith('.tsx') || normalized.endsWith('.ts')) return 'typescript'
  if (normalized.endsWith('.rs')) return 'rust'
  if (normalized.endsWith('.py')) return 'python'
  if (normalized.endsWith('.qml')) return 'qml'
  return undefined
}

function moduleGroupLabel(node: GraphNode, kind: ProjectGroupKind) {
  if (node.module) return node.module.replaceAll('::', '/')
  const file = normalizePath(node.file)
  const parts = file.split('/').filter(Boolean)
  if (parts[0] === 'frontend' && parts[1] === 'src' && parts[2]) return `frontend/${stripExtension(parts[2])}`
  if (parts[0] === 'qml' && parts[1]) return `qml/${stripExtension(parts[1])}`
  if (parts[0] === 'backend' && parts[1]) return `backend/${stripExtension(parts[1])}`
  if (parts[0] === 'crates' && parts[1]) return `crates/${parts[1]}`
  if (parts[0] === 'src') return 'src'
  return GROUP_LABELS[kind] ?? 'Other'
}

function runtimeGroup(node: GraphNode, kind: ProjectGroupKind): { label: string; kind: ProjectGroupKind } {
  const file = normalizePath(node.file).toLowerCase()
  if (kind === 'frontend') {
    if (node.language === 'qml' || file.endsWith('.qml')) return { label: 'QML Runtime', kind: 'frontend' }
    return { label: 'Browser / UI Runtime', kind: 'frontend' }
  }
  if (kind === 'backend') {
    if (node.language === 'python' || file.endsWith('.py')) return { label: 'Python Service Runtime', kind: 'backend' }
    if (node.language === 'rust' || file.endsWith('.rs')) return { label: 'Rust Service Runtime', kind: 'backend' }
    return { label: 'Backend Runtime', kind: 'backend' }
  }
  if (kind === 'shared') return { label: 'Shared Protocol Runtime', kind: 'shared' }
  if (kind === 'external') return { label: 'External Dependencies', kind: 'external' }
  if (kind === 'tests') return { label: 'Test Runtime', kind: 'tests' }
  if (kind === 'generated') return { label: 'Generated / Mocks', kind: 'generated' }
  return { label: 'Other Runtime', kind: 'unknown' }
}

function finalizeGroupMetadata(group: ProjectGroup, nodes: GraphNode[]): ProjectGroup {
  const byId = new Map(nodes.map(node => [node.id, node]))
  const groupNodes = group.nodeIds.map(id => byId.get(id)).filter((node): node is GraphNode => Boolean(node))
  const languageBreakdown: Record<string, number> = {}
  const fileCounts = new Map<string, number>()
  const keySymbols = groupNodes
    .filter(node => !FILE_NODE_TYPES.has(node.type) && KEY_SYMBOL_TYPES.has(node.type))
    .sort((a, b) => (b.connections ?? 0) - (a.connections ?? 0) || a.label.localeCompare(b.label))
    .slice(0, 8)
    .map(node => node.id)

  for (const node of groupNodes) {
    const language = node.language ?? languageFromPath(node.file)
    if (language) languageBreakdown[languageLabel(language)] = (languageBreakdown[languageLabel(language)] ?? 0) + 1
    const file = normalizePath(node.file)
    if (file) fileCounts.set(file, (fileCounts.get(file) ?? 0) + 1)
  }

  return {
    ...group,
    languageBreakdown,
    topFiles: [...fileCounts.entries()]
      .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
      .slice(0, 6)
      .map(([file]) => file),
    keySymbols,
  }
}

function addEdgeExample(bucket: AggregatedProjectEdge, edge: GraphEdge, byId: Map<string, GraphNode>) {
  if ((bucket.examples?.length ?? 0) >= 8) return
  const source = byId.get(edge.source)
  const target = byId.get(edge.target)
  bucket.examples = [
    ...(bucket.examples ?? []),
    {
      id: edge.id,
      sourceLabel: source?.label ?? edge.source,
      targetLabel: target?.label ?? edge.target,
      type: edge.type,
      sourceFile: normalizePath(source?.file),
      targetFile: normalizePath(target?.file),
    },
  ]
}

function nodeById(nodes: GraphNode[]) {
  return new Map(nodes.map(node => [node.id, node]))
}

function stripExtension(pathPart: string) {
  return pathPart.replace(/\.[^.]+$/, '')
}
