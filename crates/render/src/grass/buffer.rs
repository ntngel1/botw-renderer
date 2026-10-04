//! The game's blade buffer (`0x039255d4`, docs/research/wiiu-field-shading.md,
//! "Grass blade: VS `7f39f0470fc4928a` and vertex buffer"): one 3 m cell of `N²`
//! blades, built once from `sead::Random` seeded `0x1234567` and drawn in
//! every cell alike. Per blade: its place in the cell (a 4×4 grid by its
//! number plus 6 random bits), its lean (`Sem1`), two random bytes (`Sem8`:
//! wind phase, length); per vertex its `(u, row)` from the primitive table.

/// `sead::Random` (`0x030c48dc` seeds it, `0x030c499c` draws): xorshift128.
pub(crate) struct SeadRandom([u32; 4]);

impl SeadRandom {
    pub(crate) fn new(seed: u32) -> Self {
        let step = |previous: u32, n: u32| {
            (previous ^ (previous >> 30))
                .wrapping_mul(0x6C07_8965)
                .wrapping_add(n)
        };
        let x = step(seed, 1);
        let y = step(x, 2);
        let z = step(y, 3);
        Self([x, y, z, step(z, 4)])
    }

    fn next(&mut self) -> u32 {
        let [x, y, z, w] = self.0;
        let t = x ^ (x << 11);
        let next = w ^ (w >> 19) ^ t ^ (t >> 8);
        self.0 = [y, z, w, next];
        next
    }

    /// `sead::Random::getU32(ceil)`: `[0, ceil)` from the top of the next
    /// value.
    pub(crate) fn below(&mut self, ceil: u32) -> u32 {
        ((u64::from(self.next()) * u64::from(ceil)) >> 32) as u32
    }

    /// `[0, 1)` the game's way: the top 23 bits as the mantissa of `[1, 2)`, − 1.
    pub(crate) fn unit(&mut self) -> f32 {
        f32::from_bits((self.next() >> 9) | 0x3F80_0000) - 1.0
    }
}

/// One blade of the buffer, as its vertex bytes hold it.
#[derive(Clone, Copy, Debug)]
pub struct Blade {
    /// `Sem0` (unorm8): x and z in the cell, 0–255 over its edge.
    pub place: [u8; 2],
    /// `Sem1` (snorm8): x and z of `normalize(x, 1, z)`, `x, z ∈ [−0.5, 0.5)`,
    /// × 128 and truncated.
    pub lean: [i8; 2],
    /// `Sem8` (unorm8): the wind phase and the length factor's random.
    pub random: [u8; 2],
}

impl Blade {
    /// The lean as the vertex shader reads it (snorm8: / 127).
    pub fn lean(&self) -> [f32; 2] {
        self.lean.map(|b| (b as f32 / 127.0).max(-1.0))
    }
}

/// The cell's blades in the order the generator made them.
pub fn cell(blades_per_side: u32) -> Vec<Blade> {
    let count = blades_per_side * blades_per_side;
    let mut random = SeadRandom::new(0x0123_4567);
    let byte = |unit: f32| (unit * 255.0) as u8;
    (0..count)
        .map(|i| {
            // A 4×4 grid by the blade's number, 6 random bits within.
            let x = ((i & 3) << 6) as u8 | (random.next() >> 26) as u8;
            let z = (((i >> 2) & 3) << 6) as u8 | (random.next() >> 26) as u8;
            let (lx, lz) = (random.unit() - 0.5, random.unit() - 0.5);
            // (The game refines `frsqrte` once; exact here.)
            let scale = 128.0 / (lx * lx + 1.0 + lz * lz).sqrt();
            let lean = [(lx * scale) as i8, (lz * scale) as i8];
            let random = [byte(random.unit()), byte(random.unit())];
            Blade {
                place: [x, z],
                lean,
                random,
            }
        })
        .collect()
}

/// `(u, row)` of each vertex (`0x10339624`, the primitive of the type):
/// type 0 a narrow triangle, type 1 a diamond.
pub const VERTICES: [&[(u8, u8)]; 2] =
    [&[(2, 0), (0, 2), (1, 3)], &[(1, 0), (0, 2), (2, 2), (1, 3)]];

/// A blade's triangles, as the index writer turns them (every odd one
/// flipped to keep the winding).
pub const TRIANGLES: [&[[u32; 3]]; 2] = [&[[0, 1, 2]], &[[0, 1, 2], [2, 1, 3]]];

/// Where the drawing order that thins a cell (order 8) puts each blade: the
/// writer inserts every blade at its front, so the last made comes first.
pub fn thinning_rank(index: usize, count: usize) -> usize {
    count - 1 - index
}

/// Edge of the tile the tuft buffer fills, metres: its tufts sit in `[0, 1)`
/// of it, and the draw (`0x03705700`) splits it into 5 m quarters, sorted
/// by the generator's second order at 0.5 (inferred: the writer of the
/// tile's `gsys_shape_ex[0]` is not followed).
// SI-GRS-01: 10 m tiles on a world grid are inferred; quarter culling left out.
pub const TUFT_TILE: f32 = 10.0;

/// Per tuft type (`0x1030ab78`: primitive, tufts per side, card width in
/// tiles): 9² hexagonal cards 2 m wide (type 0), 12² triangles 4 m wide
/// (type 1) in every tile.
pub const TUFT_TYPES: [(u32, f32); 2] = [(9, 0.2), (12, 0.4)];

/// `(u, v)` of each vertex of the type's primitive (`0x1047bfd8`, 12 floats
/// a primitive: six u, six v): type 0 (primitive 4) a hexagon around the
/// tuft in `GrassCrossAlb`, type 1 (primitive 5) a triangle over its middle,
/// its base at v = 0.8.
pub const TUFT_VERTICES: [&[(f32, f32)]; 2] = [
    &[
        (0.0, 0.48),
        (0.2, 0.11),
        (0.55, 0.08),
        (0.96, 0.57),
        (0.87, 1.0),
        (0.06, 1.0),
    ],
    &[(0.25, 0.8), (0.5, 0.15), (0.75, 0.8)],
];

/// A tuft's triangles, as the index writer (`0x03709774`) lays them.
pub const TUFT_TRIANGLES: [&[[u32; 3]]; 2] =
    [&[[0, 1, 5], [5, 1, 2], [5, 2, 4], [4, 2, 3]], &[[0, 1, 2]]];

/// One vertex of the tuft buffer, as its bytes hold it.
#[derive(Clone, Copy, Debug)]
pub struct TuftVertex {
    /// `Sem0` (snorm8): x and z in the tile, the vertex shader's `+ 0.5`
    /// makes `[0, 1)` of the root.
    pub place: [i8; 2],
    /// `Sem1` (unorm8): the texture's u and v.
    pub uv: [u8; 2],
}

impl TuftVertex {
    /// Where the vertex stands in the tile, `[0, 1)` for a root (snorm8:
    /// / 127, + 0.5).
    pub fn place(&self) -> [f32; 2] {
        self.place.map(|b| (b as f32 / 127.0).max(-1.0) + 0.5)
    }

    pub fn uv(&self) -> [f32; 2] {
        self.uv.map(|b| b as f32 / 255.0)
    }
}

/// The tile's tufts of both types, vertices in the generator's order
/// (`0x03709774`, `sead::Random` seeded `0x12345678` once for both): per
/// tuft of type `t`, row by row, a root jittered in its square of the
/// `N × N` grid and a random turn; every vertex lies on the flat card
/// through the root, `width/2·(cos, sin)·(1 − 2u)` aside.
pub fn tuft_tile() -> [Vec<Vec<TuftVertex>>; 2] {
    let mut random = SeadRandom::new(0x1234_5678);
    let mut types = [Vec::new(), Vec::new()];
    for (kind, &(side, width)) in TUFT_TYPES.iter().enumerate() {
        let half = f64::from(width) * 0.5;
        for row in 0..side {
            for column in 0..side {
                let jitter_x = random.unit();
                let jitter_z = random.unit();
                let x = ((column as f32 + jitter_x) / side as f32 - 0.5) as f64;
                let z = ((row as f32 + jitter_z) / side as f32 - 0.5) as f64;
                // (The game's `cosf`/`sinf` are its own polynomials.)
                let turn = random.unit() * std::f32::consts::TAU;
                let (sin, cos) = (f64::from(turn.sin()), f64::from(turn.cos()));
                let byte = |value: f64| (value * 0.5 * 255.0) as i32 as i8;
                let unit = |value: f32| (f64::from(value) * 255.0) as u8;
                let tuft = TUFT_VERTICES[kind]
                    .iter()
                    .map(|&(u, v)| {
                        let (u64, keep) = (f64::from(u), 1.0 - f64::from(u));
                        let px = (x - half * cos) * u64 + (x + half * cos) * keep;
                        let pz = (z - half * sin) * u64 + (z + half * sin) * keep;
                        TuftVertex {
                            place: [byte(px), byte(pz)],
                            uv: [unit(u), unit(v)],
                        }
                    })
                    .collect();
                types[kind].push(tuft);
            }
        }
    }
    types
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tufts_fill_the_tile_evenly() {
        let [first, second] = tuft_tile();
        assert_eq!((first.len(), second.len()), (81, 144));
        // The middle of type 0's base is within 7 cm of its root: roots
        // fall one to a ninth of the tile each way (bar the rounding).
        let mut squares = std::collections::HashSet::new();
        for tuft in &first {
            let [x, z] = [0, 1].map(|i| (tuft[4].place()[i] + tuft[5].place()[i]) / 2.0);
            assert!((-0.1..1.1).contains(&x) && (-0.1..1.1).contains(&z));
            squares.insert(((x * 9.0).floor() as i32, (z * 9.0).floor() as i32));
        }
        assert!(squares.len() > 60, "{}", squares.len());
    }

    #[test]
    fn tuft_cards_are_as_wide_as_their_type() {
        let [first, second] = tuft_tile();
        let span = |a: TuftVertex, b: TuftVertex| {
            let (a, b) = (a.place(), b.place());
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() * TUFT_TILE
        };
        // Type 0's base runs u = 0.06 to 0.87 of a 2 m card, type 1's
        // u = 0.25 to 0.75 of a 4 m card; the bytes truncate each end by up
        // to 1/127 of the tile (7.9 cm).
        for tuft in &first {
            assert!((span(tuft[4], tuft[5]) - 1.62).abs() < 0.17);
        }
        for tuft in &second {
            assert!((span(tuft[0], tuft[2]) - 2.0).abs() < 0.17);
        }
    }

    #[test]
    fn blades_spread_over_the_cells_grid_in_turn() {
        let blades = cell(30);
        assert_eq!(blades.len(), 900);
        // Any 16 blades in a row cover every square of the 4×4 grid once.
        let squares: std::collections::HashSet<_> = blades[100..116]
            .iter()
            .map(|b| (b.place[0] >> 6, b.place[1] >> 6))
            .collect();
        assert_eq!(squares.len(), 16);
    }

    #[test]
    fn leans_are_short_and_all_ways() {
        let blades = cell(16);
        let lengths: Vec<f32> = blades
            .iter()
            .map(|b| b.lean())
            .map(|[x, z]| (x * x + z * z).sqrt())
            .collect();
        assert!(lengths.iter().all(|&l| l < 0.6));
        let mean = lengths.iter().sum::<f32>() / lengths.len() as f32;
        assert!((0.3..0.45).contains(&mean), "{mean}");
        let mean_x = blades.iter().map(|b| b.lean()[0]).sum::<f32>() / blades.len() as f32;
        assert!(mean_x.abs() < 0.05, "{mean_x}");
    }

    #[test]
    fn the_same_cell_every_time() {
        let (a, b) = (cell(16), cell(16));
        assert!(
            a.iter()
                .zip(&b)
                .all(|(a, b)| a.place == b.place && a.lean == b.lean && a.random == b.random)
        );
    }
}
