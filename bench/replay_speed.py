"""Measure release CLI replay time and verify the input schedule and collision results."""

import argparse
import json
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


PROJECT = """agge/1 {"start":"test"}
room test {"instances":[["hero","hero",0,0]]}
object hero {"script":"drive"}
script drive
step:
  if pressed("fire")
    global presses = global(presses) + 1
  end
  move down("fire"), 0
---
"""


def fixture(folder, ticks, intervals):
    project = folder / "game.agge"
    spec = folder / "replay.json"
    spacing = (ticks - 50) // intervals
    spans = []
    active = set()
    for i in range(intervals):
        start = 1 + spacing * i
        end = start + 16 + i % 24
        action = "fire" if i % 3 == 0 else f"act_{i % 12}"
        spans.append([start, end, action])
        if action == "fire":
            active.update(range(start, end + 1))

    assertions = [
        {"at": ticks, "field": "entity/hero/x", "is": len(active)},
        {"at": ticks, "field": "global/presses", "is": sum(t - 1 not in active for t in active)},
    ]
    project.write_text(PROJECT, encoding="utf-8")
    spec.write_text(json.dumps({"ticks": ticks, "input": spans, "expect": assertions}), encoding="utf-8")
    return project, spec


def collision_fixture(folder, ticks):
    project = folder / "game.agge"
    spec = folder / "replay.json"
    placements = []
    assertions = []
    for row in range(16):
        for col in range(16):
            x, y = col * 96, row * 192
            hero = f"hero_{col}_{row}"
            placements.append([hero, "hero", x + 8, y + 10])
            placements.append([f"wall_{col}_{row}", "wall", x + 64, y + 10])
            placements.append([f"spare_{col}_{row}", "wall", x + 64, y + 106])
            assertions.append({"at": ticks, "field": f"entity/{hero}/x", "is": x + 56})
    project.write_text(
        'agge/1 {"start":"test"}\nroom test ' + json.dumps({"instances": placements}) +
        '\nobject hero {"size":[8,8],"script":"drive"}\nobject wall {"size":[8,8],"solid":true}'
        '\nscript drive\nstep:\n  move 2, 0\n---\n', encoding="utf-8")
    spec.write_text(json.dumps({"ticks": ticks, "expect": assertions}), encoding="utf-8")
    return project, spec


def sample(binary, project, replay):
    start = time.perf_counter()
    result = subprocess.run([binary, "test", project, f"@{replay}"],
                            capture_output=True, text=True, encoding="utf-8", timeout=60)
    elapsed = time.perf_counter() - start
    if result.returncode or not json.loads(result.stdout)["passed"]:
        raise RuntimeError(result.stdout + result.stderr)
    return elapsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="+", type=Path)
    parser.add_argument("--samples", default=7, type=int)
    parser.add_argument("--case", choices=["small", "large", "collision"], action="append")
    args = parser.parse_args()
    if not 3 <= args.samples <= 100:
        parser.error("use 3 to 100 samples")

    results = []
    with tempfile.TemporaryDirectory(prefix="agge-replay-timing-") as root:
        cases = args.case or ["small", "large", "collision"]
        for name in cases:
            if name == "collision":
                ticks = 240
                project, replay = collision_fixture(Path(root), ticks)
                details = {"solids": 512, "movers": 256}
            else:
                ticks, spans = (3000, 30) if name == "small" else (60_000, 4096)
                project, replay = fixture(Path(root), ticks, spans)
                details = {"spans": spans}
            binaries = [binary.resolve(strict=True) for binary in args.binary]
            samples = [[] for _ in binaries]
            for binary in binaries:
                sample(binary, project, replay)
            for index in range(args.samples):
                order = range(len(binaries)) if index % 2 == 0 else reversed(range(len(binaries)))
                for i in order:
                    samples[i].append(sample(binaries[i], project, replay))
            for binary, timings in zip(binaries, samples):
                results.append({
                    "binary": str(binary), "fixture": name, "ticks": ticks, **details,
                    "median_ms": round(statistics.median(timings) * 1000, 2),
                    "samples_ms": [round(t * 1000, 2) for t in timings],
                })
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
