"""Stage a portable AGGE build, verify it, and archive it with a checksum."""

import argparse
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


def run(binary, *args):
    subprocess.run([str(binary), *map(str, args)], check=True)


def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def verify(stage, scratch, suffix):
    agge = stage / f"agge{suffix}"
    for example in sorted((stage / "examples").iterdir()):
        project = example / "game.agge"
        if not project.is_file():
            continue
        run(agge, "check", project)
        replay = example / "replay.json"
        if replay.is_file():
            run(agge, "test", project, f"@{replay}")

    # Exercise desktop-player discovery and relative asset paths from the staged tree.
    cinder = stage / "examples" / "cinder"
    exported = scratch / "Cinder Circuit"
    run(agge, "export", cinder / "game.agge", exported)
    run(agge, "test", exported / "game.agge", f"@{cinder / 'replay.json'}")
    player = stage / f"agge-player{suffix}"
    if sha256(exported / f"play{suffix}") != sha256(player):
        raise ValueError("exported game does not contain the staged desktop player")

    # Exercise an update with a version-one save on a standalone export.
    yard = stage / "examples" / "glass_yard"
    exported_yard = scratch / "Glass Yard"
    run(agge, "export", yard / "game.agge", exported_yard)
    replay = json.loads((yard / "replay.json").read_text(encoding="utf-8"))
    upgrade_case = scratch / "upgrade.json"
    upgrade_case.write_text(json.dumps(replay["cases"]["relay_from_version_one"]), encoding="utf-8")
    save = scratch / "progress.json"
    run(agge, "progress", exported_yard / "game.agge", f"@{upgrade_case}", save)
    progress = json.loads(save.read_text(encoding="utf-8"))
    if progress != {"id": "agge_glass_yard", "version": 2, "values": {"deliveries": 10, "unlocked": 1}}:
        raise ValueError("standalone save migration changed expected player progress")


def package(bin_dir, output, label):
    system = platform.system()
    suffix = ".exe" if system == "Windows" else ""
    target = f"agge-{label}-{system.lower()}-{platform.machine().lower()}"
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="AGGE release ") as directory:
        scratch = Path(directory)
        stage = scratch / target
        stage.mkdir()
        for name in ("agge", "agge-studio", "agge-player"):
            binary = f"{name}{suffix}"
            shutil.copy2(bin_dir / binary, stage / binary)
        for name in ("LICENSE", "README.md", "CONTRIBUTING.md", "AGENTS.md"):
            shutil.copy2(ROOT / name, stage / name)
        ignore = shutil.ignore_patterns(
            "*.agge.lock", "*.agge.history", "*.agge.knowledge", "*.aggesave",
            "__pycache__", ".gitkeep",
        )
        for name in ("docs", "examples", "bench"):
            shutil.copytree(ROOT / name, stage / name, ignore=ignore)
        verify(stage, scratch, suffix)
        archive = Path(shutil.make_archive(
            str(output / target), "zip" if suffix else "gztar",
            root_dir=scratch, base_dir=target,
        ))
    archive.with_name(f"{archive.name}.sha256").write_text(
        f"{sha256(archive)}  {archive.name}\n", encoding="ascii", newline="\n",
    )
    print(archive)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, default=ROOT / "target" / "release")
    parser.add_argument("--output", type=Path, default=ROOT / "target" / "dist")
    parser.add_argument("--label", default="dev")
    args = parser.parse_args()
    if not args.label or not all(char.isascii() and (char.isalnum() or char in ".-_")
                                 for char in args.label):
        parser.error("--label accepts ASCII letters, digits, dots, dashes and underscores")
    package(args.bin_dir.resolve(), args.output.resolve(), args.label)


if __name__ == "__main__":
    main()
