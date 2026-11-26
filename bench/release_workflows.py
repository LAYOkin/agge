"""Measure exact CLI JSON tokens for equivalent dependency and layout workflows."""

import argparse
import json
from pathlib import Path
import statistics
import subprocess
import tempfile
import time

import tiktoken


ROOT = Path(__file__).resolve().parents[1]


def compact(data):
    return json.dumps(data, separators=(",", ":"), ensure_ascii=False)


class Calls:
    def __init__(self, binary, encoding):
        self.binary = binary
        self.encoding = encoding
        self.requests = 0
        self.tokens = 0

    def call(self, command, project, data, output=None):
        request = compact(data)
        args = [str(self.binary), command, str(project), "-"]
        if output is not None:
            args.append(str(output))
        response = subprocess.run(
            args, input=request, capture_output=True, check=True, text=True, encoding="utf-8",
        ).stdout.strip()
        self.requests += 1
        self.tokens += sum(len(self.encoding.encode(part)) for part in (request, response))
        return json.loads(response)


def walk(calls, project, direct):
    root = "palette/brass"
    if direct:
        reply = calls.call("query", project, {"walk": root, "limit": 100})
        assert "next" not in reply
        return dict(reply["items"])
    # Use breadth-first batches, with directional filtering and a pinned revision.
    frontier, distances, revision = [root], {root: 0}, None
    while frontier:
        assert len(frontier) <= 16
        request = {"read": [{"refs": key, "direction": "in", "limit": 100} for key in frontier]}
        if revision is not None:
            request["base"] = revision
        reply = calls.call("query", project, request)
        revision = reply["revision"]
        next_layer = []
        for key, data in zip(frontier, reply["data"], strict=True):
            assert "next" not in data
            for edge in data["items"]:
                source = edge["from"]
                if source not in distances:
                    distances[source] = distances[key] + 1
                    next_layer.append(source)
        frontier = next_layer
    distances.pop(root)
    return distances


def move(calls, project, direct):
    if direct:
        reply = calls.call("query", project, {"get": "room/main", "path": "/instances/0/0"})
        edits = [{"op": "move_placements", "room": "main", "object": "crate", "by": [16, -8]}]
    else:
        reply = calls.call("query", project, {"get": "room/main", "path": "/instances"})
        edits = [
            {"op": "placement", "room": "main", "id": id, "at": [x + 16, y - 8]}
            for id, kind, x, y in reply["data"] if kind == "crate"
        ]
    calls.call("edit", project, {"base": reply["revision"], "ops": edits})
    return project.read_bytes()


def branch(calls, project, direct):
    # Three probes share a 20,000-tick prefix but have different suffixes.
    prefix = 20_000
    if direct:
        calls.call("checkpoint", project, {"ticks": prefix}, project.parent / "base.aggesave")
    values = []
    for extra in [100, 150, 200]:
        base = 0 if direct else prefix
        spec = {
            "ticks": base + extra,
            "input": [[base + 1, base + extra, "right"]],
            "sample": [[base + extra, "entity/hero/x"], [base + extra, "global/rolls"]],
        }
        if direct:
            spec["from"] = "base.aggesave"
        response = calls.call("test", project, spec)
        assert response["passed"]
        values.append(response["report"]["samples"])
    return values


def changes(calls, project, direct):
    fields = ["room", "global/counter"]
    spec = {"ticks": 127, "input": [[11, 11, "fire"], [92, 92, "fire"]]}
    if direct:
        spec["watch"] = {"fields": fields}
    else:
        spec["sample"] = {"at": list(range(128)), "fields": fields}
    report = calls.call("test", project, spec)["report"]
    if direct:
        assert "truncated" not in report["watch"]
        return report["watch"]["changes"]
    samples = report["samples"]
    return [
        [i // len(fields), i % len(fields), value]
        for i, value in enumerate(samples)
        if i < len(fields) or value != samples[i - len(fields)]
    ]


def run(binary, encoding, runs):
    instances = [[f"box{i}", "crate", (i % 8) * 32, (i // 8) * 32] for i in range(48)]
    layout = "agge/1 {}\nobject crate {}\nroom main " + compact({"instances": instances}) + "\n"
    sandbox = """agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  global rolls = global(rolls) + floor(rand() * 10)
  set x = x + down("right")
---
"""
    watch_sandbox = """agge/1 {}
room main {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  global counter = global(counter) + pressed("fire")
---
"""
    results = {}
    for name, workflow, source in [
        ("palette_impact", walk, None),
        ("move_48_placements", move, layout),
        ("branch_three_replays", branch, sandbox),
        ("watch_128_ticks", changes, watch_sandbox),
    ]:
        variants = {}
        expected = None
        for direct in (False, True):
            times, counts = [], set()
            for i in range(runs + 1):
                with tempfile.TemporaryDirectory() as temporary:
                    if source is None:
                        project = ROOT / "examples" / "pixel_foundry" / "game.agge"
                    else:
                        project = Path(temporary) / "game.agge"
                        project.write_text(source, encoding="utf-8")
                    calls = Calls(binary, encoding)
                    start = time.perf_counter()
                    answer = workflow(calls, project, direct)
                    elapsed = (time.perf_counter() - start) * 1000
                if expected is None:
                    expected = answer
                assert expected == answer, name
                if i > 0:  # Exclude one warm-up per variant.
                    times.append(elapsed)
                    counts.add((calls.requests, calls.tokens))
            assert len(counts) == 1
            requests, tokens = counts.pop()
            variants["compact" if direct else "baseline"] = {
                "requests": requests, "tokens": tokens, "median_ms": round(statistics.median(times), 2),
            }
        results[name] = variants
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--encoding", default="o200k_base")
    parser.add_argument("--runs", type=int, default=5)
    args = parser.parse_args()
    if args.runs < 1:
        parser.error("--runs must be positive")
    results = run(args.binary.resolve(), tiktoken.get_encoding(args.encoding), args.runs)
    print(json.dumps({"encoding": args.encoding, "runs": args.runs, "results": results}, indent=2))


if __name__ == "__main__":
    main()
