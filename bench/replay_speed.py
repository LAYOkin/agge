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


def tile_fixture(folder, ticks):
    project = folder / "game.agge"
    spec = folder / "replay.json"
    placements, assertions, rows = [], [], []
    for index in range(256):
        hero = f"hero_{index}"
        placements.append([hero, "hero", 8, index * 8])
        assertions.append({"at": ticks, "field": f"entity/{hero}/x", "is": 58})
        rows.append((".a" * 4) + "#" + (".a" * 27) + ".")
    project.write_text('agge/1 {"start":"test"}\nroom test ' +
                       json.dumps({"instances": placements, "maps": ["hall"]}) +
                       '\nobject hero {"size":[6,6],"script":"drive"}' +
                       '\nscript drive\nstep:\n  move 2, 0\n---\ntilemap hall ' +
                       json.dumps({"cell":8,"rows":rows,"tiles":{
                           "a":[[62,72,85,255]],"#":[[100,110,140,255],True]}}) + '\n', encoding="utf-8")
    spec.write_text(json.dumps({"ticks": ticks, "expect": assertions}), encoding="utf-8")
    return project, spec


def overlap_fixture(folder, ticks):
    project = folder / "game.agge"
    spec = folder / "replay.json"
    placements, assertions = [], []
    for index in range(1024):
        x, y = (index % 32) * 128, (index // 32) * 128
        placements.append([f"target_{index}", "target", x, y])
        if index % 8 == 0:
            probe = f"probe_{index}"
            placements.append([probe, "probe", x + (4 if index % 16 == 0 else 32), y])
            assertions.append({"at": ticks, "field": f"entity/{probe}/hits",
                               "is": ticks if index % 16 == 0 else 0})
    project.write_text(
        'agge/1 {}\nroom main ' + json.dumps({"instances": placements}) +
        '\nobject target {"size":[8,8]}\nobject probe {"size":[8,8],"script":"sense"}'
        '\nscript sense\ncreate:\n  set hits = 0\nstep:\n  set hits = hits + touch("target")\n---\n',
        encoding="utf-8")
    spec.write_text(json.dumps({"ticks": ticks, "expect": assertions}), encoding="utf-8")
    return project, spec


def idle_fixture(folder, ticks):
    project = folder / "game.agge"
    replay = folder / "replay.json"
    placements = [["hero", "hero", 0, 0]]
    placements.extend([f"prop_{i}", "prop", (i % 100) * 4, (i // 100) * 4] for i in range(4000))
    project.write_text(
        'agge/1 {}\nroom main ' + json.dumps({"instances": placements}) +
        '\nobject prop {"hidden":true}\nobject hero {"script":"drive"}' +
        '\nscript drive\nstep:\n  move 1, 0\n---\n',
        encoding="utf-8")
    replay.write_text(json.dumps({"ticks": ticks, "expect": [
        {"at": ticks, "field": "entity/hero/x", "is": ticks},
        {"at": ticks, "field": "count/prop", "is": 4000},
    ]}), encoding="utf-8")
    return project, replay


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
    cases = ["small", "large", "collision", "tiles", "overlap", "idle"]
    parser.add_argument("--case", choices=cases, action="append")
    args = parser.parse_args()
    if not 3 <= args.samples <= 100:
        parser.error("use 3 to 100 samples")

    results = []
    with tempfile.TemporaryDirectory(prefix="agge-replay-timing-") as root:
        for name in args.case or cases:
            if name == "collision":
                ticks = 240
                project, replay = collision_fixture(Path(root), ticks)
                details = {"solids": 512, "movers": 256}
            elif name == "tiles":
                ticks = 480
                project, replay = tile_fixture(Path(root), ticks)
                details = {"cells": 64 * 256, "movers": 256}
            elif name == "overlap":
                ticks = 120
                project, replay = overlap_fixture(Path(root), ticks)
                details = {"targets": 1024, "probes": 128}
            elif name == "idle":
                ticks = 10_000
                project, replay = idle_fixture(Path(root), ticks)
                details = {"static": 4000, "movers": 1}
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
