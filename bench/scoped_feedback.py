"""Verify scoped media and tile feedback against equivalent expanded workflows."""

import argparse
import hashlib
import json
from pathlib import Path
import statistics
import struct
import subprocess
import tempfile
import time
import zlib

import tiktoken


def call(binary, command, project, request, output=None):
    args = [str(binary), command, str(project), "-"]
    if output is not None:
        args.append(str(output))
    payload = json.dumps(request, separators=(",", ":"))
    raw = subprocess.run(args, input=payload, capture_output=True, check=True, text=True).stdout.strip()
    return payload, raw


def png(size):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    rows = (b"\x00" + b"\x44\x66\x88\xff" * size) * size
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def run(baseline, compact, repetitions):
    encoding = tiktoken.get_encoding("o200k_base")
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        project = root / "game.agge"
        tilemap = {
            "cell": 4, "tiles": {"a": [[24, 45, 56, 255]], "b": [[52, 94, 98, 255]]},
            "rows": ["aabb" * 32] * 128,
        }
        source = [
            'agge/1 {"size":[128,128]}',
            'room main {"maps":["deck"],"instances":[["sample","ship",1,1]]}',
            'object ship {"sprite":"loaded"}',
            'sprite loaded {"file":"visible.png"}',
            "tilemap deck " + json.dumps(tilemap, separators=(",", ":")),
        ]
        (root / "visible.png").write_bytes(png(16))
        unused = png(512)
        for i in range(32):
            (root / f"unused{i}.png").write_bytes(unused)
            source.append(f'sprite unused{i} {{"file":"unused{i}.png"}}')
        project.write_text("\n".join(source) + "\n", encoding="utf-8")
        output = root / "cap.png"
        times = {"baseline": [], "scoped": []}
        hashes = set()
        for i in range(repetitions + 1):
            order = [("baseline", baseline), ("scoped", compact)]
            if i % 2 == 1:
                order.reverse()
            for name, binary in order:
                start = time.perf_counter()
                call(binary, "capture", project, {"ticks":0,"crop":[0,0,32,24]}, output)
                elapsed = (time.perf_counter() - start) * 1000
                hashes.add(hashlib.sha256(output.read_bytes()).hexdigest())
                if i != 0:
                    times[name].append(elapsed)
        assert len(hashes) == 1, "captures differ"
        fields = [f"tile/deck/{x}/{y}" for y in range(6) for x in range(8)]
        expanded = call(compact, "inspect", project, {"ticks":0,"sample":{"at":[0],"fields":fields}})
        scoped = call(compact, "inspect", project, {"ticks":0,"tiles":{"map":"deck","area":[0,0,8,6]}})
        assert json.loads(expanded[1])["report"]["samples"] == list("".join(json.loads(scoped[1])["tiles"]["rows"]))
        tokens = lambda transcript: sum(len(encoding.encode(part)) for part in transcript)
        return {
            "repetitions": repetitions,
            "capture": {
                "baseline_ms": round(statistics.median(times["baseline"]), 2),
                "scoped_ms": round(statistics.median(times["scoped"]), 2),
                "sha256": next(iter(hashes)),
                "unrelated_sheets": 32,
            },
            "tile_region": {"expanded_tokens": tokens(expanded), "scoped_tokens": tokens(scoped)},
        }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("compact", type=Path)
    parser.add_argument("--repetitions", default=7, type=int)
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error("--repetitions must be positive")
    print(json.dumps(run(args.baseline.resolve(), args.compact.resolve(), args.repetitions), indent=2))


if __name__ == "__main__":
    main()
