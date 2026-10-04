//! `bake`: converts the user's own game dump into `assets/` once. The only
//! program that reads game files; the renderer reads what it writes.
//!
//! ```text
//! cargo bake [--game <dir>]... [--region <place>] [--radius <m>] [--out <dir>] [--only <steps>]
//! ```

mod characters;
mod clips;
mod effects;
mod eft;
mod elink;
mod emission;
mod grass;
mod material_anims;
mod models;
mod npcs;
mod objects;
mod places;
mod sky;
mod terrain;
mod texture_srt;
mod trees;
mod umii;
mod water;
mod xlu;

use std::path::PathBuf;
use std::process::ExitCode;

use botw_formats::content::ContentRoots;

const USAGE: &str = "\
usage: bake [options]
  --game <dir>     a dump folder (base, update, DLC: lowest priority first);
                   repeatable, default: game_dirs in renderer.toml
  --region <name>  the place to bake around (a location marker), default hateno
  --radius <m>     half the side of the region's square, default 1000
  --out <dir>      default assets/ in the workspace
  --only <steps>   comma-separated subset of: places, light, sky, terrain,
                   terrain-textures, water, grass, objects, models, trees,
                   effects, elink, characters, npcs (models and effects
                   read what objects wrote, elink what effects wrote)
";

const STEPS: [&str; 14] = [
    "places",
    "light",
    "sky",
    "terrain",
    "terrain-textures",
    "water",
    "grass",
    "objects",
    "models",
    "trees",
    "effects",
    "elink",
    "characters",
    "npcs",
];

struct Args {
    game: Vec<PathBuf>,
    region: String,
    radius: f32,
    out: PathBuf,
    only: Vec<String>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut args = Args {
        game: Vec::new(),
        region: "hateno".into(),
        radius: 1000.0,
        out: workspace.join("assets"),
        only: STEPS.iter().map(|s| s.to_string()).collect(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--game" => args.game.push(value()?.into()),
            "--region" => args.region = value()?,
            "--radius" => args.radius = value()?.parse().map_err(|_| "--radius: not a number")?,
            "--out" => args.out = value()?.into(),
            "--only" => {
                args.only = value()?.split(',').map(str::to_owned).collect();
                if let Some(bad) = args.only.iter().find(|s| !STEPS.contains(&s.as_str())) {
                    return Err(format!("--only: unknown step {bad}"));
                }
            }
            "-h" | "--help" => return Ok(None),
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    if args.game.is_empty() {
        args.game = config_game_dirs(&workspace.join("renderer.toml"))?;
    }
    Ok(Some(args))
}

/// `game_dirs` from the per-machine config.
fn config_game_dirs(path: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    #[derive(serde::Deserialize)]
    struct Config {
        game_dirs: Vec<PathBuf>,
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("no --game and {}: {e}", path.display()))?;
    let config: Config = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(config.game_dirs)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), String> {
    let (roots, rejected) = ContentRoots::resolve(&args.game);
    for path in rejected {
        eprintln!(
            "warning: {} does not look like BotW content",
            path.display()
        );
    }
    if roots.is_empty() {
        return Err("no game content found".into());
    }
    let step = |name: &str| args.only.iter().any(|s| s == name);
    let started = std::time::Instant::now();

    // The region is always found from the markers, even when places.ron is
    // not rewritten.
    let places = places::read(&roots)?;
    let place = places
        .find(&args.region)
        .ok_or_else(|| format!("no location marker named {}", args.region))?;
    println!(
        "region {} (marker {}) at {:?}, radius {} m",
        place.name, place.marker, place.position, args.radius
    );
    if step("places") {
        asset_format::write_ron(&args.out.join(asset_format::paths::PLACES), &places)
            .map_err(|e| e.to_string())?;
        println!("places: {} markers", places.places.len());
    }

    if step("light") {
        bake_light(&roots, &args.out)?;
    }

    if step("sky") {
        sky::bake(&roots, &args.out)?;
    }

    let tscb = terrain::read_tscb(&roots)?;
    if step("terrain") {
        let region = terrain::Region {
            center: [place.position[0], place.position[2]],
            radius: args.radius,
        };
        terrain::bake_tiles(&roots, &tscb, &place.name, region, &args.out)?;
    }
    if step("terrain-textures") {
        terrain::bake_textures(&roots, &tscb, &args.out)?;
    }
    if step("water") {
        water::bake(&roots, &args.out)?;
    }
    if step("grass") {
        grass::bake(&roots, &args.out)?;
    }
    if step("objects") {
        let region = objects::Region {
            center: [place.position[0], place.position[2]],
            radius: args.radius,
        };
        objects::bake(&roots, &region, &args.out)?;
    }
    if step("models") {
        models::bake(&roots, &args.out)?;
    }
    if step("trees") {
        trees::bake(&roots, &args.out)?;
    }
    if step("effects") {
        effects::bake(&roots, &args.out)?;
    }
    if step("elink") {
        elink::bake(&roots, &args.out)?;
    }
    if step("characters") {
        characters::bake(&roots, &args.out)?;
    }
    if step("npcs") {
        let region = objects::Region {
            center: [place.position[0], place.position[2]],
            radius: args.radius,
        };
        npcs::bake(&roots, &region, &args.out)?;
    }
    println!("done in {:.1} s", started.elapsed().as_secs_f32());
    Ok(())
}

/// The ambient occlusion's rotations (`SystemModel.Tex2` → `ssao`).
fn bake_light(roots: &ContentRoots, out: &std::path::Path) -> Result<(), String> {
    let noise = botw_formats::system_model::load_ssao_noise(roots)
        .map_err(|e| format!("ssao noise: {e}"))?
        .ok_or("ssao noise: no Pack/Bootup_Graphics.pack")?;
    let image = asset_format::light::RgImage {
        width: noise.width,
        height: noise.height,
        texels: noise.texels,
    };
    asset_format::write_ron(&out.join(asset_format::paths::SSAO_NOISE), &image)
        .map_err(|e| e.to_string())?;
    println!("light: ssao noise {}x{}", image.width, image.height);
    Ok(())
}
