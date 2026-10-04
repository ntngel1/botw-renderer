//! Prints the renderer's base environment from `Pack/Bootup.pack` →
//! `Env/env.sgenvb`: lights and fogs, colour correction, bloom, the sky's
//! scattering and its clouds as drawn, the light maps (curves sampled over
//! 0–1, rim light),
//! shadows and the projected cloud shadow (projector, texture levels), the
//! cloud textures by number; plus `EnvPaletteStatic` and the layers' cloud
//! texture numbers from `normal.bwinfo`.
//!
//! `cargo run -p botw-formats --example envset_info -- <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::env::EnvParams;
use botw_formats::envset::EnvSet;

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let set = EnvSet::load(&roots)
        .unwrap()
        .expect("no Env/env.sgenvb in Pack/Bootup.pack");
    println!("read: {:?}", set.read);
    println!("main light: {:?}", set.objects.main_light);
    println!("hemisphere: {:?}", set.objects.hemisphere);
    for fog in &set.objects.fogs {
        println!("fog: {fog:?}");
    }
    let c = &set.color;
    println!(
        "colour correction: enable {} hue {} saturation {} brightness {} gamma {}, {} level curves, toy camera {}",
        c.enable,
        c.hue,
        c.saturation,
        c.brightness,
        c.gamma,
        c.levels.len(),
        c.toycam_enable
    );
    println!("bloom: {:?}", set.bloom);
    println!("sky: {:?}", set.sky);
    println!("clouds: {:?}", set.clouds);
    for (name, curve) in &set.light_maps.curves {
        let samples: Vec<String> = curve.bake(11).iter().map(|v| format!("{v:.2}")).collect();
        println!("curve {name:16} {:?} [{}]", curve.kind, samples.join(" "));
    }
    for map in &set.light_maps.maps {
        let main = map.main_light().map(|i| {
            format!(
                "{} calc {} effect {} pow {}",
                i.lut, i.calc_type, i.effect, i.power
            )
        });
        println!(
            "light map {:20} rim {:?} | main light {}",
            map.name,
            map.rim,
            main.unwrap_or_default()
        );
    }
    println!("shadows: {:?}", set.shadows);
    for projector in &set.objects.projectors {
        println!("projector: {projector:?}");
    }
    if let Some(reference) = &set.cloud_shadow_texture {
        match EnvSet::load_texture(&roots, reference) {
            Ok(Some(t)) => {
                let level0 = t.level_bytes(0);
                println!(
                    "cloud shadow texture {reference:?}: {:?} {}x{}, {} levels, {} bytes ({} at level 0)",
                    t.format,
                    t.width,
                    t.height,
                    t.mip_levels,
                    t.data.len(),
                    level0
                );
            }
            Ok(None) => println!("cloud shadow texture {reference:?}: not in the set"),
            Err(error) => println!("cloud shadow texture {reference:?}: {error}"),
        }
    }
    let textures = EnvSet::load_cloud_textures(&roots).unwrap_or_else(|error| {
        println!("cloud textures: {error}");
        Vec::new()
    });
    for (number, t) in textures.iter().enumerate() {
        println!(
            "cloud texture {number}: {} {:?} {}x{}, {} levels, channels {:?}",
            t.name, t.format, t.width, t.height, t.mip_levels, t.component_select
        );
    }
    if let Ok(Some(params)) = EnvParams::load(&roots) {
        println!("EnvPaletteStatic: {:?}", params.palette_static);
        let layers = params.clouds.as_ref().map_or(&[][..], |c| &c.layers[..]);
        for (layer, sky) in layers.iter().enumerate() {
            let numbers = &sky.textures;
            let name = |n: i32| {
                let index = EnvSet::cloud_texture_index(n, textures.len());
                textures.get(index).map_or("-", |t| t.name.as_str())
            };
            println!(
                "layer {} textures {numbers:?}: base {} / {}, noise {} / {}",
                2 * layer,
                name(numbers.base),
                name(numbers.base_blend),
                name(numbers.noise),
                name(numbers.noise_blend)
            );
        }
    }
}
