//! Presentation vocabulary tied to the existing measurements, never new analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Metric {
    SourceLoc,
    PhysicalLoc,
    Functions,
    Length,
    Depth,
    FlowNodes,
    Changes,
    Churn,
    CoChange,
    Candidates,
    Verified,
    Unclassified,
    Coverage,
    Calls,
    Status,
}
pub(super) struct Explanation {
    pub what: &'static str,
    pub why: &'static str,
    pub limitations: &'static str,
    pub next: &'static str,
}
impl Metric {
    pub const ALL: [Self; 15] = [
        Self::SourceLoc,
        Self::PhysicalLoc,
        Self::Functions,
        Self::Length,
        Self::Depth,
        Self::FlowNodes,
        Self::Changes,
        Self::Churn,
        Self::CoChange,
        Self::Candidates,
        Self::Verified,
        Self::Unclassified,
        Self::Coverage,
        Self::Calls,
        Self::Status,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::SourceLoc => "Source LOC",
            Self::PhysicalLoc => "Physical LOC",
            Self::Functions => "Functions",
            Self::Length => "Median / P95 function length",
            Self::Depth => "Max control depth",
            Self::FlowNodes => "Control-flow nodes",
            Self::Changes => "Git changes",
            Self::Churn => "Git churn",
            Self::CoChange => "Co-change",
            Self::Candidates => "Review candidates",
            Self::Verified => "Verified Imports",
            Self::Unclassified => "Unclassified Imports",
            Self::Coverage => "Module coverage / resolution",
            Self::Calls => "Incoming / outgoing calls",
            Self::Status => "Partial / truncated / unknown",
        }
    }
    pub fn explanation(self) -> Explanation {
        let (what, why, limitations, next) = match self {
            Self::SourceLoc => (
                "Nonblank lines in measured source/test files; includes comments. Provenance: bounded repository text scan.",
                "Locate substantial source areas to browse.",
                "Not syntactic SLOC, importance or exclusive module size. Skipped files make totals partial.",
                "Open a source file from Largest source files; inspect its Rust symbols."),
            Self::PhysicalLoc => (
                "Physical text lines, including blank lines and comments. Function length spans the declaration through its closing body line.",
                "Understand reading volume and find long declarations.",
                "Formatting changes length; macros are not expanded. Repository totals include non-source text.",
                "Open a measured function and read its exact source."),
            Self::Functions => (
                "Rust function/method bodies found by the bounded syn pass, including test-classified bodies and trait defaults.",
                "Estimate the measured callable structure and find navigation candidates.",
                "Not RA symbol count. Bodyless declarations, closures and macro-generated functions are not measured; cfg is not evaluated.",
                "Choose a ranked function; RA verifies its exact anchor before Calls."),
            Self::Length => (
                "Nearest-rank physical function lengths: sorted rank ceil(N*50/100) and ceil(N*95/100). P95 has at least 95% of measured lengths at or below it.",
                "Compare typical declaration length with the long tail.",
                "Median is the lower middle for even N, not an average. Empty samples return 0; partial samples do not describe unmeasured bodies.",
                "Expand Longest functions; inspect the source before judging readability."),
            Self::Depth => (
                "Maximum nested if/match/for/while/loop expressions in a function. Else-if chains stay at the same depth.",
                "Find nested control structures that may deserve a closer read.",
                "Not cyclomatic complexity or evidence of a bug. Plain blocks, match arms, nested items and closures add no control depth.",
                "Expand Deepest control flow and Enter opens the exact function anchor."),
            Self::FlowNodes => (
                "Count of if/match/for/while/loop expressions visited in a function body by syn.",
                "Describe how many control constructs occur, independently of their nesting.",
                "Not cyclomatic complexity, branch count or runtime cost; closures and nested items are excluded.",
                "Inspect a measured function; use RA Calls for static call relationships."),
            Self::Changes => (
                "Number of analyzed committed changes touching a file in the displayed day window. Provenance: bounded local Git history.",
                "Find files frequently involved in recent work.",
                "Not defects or a risk score; uncommitted edits are excluded. Shallow history and scan limits can omit changes.",
                "Select a hotspot, inspect evidence, then Enter opens its file."),
            Self::Churn => (
                "Sum of known added and deleted lines from committed file numstat records.",
                "Distinguish change volume from the number of touching commits.",
                "Not unique changed lines or effort. Binary/unavailable counts stay unknown; formatting can dominate churn.",
                "Open the current file; compare structure and relevant call sites."),
            Self::CoChange => (
                "Number of analyzed commits where this path and another path changed together.",
                "Suggest another file to examine for a possible shared change context.",
                "Not a call/dependency edge or proof of coupling. Large commits and history budgets can make pairs partial.",
                "Read the named companion file via Explorer before inferring a relationship."),
            Self::Candidates => (
                "Current file with at least 3 touching commits AND source LOC >=400, longest function >=80 lines, or control depth >=4.",
                "Explain why a measured file is offered for investigation.",
                "Heuristic review candidate, not a defect or absolute priority. Structural-only routes work without Git; rankings are bounded.",
                "Choose Review attention areas; inspect evidence, source and a verified callable."),
            Self::Verified => (
                "Conservatively verified syntactic import records between known modules. Provenance: existing declared-module/import pass.",
                "Explore a limited source-level relationship with Imports / Used by.",
                "Records are not runtime calls or distinct module counts. No macro expansion, general alias/re-export resolution or cfg evaluation.",
                "In Module Map use v for relationships, o for source, then inspect an RA callable."),
            Self::Unclassified => (
                "Observed import records the conservative module pass cannot verify.",
                "Expose uncertainty rather than guess a dependency.",
                "Not broken imports or defects. Module Map is not a complete semantic dependency graph.",
                "Use a in Module Map for retained reasons; inspect exact source."),
            Self::Coverage => (
                "Coverage reports discovery losses/errors; resolution separately reports limits on classifying observed module/import relations.",
                "Decide which parts of the map are supported and which need source inspection.",
                "Complete discovery does not mean complete semantic resolution. Physical subtree totals deduplicate shared files; exclusive inline totals stay unknown.",
                "Read Analysis status and expand a for reasons; o opens the module source."),
            Self::Calls => (
                "Incoming callers and outgoing callees returned by RA call hierarchy for the exact selected callable and scope.",
                "Follow statically discovered call relationships from a binary or symbol.",
                "Not a proven runtime execution trace. Scope, paging, budgets, dynamic dispatch and indexing affect visibility; zero while indexing is provisional.",
                "Enter changes Focus; j/k changes Selection only. g toggles Graph/List; x expands a direct neighbor; Esc returns."),
            Self::Status => (
                "Partial: some data unavailable. Truncated: a stated budget/window omitted results. Unknown: no verified measurement.",
                "Keep absence of evidence separate from an observed zero.",
                "Loading, failed and empty are different states. A bounded ranking is not the full set of functions or files.",
                "Check analysis readiness and retained reasons; retry a specific route when ready."),
        };
        Explanation {
            what,
            why,
            limitations,
            next,
        }
    }
}

pub(super) fn lines(metric: Metric) -> Vec<ratatui::text::Line<'static>> {
    let e = metric.explanation();
    let mut lines = vec![super::ui::heading(metric.label())];
    for (label, text) in [
        ("WHAT", e.what),
        ("WHY", e.why),
        ("LIMITATIONS", e.limitations),
        ("NEXT ACTION", e.next),
    ] {
        lines.push(super::ui::heading(label));
        lines.push(ratatui::text::Line::raw(text));
    }
    lines
}
