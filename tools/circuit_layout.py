#!/usr/bin/env python3
"""Draws a circuit's layout and height profile, and reports where it may
not ride well, without building the game.

Reads a circuit's file in RON (`assets/circuits/NAME.ron`, see the engine's
`src/circuits/data.rs`, with what of it is left in Rust in the engine's
`src/circuits/NAME.rs`, if anything), or a circuit written in Rust, and
works its lap out as the engine does (its `src/track.rs`): the centre line
through `CONTROL_POINTS` (closed centripetal Catmull-Rom, resampled every
`SAMPLE_SPACING` metres) and the heights along it from `ELEVATION` (Hermite with Catmull-Rom
tangents, over `BASE_ELEVATION`). Branches, narrows and banking are left
out: this is the lap's centre line only.

- `--plan FILE.png`: the lap from above, north (-Z) up as on the race map,
  with its walls, its control points (red), a label every `--every` metres
  along it with its height, and what the circuit's source draws round it:
  its coast (`COAST` or `SHORE`; in RON, an `Outline` or
  `OutlineWithLakes`, and the coast's lakes, islets and rivers), hills (`massif`, brown; `climbed_massif`, green, with
  their heights), and a dam's lake (`LAKE`) and river (`RIVER`; in RON, the
  dam's `lake` and `river`). `--box X0 Z0 X1 Z1` zooms in (metres).
- `--profile FILE.png`: height, grade and the radius of the crests and
  dips along the lap (or `--along FROM TO` of it), with the grade the game
  allows and the crest radius the bike takes off over at its top speed.
- Always: the lap's length, its tightest bend and steepest grade against
  the limits of the tests (`curves_are_drivable`,
  `slopes_and_banking_stay_rideable`), and the crests the bike takes off
  over at top speed.

Usage:
    python3 tools/circuit_layout.py assets/circuits/bahia.ron \\
        [--plan plan.png] [--box -80 0 520 820] [--every 50] \\
        [--profile profile.png] [--along 0 600]

From a game's repository holding the engine in `motorust-engine/`:
    python3 motorust-engine/tools/circuit_layout.py assets/circuits/NAME.ron

Needs numpy (and matplotlib for the pictures).
"""

import argparse
import os
import re
import sys

import numpy as np

# As in the engine's src/track.rs.
SAMPLE_SPACING = 2.0
BASE_ELEVATION = 2.0
WALL_DISTANCE = 11.5
MIN_RADIUS = 2.5 * WALL_DISTANCE
MAX_GRADE = 0.13
# The bike's top speed (the engine's src/bike/mod.rs, `max_speed`, m/s), and the gravity
# it takes off against there (`RISE_GRAVITY`, twice the real one, `settle`):
# on a crest it leaves the road on its way up to the top once the road would
# have to pull it down harder than that, so over a crest tighter than
# TOP_SPEED² / RISE_GRAVITY it leaves the road flat out.
TOP_SPEED = 80.0
G = 9.81
RISE_GRAVITY = 2.0 * G
# How far either side of a point the vertical radius is measured over (m).
VERTICAL_WINDOW = 10.0

NUMBER = r"-?\d+(?:\.\d+)?"


def const_block(text, name):
    """The body of `const NAME: [...] = [ ... ];`, or None."""
    m = re.search(r"const %s: \[[^\]]*\] = \[(.*?)\n\];" % name, text, re.S)
    return m.group(1) if m else None


def ron_list(text, name):
    """The body of `name: [ ... ],` in a circuit's RON file, or None."""
    m = re.search(r"\b%s: \[(.*?)\n\s*\]," % name, text, re.S)
    return m.group(1) if m else None


def vec2s(block):
    """Every `Vec2::new(x, z)` in `block`, in order."""
    pattern = r"Vec2::new\(\s*(%s),\s*(%s)\s*\)" % (NUMBER, NUMBER)
    return np.array([(float(x), float(z)) for x, z in re.findall(pattern, block)])


def pairs(block):
    """Every `(a, b)` of plain numbers in `block`, one per line, in order."""
    pattern = r"^\s*\(\s*(%s),\s*(%s)\s*\),?" % (NUMBER, NUMBER)
    return [(float(a), float(b)) for a, b in re.findall(pattern, block, re.M)]


def catmull_rom(p0, p1, p2, p3, u):
    """Points at `u` (an array in [0, 1)) between p1 and p2 of the
    centripetal Catmull-Rom curve (Barry-Goldman), as the game's."""
    t0 = 0.0
    t1 = t0 + np.sqrt(np.linalg.norm(p1 - p0))
    t2 = t1 + np.sqrt(np.linalg.norm(p2 - p1))
    t3 = t2 + np.sqrt(np.linalg.norm(p3 - p2))
    t = (t1 + (t2 - t1) * u)[:, None]

    def lerp(a, b, ta, tb):
        if abs(tb - ta) < 1e-6:
            return np.broadcast_to(a, (len(t), 2))
        return a + (b - a) * ((t - ta) / (tb - ta))

    a1, a2, a3 = lerp(p0, p1, t0, t1), lerp(p1, p2, t1, t2), lerp(p2, p3, t2, t3)
    b1 = a1 + (a2 - a1) * ((t - t0) / (t2 - t0))
    b2 = a2 + (a3 - a2) * ((t - t1) / (t3 - t1))
    return b1 + (b2 - b1) * ((t - t1) / (t2 - t1))


def closed_curve(points, spacing):
    """Points about `spacing` apart along the closed curve through
    `points`."""
    n = len(points)
    u = np.arange(512) / 512.0
    dense = np.concatenate(
        [
            catmull_rom(points[(i - 1) % n], points[i], points[(i + 1) % n], points[(i + 2) % n], u)
            for i in range(n)
        ]
    )
    loop = np.vstack([dense, dense[:1]])
    lengths = np.concatenate([[0.0], np.cumsum(np.linalg.norm(np.diff(loop, axis=0), axis=1))])
    count = int(round(lengths[-1] / spacing))
    targets = np.arange(count) * lengths[-1] / count
    return np.stack(
        [np.interp(targets, lengths, loop[:, 0]), np.interp(targets, lengths, loop[:, 1])], axis=1
    )


def profile_height(points, lap, s):
    """The height of the closed profile through `points` at `s`."""
    n = len(points)

    def at(i):
        laps, wrapped = divmod(i, n)
        return points[wrapped][0] + laps * lap, points[wrapped][1]

    s = s % lap
    i = max((k for k, p in enumerate(points) if p[0] <= s), default=0)
    p0, p1, p2, p3 = at(i - 1), at(i), at(i + 1), at(i + 2)
    span = p2[0] - p1[0]
    m1 = (p2[1] - p0[1]) / (p2[0] - p0[0]) * span
    m2 = (p3[1] - p1[1]) / (p3[0] - p1[0]) * span
    t = (s - p1[0]) / span
    t2, t3 = t * t, t * t * t
    return (
        (2 * t3 - 3 * t2 + 1) * p1[1]
        + (t3 - 2 * t2 + t) * m1
        + (-2 * t3 + 3 * t2) * p2[1]
        + (t3 - t2) * m2
    )


class Lap:
    """A circuit's lap, as the game works it out."""

    def __init__(self, path):
        self.text = open(path).read()
        if path.endswith(".ron"):
            # Its comments name things too, written as its values are.
            self.text = re.sub(r"//[^\n]*", "", self.text)
        self.ron = path.endswith(".ron")
        if self.ron:
            # What its file names in Rust (a dam's or a pit's water, say) is
            # in the engine's `src/circuits/NAME.rs`, if there: read with it.
            source = os.path.join(
                os.path.dirname(os.path.abspath(__file__)),
                "..",
                "src",
                "circuits",
                os.path.basename(path)[: -len(".ron")] + ".rs",
            )
            if os.path.exists(source):
                self.text += open(source).read()
            control = np.array(pairs(ron_list(self.text, "control_points")))
            self.elevation = pairs(ron_list(self.text, "elevation"))
        else:
            control = vec2s(const_block(self.text, "CONTROL_POINTS"))
            self.elevation = pairs(const_block(self.text, "ELEVATION"))
        self.line = closed_curve(control, SAMPLE_SPACING)
        self.control = control
        n = len(self.line)
        loop = np.vstack([self.line, self.line[:1]])
        self.step = np.linalg.norm(np.diff(loop, axis=0), axis=1).sum() / n
        self.length = n * self.step
        self.along = np.arange(n) * self.step
        self.height = np.array(
            [BASE_ELEVATION + profile_height(self.elevation, self.length, s) for s in self.along]
        )

    def index_at(self, along):
        return int(round(along / self.step)) % len(self.line)

    def curvature(self):
        """Signed curvature at every point (1/m), as `Track::curvature`:
        positive turning left."""
        prev, next_ = np.roll(self.line, 1, axis=0), np.roll(self.line, -1, axis=0)
        h1 = np.arctan2(*(self.line - prev).T[::-1])
        h2 = np.arctan2(*(next_ - self.line).T[::-1])
        turn = (h2 - h1 + np.pi) % (2 * np.pi) - np.pi
        mean = 0.5 * (
            np.linalg.norm(self.line - prev, axis=1) + np.linalg.norm(next_ - self.line, axis=1)
        )
        # Z points south, so a turn the maths call positive is to the right.
        return -turn / mean

    def grade(self):
        """The grade from each point to the next, as the tests measure it."""
        return (np.roll(self.height, -1) - self.height) / self.step

    def vertical_curvature(self):
        """How the grade changes along the lap (1/m): negative over crests,
        positive in dips."""
        k = max(1, int(round(VERTICAL_WINDOW / self.step)))
        h = self.height
        return (np.roll(h, -k) - 2 * h + np.roll(h, k)) / (k * self.step) ** 2

    def normals(self):
        tangent = np.roll(self.line, -1, axis=0) - np.roll(self.line, 1, axis=0)
        tangent /= np.linalg.norm(tangent, axis=1)[:, None]
        # The left of the way the lap runs, with north (-Z) up.
        return np.stack([tangent[:, 1], -tangent[:, 0]], axis=1)


def runs(mask):
    """(first, last) index of each run of True in `mask`."""
    out, start = [], None
    for i, flag in enumerate(mask):
        if flag and start is None:
            start = i
        elif not flag and start is not None:
            out.append((start, i - 1))
            start = None
    if start is not None:
        out.append((start, len(mask) - 1))
    return out


def report(lap):
    curvature = lap.curvature()
    radius = 1.0 / np.maximum(np.abs(curvature), 1e-9)
    tightest = int(np.argmin(radius))
    grade = lap.grade()
    steepest = int(np.argmax(np.abs(grade)))
    print(f"lap: {lap.length:.1f} m, {len(lap.line)} points {lap.step:.4f} m apart")
    print(
        f"tightest bend: {radius[tightest]:.1f} m at {lap.along[tightest]:.0f} m "
        f"(the tests want over {MIN_RADIUS:.2f} m)"
        + ("  <-- TOO TIGHT" if radius[tightest] <= MIN_RADIUS else "")
    )
    print(
        f"steepest grade: {grade[steepest] * 100:.1f} % at {lap.along[steepest]:.0f} m "
        f"(the tests want under {MAX_GRADE * 100:.0f} %)"
        + ("  <-- TOO STEEP" if abs(grade[steepest]) >= MAX_GRADE else "")
    )
    takeoff = TOP_SPEED**2 / RISE_GRAVITY
    vertical = lap.vertical_curvature()
    crests = runs(vertical < -1.0 / takeoff)
    print(f"crests the bike leaves the road over at {TOP_SPEED * 3.6:.0f} km/h (radius under {takeoff:.0f} m):")
    if not crests:
        print("  none")
    for first, last in crests:
        top = first + int(np.argmin(vertical[first : last + 1]))
        print(
            f"  {lap.along[first]:.0f}-{lap.along[last]:.0f} m: {-1.0 / vertical[top]:.0f} m "
            f"at {lap.along[top]:.0f} m, {lap.height[top]:.1f} m up, flying from "
            f"{np.sqrt(RISE_GRAVITY * -1.0 / vertical[top]) * 3.6:.0f} km/h"
        )


def plot_plan(lap, out, box, every):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(figsize=(11, 11))
    for name in ("COAST", "SHORE"):
        block = const_block(lap.text, name)
        if block is None:
            continue
        coast = vec2s(block)
        if len(coast) == 0:
            # A coast given as (x, z) pairs.
            coast = np.array(pairs(block))
        if len(coast) > 2:
            ring = closed_curve(coast, 4.0)
            ax.plot(*np.vstack([ring, ring[:1]]).T, color="#2a5db0", lw=0.8)
    lake = const_block(lap.text, "LAKE")
    if lake is not None:
        ax.fill(*closed_curve(vec2s(lake), 3.0).T, color="#3060c0", alpha=0.45, lw=0)
    river = const_block(lap.text, "RIVER")
    if river is not None and len(vec2s(river)) > 1:
        ax.plot(*vec2s(river).T, color="#20a0c0", lw=3)
    if lap.ron:
        # A file's coast as an outline, and its dam's lake and river (a
        # point and a height each: `((x, z), h)`).
        outline = re.search(r"(?:coast: Outline\(|outline: )\[(.*?)\n\s*\]", lap.text, re.S)
        if outline:
            ring = closed_curve(np.array(pairs(outline.group(1))), 4.0)
            ax.plot(*np.vstack([ring, ring[:1]]).T, color="#2a5db0", lw=0.8)
        # The coast's lakes and islets (`(center: (x, z), radius: r, ...)`),
        # and its rivers (a point and how far its banks are each:
        # `((x, z), w)`), as their middles.
        coast = re.search(r"\bcoast: (.*?)\n        hills:", lap.text, re.S)
        if coast:
            circle = r"\(\s*center: \((%s), (%s)\),\s*radius: (%s)" % ((NUMBER,) * 3)
            islets = coast.group(1).find("islets:")
            for match in re.finditer(circle, coast.group(1)):
                x, z, r = map(float, match.groups())
                islet = 0 <= islets < match.start()
                ax.add_patch(
                    plt.Circle((x, z), r, color="#8b5a2b" if islet else "#3060c0", alpha=0.45, lw=0)
                )
            for river in re.findall(r"points: \[(.*?)\n\s*\]", coast.group(1), re.S):
                pattern = r"\(\(\s*(%s),\s*(%s)\s*\)" % (NUMBER, NUMBER)
                points = np.array([(float(x), float(z)) for x, z in re.findall(pattern, river)])
                if len(points) > 1:
                    ax.plot(*points.T, color="#20a0c0", lw=3)
        lake = ron_list(lap.text, "lake")
        if lake is not None:
            ax.fill(*closed_curve(np.array(pairs(lake)), 3.0).T, color="#3060c0", alpha=0.45, lw=0)
        river = ron_list(lap.text, "river")
        if river is not None:
            pattern = r"\(\(\s*(%s),\s*(%s)\s*\)" % (NUMBER, NUMBER)
            points = np.array([(float(x), float(z)) for x, z in re.findall(pattern, river)])
            if len(points) > 1:
                ax.plot(*points.T, color="#20a0c0", lw=3)
    massif = r"(climbed_massif|massif)\(\s*(%s),\s*(%s),\s*(%s),\s*(%s)\s*\)" % ((NUMBER,) * 4)
    # In RON: `(center: (x, z), radius: r, height: h)`, `climbed: true` if
    # the road climbs it.
    ron_massif = (
        r"\(center: \((%s), (%s)\), radius: (%s), height: (%s)(, climbed: true)?\)" % ((NUMBER,) * 4)
    )
    found = re.findall(massif, lap.text) + [
        ("climbed_massif" if climbed else "massif", x, z, r, h)
        for x, z, r, h, climbed in re.findall(ron_massif, lap.text)
    ]
    for kind, x, z, r, h in found:
        x, z, r = float(x), float(z), float(r)
        color = "#2f8f2f" if kind == "climbed_massif" else "#8b5a2b"
        ax.add_patch(plt.Circle((x, z), r, fill=False, color=color, ls="--", lw=0.8))
        ax.text(x, z, f"{float(h):.0f} m", color=color, fontsize=7, ha="center", clip_on=True)
    normal = lap.normals()
    ax.plot(*lap.line.T, "k-", lw=1.2)
    for side in (1, -1):
        ax.plot(*(lap.line + side * WALL_DISTANCE * normal).T, "k:", lw=0.6)
    ax.plot(*lap.control.T, "r.", ms=5)
    for along in np.arange(0, lap.length, every):
        i = lap.index_at(along)
        ax.text(
            *(lap.line[i] + 14 * normal[i]),
            f"{along:.0f}: {lap.height[i]:.0f}",
            fontsize=7,
            color="m",
            ha="center",
            clip_on=True,
        )
    if box:
        ax.set_xlim(box[0], box[2])
        ax.set_ylim(box[3], box[1])
    else:
        ax.invert_yaxis()
    ax.set_aspect("equal")
    ax.grid(True, lw=0.3)
    ax.set_xlabel("x (m, east)")
    ax.set_ylabel("z (m, south)")
    fig.savefig(out, dpi=90, bbox_inches="tight")


def plot_profile(lap, out, along_range):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    first, last = along_range or (0.0, lap.length)
    pick = (lap.along >= first) & (lap.along <= last)
    s = lap.along[pick]
    fig, (top, middle, bottom) = plt.subplots(3, 1, figsize=(12, 9), sharex=True)
    top.plot(s, lap.height[pick], "k-")
    top.plot(
        [p for p, _ in lap.elevation if first <= p <= last],
        [BASE_ELEVATION + h for p, h in lap.elevation if first <= p <= last],
        "r.",
    )
    top.set_ylabel("height (m)")
    middle.plot(s, lap.grade()[pick] * 100, "b-")
    for limit in (MAX_GRADE, -MAX_GRADE):
        middle.axhline(limit * 100, color="r", lw=0.6, ls="--")
    middle.set_ylabel("grade (%)")
    vertical = lap.vertical_curvature()[pick]
    bottom.plot(s, vertical * 1000, "g-")
    bottom.axhline(-1000 * RISE_GRAVITY / TOP_SPEED**2, color="r", lw=0.6, ls="--")
    bottom.set_ylabel("1 / vertical radius (1/km)\ncrests < 0 < dips")
    bottom.set_xlabel("along the lap (m)")
    for axis in (top, middle, bottom):
        axis.grid(True, lw=0.3)
    fig.savefig(out, dpi=90, bbox_inches="tight")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("circuit", help="a circuit's file, e.g. assets/circuits/bahia.ron")
    parser.add_argument("--plan", help="draw the lap from above to this PNG")
    parser.add_argument("--box", nargs=4, type=float, metavar=("X0", "Z0", "X1", "Z1"))
    parser.add_argument("--every", type=float, default=100.0, help="label every this many metres")
    parser.add_argument("--profile", help="draw the height profile to this PNG")
    parser.add_argument("--along", nargs=2, type=float, metavar=("FROM", "TO"))
    args = parser.parse_args()
    lap = Lap(args.circuit)
    report(lap)
    if args.plan:
        plot_plan(lap, args.plan, args.box, args.every)
    if args.profile:
        plot_profile(lap, args.profile, args.along)


if __name__ == "__main__":
    sys.exit(main())
