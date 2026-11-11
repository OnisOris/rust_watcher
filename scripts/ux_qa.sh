#!/usr/bin/env bash
# Real-PTY developer QA. Set QA_PYTHON to a Python with pyte and Pillow.
# Example: QA_PYTHON=/tmp/wt-ux-qa/bin/python QA_BINARY=/tmp/wt-reviewed bash scripts/ux_qa.sh
# Pin/copy a reviewed binary before replay; concurrent Cargo builds may replace target/debug/wt.
set -euo pipefail
qa_python="${QA_PYTHON:-python3}"
qa_binary="${QA_BINARY:-target/debug/wt}"
qa_output="${QA_OUTPUT:-docs/ux/evidence/v2}"
"$qa_python" scripts/ux_pty.py --binary "$qa_binary" --project . --scenario docs/ux/scenarios/home.json --output "$qa_output/home"
for scenario in execution architecture; do
  "$qa_python" scripts/ux_pty.py --binary "$qa_binary" --project tests/fixtures/workspace_project --scenario "docs/ux/scenarios/$scenario.json" --output "$qa_output/$scenario"
done
"$qa_python" scripts/ux_pty.py --binary "$qa_binary" --project tests/fixtures/simple_project --scenario docs/ux/scenarios/search.json --output "$qa_output/search"
"$qa_python" scripts/ux_pty.py --binary "$qa_binary" --project . --scenario docs/ux/scenarios/review.json --output "$qa_output/review"
qa_path_dir="$(mktemp -d)"
trap 'rm -rf "$qa_path_dir"' EXIT
ln -s "$(command -v cargo)" "$qa_path_dir/cargo"
ln -s "$(command -v rustc)" "$qa_path_dir/rustc"
PATH="$qa_path_dir:/usr/bin:/bin" "$qa_python" scripts/ux_pty.py --binary "$qa_binary" --project tests/fixtures/workspace_project --scenario docs/ux/scenarios/unavailable.json --output "$qa_output/unavailable"
