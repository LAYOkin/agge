"""Measure the context cost of equivalent AGGE context and inspection workflows."""

import argparse
import json
from pathlib import Path

import tiktoken

from run import compact, run


def measure(binary, root, encoding):
    def invoke(transcript, command, payload):
        request = compact(payload)
        result = run([binary, command, "game.agge", "-"], root, request)
        result.check_returncode()
        transcript.extend([f"agge {command} game.agge -", request, result.stdout.strip()])
        return json.loads(result.stdout)

    def workflow(name, messages):
        return {"workflow": name, "calls": len(messages) // 3, "tokens": sum(
            len(encoding.encode_ordinary(message)) for message in messages), "messages": messages}

    reads = [{"get": "room/table", "path": "/maps"}, {"get": "object/ball", "path": "/script"},
             {"get": "script/ball", "event": "hit_x"}, {"get": "script/ball", "event": "hit_y"},
             {"refs": "object/ball"}, {"refs": "script/ball"}]
    separate, batch = [], []
    answers = [invoke(separate, "query", query) for query in reads]
    combined = invoke(batch, "query", {"read": reads})
    assert all(answer.pop("revision") == combined["revision"] for answer in answers)
    assert answers == combined["data"]

    snapshots, samples = [], []
    ticks = [1, 30, 43, 56, 79, 95]
    input_spans = [[1, 1, "fire"]]
    fields = ["x", "y", "vx", "vy"]
    values = []
    for tick in ticks:
        result = invoke(snapshots, "inspect", {"ticks": tick, "input": input_spans,
                                             "select": {"placement": "ball"}})
        assert result["passed"]
        entity, = result["state"]["entities"]
        values.extend(entity[field] for field in fields)
    result = invoke(samples, "test", {"ticks": ticks[-1], "input": input_spans, "sample": {
        "at": ticks, "fields": [f"entity/ball/{field}" for field in fields]}})
    assert result["passed"] and result["report"]["samples"] == values

    all_forms, one_form = [], []
    operations = invoke(all_forms, "query", {"schema": "edits"})
    placement = invoke(one_form, "query", {"schema": "edit/placement"})
    assert placement == next(item for item in operations if item["op"] == "placement")

    return [workflow(name, messages) for name, messages in [
        ("six queries", separate), ("batched reads", batch),
        ("six replay snapshots", snapshots), ("one sampled replay", samples),
        ("all edit forms", all_forms), ("one edit form", one_form),
    ]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agge", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1] / "examples" / "bankshot"
    results = measure(args.agge.resolve(), root, tiktoken.get_encoding("o200k_base"))
    if args.output:
        args.output.write_text(json.dumps({"encoding": "o200k_base", "workflows": results},
                                         indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    for result in results:
        print(f'{result["workflow"]}: {result["tokens"]} tokens across {result["calls"]} calls')


if __name__ == "__main__":
    main()
