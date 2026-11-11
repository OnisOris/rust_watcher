# Product UX v2 design

Keep five views, one RA session, existing Journey and exact anchors. First frame
still precedes worker startup. No new analysis pass, dependency or JSON change.

Overview starts with execution, architecture, symbol search and attention areas.
Each route has a description and availability text derived from current state.
Execution reuses the binary picker even with missing RA (source remains useful).
Search explicitly means Rust symbol search. Attention areas include Git rows and
structural rankings, so missing Git does not disable review.

Technical groups remain reachable below tasks. Metric topics are selectable in
one collapsed group: short WHAT/WHY/LIMITATIONS/NEXT in Details, full contextual
help through ?. Help scrolls with existing j/k and Esc closes it. Empty and partial
measurements must be distinguished from zero; provenance is local measured data.

Review evidence is a small typed presentation model, built purely from existing
reports. Stable evidence order: committed changes, source LOC, longest function,
nesting. Actions retain exact file/function anchors. Never invent a function
location from a file aggregate or declare a defect from size/change frequency.

Breadcrumb derives from bounded Journey return points (identifiers only). It
shows origin and destination; exact focus/selection remain in existing inspectors.
Module source→callable→Calls uses the existing bridge and return mechanism.
Graph selection, focus, layout cache and explicit pagination are preserved.

80×20 keeps tasks and status visible; 120×35 adds descriptions and Details.
160×45 shows additional measured context without enlarging empty task cards.
Use existing Enter, Tab, Esc and ?; preserve all hotkeys.
