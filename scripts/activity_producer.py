#!/usr/bin/env python3
"""Test adapter producer for wt --activity-stream; not native agent telemetry."""
import argparse
import datetime
import json

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("stream")
parser.add_argument("event", choices=["read", "write", "search", "command", "test"])
parser.add_argument("path", nargs="?")
args = parser.parse_args()
record = dict(version=1, session="test-adapter-01", source="test-producer", event=args.event,
              timestamp=datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))
if args.path:
    record["path"] = args.path
with open(args.stream, "a", encoding="utf-8") as stream:
    stream.write(json.dumps(record, ensure_ascii=False) + "\n")
