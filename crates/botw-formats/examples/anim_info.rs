//! Lists a BFRES file's skeletal animations, or prints one: its bones, their
//! curves and the pose at a few frames.
//!
//! `cargo run -p botw-formats --example anim_info -- <file.sbfres> [animation] [filter] [every N frames]`

use botw_formats::bfres::Bfres;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path");
    let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
    let bfres = Bfres::parse(&bytes).unwrap();
    let Some(name) = args.next() else {
        let names: Vec<&str> = bfres.skeletal_anim_names().collect();
        println!("{} animations: {}", names.len(), names.join(" "));
        return;
    };
    let filter = args.next().unwrap_or_default();
    let step: Option<usize> = args.next().and_then(|s| s.parse().ok());
    let anim = bfres.skeletal_anim(&name).unwrap().expect("no such animation");
    println!("{}: {} frames, looping {}, euler {}, {} bones", anim.name, anim.frame_count, anim.looping, anim.euler, anim.bones.len());
    for bone in anim.bones.iter().filter(|b| b.name.contains(&filter)) {
        let curves: Vec<String> = bone.curves.iter().map(|c| format!("{:?} {:?} {} keys", c.target, c.kind, c.frames.len())).collect();
        println!("  {} flags {:#010x} [{}]", bone.name, bone.flags, curves.join(", "));
        let frames: Vec<f32> = match step {
            Some(step) => (0..=anim.frame_count as usize).step_by(step.max(1)).map(|f| f as f32).collect(),
            None => vec![0.0, anim.frame_count / 2.0, anim.frame_count],
        };
        for frame in frames {
            let pose = bone.sample(frame);
            let round = |v: &[f32]| v.iter().map(|x| format!("{x:.3}")).collect::<Vec<_>>().join(" ");
            println!(
                "    f{frame:5.1}: s {:?} r {:?} t {:?}",
                pose.scale.map(|v| round(&v)),
                pose.rotation.map(|v| round(&v)),
                pose.translation.map(|v| round(&v))
            );
        }
    }
}
