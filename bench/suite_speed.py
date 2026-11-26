"""Compare 24 independently verified replays with one compiled replay suite."""

import argparse
import json
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


def measure(binary, folder, batched):
    expected = [0, 0]
    start = time.perf_counter()
    for request in ["suite.json"] if batched else [f"case_{i}.json" for i in range(24)]:
        result = subprocess.run([binary, "test", "game.agge", f"@{request}"], cwd=folder,
                                capture_output=True, text=True, encoding="utf-8", timeout=30, check=True)
        payload = json.loads(result.stdout)
        if not payload["passed"]:
            raise RuntimeError(result.stdout)
        report = payload if batched else payload["report"]
        expected[0] += report["assertions"]
        expected[1] += report["ticks"]
    if expected != [48, sum(range(1, 25))]:
        raise RuntimeError(f"unexpected outcome {expected}")
    return round(1000 * (time.perf_counter() - start), 2)


def measure_mcp(binary, folder):
    requests = [
        {"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
    ]
    for i in range(24):
        requests.append({"jsonrpc": "2.0", "id": i + 1, "method": "tools/call",
                         "params": {"name": "agge_query", "arguments": {"refs": f"script/utility_{i}"}}})
    inputs = "\n".join(map(json.dumps, requests)) + "\n"
    start = time.perf_counter()
    result = subprocess.run([binary, "mcp", "game.agge"], cwd=folder, input=inputs,
                            capture_output=True, text=True, encoding="utf-8", timeout=30, check=True)
    elapsed = round(1000 * (time.perf_counter() - start), 2)
    replies = list(map(json.loads, result.stdout.splitlines()))
    if len(replies) != 25:
        raise RuntimeError("wrong MCP reply count")
    for i, reply in enumerate(replies[1:]):
        returned = json.loads(reply["result"]["content"][0]["text"])
        if returned["items"] != [{"from": f"script/utility_{i}", "to": f"script/utility_{i}/fn/value", "at": "line/2"}]:
            raise RuntimeError(f"incorrect dependency result: {returned}")
    return elapsed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--samples", type=int, default=7)
    parser.add_argument("--mcp-baseline", type=Path)
    args = parser.parse_args()
    if not 3 <= args.samples <= 100:
        parser.error("use 3 to 100 samples")
    binary = args.binary.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="agge-suite-") as scratch:
        root = Path(scratch)
        source = ['agge/1 {"start":"test"}', 'room test {"instances":[["hero","hero",0,0]]}',
                  'object hero {"script":"drive"}', 'script drive', 'step:', '  move down("right"), 0', '---']
        for i in range(512):
            source.extend((f"script utility_{i}", f"fn value(n) = n * 2 + {i}",
                           "fn use(n) = value(n)", "---"))
        (root / "game.agge").write_text("\n".join(source) + "\n", encoding="utf-8")
        cases = {}
        for i in range(24):
            ticks = i + 1
            case = {"ticks": ticks, "input": [[1, ticks, "right"]], "expect": [
                {"at": 0, "field": "entity/hero/x", "is": 0},
                {"at": ticks, "field": "entity/hero/x", "is": ticks},
            ]}
            name = f"case_{i}"
            cases[name] = case
            (root / f"{name}.json").write_text(json.dumps(case), encoding="utf-8")
        (root / "suite.json").write_text(json.dumps({"cases": cases}), encoding="utf-8")
        samples = {False: [], True: []}
        for value in samples:
            measure(binary, root, value)
        for i in range(args.samples):
            for suite in (False, True) if i % 2 == 0 else (True, False):
                samples[suite].append(measure(binary, root, suite))
        results = {"cases": 24, "assertions": 48, "scripts": 513, "runs": [
            {"suite": suite, "median_ms": statistics.median(times), "samples_ms": times}
            for suite, times in samples.items()]}
        if args.mcp_baseline:
            baseline = args.mcp_baseline.resolve(strict=True)
            mcp = {False: [], True: []}
            for exe in (baseline, binary):
                measure_mcp(exe, root)
            for i in range(args.samples):
                for updated in (False, True) if i % 2 == 0 else (True, False):
                    mcp[updated].append(measure_mcp(binary if updated else baseline, root))
            results["mcp"] = {"queries": 24, "runs": [
                {"cached": new, "median_ms": statistics.median(times), "samples_ms": times}
                for new, times in mcp.items()]}
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
