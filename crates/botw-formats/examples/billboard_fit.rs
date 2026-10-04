//! Matches the far-tree billboards in `Terrain.Tex1` (`Tree0Alb`,
//! `Tree1Alb`, one layer per actor and view angle, named `<actor>_R<deg>`)
//! against the actor's model: for each layer, finds the view direction
//! whose silhouette fits the layer's alpha best, and how the image frames
//! the model (metres per pixel, where the model's origin lands).
//!
//! `cargo run -p botw-formats --example billboard_fit -- <Terrain.Tex1.sbfres> <texture> <actor> <model.sbfres> [model name]`

use botw_formats::bfres::{Bfres, assemble_texture};

const GRID: usize = 64;

fn main() {
    let mut args = std::env::args().skip(1);
    let tex1 = args.next().expect("Terrain.Tex1");
    let texture = args.next().expect("texture");
    let actor = args.next().expect("actor");
    let model_file = args.next().expect("model file");
    let model_name = args.next().unwrap_or_else(|| actor.clone());
    let read = |p: &str| botw_formats::yaz0::decompress_if(&std::fs::read(p).unwrap()).unwrap().into_owned();

    let tex_bytes = read(&tex1);
    let bfres = Bfres::parse(&tex_bytes).unwrap();
    let raw = bfres.texture(&texture).unwrap().expect("texture");
    let files: Vec<String> =
        raw.user_data.iter().find(|(k, _)| k == "file").map(|(_, v)| v.split(',').map(str::to_owned).collect()).unwrap_or_default();
    let image = assemble_texture(&raw, None).unwrap();
    let (w, h) = (image.width as usize, image.height as usize);

    let model_bytes = read(&model_file);
    let model = Bfres::parse(&model_bytes).unwrap().models().unwrap().into_iter().find(|m| m.name == model_name).expect("model");
    let mut triangles: Vec<[[f32; 3]; 3]> = Vec::new();
    for shape in &model.shapes {
        let Some(buffer) = model.vertex_buffers.get(shape.vertex_buffer as usize) else { continue };
        let Some(positions) = buffer.attribute("_p0") else { continue };
        let Some(lod) = shape.lods.first() else { continue };
        for t in lod.indices.as_chunks::<3>().0 {
            let p = |i: u32| {
                let v = positions.values[i as usize];
                [v[0], v[1], v[2]]
            };
            triangles.push([p(t[0]), p(t[1]), p(t[2])]);
        }
    }
    println!("{} triangles", triangles.len());

    for (layer, file) in files.iter().enumerate() {
        let Some(angle) = file.strip_prefix(&format!("{actor}_R")).and_then(|r| r.parse::<f32>().ok()) else { continue };
        let rgba = image.decode_layer_rgba8(0, layer as u32).unwrap();
        let alpha: Vec<bool> = rgba.chunks(4).map(|p| p[3] > 40).collect();
        let (mut x0, mut x1, mut y0, mut y1) = (w, 0, h, 0);
        for y in 0..h {
            for x in 0..w {
                if alpha[y * w + x] {
                    (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
                }
            }
        }
        let mask = resample(&alpha, w, (x0, x1 + 1, y0, y1 + 1));
        let mut best = (0.0f32, 0.0f32, false, [0.0f32; 4]);
        for step in 0..120 {
            let phi = step as f32 * 3.0;
            for mirror in [false, true] {
                let (silhouette, extents) = silhouette(&triangles, phi.to_radians(), mirror);
                let iou = iou(&silhouette, &mask);
                if iou > best.0 {
                    best = (iou, phi, mirror, extents);
                }
            }
        }
        let [u0, u1, v0, v1] = best.3;
        let mpp_x = (u1 - u0) / (x1 + 1 - x0) as f32;
        let mpp_y = (v1 - v0) / (y1 + 1 - y0) as f32;
        // Pixel column of u = 0 and row of y = 0.
        let origin_x = x0 as f32 - u0 / mpp_x;
        let origin_y = (y1 + 1) as f32 + v0 / mpp_y;
        println!(
            "{file}: R {angle:5.1} best view {:5.1} mirror {} IoU {:.2} | px x {x0}..{x1} y {y0}..{y1} | model u {u0:.2}..{u1:.2} y {v0:.2}..{v1:.2} | m/px {mpp_x:.3} {mpp_y:.3} origin px ({origin_x:.1}, {origin_y:.1}) image {:.2} x {:.2} m",
            best.1, best.2, best.0, mpp_x * w as f32, mpp_y * h as f32
        );
    }
}

/// Crops a mask to `(x0, x1, y0, y1)` and resamples it to `GRID × GRID`.
fn resample(mask: &[bool], w: usize, (x0, x1, y0, y1): (usize, usize, usize, usize)) -> Vec<bool> {
    let mut out = vec![false; GRID * GRID];
    for gy in 0..GRID {
        for gx in 0..GRID {
            let x = x0 + (gx * (x1 - x0)) / GRID;
            let y = y0 + (gy * (y1 - y0)) / GRID;
            out[gy * GRID + gx] = mask[y * w + x];
        }
    }
    out
}

/// The model's orthographic silhouette seen from direction `phi` (camera
/// at (sin φ, 0, cos φ), looking at the origin), fitted to its bounding
/// box on a `GRID × GRID` raster (row 0 at the top), and the box (u0, u1,
/// y0, y1) in metres. Screen right is (cos φ, 0, −sin φ).
fn silhouette(triangles: &[[[f32; 3]; 3]], phi: f32, mirror: bool) -> (Vec<bool>, [f32; 4]) {
    let (sin, cos) = phi.sin_cos();
    let project = |p: [f32; 3]| {
        let u = p[0] * cos - p[2] * sin;
        (if mirror { -u } else { u }, p[1])
    };
    let projected: Vec<[(f32, f32); 3]> = triangles.iter().map(|t| t.map(project)).collect();
    let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for t in &projected {
        for &(u, v) in t {
            (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
        }
    }
    let mut raster = vec![false; GRID * GRID];
    let to_px = |(u, v): (f32, f32)| ((u - u0) / (u1 - u0) * GRID as f32, (v1 - v) / (v1 - v0) * GRID as f32);
    for t in &projected {
        let [a, b, c] = t.map(to_px);
        let (minx, maxx) = (a.0.min(b.0).min(c.0).floor().max(0.0) as usize, a.0.max(b.0).max(c.0).ceil().min(GRID as f32) as usize);
        let (miny, maxy) = (a.1.min(b.1).min(c.1).floor().max(0.0) as usize, a.1.max(b.1).max(c.1).ceil().min(GRID as f32) as usize);
        let edge = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0);
        let area = edge(a, b, c);
        if area.abs() < 1e-9 {
            continue;
        }
        for y in miny..maxy {
            for x in minx..maxx {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let (w0, w1, w2) = (edge(b, c, p) / area, edge(c, a, p) / area, edge(a, b, p) / area);
                if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
                    raster[y * GRID + x] = true;
                }
            }
        }
    }
    (raster, [u0, u1, v0, v1])
}

fn iou(a: &[bool], b: &[bool]) -> f32 {
    let both = a.iter().zip(b).filter(|(x, y)| **x && **y).count();
    let either = a.iter().zip(b).filter(|(x, y)| **x || **y).count();
    both as f32 / either.max(1) as f32
}
