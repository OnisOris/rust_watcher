#!/usr/bin/env python3
"""Verify saved real-PTY acceptance evidence (QA-only Pillow dependency)."""
import hashlib
import json
from pathlib import Path

from PIL import Image


def main():
    root = Path("docs/ux/evidence/v2")
    with open("target/debug/wt", "rb") as binary:
        digest = hashlib.file_digest(binary, "sha256").hexdigest()
    required = {
        "home": ["01-overview-120x35", "02-overview-80x20", "03-selected-depth",
                 "04-depth-interpretation", "09-indexing"],
        "review": ["05-review-evidence"],
        "architecture": ["06-module-inspector", "08-return-original-module"],
        "execution": ["07-call-graph"],
        "unavailable": ["10-ra-unavailable"],
    }
    for group, captures in required.items():
        manifest = json.loads((root / group / "session.json").read_text())
        assert manifest["binary_sha256"] == digest, group
        assert manifest["events"][-1]["exit_code"] == 0, group
        recorded = {e["step"]["capture"] for e in manifest["events"]
                    if "capture" in e.get("step", {})}
        for capture in captures:
            assert capture in recorded, capture
            assert (root / group / f"{capture}.txt").read_text().strip(), capture
            with Image.open(root / group / f"{capture}.png") as image:
                assert image.size == ((816, 436) if capture == "02-overview-80x20"
                                      else (1216, 751)), capture
    before = (root / "architecture/06-module-inspector.txt").read_text().splitlines()
    after = (root / "architecture/08-return-original-module.txt").read_text().splitlines()
    assert before[3:] == after[3:], "module selection/focus/page was not restored"
    for capture in ["07-call-graph", "graph-restored"]:
        assert "Focus: main · app/src/main.rs:1:4" in (root / "execution" / f"{capture}.txt").read_text()
    assert "nested control structures" in (root / "home/03-selected-depth.txt").read_text()
    assert "Not cyclomatic complexity" in (root / "home/04-depth-interpretation.txt").read_text()
    assert "RA ◐ Indexing" in (root / "home/09-indexing.txt").read_text()
    assert "RA × Error" in (root / "unavailable/10-ra-unavailable.txt").read_text()
    assert "Git changes:" in (root / "review/05-review-evidence.txt").read_text()
    assert "Suggested investigation" in (root / "review/05-review-evidence.txt").read_text()
    assert "calculate" in (root / "search/exact-symbol-inspector.txt").read_text()
    print("PASS: ten real-PTY captures, binary provenance, exact Calls focus and module return")


if __name__ == "__main__":
    main()
