# Wii U Cemu shader capture sessions

## Identity and scope

Observed 2026-09-28: Cemu 2.6, Vulkan, macOS Apple GPU; BotW EU title
`00050000101c9500`, update v208. The installed update RPX matches SHA256
`ba58da5b95ce929e005d058ceb08b9b2788d1ab2bbc8a6c189bbadca0bb34d30` from
[CPU analysis](wiiu-render-cpu.md). Retail version remains unconfirmed.
Use [exact-byte matching](wiiu-shader-containers.md#matching-a-cemu-runtime-dump)
against the existing 36,112 extracted code files; preserve all aliases.

The user reported visiting every suggested route point briefly, including
night, water and lava, without testing all weather conditions. This is a
user observation, not a file-to-scene mapping or matched-frame capture.
They confirmed restarting Cemu without re-enabling shader dumping, and
then re-enabled dumping. The user controls all game launches.

## Local snapshots

Snapshots live under the configured `reference` root's `cemu-sessions/`.
Each directory contains `shaders/`, a SHA256/size/mtime `snapshot.json`,
`cemu-log.txt` and exact-byte `matches.json`. Keep these outside Git.
These are accumulated-directory snapshots, not isolated scene captures.

| Snapshot | Binary files | Unique byte hashes | Matched files | Unmatched |
|---|---:|---:|---:|---:|
| `20260928T011247Z-first-observed` | 50 | 45 | 36 | 14 |
| `20260928T011820Z-dump-enabled` | 124 | 108 | 90 | 34 |
| `20260928T012415Z-cache-replay` | 3536 | 2442 | 2466 | 1070 |

Each binary has a corresponding translated text file. The first snapshot's
source mtimes are 00:15–00:18 UTC (04:15–04:18 local), preceding the logged
current Cemu start at 04:32 and title launch at 04:33. It cannot represent
the entire reported tour. Its `observations.json` records the later user
clarification separately from the original snapshot metadata.

The second snapshot was taken while the directory was growing. Individual
files were stable across each read; no unstable files were skipped, but
the directory snapshot is not atomic. Its log still starts at 04:32, so
startup cache replay has **not** been observed in this snapshot.

Matched file counts by archive family (not unique programs or draws):

| Family | First snapshot | Dump-enabled snapshot |
|---|---:|---:|
| `uking_mat` | 34 | 79 |
| `agl_common` | 2 | 2 |
| `uking_sys` | 0 | 7 |
| `uking_pass_shader` | 0 | 2 |

In the second snapshot, the pass matches are VS candidates 270/274 and PS 271. They do not
establish the previously investigated tone-map PS 547/549 or sky PS 419.
No terrain-water archive match was present in those first two snapshots. Absence does not prove
absence from rendering. Unmatched programs remain unresolved; do not infer
their family or blame graphics mods without checking the bytes and sources.

## Verified startup replay

The user performed another launch. The new log records Cemu start at
05:22:36 local, title mount at 05:22:40, and title run at 05:22:42. An initial
inspection found 3,531 binary/text pairs, all written at 01:22:42–01:22:43
UTC (05:22 local). Combined with the cache-reader source below, this is
strong evidence that startup replay recovered the accumulated programs.
The later saved snapshot contains 3,536 pairs; gameplay can add more files.
No unstable files were skipped during the snapshot.

Exact matches now span 15 archive families, including 7 terrain-water dump
files, 21 terrain files, 77 pass-shader files and 52 postfx-technique files.
These family counts count files, not unique programs, and are not additive
where code aliases cross archive boundaries. The saved `summary.json`
contains the full family breakdown.

The previously studied tone-map PS 547 is present as
`37040a485a29d54e_00000000000003c9_ps.bin`. No exact match was found for
LUT variant PS 549, sky PS 419 or terrain-water program 0 VS. These are
remaining identification/coverage questions, not requests to repeat the
route. Other variants, runtime changes and unmatched code need examination
before asking for a specific scene. The 1,070 unmatched files are retained.
An exact match alone does not establish a unique variant when aliases exist.

The dump is sufficient to continue research. Do not ask the user to collect
all locations or weather upfront. Request a targeted capture only when an
identified missing program or uniform question requires it.

The dump is incomplete by construction: it contains only programs that the
user's sessions (or the replayed cache) have triggered. A program or
variant missing from it may still exist in the game. Do not treat its
absence as evidence, and do not substitute a guessed shader. Tell the user
which program is missing and ask them to trigger it by playing, naming the
scene, time of day, weather and effect that should use it.

## Recover cached shaders before repeating the route

Cemu 2.6's cache readers call both translated and raw shader dump functions
when loading vertex, geometry and pixel entries; see
[`LatteShaderCache_readSeparable*Shader`](https://github.com/cemu-project/Cemu/blob/v2.6/src/Cafe/HW/Latte/Core/LatteShaderCache.cpp#L687),
especially lines 727–728, 768–769 and 805–806. This source fact supports
trying cache replay before another full tour; the local startup result is
recorded above.

1. After opening Cemu, enable **Debug → Dump → Dump shaders before launching
   BotW**. Recheck the toggle after every Cemu restart.
2. Launch BotW normally and allow cache loading to complete. Keep the cache.
3. Snapshot the dump and run the matcher again; compare family coverage and
   exact target programs. Only then request missing scenarios from the user.

Observed local paths on this machine:

- Dump: `~/Library/Application Support/Cemu/dump/shaders`.
- Transferable cache:
  `~/Library/Caches/Cemu/shaderCache/transferable/00050000101c9500_shaders.bin`
  (5,758,689 bytes when inspected). It is outside the user-data directory.

Loading an accumulated cache may recover programs from earlier encounters;
it cannot establish which scene used them or enumerate every possible game
variant. Shader bytes/text also do not capture uniform values, textures,
draw order or final-frame fidelity. Keep weather coverage and matched-scene
A/B open. Do not delete caches or relaunch the game on the user's behalf.

## Next check

Reuse the saved startup snapshot. Analyze matched translated shaders and
preserve native aliases, starting with tone-map PS 547 and the observed
terrain-water candidates. Investigate unmatched files and absent target
programs before requesting targeted scenarios. Continue CPU uniform tracing;
no extraction rerun or further broad user tour is needed.
