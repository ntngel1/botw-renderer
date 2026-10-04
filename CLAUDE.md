# botw рендерер: agent instructions

BotW world renderer: a free camera, weather, effects and character animation
previews from baked assets. Setup and rendering status: [README.md](README.md).

## Rules
- Reply in Russian; repository documentation, code comments and commit messages in English.
- Visuals strictly by the game: preserve recovered shading math for field,
  characters, fog, clouds, water, grass, post processing, sky and cube maps.
  No fits or "looks closer" tweaks. A temporary stand-in gets an SI-ID,
  a `// SI-…` mark in code and a note in `docs/research/`.
- No port of Link/enemy logic; animation uses the game's clips and skeletons.
- `render` reads only `assets/`; only `bake` reads the game dump
  (`botw-formats`). Formats of `assets/` live in `asset-format`.
- Never commit game data or anything converted from it (`assets/`, captures
  and `renderer.toml` are git-ignored).
- Commit small working steps; pushing needs the user's permission.

## Commands (release by default)
- `cargo bake [--region hateno] [--only places,light,sky,terrain,terrain-textures,water,objects,models]`
  — dump paths from `renderer.toml` (`game_dirs`), output `assets/`.
- `cargo render [--place <name>] [--camera x,y,z,yaw,pitch] [--time HH:MM]
  [--weather <name>] [--screenshot captures/x.png]`.
- `cargo test --workspace --exclude botw-formats`.
- `cargo fmt -p asset-format -p bake -p render`.
- Use the checkout-local `target/` directory; do not share it between worktrees.

## Layout
- `crates/botw-formats` — game format parser (bake only).
- `crates/asset-format` — layout and formats of `assets/`.
- `crates/bake` — dump → `assets/`.
- `crates/render` — Bevy world renderer and interactive viewer.
