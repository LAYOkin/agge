"""Run paired Codex sessions on fresh AGGE and Godot benchmark projects."""

import argparse
import json
import subprocess
from pathlib import Path

from agent_trials import check, report
from run import TASKS, agge_seed, godot_seed


def run_trial(args, folder, engine, task):
    binary = getattr(args, engine).resolve()
    project = "game.agge" if engine == "agge" else "project.godot"
    if engine == "agge":
        agge_seed(folder / project, task)
        hint = 'The CLI supports query, edit and test. Query {"schema":"edit/OP"} for a single edit form.'
    else:
        godot_seed(folder, task)
        hint = "Godot can run project scripts headlessly with --headless --script FILE."
    prompt = (f"{TASKS[task]}\n\nEdit the {engine} fixture at {project} in this directory. "
              f"The engine executable is {binary}. {hint} Verify your work locally. "
              "Keep changes within this fixture and use local tools only.")
    command = [str(args.codex), "exec", "--ignore-user-config", "--ephemeral", "--json",
               "--skip-git-repo-check", "--sandbox", "workspace-write", "-C", str(folder),
               "-m", args.model, "-c", f'model_reasoning_effort="{args.effort}"', "-"]
    (folder / "prompt.txt").write_text(prompt + "\n", encoding="utf-8")
    log = folder / "usage.jsonl"
    with log.open("wb") as stdout, (folder / "stderr.txt").open("wb") as stderr:
        try:
            process = subprocess.run(command, input=prompt.encode("utf-8"), stdout=stdout,
                                     stderr=stderr, cwd=folder, timeout=args.timeout, check=False)
            status = process.returncode
        except subprocess.TimeoutExpired:
            status = "timeout"
    correct, evidence = check(engine, task, folder, binary)
    (folder / "checker.txt").write_text(evidence + "\n", encoding="utf-8")
    return {"completed": status == 0, "correct": correct, "status": status}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex", default="codex")
    parser.add_argument("--agge", required=True, type=Path)
    parser.add_argument("--godot", required=True, type=Path)
    parser.add_argument("--model", required=True)
    parser.add_argument("--effort", default="low", choices=["low", "medium", "high", "xhigh"])
    parser.add_argument("--task", action="append", choices=TASKS)
    parser.add_argument("--repeats", type=int, default=1)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.repeats < 1 or args.timeout < 1:
        parser.error("repeats and timeout must be positive")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    manifest = output / "trials.json"
    rows = []
    for repetition in range(args.repeats):
        for task_index, task in enumerate(dict.fromkeys(args.task or TASKS)):
            order = ("agge", "godot") if (task_index + repetition) % 2 == 0 else ("godot", "agge")
            for engine in order:
                folder = output / f"{task}-{engine}-{repetition + 1}"
                folder.mkdir()
                result = run_trial(args, folder, engine, task)
                rows.append({"model": args.model, "effort": args.effort, "task": task,
                             "engine": engine, "trial": str(repetition + 1), "format": "codex",
                             "log": f"{folder.name}/usage.jsonl", **result})
                manifest.write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
                print(f"{task} {engine} #{repetition + 1}: {result}", flush=True)
    print(json.dumps(report(manifest), indent=2))


if __name__ == "__main__":
    main()
