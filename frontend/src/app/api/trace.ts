import type { TraceExplanation } from '../types'

export interface TraceHighlights {
  nodeIds: Set<string>
  edgeIds: Set<string>
}

export function deriveTraceHighlights(trace: TraceExplanation | null): TraceHighlights | null {
  if (!trace) return null
  return {
    nodeIds: new Set(trace.steps.flatMap(step => step.nodeId ? [step.nodeId] : [])),
    edgeIds: new Set(trace.steps.flatMap(step => step.edgeId ? [step.edgeId] : [])),
  }
}

export function traceToMarkdown(trace: TraceExplanation) {
  const lines = [`# ${trace.title}`, '', trace.summary]
  if (trace.warnings.length) {
    lines.push('', '## Warnings', ...trace.warnings.map(warning => `- ${warning}`))
  }
  lines.push('', '## Steps')
  trace.steps.forEach((step, index) => {
    const where = [step.language, step.file, step.line ? `L${step.line}` : undefined].filter(Boolean).join(' · ')
    lines.push(`${index + 1}. **${step.kind}** ${step.title}`)
    if (where) lines.push(`   - Location: ${where}`)
    if (step.confidence) lines.push(`   - Confidence: ${step.confidence}`)
    if (step.reachability) lines.push(`   - Reachability: ${step.reachability}`)
    if (step.evidence) lines.push(`   - Evidence: \`${shortEvidence(step.evidence)}\``)
  })
  return lines.join('\n')
}

function shortEvidence(evidence: string) {
  const compact = evidence.replace(/\s+/g, ' ').trim()
  return compact.length > 96 ? `${compact.slice(0, 93)}...` : compact
}
