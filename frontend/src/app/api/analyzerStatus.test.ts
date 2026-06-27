import { describe, expect, it } from 'vitest'
import {
  analyzerCapabilityLabel,
  filterApplicableAnalyzers,
  sortAnalyzers,
  summarizeAnalyzers,
} from './analyzerStatus'
import type { AnalyzerServiceStatus } from '../types'

function analyzer(partial: Partial<AnalyzerServiceStatus> & Pick<AnalyzerServiceStatus, 'id' | 'kind' | 'engine' | 'status'>): AnalyzerServiceStatus {
  return {
    label: partial.id,
    mode: null,
    message: null,
    capabilities: ['Symbols'],
    filesIndexed: 1,
    provider: 'local',
    billable: false,
    ...partial,
  }
}

describe('analyzer status helpers', () => {
  it('summarizes ready fallback and error analyzers', () => {
    const summary = summarizeAnalyzers([
      analyzer({ id: 'rust', kind: 'Rust', engine: 'RustAnalyzer', status: 'Ready' }),
      analyzer({ id: 'py-parser', kind: 'Python', engine: 'Parser', status: 'Ready' }),
      analyzer({ id: 'py-ty', kind: 'Python', engine: 'Ty', status: 'Fallback' }),
      analyzer({ id: 'qml', kind: 'Qml', engine: 'QmlParser', status: 'Error' }),
    ])

    expect(summary.ready).toBe(2)
    expect(summary.fallback).toBe(1)
    expect(summary.error).toBe(1)
    expect(summary.label).toBe('Analyzers · 1 error · 2 ready · 1 fallback')
  })

  it('filters analyzers for languages that were not detected', () => {
    const analyzers = [
      analyzer({ id: 'rust-analyzer', kind: 'Rust', engine: 'RustAnalyzer', status: 'Fallback', filesIndexed: 0 }),
      analyzer({ id: 'typescript-language-server', kind: 'TypeScript', engine: 'TypeScriptLanguageServer', status: 'Ready', filesIndexed: 0 }),
      analyzer({ id: 'python-ty', kind: 'Python', engine: 'Ty', status: 'Ready', filesIndexed: 50 }),
    ]

    expect(filterApplicableAnalyzers(analyzers).map(item => item.id)).toEqual(['python-ty'])
    expect(summarizeAnalyzers(analyzers).label).toBe('Analyzers · 1 ready')
  })

  it('sorts analyzers by language and semantic engines before parser fallbacks', () => {
    const sorted = sortAnalyzers([
      analyzer({ id: 'qml-parser', kind: 'Qml', engine: 'QmlParser', status: 'Ready' }),
      analyzer({ id: 'qmlls', kind: 'Qml', engine: 'QmlLanguageServer', status: 'Ready' }),
      analyzer({ id: 'python-parser', kind: 'Python', engine: 'Parser', status: 'Ready' }),
      analyzer({ id: 'python-ty', kind: 'Python', engine: 'Ty', status: 'Ready' }),
      analyzer({ id: 'rust-analyzer', kind: 'Rust', engine: 'RustAnalyzer', status: 'Ready' }),
      analyzer({ id: 'typescript-language-server', kind: 'TypeScript', engine: 'TypeScriptLanguageServer', status: 'Ready' }),
      analyzer({ id: 'typescript-parser', kind: 'TypeScript', engine: 'TypeScriptParser', status: 'Ready' }),
    ])

    expect(sorted.map(item => item.id)).toEqual([
      'rust-analyzer',
      'typescript-language-server',
      'typescript-parser',
      'python-ty',
      'python-parser',
      'qmlls',
      'qml-parser',
    ])
  })

  it('labels analyzer capabilities compactly', () => {
    expect(analyzerCapabilityLabel('SemanticCalls')).toBe('calls')
    expect(analyzerCapabilityLabel('TypeDefinitions')).toBe('type defs')
  })

  it('summary label does not expose hardcoded python status text', () => {
    const summary = summarizeAnalyzers([
      analyzer({ id: 'python-ty', kind: 'Python', engine: 'Ty', status: 'Ready' }),
    ])

    expect(summary.label).toBe('Analyzers · 1 ready')
    expect(summary.label).not.toContain('python ty ready')
  })
})
