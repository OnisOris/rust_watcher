import type { DependencyMatrixModel } from '../views/architecture/architectureTypes'
import type { ProjectMapModel } from '../views/architecture/architectureTypes'

export function buildDependencyMatrixModel(projectMap: ProjectMapModel): DependencyMatrixModel {
  return {
    groups: projectMap.groups,
    cells: projectMap.edges
      .filter(edge => edge.count > 0)
      .map(edge => ({
        sourceGroupId: edge.sourceGroupId,
        targetGroupId: edge.targetGroupId,
        count: edge.count,
        edgeTypes: { ...edge.edgeTypes },
        underlyingEdgeIds: [...edge.underlyingEdgeIds],
      })),
  }
}
