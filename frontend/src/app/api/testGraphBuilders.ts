import type { EdgeType, GraphEdge, GraphNode, NodeType } from '../types'

type NodeOverrides = Partial<GraphNode> & {
  id?: string
  type?: NodeType
}

type EdgeOverrides = Partial<GraphEdge> & {
  source: string
  target: string
  type?: EdgeType
}

export function makeNode(overrides: NodeOverrides = {}): GraphNode {
  const type = overrides.type ?? 'Function'
  const label = overrides.label ?? overrides.file?.split('/').pop() ?? type
  const id = overrides.id ?? stableNodeId(type, label, overrides.file)

  return {
    id,
    type,
    label,
    x: 0,
    y: 0,
    vx: 0,
    vy: 0,
    ...overrides,
  }
}

export function makeEdge(overrides: EdgeOverrides): GraphEdge {
  const type = overrides.type ?? 'Calls'
  const id = overrides.id ?? `edge:${overrides.source}->${overrides.target}:${type}`

  return {
    id,
    source: overrides.source,
    target: overrides.target,
    type,
    ...overrides,
  }
}

export function makeFileNode(path: string, language?: string): GraphNode {
  return makeNode({
    id: `file:${path}`,
    type: 'File',
    label: path.split('/').pop() ?? path,
    file: path,
    language,
  })
}

export function makeFunctionNode(label: string, file: string, language?: string): GraphNode {
  return makeNode({
    id: `function:${file}:${label}`,
    type: 'Function',
    label,
    file,
    language,
  })
}

export function makeEndpointNode(method: string, path: string): GraphNode {
  const upperMethod = method.toUpperCase()
  const label = `${upperMethod} ${path}`
  return makeNode({
    id: `endpoint:${label}`,
    type: 'Endpoint',
    label,
    file: 'src/routes.rs',
    language: 'rust',
  })
}

function stableNodeId(type: NodeType, label: string, file?: string) {
  return `node:${type}:${file ? `${file}:` : ''}${label}`.replaceAll(/\s+/g, '-')
}
