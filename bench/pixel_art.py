"""Compare identical pixel output and measure AGGE sprite authoring transcripts."""

import argparse
import hashlib
import itertools
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

import tiktoken


def compact(value):
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False)


def run(binary, command, project, payload=None, *extra):
    args = [binary, command, project]
    if payload is not None:
        args.append("-")
    result = subprocess.run(
        [*args, *extra], input=None if payload is None else compact(payload),
        capture_output=True, encoding="utf-8", timeout=30,
    )
    if result.returncode:
        raise RuntimeError(f"{command}: {result.stderr}{result.stdout}")
    return json.loads(result.stdout)


def put(kind, id, data):
    return {"op": "put", "kind": kind, "id": id, "data": data}


def raster(binary, project, art, frame, size):
    return run(binary, "query", project, {"pixel": art, "frame": frame, "area": [0, 0, *size]})["data"]


def runs(row):
    words = []
    for key, run_pixels in itertools.groupby(row):
        count = len(list(run_pixels))
        words.append(str(count) + key if count > 2 else key * count)
    return "".join(words)


def quality(rows):
    covered = {(x, y) for y, row in enumerate(rows) for x, key in enumerate(row) if key != "."}
    colors = set("".join(rows)) - {"."}
    width = 1 + max(x for x, _ in covered) - min(x for x, _ in covered)
    height = 1 + max(y for _, y in covered) - min(y for _, y in covered)
    pending = covered.copy()
    largest = 0
    while pending:
        group = [pending.pop()]
        count = 0
        while group:
            x, y = group.pop()
            count += 1
            for dx, dy in itertools.product((-1, 0, 1), repeat=2):
                neighbor = (x + dx, y + dy)
                if neighbor in pending:
                    pending.remove(neighbor)
                    group.append(neighbor)
        largest = max(largest, count)
    return {"ink_pixels": len(covered), "swatches": len(colors), "bounds": [width, height],
            "largest_island_percent": round(100 * largest / len(covered), 1)}


def difference(first, second):
    return sum(a != b for left, right in zip(first, second) for a, b in zip(left, right))


def measure(binary, project, temp):
    encoding = tiktoken.get_encoding("o200k_base")
    tokens = lambda payload: len(encoding.encode_ordinary(compact(payload)))

    def get(id):
        return run(binary, "query", project, {"get": id})["data"]

    metadata = run(binary, "query", project, {"pixel": "diver"})
    size, frames = metadata["size"], metadata["frames"]
    poses = ["base", *frames]
    pixels = {name: raster(binary, project, "diver", name, size) for name in poses}
    all_colors = put("palette", "brass", get("palette/brass"))
    authored = {"ops": [all_colors, *[
        put("pixel", id, get(f"pixel/{id}")) for id in ("helm", "spanner", "diver")
    ]]}
    literal = f"canvas {size[0]} {size[1]}\nstyle brass"
    for name in poses:
        if name != "base":
            literal += f"\nframe {name}\nlayer merged\n  clear 0 0 {size[0]} {size[1]}"
        else:
            literal += "\nlayer merged"
        literal += "\n  rows 0 0 " + "/".join(map(runs, pixels[name]))
    flattened = {"ops": [all_colors, put("pixel", "diver", literal)]}

    png = {}
    for workflow, payload in [("authored", authored), ("expanded_rle", flattened)]:
        candidate = temp / f"{workflow}.agge"
        run(binary, "init", candidate)
        run(binary, "edit", candidate, payload)
        sheet = temp / f"{workflow}.png"
        run(binary, "art", candidate, {"pixel": "diver", "frames": poses}, sheet)
        png[workflow] = sheet.read_bytes()
        assert all(raster(binary, candidate, "diver", pose, size) == pixels[pose] for pose in poses)
    assert png["authored"] == png["expanded_rle"]

    copy = temp / "visor.agge"
    shutil.copyfile(project, copy)
    base = metadata["revision"]
    patch = {"base": base, "ops": [
        {"op": "pixel_patch", "id": "diver", "frame": "idle", "layer": "head", "draw": "px w 23 9"}
    ]}
    result = run(binary, "edit", copy, patch)
    changed = run(binary, "query", copy, {"get": "pixel/diver"})["data"]
    after = raster(binary, copy, "diver", "idle", size)
    assert difference(pixels["idle"], after) == 1
    run(binary, "undo", copy, None, result["revision"])
    replacement = {"base": base, "ops": [put("pixel", "diver", changed)]}
    equivalent = run(binary, "edit", copy, replacement)
    assert result == equivalent
    assert raster(binary, copy, "diver", "idle", size) == after

    diver = quality(pixels["idle"])
    weld_changes = difference(pixels["idle"], pixels["weld_1"])
    recoil_changes = difference(pixels["recoil_0"], pixels["recoil_3"])
    crab = quality(raster(binary, project, "scuttler", "rest", [40, 36]))
    hatch = quality(raster(binary, project, "bulkhead", "base", [48, 32]))
    assert diver["swatches"] >= 20 and diver["largest_island_percent"] >= 90
    assert crab["swatches"] >= 16 and min(crab["bounds"]) >= 29
    assert hatch["swatches"] >= 14 and hatch["bounds"] == [48, 30]
    assert weld_changes >= 50 and recoil_changes >= 20
    return {
        "encoding": "o200k_base",
        "creation_tokens": {"authored": tokens(authored), "expanded_rle": tokens(flattened)},
        "one_pixel_edit_tokens": {"local_patch": tokens(patch), "source_replacement": tokens(replacement)},
        "quality": {"diver": diver, "scuttler": crab, "bulkhead": hatch,
                    "weld_changed_pixels": weld_changes, "recoil_changed_pixels": recoil_changes,
                    "matching_poses": len(poses), "packed_png_sha256": hashlib.sha256(png["authored"]).hexdigest()},
    }


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agge", type=Path, required=True)
    parser.add_argument("--project", type=Path, default=root / "examples/pixel_foundry/game.agge")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="agge-pixel-benchmark-") as temporary:
        results = measure(args.agge.resolve(), args.project.resolve(), Path(temporary))
    report = json.dumps(results, indent=2) + "\n"
    if args.output:
        args.output.write_text(report, encoding="utf-8")
    print(report, end="")


if __name__ == "__main__":
    main()
