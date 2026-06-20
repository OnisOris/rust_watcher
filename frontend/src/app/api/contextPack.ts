import type { ContextPack, ContextSnippet } from '../types'

export function summarizeContextPack(pack: ContextPack) {
  return `${pack.snippets.length} snippet${pack.snippets.length === 1 ? '' : 's'} · ${pack.nodes.length} node${pack.nodes.length === 1 ? '' : 's'} · ${pack.edges.length} edge${pack.edges.length === 1 ? '' : 's'}`
}

export function contextSnippetLabel(snippet: ContextSnippet) {
  return `${snippet.file}:L${snippet.startLine}-L${snippet.endLine}`
}

export function contextPackToMarkdown(pack: ContextPack) {
  const lines = [`# ${pack.title}`, '', pack.summary || summarizeContextPack(pack)]
  if (pack.warnings.length) {
    lines.push('', '## Warnings', ...pack.warnings.map(warning => `- ${warning}`))
  }
  if (pack.nodes.length || pack.edges.length) {
    lines.push('', '## Graph Context')
    if (pack.nodes.length) {
      lines.push('', '### Nodes')
      for (const node of pack.nodes) {
        lines.push(`- ${node.label} (${node.type}${node.language ? `, ${node.language}` : ''})${node.file ? ` - ${node.file}${node.line ? `:${node.line}` : ''}` : ''}`)
      }
    }
    if (pack.edges.length) {
      lines.push('', '### Edges')
      for (const edge of pack.edges) {
        lines.push(`- ${edge.type}: ${edge.source} -> ${edge.target}${edge.confidence ? ` [${edge.confidence}]` : ''}${edge.dataFlowKind ? ` (${edge.dataFlowKind})` : ''}`)
        if (edge.evidence) lines.push(`  - Evidence: \`${shortEvidence(edge.evidence)}\``)
      }
    }
  }
  if (pack.diagnostics.length) {
    lines.push('', '## Diagnostics')
    for (const diagnostic of pack.diagnostics) {
      lines.push(`- ${diagnostic.severity}: ${diagnostic.file}${diagnostic.range ? `:${diagnostic.range.start.line + 1}` : ''} - ${diagnostic.message}`)
    }
  }
  if (pack.snippets.length) {
    lines.push('', '## Source Snippets')
    const snippetsByFile = new Map<string, ContextSnippet[]>()
    for (const snippet of pack.snippets) {
      const list = snippetsByFile.get(snippet.file) ?? []
      list.push(snippet)
      snippetsByFile.set(snippet.file, list)
    }
    for (const [file, snippets] of snippetsByFile.entries()) {
      lines.push('', `### ${file}`)
      for (const snippet of snippets) {
        lines.push('', `#### ${contextSnippetLabel(snippet)} - ${snippet.reason}`)
        lines.push('```' + (snippet.language ?? ''))
        lines.push(withLineNumbers(snippet))
        lines.push('```')
      }
    }
  }
  return lines.join('\n')
}

function withLineNumbers(snippet: ContextSnippet) {
  return snippet.code
    .split('\n')
    .map((line, index) => `${String(snippet.startLine + index).padStart(4, ' ')} | ${line}`)
    .join('\n')
}

function shortEvidence(evidence: string) {
  const compact = evidence.replace(/\s+/g, ' ').trim()
  return compact.length > 96 ? `${compact.slice(0, 93)}...` : compact
}
