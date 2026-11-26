"""Time matched edit workflows, including fixture setup and independent verification."""

import argparse
import json
import statistics
import tempfile
import time
from pathlib import Path

from run import TASKS, agge_trial, godot_trial


def trial(folder, binary, engine, task):
    start = time.perf_counter()
    folder.mkdir()
    result = (agge_trial if engine == "agge" else godot_trial)(binary, folder, task)
    elapsed = round(1000 * (time.perf_counter() - start), 2)
    if not result["completed"] or not result["correct"]:
        raise RuntimeError(f"{task} on {engine} failed: {result['check']}")
    return elapsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agge", required=True, type=Path)
    parser.add_argument("--godot", required=True, type=Path)
    parser.add_argument("--samples", default=5, type=int)
    parser.add_argument("--task", action="append", choices=TASKS)
    args = parser.parse_args()
    if not 3 <= args.samples <= 100:
        parser.error("use 3 to 100 samples")
    binaries = {name: getattr(args, name).resolve(strict=True) for name in ("agge", "godot")}
    results = []
    with tempfile.TemporaryDirectory(prefix="agge-edit-timing-") as scratch:
        root = Path(scratch)
        for task in dict.fromkeys(args.task or TASKS):
            times = {name: [] for name in binaries}
            for i in range(args.samples + 1):
                order = ("agge", "godot") if i % 2 == 0 else ("godot", "agge")
                for name in order:
                    ms = trial(root / f"{task}-{name}-{i}", binaries[name], name, task)
                    if i:
                        times[name].append(ms)
            for name, samples in times.items():
                results.append({"task": task, "engine": name, "median_ms": statistics.median(samples),
                                "samples_ms": samples, "completed": len(samples), "correct": len(samples)})
    print(json.dumps({"scope": "fresh fixture setup, edit, and independent verification", "runs": results}, indent=2))


if __name__ == "__main__":
    main()
