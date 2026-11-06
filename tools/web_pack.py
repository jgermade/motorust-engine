#!/usr/bin/env python3
"""Writes the content packs' files for the web build, apart from the .wasm.

A web build has none of the packs' files built in (see src/content.rs): the
page downloads them and hands them to the game. This writes, into OUT:

- `content.pack`: every file the packs' `pack.txt` list, the packs laid one
  over the other (a path is taken from the first pack given that lists it,
  so the game's pack goes first, the engine's last), except the songs;
- `content/<path>`: the songs (`.mp3`), loose, which the game's asset server
  fetches from beside the page only when one is to play.

`content.pack` is read by `unpack` in src/content.rs; keep the two in step.
It is `MRPK`, its version (1) and how many files it has, each a
little-endian u32, then each file: the length of its path, its path
(UTF-8), the length of its bytes (each a u32) and its bytes.

Usage (the game's web/build.sh runs it):

    tools/web_pack.py OUT GAME_ASSETS_FOLDER... ENGINE_ASSETS_FOLDER

Needs Python 3 and nothing else.
"""

import os
import shutil
import struct
import sys

# What is fetched loose, when it is needed, rather than with the pack.
LOOSE = (".mp3",)


def listed(folder):
    """The files a pack's pack.txt lists, in its order."""
    with open(os.path.join(folder, "pack.txt"), encoding="utf-8") as lines:
        return [
            line.strip()
            for line in lines
            if line.strip() and not line.strip().startswith("#")
        ]


def main(out, folders):
    os.makedirs(out, exist_ok=True)
    loose_root = os.path.join(out, "content")
    shutil.rmtree(loose_root, ignore_errors=True)
    packed, loose, seen = [], 0, set()
    for folder in folders:
        for path in listed(folder):
            if path in seen:
                continue
            seen.add(path)
            source = os.path.join(folder, path)
            if path.endswith(LOOSE):
                target = os.path.join(loose_root, path)
                os.makedirs(os.path.dirname(target), exist_ok=True)
                shutil.copyfile(source, target)
                loose += os.path.getsize(source)
            else:
                packed.append((path, source))
    with open(os.path.join(out, "content.pack"), "wb") as pack:
        pack.write(b"MRPK" + struct.pack("<II", 1, len(packed)))
        for path, source in packed:
            with open(source, "rb") as file:
                data = file.read()
            name = path.encode("utf-8")
            pack.write(struct.pack("<I", len(name)) + name)
            pack.write(struct.pack("<I", len(data)) + data)
    size = os.path.getsize(os.path.join(out, "content.pack"))
    print(
        f"content.pack: {len(packed)} files, {size / 1048576:.1f} MB; "
        f"content/: {len(seen) - len(packed)} files, {loose / 1048576:.1f} MB"
    )


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2:])
