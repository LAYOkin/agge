"""A matched one-shot sprite animation task and native runtime checkers."""

PROMPT = "Create a one-shot animation named burst from six consecutive 16×16 sheet cells starting at frame 2. Play it at 24 fps and hold the last frame."
FRAMES = [[i % 4 * 16, i // 4 * 16, 16, 16] for i in range(8)]
AGGE_EDITS = [{"op": "clip", "id": "burst", "sprite": "sheet", "first": 2,
               "count": 6, "fps": 24, "mode": "once"}]
AGGE_PROBE = {"preview": True, "ops": [
    {"op": "put", "kind": "object", "id": "probe", "data": {"animation": "burst"}},
    {"op": "place", "room": "arena", "object": "probe", "id": "probe", "at": [0, 0]},
], "verify": [{"ticks": 40, "expect": [
    {"at": tick, "field": "frame/probe", "is": frame} for tick, frame in [(0, 2), (6, 4), (12, 6), (40, 7)]
]}]}

GODOT_EDIT = '''extends SceneTree

func _initialize() -> void:
    var frames = SpriteFrames.new()
    frames.add_animation("burst")
    frames.set_animation_speed("burst", 24)
    frames.set_animation_loop("burst", false)
    var sheet = ImageTexture.create_from_image(Image.load_from_file("res://terrain.png"))
    for i in range(2, 8):
        var cell = AtlasTexture.new()
        cell.atlas = sheet
        cell.region = Rect2((i % 4) * 16, (i / 4) * 16, 16, 16)
        frames.add_frame("burst", cell)
    quit(ResourceSaver.save(frames, "res://clips.tres"))
'''

GODOT_CHECK = '''extends SceneTree

func _initialize() -> void:
    call_deferred("verify")

func verify() -> void:
    var frames = load("res://clips.tres") as SpriteFrames
    if frames == null or not frames.has_animation("burst"):
        quit(1)
        return
    if frames.get_frame_count("burst") != 6 or frames.get_animation_loop("burst") or frames.get_animation_speed("burst") != 24:
        quit(1)
        return
    for i in range(6):
        var cell = frames.get_frame_texture("burst", i) as AtlasTexture
        var j = i + 2
        if cell == null or cell.atlas == null or cell.region != Rect2((j % 4) * 16, (j / 4) * 16, 16, 16):
            quit(1)
            return
    var probe = AnimatedSprite2D.new()
    root.add_child(probe)
    probe.sprite_frames = frames
    probe.play("burst")
    create_timer(2).timeout.connect(func(): quit(1))
    await probe.animation_finished
    var correct = probe.frame == 5 and not probe.is_playing()
    probe.queue_free()
    quit(0 if correct else 1)
'''
