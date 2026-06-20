import type { AnalyzerCapability, AnalyzerEngine, AnalyzerKind, AnalyzerServiceStatus, AnalyzerStatus } from '../types'

const KIND_ORDER: Record<AnalyzerKind, number> = {
  Rust: 0,
  TypeScript: 1,
  Python: 2,
  Qml: 3,
  Other: 4,
}

const SEMANTIC_ENGINES = new Set<AnalyzerEngine>([
  'RustAnalyzer',
  'Ty',
  'TypeScriptLanguageServer',
  'QmlLanguageServer',
])

export interface AnalyzerSummary {
  ready: number
  fallback: number
  error: number
  indexing: number
  stale: number
  total: number
  label: string
}

export function summarizeAnalyzers(analyzers: AnalyzerServiceStatus[] = []): AnalyzerSummary {
  const ready = analyzers.filter(analyzer => analyzer.status === 'Ready').length
  const fallback = analyzers.filter(analyzer => analyzer.status === 'Fallback').length
  const error = analyzers.filter(analyzer => analyzer.status === 'Error').length
  const indexing = analyzers.filter(analyzer => analyzer.status === 'Starting' || analyzer.status === 'Indexing').length
  const stale = analyzers.filter(analyzer => analyzer.status === 'Stale').length
  const parts: string[] = []
  if (error) parts.push(`${error} error`)
  if (indexing) parts.push(`${indexing} indexing`)
  if (ready) parts.push(`${ready} ready`)
  if (fallback) parts.push(`${fallback} fallback`)
  if (stale) parts.push(`${stale} stale`)
  return {
    ready,
    fallback,
    error,
    indexing,
    stale,
    total: analyzers.length,
    label: `Analyzers · ${parts.length ? parts.join(' · ') : 'none'}`,
  }
}

export function analyzerStatusColor(status: AnalyzerStatus) {
  switch (status) {
    case 'Ready':
      return '#10B981'
    case 'Error':
      return '#DC2626'
    case 'Starting':
    case 'Indexing':
    case 'Fallback':
    case 'Stale':
      return '#D97706'
  }
}

export function analyzerCapabilityLabel(capability: AnalyzerCapability) {
  switch (capability) {
    case 'Symbols':
      return 'symbols'
    case 'Diagnostics':
      return 'diagnostics'
    case 'References':
      return 'refs'
    case 'Definitions':
      return 'defs'
    case 'TypeDefinitions':
      return 'type defs'
    case 'CallHierarchy':
      return 'call hierarchy'
    case 'SemanticCalls':
      return 'calls'
    case 'SemanticTokens':
      return 'tokens'
    case 'Formatting':
      return 'formatting'
  }
}

export function analyzerEngineLabel(engine: AnalyzerEngine) {
  switch (engine) {
    case 'RustAnalyzer':
      return 'rust-analyzer'
    case 'Ty':
      return 'ty'
    case 'TypeScriptParser':
      return 'TS parser'
    case 'TypeScriptLanguageServer':
      return 'TS language server'
    case 'QmlParser':
      return 'QML parser'
    case 'QmlLanguageServer':
      return 'QML language server'
    case 'TreeSitter':
      return 'tree-sitter'
    case 'Parser':
      return 'parser'
    case 'Other':
      return 'other'
  }
}

export function sortAnalyzers(analyzers: AnalyzerServiceStatus[] = []) {
  return [...analyzers].sort((left, right) => {
    const kind = KIND_ORDER[left.kind] - KIND_ORDER[right.kind]
    if (kind !== 0) return kind
    const semantic = Number(!SEMANTIC_ENGINES.has(left.engine)) - Number(!SEMANTIC_ENGINES.has(right.engine))
    if (semantic !== 0) return semantic
    return left.label.localeCompare(right.label)
  })
}
