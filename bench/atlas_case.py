"""Matched terrain-atlas fixtures for the transcript benchmark."""

import struct
import zlib
from pathlib import Path


PROMPT = (
    "Slice the 64x32 terrain PNG into 16x16 tiles. Paint all eight frames in "
    "row-major order in one row and give only the last four solid collision."
)
AGGE_EDITS = [
    {"op": "slice", "id": "terrain", "cell": [16, 16], "grid": [4, 2]},
    {"op": "atlas", "map": "layout", "sprite": "terrain", "keys": "abcdefgh", "solid": "efgh"},
    {"op": "stamp", "map": "layout", "at": [0, 0], "rows": ["abcdefgh"]},
]
AGGE_PROJECT = """agge/1 {"title":"Atlas fixture","start":"arena","size":[128,40]}
room arena {"instances":[["hero","hero",0,16]],"maps":["layout"]}
object hero {"size":[8,8],"script":"drive"}
sprite terrain {"file":"terrain.png"}
tilemap layout {"cell":16,"at":[0,16],"tiles":{},"rows":["........"]}
script drive
step:
  move 128, 0
---
"""
AGGE_REPLAY = {"ticks": 1, "expect": [{"at": 1, "field": "entity/hero/x", "is": 56}]}

GODOT_EDIT = '''extends SceneTree

func _initialize() -> void:
    var arena = load("res://arena.tscn").instantiate()
    var layer = TileMapLayer.new()
    layer.name = "Terrain"
    var tiles = TileSet.new()
    var atlas = TileSetAtlasSource.new()
    atlas.texture = ImageTexture.create_from_image(Image.load_from_file("res://terrain.png"))
    tiles.add_physics_layer()
    var source = tiles.add_source(atlas)
    layer.tile_set = tiles
    for i in range(8):
        var cell = Vector2i(i % 4, i >> 2)
        atlas.create_tile(cell)
        if i >= 4:
            var data = atlas.get_tile_data(cell, 0)
            data.add_collision_polygon(0)
            data.set_collision_polygon_points(0, 0, PackedVector2Array([
                Vector2(-8,-8), Vector2(8,-8), Vector2(8,8), Vector2(-8,8)
            ]))
        layer.set_cell(Vector2i(i, 0), source, cell)
    arena.add_child(layer)
    layer.owner = arena
    var packed = PackedScene.new()
    var error = packed.pack(arena)
    if error == OK:
        error = ResourceSaver.save(packed, "res://arena.tscn")
    arena.free()
    quit(error)
'''
GODOT_CHECK = '''extends SceneTree

func _initialize() -> void:
    var arena = load("res://arena.tscn").instantiate()
    var layer = arena.get_node_or_null("Terrain")
    if layer == null:
        arena.free()
        quit(1)
        return
    var tiles = layer.tile_set
    var good = layer.get_used_cells().size() == 8 and tiles.tile_size == Vector2i(16, 16)
    var corners = PackedVector2Array([Vector2(-8,-8), Vector2(8,-8), Vector2(8,8), Vector2(-8,8)])
    for i in range(8):
        var cell = Vector2i(i, 0)
        var data = layer.get_cell_tile_data(cell)
        var wanted = Vector2i(i % 4, i >> 2)
        good = good and data != null and layer.get_cell_atlas_coords(cell) == wanted
        if data != null:
            var atlas = tiles.get_source(layer.get_cell_source_id(cell))
            var color = Color8(23 + 29 * i, 192 - 13 * i, 35 + 17 * i, 255)
            good = good and atlas.texture_region_size == Vector2i(16, 16)
            good = good and atlas.texture.get_image().get_pixelv(wanted * 16 + Vector2i(8, 8)).is_equal_approx(color)
            good = good and data.get_collision_polygons_count(0) == int(i >= 4)
            if i >= 4 and data.get_collision_polygons_count(0) == 1:
                good = good and data.get_collision_polygon_points(0, 0) == corners
    arena.free()
    quit(0 if good else 1)
'''


def write_png(path: Path):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    def pixel(x, y):
        i = x // 16 + 4 * (y // 16)
        return 23 + 29 * i, 192 - 13 * i, 35 + 17 * i, 255

    rows = b"".join(b"\0" + bytes(channel for x in range(64) for channel in pixel(x, y)) for y in range(32))
    header = struct.pack(">IIBBBBB", 64, 32, 8, 6, 0, 0, 0)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
