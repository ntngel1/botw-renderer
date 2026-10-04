//! Prints the time-of-day environment from `WorldMgr/normal.bwinfo`: the
//! palettes of a palette set and the blended light every hour.
//!
//! `cargo run -p botw-formats --example env_info -- <content dir>... [--set N]`

use botw_formats::content::ContentRoots;
use botw_formats::env::{DIVISIONS, EnvParams};

fn main() {
    let mut dirs = Vec::new();
    let mut set = 0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--set" => set = args.next().and_then(|s| s.parse().ok()).expect("--set N"),
            _ => dirs.push(arg),
        }
    }
    let (roots, _) = ContentRoots::resolve(&dirs);
    let params = EnvParams::load(&roots)
        .unwrap()
        .expect("no Pack/TitleBG.pack");
    println!(
        "{} palettes, {} palette sets, sun {:?}",
        params.palettes.len(),
        params.palette_sets.len(),
        params.sun
    );
    for (index, climate) in params.climates.iter().enumerate() {
        let name = botw_formats::eco::CLIMATES
            .get(index)
            .copied()
            .unwrap_or("?");
        println!(
            "climate {index} {name}: PaletteSetSelect {} (sets {:?}), FeatureColor {:?}",
            climate.palette_set,
            climate.palette_sets(),
            climate.influence.feature_color
        );
    }
    for (index, influence) in params.weather_influences.iter().enumerate() {
        println!("weather influence {index}: {influence:?}");
    }
    for division in 0..DIVISIONS {
        println!("division {division}: {:?}", params.palette(set, division));
    }
    for hour in 0..24 {
        let p = params.at(set, hour as f32);
        println!(
            "{hour:02}:00 light {:.2?} x{:.2}  sky sun {:.2?} x{:.1}  cloud shadows {}",
            p.light_color,
            p.light_intensity,
            p.sky_sun_color,
            p.sky_sun_intensity,
            p.cloud_shadow_on
        );
    }
}
