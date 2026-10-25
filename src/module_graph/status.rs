//! Coverage is execution completeness, not semantic name-resolution completeness.
use super::*;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverageStatus {
    #[default]
    Complete,
    Partial,
    Error,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionStatus {
    #[default]
    VerifiedOnly,
    Limited,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RelationIssueKind {
    UnrootedImport,
    UnsupportedImport,
    AmbiguousImport,
    MacroOpaque,
    UnsupportedScope,
    MissingModuleSource,
    UnsupportedDeclaration,
    AmbiguousDeclaration,
    RecursiveSource,
    ParseFailure,
    ReadFailure,
    ResourceLimit,
    Cancelled,
}
impl RelationIssueKind {
    pub fn loses_coverage(self) -> bool {
        matches!(
            self,
            Self::MissingModuleSource
                | Self::UnsupportedDeclaration
                | Self::AmbiguousDeclaration
                | Self::RecursiveSource
                | Self::ParseFailure
                | Self::ReadFailure
                | Self::ResourceLimit
                | Self::Cancelled
        )
    }
}
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionSummary {
    pub verified_imports: usize,
    pub unrooted_imports: usize,
    pub ambiguous_imports: usize,
    pub unsupported_imports: usize,
    pub opaque_declarations: usize,
    pub unsupported_scopes: usize,
    pub missing_sources: usize,
    pub unsupported_declarations: usize,
    pub ambiguous_declarations: usize,
    pub recursive_sources: usize,
    pub parse_failures: usize,
    pub read_failures: usize,
    pub resource_skips: usize,
    pub cancelled: usize,
    pub conditional_relations: usize,
}
impl ResolutionSummary {
    pub fn unclassified_imports(&self) -> usize {
        self.unrooted_imports + self.ambiguous_imports + self.unsupported_imports
    }
    pub fn coverage_losses(&self) -> usize {
        self.missing_sources
            + self.unsupported_declarations
            + self.ambiguous_declarations
            + self.recursive_sources
            + self.parse_failures
            + self.read_failures
            + self.resource_skips
            + self.cancelled
    }
    pub(super) fn record(&mut self, kind: RelationIssueKind) {
        use RelationIssueKind::*;
        *match kind {
            UnrootedImport => &mut self.unrooted_imports,
            UnsupportedImport => &mut self.unsupported_imports,
            AmbiguousImport => &mut self.ambiguous_imports,
            MacroOpaque => &mut self.opaque_declarations,
            UnsupportedScope => &mut self.unsupported_scopes,
            MissingModuleSource => &mut self.missing_sources,
            UnsupportedDeclaration => &mut self.unsupported_declarations,
            AmbiguousDeclaration => &mut self.ambiguous_declarations,
            RecursiveSource => &mut self.recursive_sources,
            ParseFailure => &mut self.parse_failures,
            ReadFailure => &mut self.read_failures,
            ResourceLimit => &mut self.resource_skips,
            Cancelled => &mut self.cancelled,
        } += 1;
    }
}
