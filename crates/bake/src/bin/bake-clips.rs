//! List/bake extra clips without replacing the character's outfit or skeleton.
#[path = "../clips.rs"]
mod clips;

use asset_format::{character::CharacterDef, paths};
use botw_formats::{actor::ActorPacks, bfres::Bfres, content::ContentRoots};
use std::{path::PathBuf, process::ExitCode, sync::Arc};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let usage = "bake-clips --game <dump> [--game <update>] --assets <baked> --character <name> [--list | <clip> ...]";
    let mut game = Vec::<PathBuf>::new();
    let mut assets = None::<PathBuf>;
    let mut character = None::<String>;
    let mut names = Vec::new();
    let mut list = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--game" => game.push(args.next().ok_or(usage)?.into()),
            "--assets" => assets = Some(args.next().ok_or(usage)?.into()),
            "--character" => character = Some(args.next().ok_or(usage)?),
            "--list" => list = true,
            "--help" | "-h" => {
                println!("{usage}");
                return Ok(());
            }
            _ if !arg.starts_with('-') => names.push(arg),
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    let assets = assets.ok_or(usage)?;
    let character = character.ok_or(usage)?;
    let safe = |name: &str| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    };
    if !safe(&character)
        || names.iter().any(|n| !safe(n))
        || (list && !names.is_empty())
        || (!list && names.is_empty())
    {
        return Err(usage.into());
    }
    let path = assets.join(paths::character(&character));
    let mut def: CharacterDef = asset_format::read_ron(&path)?;
    if !safe(&def.animations) {
        return Err("unsafe animation set".into());
    }
    let (roots, rejected) = ContentRoots::resolve(game);
    if roots.is_empty() || !rejected.is_empty() {
        return Err("invalid --game folders".into());
    }
    let title = roots
        .find("Pack/TitleBG.pack")
        .map(std::fs::read)
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots, title);
    let bytes = packs
        .file(&format!("Model/{}.sbfres", def.animations))?
        .ok_or("animation set not found")?;
    let bfres = Bfres::parse(&bytes)?;
    if list {
        for name in bfres.skeletal_anim_names() {
            let anim = bfres.skeletal_anim(name)?.ok_or("missing animation")?;
            println!(
                "{name}\t{}\t{}",
                anim.frame_count,
                if anim.looping { "loop" } else { "once" }
            );
        }
        return Ok(());
    }
    // Resolve every requested clip before touching the existing descriptor.
    let baked: Vec<_> = names
        .iter()
        .map(|name| {
            let anim = bfres
                .skeletal_anim(name)?
                .ok_or_else(|| format!("no animation {name}"))?;
            Ok::<_, Box<dyn std::error::Error>>((name, clips::sample_clip(&anim).to_glb()?))
        })
        .collect::<Result<_, _>>()?;
    for (name, glb) in baked {
        asset_format::write(
            &assets.join(paths::character_clip(&def.animations, name)),
            &glb,
        )?;
        if !def.clips.contains(name) {
            def.clips.push(name.clone());
        }
        println!("baked {name}");
    }
    asset_format::write_ron(&path, &def)?;
    Ok(())
}
