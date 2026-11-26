"""Equivalent editable collision fixtures and native runtime checkers."""

PROMPT = "Make the ball rebound at full speed when it hits the wall. Preserve its initial position and rightward speed."
AGGE_PROJECT = '''agge/1 {"start":"arena","hz":64}
room arena {"instances":[["ball","ball",0,0],["wall","wall",12,0]]}
object ball {"size":[8,8],"script":"ball"}
object wall {"size":[8,16],"solid":true}
script ball
create:
  set vx = 320
---
'''
AGGE_EDITS = [{"op": "script_event", "id": "ball", "event": "hit_x", "body": "  set vx = -impact()"}]
AGGE_REPLAY = {"ticks": 2, "expect": [
    {"at": tick, "field": f"entity/ball/{field}", "is": value}
    for tick, x, vx in [(0, 0, 320), (1, 4, -320), (2, -1, -320)]
    for field, value in [("x", x), ("vx", vx), ("y", 0)]
]}

GODOT_ARENA = '''[gd_scene load_steps=4 format=3]
[ext_resource type="Script" path="res://ball.gd" id="1"]
[sub_resource type="RectangleShape2D" id="ball"]
size = Vector2(8, 8)
[sub_resource type="RectangleShape2D" id="wall"]
size = Vector2(8, 16)
[node name="Arena" type="Node2D"]
[node name="Ball" type="CharacterBody2D" parent="."]
script = ExtResource("1")
[node name="Shape" type="CollisionShape2D" parent="Ball"]
position = Vector2(4, 4)
shape = SubResource("ball")
[node name="Wall" type="StaticBody2D" parent="."]
position = Vector2(12, 0)
[node name="Shape" type="CollisionShape2D" parent="Wall"]
position = Vector2(4, 8)
shape = SubResource("wall")
'''
GODOT_BALL = '''extends CharacterBody2D

func _ready() -> void:
    velocity = Vector2(320, 0)

func _physics_process(delta: float) -> void:
    if move_and_collide(velocity * delta, false, 0):
        velocity = Vector2.ZERO
'''
GODOT_EDIT = '''from pathlib import Path

path = Path("ball.gd")
source = path.read_text()
before = "if move_and_collide(velocity * delta, false, 0):\\n        velocity = Vector2.ZERO"
after = "var hit = move_and_collide(velocity * delta, false, 0)\\n    if hit:\\n        velocity = velocity.bounce(hit.get_normal())"
assert source.count(before) == 1
path.write_text(source.replace(before, after))
'''
GODOT_CHECK = '''extends SceneTree

func _initialize() -> void:
    call_deferred("verify")

func verify() -> void:
    var arena = load("res://arena.tscn").instantiate()
    root.add_child(arena)
    var ball = arena.get_node("Ball")
    ball.set_physics_process(false)
    await physics_frame
    var good = ball.position == Vector2.ZERO and ball.velocity == Vector2(320, 0)
    ball._physics_process(1.0 / 64)
    good = good and ball.position.distance_to(Vector2(4, 0)) < 0.1 and ball.velocity == Vector2(-320, 0)
    ball._physics_process(1.0 / 64)
    good = good and ball.position.distance_to(Vector2(-1, 0)) < 0.1 and ball.velocity == Vector2(-320, 0)
    arena.free()
    quit(0 if good else 1)
'''
