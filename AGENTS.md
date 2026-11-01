# Instructions for AI Agents (AGENTS.md)

This document establishes the mandatory operational rules, documentation protocols, and coding conventions for all AI agents working on **Motorust Engine**: everything that makes a motorbike racing game in Bevy, knowing nothing of the content it runs.

---

## 1. Language Policy

- **Documentation & Agent Guidelines:** `AGENTS.md`, `README.md`, `assets/CREDITS.md` and every file in `RECORD/` must be written in **English**.
- **Source Code Comments:** All code comments, docstrings (`//!` and `///`), function documentation, and inline explanations across all languages (`Rust`, `WGSL`, `Python`, `YAML`, etc.) must be written in **English**. Comments still in Spanish are translated when the code around them is touched.
- **User Interactions:** If the user prompts in Spanish (or another language), communicate respectfully with the user in their preferred language, but ensure that all code comments, commits, and `RECORD/` entries remain strictly in English.

---

## 2. Mandatory Activity Recording (`RECORD/`)

Every non-trivial modification, feature development, bug fix, refactor, architectural change, or task execution **MUST** be recorded in the `RECORD/` directory.

### 2.1 Append-Only vs. Read-Only Discipline
- **`.WIP.md` files are append-only:** While work on a topic is in progress, new entries must be appended to the active `.WIP.md` file. Never delete, truncate, or rewrite historical entries.
- **`.completed.md` files are strictly read-only:** Once a task has concluded, passed tests, and been renamed to `.completed.md`, it becomes an immutable historical record. Any subsequent work must be tracked in a new `.WIP.md` file.

### 2.2 File Naming Convention & Status Suffixes
- Files in `RECORD/` MUST follow the format `RECORD/YYYY-MM-DD.<topic-slug>.<status>.md`, the status either `WIP` (work in progress, append-only) or `completed` (concluded, verified with `cargo test`, read-only).

### 2.3 Record Entry Format
Each entry must contain:
1. **Header & Context:** Date, task title, and associated commit or PR if known.
2. **User Request:** Direct summary of what the user asked for.
3. **Implementation Details:** What was done, including files modified, algorithms used, and design rationale.
4. **Verification & Testing:** Test results (`cargo test`), format checks (`cargo fmt`), clippy status, and visual/performance notes.

---

## 3. Technical Standards & Architecture

1. **Rust & Bevy Architecture:**
   - The engine is a library crate (`motorust_engine`) on Bevy (ECS). A game depends on it, lays its content pack over the engine's (`content::register_pack`), registers its themes and terrain functions, and builds its app with `motorust_engine::app()`.
   - Systems are split cleanly across plugins (`BikePlugin`, `TrackPlugin`, `WorldPlugin`, etc.).
   - Physics and simulation run in a fixed time step (`FixedUpdate` at 120 Hz) with interpolated rendering (`RunFixedMainLoopSystems::AfterFixedMainLoop`) to ensure consistent behavior across all monitor refresh rates and hardware.

2. **The engine knows nothing of a game's content:**
   - No game's circuits, models, songs, themes or names in its code, its comments or its tests: what a game adds, it adds through the points the engine offers (content packs, themes, terrain functions, plugins).
   - Its tests run on its own content (`assets/`: the demo circuit, the demo roster, the textures, the font, the gamepads' art). The checks a game's tests should run on its own content are written to take it (`pub fn NAME_on(circuits, ...)` in a module's tests, visible with the `testing` feature), with a `#[test]` running them on the engine's demo where they mean something there.
   - Everything in `assets/` is under an open licence, credited in `assets/CREDITS.md`.

3. **Performance & GPU Batching:**
   - High frame rate and 60+ FPS performance are top priorities.
   - Reuse shared mesh handles and materials wherever possible (e.g., batched tree trunks, tree crowns, grass particles) to maximize GPU instancing.
   - Avoid allocating or spawning unbatched heavy entities in per-frame update loops.

4. **Testing & Code Cleanliness:**
   - All pure logic (physics steps, steering, terrain shaping, audio synthesis, camera math) must have unit tests.
   - Run and verify `cargo test` before concluding tasks.
   - Keep code cleanly formatted with `cargo fmt --all --check`.
   - Ensure `cargo clippy --all-targets -- -D warnings` passes.

---

## 4. Tools (`tools/`)

- Scripts that are useful beyond the task at hand, above all for working on circuits (drawing a layout and its height profile, checking it against the limits the tests set), live in `tools/`: not in a scratch directory, and not inside `src/`.
- Each tool opens with a header saying what it does, how to run it and what it needs. Where it mirrors the engine's own code (as `tools/circuit_layout.py` mirrors the centre line and the profile of `src/track.rs`), keep the two in step.
- List every tool in `README.md`. One-off scratch scripts and temporary harnesses (e.g. for capturing screenshots) are not committed.

---

## 5. Licence

- The engine's code is dual-licensed, MIT or Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`). Content added to `assets/` must be under a licence that allows it to be redistributed with the engine, and be credited in `assets/CREDITS.md`.
