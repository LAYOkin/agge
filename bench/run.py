"""Execute matched AGGE and Godot edits, then independently check the results."""
import argparse
import json
import subprocess
import tempfile
from pathlib import Path

import tiktoken

import atlas_case
import bounce_case


GRID = "Place a 12×8 grid of cells in arena at (24,32), spaced (32,24). Name them cell_x_y."
RENAME = "Rename arena to vault. Preserve startup and the return transition from return_room."
SPRINT = "Update drive so holding fire doubles horizontal movement. Keep vertical speed and the starting shield unchanged."
TASKS = {"grid": GRID, "rename": RENAME, "sprint": SPRINT, "atlas": atlas_case.PROMPT, "bounce": bounce_case.PROMPT}
AGGE_AXES = [
    '  set dx = (down("right") - down("left")) * 120 * dt',
    '  set dy = (down("down") - down("up")) * 120 * dt',
]
GODOT_PROJECT = 'config_version=5\n[application]\nrun/main_scene="res://arena.tscn"\n'
GODOT_ARENA = '[gd_scene format=3]\n[node name="Arena" type="Node2D"]\n'
GODOT_CELL = '[gd_scene format=3]\n[node name="Cell" type="Node2D"]\n'
GODOT_RETURN = '''[gd_scene load_steps=2 format=3]
[ext_resource type="Script" path="res://return.gd" id="1"]
[node name="Return" type="Node2D"]
script = ExtResource("1")
'''
GODOT_RETURN_SCRIPT = '''extends Node2D

func advance() -> Error:
    return get_tree().change_scene_to_file("res://arena.tscn")
'''
GODOT_GRID_EDIT = '''extends SceneTree

func _initialize() -> void:
    var arena = load("res://arena.tscn").instantiate()
    var cell = load("res://cell.tscn")
    for y in range(8):
        for x in range(12):
            var item = cell.instantiate()
            item.name = "cell_%d_%d" % [x, y]
            item.position = Vector2(24 + x * 32, 32 + y * 24)
            arena.add_child(item)
            item.owner = arena
    var packed = PackedScene.new()
    var err = packed.pack(arena)
    if err == OK:
        err = ResourceSaver.save(packed, "res://arena.tscn")
    arena.free()
    quit(err)
'''
GODOT_GRID_CHECK = '''extends SceneTree

func _initialize() -> void:
    var arena = load("res://arena.tscn").instantiate()
    var good = arena.get_child_count() == 96
    for y in range(8):
        for x in range(12):
            var item = arena.get_node_or_null("cell_%d_%d" % [x, y])
            good = good and item != null
            if item != null:
                good = good and item.scene_file_path == "res://cell.tscn"
                good = good and item.position == Vector2(24 + x * 32, 32 + y * 24)
    arena.free()
    quit(0 if good else 1)
'''
GODOT_RENAME_EDIT = '''from pathlib import Path

old, new = "arena.tscn", "vault.tscn"
Path(old).rename(new)
for name in ("project.godot", "return.gd"):
    path = Path(name)
    path.write_text(path.read_text().replace("res://" + old, "res://" + new))
'''
GODOT_RENAME_CHECK = '''extends SceneTree

func _initialize() -> void:
    call_deferred("verify")

func verify() -> void:
    if ProjectSettings.get_setting("application/run/main_scene") != "res://vault.tscn":
        quit(1)
        return
    if FileAccess.file_exists("res://arena.tscn"):
        quit(1)
        return
    if change_scene_to_file("res://return_room.tscn") != OK:
        quit(1)
        return
    await scene_changed
    if current_scene.advance() != OK:
        quit(1)
        return
    await scene_changed
    quit(0 if current_scene.scene_file_path == "res://vault.tscn" else 1)
'''

GODOT_DRIVE = '''extends Node2D

var shield := 0

func _ready() -> void:
    shield = 7

func _physics_process(delta: float) -> void:
    var dx = Input.get_axis("ui_left", "ui_right") * 120 * delta
    var dy = Input.get_axis("ui_up", "ui_down") * 120 * delta
    position += Vector2(dx, dy)
'''
GODOT_SPRINT_EDIT = '''from pathlib import Path

path = Path("drive.gd")
source = path.read_text()
before = "position += Vector2(dx, dy)"
after = 'position += Vector2(dx * (2 if Input.is_action_pressed("ui_accept") else 1), dy)'
assert source.count(before) == 1
path.write_text(source.replace(before, after))
'''
GODOT_SPRINT_CHECK = '''extends SceneTree

func _initialize() -> void:
    call_deferred("verify")

func verify() -> void:
    var arena = load("res://arena.tscn").instantiate()
    root.add_child(arena)
    var pilot = arena.get_node("Hero")
    var good = pilot.shield == 7 and pilot.position == Vector2(10, 20)
    Input.action_press("ui_right")
    Input.action_press("ui_down")
    pilot._physics_process(0.25)
    good = good and pilot.position == Vector2(40, 50)
    Input.action_press("ui_accept")
    pilot._physics_process(0.25)
    good = good and pilot.position == Vector2(100, 80)
    Input.action_release("ui_right")
    Input.action_release("ui_down")
    Input.action_release("ui_accept")
    arena.free()
    quit(0 if good else 1)
'''
GODOT_CHECKS = {"grid": GODOT_GRID_CHECK, "rename": GODOT_RENAME_CHECK, "sprint": GODOT_SPRINT_CHECK, "atlas": atlas_case.GODOT_CHECK, "bounce": bounce_case.GODOT_CHECK}


def compact(obj):
    return json.dumps(obj, separators=(",", ":"))


def run(args, cwd, data=None):
    return subprocess.run(args, cwd=cwd, input=data, text=True, encoding="utf-8",
                          capture_output=True, timeout=20, check=False)


def receipt(result):
    return (result.stdout + result.stderr).strip()


def agge_seed(path, task):
    source = '''agge/1 {"title":"Fixture","start":"arena"}
room arena {}
room return_room {"instances":[["portal","portal",0,0]]}
object cell {}
object portal {"hidden":true,"script":"return"}
script return
step:
  goto arena
---
'''
    if task == "sprint":
        source = source.replace("room arena {}", 'room arena {"instances":[["hero","pilot",10,20]]}')
        source += 'object pilot {"script":"drive"}\nscript drive\ncreate:\n  set shield = 7\nstep:\n'
        source += "\n".join([*AGGE_AXES, "  move dx, dy", "---", ""])
    if task == "atlas":
        atlas_case.write_png(path.parent / "terrain.png")
        source = atlas_case.AGGE_PROJECT
    if task == "bounce":
        source = bounce_case.AGGE_PROJECT
    path.write_text(source, encoding="utf-8")


def godot_seed(folder, task):
    fixtures = {"project.godot": GODOT_PROJECT, "arena.tscn": GODOT_ARENA,
                "cell.tscn": GODOT_CELL, "return_room.tscn": GODOT_RETURN,
                "return.gd": GODOT_RETURN_SCRIPT}
    if task == "sprint":
        fixtures["arena.tscn"] = '''[gd_scene load_steps=2 format=3]
[ext_resource type="Script" path="res://drive.gd" id="1"]
[node name="Arena" type="Node2D"]
[node name="Hero" type="Node2D" parent="."]
position = Vector2(10, 20)
script = ExtResource("1")
'''
        fixtures["drive.gd"] = GODOT_DRIVE
    if task == "atlas":
        atlas_case.write_png(folder / "terrain.png")
    if task == "bounce":
        fixtures["arena.tscn"] = bounce_case.GODOT_ARENA
        fixtures["ball.gd"] = bounce_case.GODOT_BALL
    for path, content in fixtures.items():
        (folder / path).write_text(content, encoding="utf-8")


def agge_check(agge, folder, task):
    if task == "grid":
        assertions = [{"at":0,"field":"count/cell","is":96}]
        for y in range(8):
            for x in range(12):
                for prop, value in (("object","cell"), ("x",24+x*32), ("y",32+y*24)):
                    assertions.append({"at":0,"field":f"entity/cell_{x}_{y}/{prop}","is":value})
        specs = [{"ticks":0,"expect":assertions}]
    elif task == "rename":
        old = run([agge,"query","game.agge","-"],folder,compact({"get":"room/arena"}))
        if old.returncode == 0:
            return False, "old room still exists"
        specs = [{"ticks":0,"expect":[{"at":0,"field":"room","is":"vault"}]},
                 {"room":"return_room","ticks":1,"expect":[{"at":1,"field":"room","is":"vault"}]}]
    elif task == "atlas":
        validation = run([agge,"check","game.agge"],folder)
        sprite = run([agge,"query","game.agge","-"],folder,compact({"get":"sprite/terrain","path":"/frames"}))
        layout = run([agge,"query","game.agge","-"],folder,compact({"get":"tilemap/layout"}))
        if validation.returncode or sprite.returncode or layout.returncode:
            return False, "\n".join(map(receipt, [validation, sprite, layout]))
        frames = [[i % 4 * 16, i // 4 * 16, 16, 16] for i in range(8)]
        tiles = json.loads(layout.stdout)["data"]
        if (json.loads(sprite.stdout)["data"] != frames or tiles.get("atlas") != "terrain"
                or tiles["rows"] != ["abcdefgh"]
                or any(tiles["tiles"].get(key) != [i, i >= 4] for i, key in enumerate("abcdefgh"))):
            return False, "incorrect atlas assignment"
        specs = [atlas_case.AGGE_REPLAY]
    elif task == "bounce":
        specs = [bounce_case.AGGE_REPLAY]
    else:
        assertions = [{"at":0,"field":"entity/hero/shield","is":7}]
        for at, x, y in ((0,10,20), (15,40,50), (30,100,80)):
            for prop, value in (("x",x), ("y",y)):
                assertions.append({"at":at,"field":f"entity/hero/{prop}","is":value})
        specs = [{"ticks":30,"input":[[1,30,"right"],[1,30,"down"],[16,30,"fire"]],"expect":assertions}]
    results = [run([agge,"test","game.agge","-"],folder,compact(spec)) for spec in specs]
    return all(r.returncode == 0 for r in results), "\n".join(map(receipt,results))


def agge_trial(agge, folder, task):
    agge_seed(folder / "game.agge", task)
    if task == "grid":
        ops = [{"op":"grid", "room":"arena", "object":"cell", "prefix":"cell",
                "at":[24,32], "step":[32,24], "size":[12,8]}]
    elif task == "rename":
        ops = [{"op":"rename","kind":"room","id":"arena","to":"vault"}]
    elif task == "atlas":
        ops = atlas_case.AGGE_EDITS
    elif task == "bounce":
        ops = bounce_case.AGGE_EDITS
    else:
        ops = [{"op":"splice","id":"drive","at":[6,1],"text":'  move dx*(1+down("fire")),dy'}]
    payload = compact({"ops": ops})
    result = run([agge, "edit", "game.agge", "-"], folder, payload)
    correct, checks = agge_check(agge, folder, task)
    return {"messages":[TASKS[task], "agge edit game.agge -", payload, receipt(result)],
            "completed":result.returncode == 0, "correct":correct, "check":checks}


def godot_trial(godot, folder, task):
    godot_seed(folder, task)
    if task in ("grid", "atlas"):
        filename = "edit.gd"
        source = GODOT_GRID_EDIT if task == "grid" else atlas_case.GODOT_EDIT
        command = [godot, "--headless", "--quiet", "--script", filename]
        display = "godot --headless --quiet --script edit.gd"
    else:
        import sys
        filename = "edit.py"
        source = {"rename": GODOT_RENAME_EDIT, "sprint": GODOT_SPRINT_EDIT,
                  "bounce": bounce_case.GODOT_EDIT}[task]
        command = [sys.executable, filename]
        display = "python edit.py"
    (folder / filename).write_text(source, encoding="utf-8")
    (folder / "check.gd").write_text(GODOT_CHECKS[task], encoding="utf-8")
    result = run(command, folder)
    checked = run([godot, "--headless", "--quiet", "--script", "check.gd"], folder)
    return {"messages":[TASKS[task], f"write {filename}", source, display, receipt(result)],
            "completed":result.returncode == 0, "correct":checked.returncode == 0, "check":receipt(checked)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--agge", type=Path, required=True)
    parser.add_argument("--godot", type=Path, required=True)
    parser.add_argument("--encoding", default="o200k_base")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    encoding = tiktoken.get_encoding(args.encoding)
    rows = []
    agge = args.agge.resolve()
    godot = args.godot.resolve()
    with tempfile.TemporaryDirectory(prefix="agge-trials-") as scratch:
        for task in TASKS:
            for engine, executable, trial in (("agge", agge, agge_trial), ("godot", godot, godot_trial)):
                folder = Path(scratch) / f"{task}_{engine}"
                folder.mkdir()
                result = trial(executable, folder, task)
                counts = [len(encoding.encode_ordinary(s)) for s in result["messages"]]
                rows.append({"task":task, "engine":engine, "tokens":sum(counts),
                             "message_tokens":counts, **result})
    version = run([godot,"--version"], Path.cwd()).stdout.strip()
    report = {"scope":"scripted edit transcript", "encoding":args.encoding, "godot":version, "trials":rows}
    if args.output:
        args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    for row in rows:
        print(f'{row["task"]:8} {row["engine"]:6} {row["tokens"]:4} tokens '
              f'completed={row["completed"]} correct={row["correct"]}')
        if not row["correct"]:
            print(row["check"])
    return 0 if all(r["completed"] and r["correct"] for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())

