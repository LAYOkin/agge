"""Create a benchmark fixture, check an agent's output, or summarize usage logs."""
import argparse
import json
import statistics
import tempfile
from pathlib import Path

from run import GODOT_CHECKS, TASKS, agge_check, agge_seed, godot_seed, receipt, run


def check(engine, task, folder, binary):
    if engine == "godot":
        with tempfile.NamedTemporaryFile(dir=folder, suffix=".gd", delete=False) as checker:
            path = Path(checker.name)
        try:
            path.write_text(GODOT_CHECKS[task], encoding="utf-8")
            result = run([binary, "--headless", "--quiet", "--script", path.name], folder)
            return result.returncode == 0, receipt(result)
        finally:
            path.unlink()
    return agge_check(binary, folder, task)


def usage(path, kind):
    if kind == "codex":
        events = (json.loads(line) for line in path.read_text(encoding="utf-8-sig").splitlines() if line.strip())
        records = [event["usage"] for event in events if event.get("type") == "turn.completed"]
    else:
        records = [json.loads(path.read_text(encoding="utf-8-sig"))]
    if not records:
        return None
    fields = ("input_tokens", "cached_input_tokens", "output_tokens")
    for record in records:
        for key in fields:
            value = record.get(key, 0) if key == "cached_input_tokens" else record[key]
            if type(value) is not int or value < 0:
                raise ValueError(f"{path}: invalid {key}")
        if record.get("cached_input_tokens",0) > record["input_tokens"]:
            raise ValueError(f"{path}: cached input exceeds total input")
    return {key:sum(rec.get(key,0) for rec in records) for key in fields}


def report(manifest):
    rows = json.loads(manifest.read_text(encoding="utf-8-sig"))
    groups = {}
    seen = set()
    for row in rows:
        group = (row["model"],row["task"],row["engine"])
        identity = (*group,row["trial"])
        if identity in seen or group[1] not in TASKS or group[2] not in ("agge","godot"):
            raise ValueError(f"invalid trial or duplicate ID {identity}")
        seen.add(identity)
        if type(row["completed"]) is not bool or type(row["correct"]) is not bool:
            raise ValueError(f"{identity}: completed and correct must be booleans")
        measured = usage(manifest.parent / row["log"],row.get("format","normalized"))
        groups.setdefault(group,[]).append((row,measured))
    data = []
    for (model, task, engine), entries in sorted(groups.items()):
        measured = [tokens for _, tokens in entries if tokens is not None]
        data.append({"model":model,"task":task,"engine":engine,"trials":len(entries),
                     "completed":sum(r["completed"] for r,_ in entries),
                     "correct":sum(r["correct"] for r,_ in entries),
                     "usage_records":len(measured),
                     "median_tokens":statistics.median(t["input_tokens"] + t["output_tokens"] for t in measured) if measured else None,
                     "median_cached":statistics.median(t["cached_input_tokens"] for t in measured) if measured else None,
                     "median_correct_tokens":statistics.median(
                         t["input_tokens"] + t["output_tokens"] for r,t in entries if t is not None and r["correct"]
                     ) if any(t is not None and r["correct"] for r,t in entries) else None})
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action",required=True)
    for action in ("seed","check"):
        case = actions.add_parser(action)
        case.add_argument("engine",choices=["agge","godot"])
        case.add_argument("task",choices=TASKS)
        case.add_argument("folder",type=Path)
        if action == "check":
            case.add_argument("binary",type=Path)
    summarize = actions.add_parser("report")
    summarize.add_argument("manifest",type=Path)
    args = parser.parse_args()
    if args.action == "report":
        print(json.dumps(report(args.manifest),indent=2))
        return 0
    if args.action == "seed":
        args.folder.mkdir(parents=True,exist_ok=False)
        if args.engine == "agge":
            agge_seed(args.folder / "game.agge", args.task)
        else:
            godot_seed(args.folder, args.task)
        print(TASKS[args.task])
        return 0
    result, details = check(args.engine,args.task,args.folder.resolve(),args.binary.resolve())
    print(json.dumps({"correct":result,"details":details}))
    return 0 if result else 1


if __name__ == "__main__":
    raise SystemExit(main())

