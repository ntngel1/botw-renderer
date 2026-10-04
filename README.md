# botw рендерер

An experimental **The Legend of Zelda: Breath of the Wild** world renderer built
with Rust and Bevy. World geometry, models, textures, skeletons and animation
clips come from your own game dump. Explore the world with a free camera and
preview character animations in the debug menu.

**Status as of October 4, 2026: working research prototype, version 0.1.0.**
Hateno is the main verified region. Rendering fidelity, effects and NPC assembly
remain incomplete.

## Implemented

| Subsystem | Current state |
|---|---|
| Asset preparation | `bake` converts the dump into terrain, surface textures, objects, trees, grass, environment tables, water, effects and characters. Base game, update and DLC directories are layered in priority order. |
| World | Terrain and object streaming, LOD, near and distant trees, grass, a free camera and a debug menu. The default bake covers a square around Hateno with a 1000 m half-width. |
| Lighting | Game sky and environment tables, time of day, fog, clouds and cloud shadows, environment cube map, AO, emission, tone curve and bloom. Documented approximations remain. |
| Weather and effects | Particles, ELink, rain, snow, wet surfaces, thunderstorms and volume masks. Some families and playback conditions are incomplete. |
| Water and glass | Terrain water, translucent materials and waterfall families found around Hateno, with material animation. |
| Link | Skeleton and outfit, native clips, animation blending and facial animation. |
| NPCs | Experimental Hylian UMii assembly: body and face parts, materials and map placements. A verified bake contains 58 placements / 57 actors. Preview is opt-in through `--villagers`. |

## Not finished

- Complete UMii assembly and proportions: individual facial transforms, mouth geometry, head attachment, beard jaw forwarding, height and weight. Native tables are being read, but their complete application pipeline is not connected. Other races, Mii-based characters and the full NPC roster are unsupported.
- Full rendering fidelity: differences remain in shadows, character lighting, translucent passes, cube maps, the material combiner and some dynamic inputs. Materials and effects outside the verified region require further work.
- Every particle family and its native draw order, some CPU fields, stripe plugins, weather conditions and interactions. Cloud caps still need comparison with the original game.
- Full map coverage, all outfits, enemies and equipment. Player controls, combat and actor AI are outside the current renderer.


## How it was built

Graphics formulas and behavior were recovered from game resources, Wii U v208
analysis in Ghidra, and shaders investigated through Cemu. The Rust/WGSL
implementation uses Bevy infrastructure. Unverified behavior and approximations
carry `SI-…` annotations. Full parity with the original game has not been
established. Research notes live in `docs/research/`.

```text
Your Wii U dump → bake / botw-formats → assets/ → render (Bevy)
```

| Directory | Purpose |
|---|---|
| `crates/botw-formats` | Game format readers used during baking. |
| `crates/asset-format` | Prepared asset formats: RON, GLB, textures and tables. |
| `crates/bake` | World, model and animation clip conversion. |
| `crates/render` | World renderer and character animation previews. |
| `docs/research/` | Rendering and game format research. |

## Required ROM / game dump

The verified input is **The Legend of Zelda: Breath of the Wild for Wii U, update 1.5.0 (title version v208)**, as an **extracted, decrypted game dump**. The local verified configuration uses the European base game `WUP-P-ALZP`, update v208 and DLC v80. Use this combination to reproduce the verified setup. Other regions and versions have not been separately verified; the loader does not enforce a version number.

The base game and update are required. DLC is an additional layer; remove its configuration entry if you do not have it. Recorded local results used DLC; a separate full run without it is not claimed here.

Files such as `.wud`, `.wux`, `.wua`, `.nsp`, `.xci`, archives and encrypted packages are not read directly. The loader expects directories such as `Terrain`, `Model` and `Pack` inside `content/`. It recognizes a Switch `romfs/` directory name, but that **does not establish Switch format support**. Use Wii U data.

Keep your dump outside this repository. The game, keys, prepared assets are not included; use a dump of your own copy. An emulator is not required to run this renderer.

Example layout:

```text
/path/to/botw-wiiu/
├── base/
│   └── content/
│       ├── Terrain/A/MainField.tscb
│       ├── Model/
│       ├── Pack/
│       └── Actor/
├── update/
│   └── content/
│       ├── Model/
│       └── Pack/
└── dlc/                      # optional
    └── content/0010/
        └── Pack/
```

Each `game_dirs` entry can point to the dump directory (`base/`), `content/` itself, or `content/0010/` for DLC. Order matters: **base → update → DLC**. Later directories override earlier ones.

## Requirements

- Rust and Cargo. The current verification uses **Rust 1.98.1**, edition 2024. Dependencies are pinned in `Cargo.lock`, including Bevy 0.19.1. A minimum Rust version has not been established separately.
- A system C/C++ toolchain and linker. On macOS, install Xcode Command Line Tools (`xcode-select --install`).
- A GPU and drivers supported by Bevy/wgpu, plus a graphical session. The main verified platform is macOS / Apple GPU. Linux and Windows have not been separately verified. 
- Disk space for the dump, Rust build and prepared assets.

## Setup and first run

Run every command from the checkout root. Check the tools and fetch dependencies first:

```bash
rustc --version
cargo --version
cargo fetch --locked
```

Create a local `renderer.toml`:

```toml
game_dirs = [
    "/path/to/botw-wiiu/base",
    "/path/to/botw-wiiu/update",
    "/path/to/botw-wiiu/dlc", # remove this entry if you have no DLC
]
```

Prepare the assets and open the world:

```bash
cargo bake --region hateno
cargo render
```

Alternatively, pass the dump paths explicitly:

```bash
cargo bake --game /path/to/botw-wiiu/base \
  --game /path/to/botw-wiiu/update --region hateno --out assets
```

`cargo bake` and `cargo render` are release-build aliases in `.cargo/config.toml`. Build artifacts stay in the checkout-local `target/` directory. No neighboring project directory is needed. Run bake again after asset format or baker changes.

### Baking the whole main map

The default bake prepares detailed terrain and objects around Hateno. For
free-camera exploration across the entire main map, use a square large enough
to cover MainField:

```bash
cargo bake --region hateno --radius 12000
cargo render
```

`--radius` is the square's half-width in meters. A 12000 m half-width centered
on Hateno covers the entire main map. The command uses the dump paths in
`renderer.toml` and writes to `assets/` by default. It takes more time and disk
space than a regional bake; the renderer still streams nearby terrain and
objects instead of loading the entire world at once. Hateno remains the main
verified region for rendering fidelity.

To expand detailed object/model coverage in an existing bake without repeating
all steps:

```bash
cargo bake --region hateno --radius 12000 --only objects,models,effects,elink
```

This also updates the effects that depend on the object/model selection. It
does not expand detailed terrain coverage; use the full bake above for that.
Outside the area with detailed assets, the viewer keeps available far models
and tree billboards visible up close until their detailed replacements are
ready. Coarse bridges, buildings or rocks there require baking detailed assets;
increasing the viewer's LOD distances alone cannot supply missing models.

Camera controls: `WASD` moves, `Q/E` moves down/up, `Shift` speeds up, and the wheel changes speed. Hold the right mouse button to look around, or click the left button to capture the cursor. `Esc` releases it. Numbered viewpoints may lie outside the baked region.

The debug menu opens on startup. `F2` toggles it and `F1` toggles diagnostics. Change camera parameters, time, weather, climate, terrain/object/grass settings, post processing and character animation. The environment editor exposes all loaded palette and renderer fields, with search and reset. Experimental camera haze (`FieldEnvEffect`) is disabled by default. Use WASD/QE to fly, right mouse to look and Shift to boost.

Useful commands:

```bash
# Hold a specific time of day and weather.
cargo render --time 18:00 --weather rain

# Loop a Link animation; preview experimental villagers.
cargo render --character link --clip Nml_Move_Run
cargo render --villagers

# Save a view after the world loads, then exit.
mkdir -p captures
cargo render --time 10:00 --weather bluesky --screenshot captures/hateno.png

# Available options.
cargo bake --help
cargo render --help
```

## Verification and troubleshooting

```bash
cargo test --locked --workspace --exclude botw-formats
cargo fmt --check -p asset-format -p bake -p render
```

The default test run excludes integration tests that need local game assets.
Rebuild assets after asset format or baker changes. For missing resources,
check update v208, the layer order and that the dump directories contain
`Terrain`, `Model` and `Pack`.

## Publication and rights

This repository contains source code and research notes. `assets/`, `captures/`, `renderer.toml`, local worktrees, ROM containers and keys are excluded from Git. Do not add game data with `git add -f`.

This is an unofficial research project with no affiliation with Nintendo. The game name and game imagery belong to their respective rights holders. A license for this project's own code has not been selected. Dependency and third-party material licenses apply separately.
