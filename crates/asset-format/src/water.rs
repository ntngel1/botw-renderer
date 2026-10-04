//! The water's shading data: the game's water table (`WaterAlb`, a mirror
//! of `botw-formats::terrain::textures::WaterTable`) and the `TeraWater`
//! material's parameters, resolved by `bake` from `Pack/TitleBG.pack` →
//! `Model/Terrain.sbfres`. The water's maps (`WaterNrm`, `WaterEmm`) are
//! KTX2 arrays next to them (see the crate's layout).

/// Texels per water kind in `WaterAlb`.
pub const WATER_TABLE_TEXELS: usize = 7;
/// Water kinds (layers) in `WaterAlb`, `WaterNrm` and `WaterEmm`.
pub const WATER_KINDS: usize = 8;

/// `WaterAlb`: a 7×1 RGBA16F texture array, one layer per water kind (the
/// kinds of `.water.extm`). On Wii U the game reads it back on the CPU and
/// hands the RGB of each kind's seven texels to the water shader
/// (`uking_terrain_water`) as uniforms; what each texel does there is in
/// `docs/research/wiiu-water-variants.md` (e.g. texel 1 the deep colour,
/// texels 2 and 3 the opacity's rate and offset per metre of water, texel
/// 6 the vertex waves).
///
/// Values are linear and as stored.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct WaterTable {
    pub kinds: [[[f32; 4]; WATER_TABLE_TEXELS]; WATER_KINDS],
}

/// `TeraWater`'s material parameters that the water shader reads (the
/// game's G-buffer program, its offsets matched to the FMAT names in
/// docs/research/wiiu-water-variants.md).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct TeraWater {
    /// `tex_srt0–5`: mode, scale x/y, rotation, translation x/y.
    pub tex_srt: [[f32; 6]; 6],
    pub indirect_scale2: [f32; 2],
    pub indirect_scale4: [f32; 2],
    pub const_color2: [f32; 4],
    pub const_color3: [f32; 4],
    pub const_color5: [f32; 4],
    pub const_vector0: [f32; 4],
    pub const_vector1: [f32; 4],
    pub const_value2: f32,
    pub const_value3: f32,
    pub const_value6: f32,
}

impl TeraWater {
    /// Only without the baked material: the values of the Wii U v208 FMAT
    /// (`Pack/TitleBG.pack` → `Model/Terrain.sbfres`).
    pub const STAND_IN: Self = Self {
        tex_srt: [
            [0.0, 3.0, 3.0, 0.0, 0.0, 0.0],
            [0.0, 5.0, 5.0, 0.314_159_27, 0.0, 0.0],
            [0.0, 4.0, 4.0, 0.0, 0.0, 0.0],
            [0.0, 0.0125, 0.0125, 0.0, 0.0, 0.0],
            [0.0, 0.2, 0.2, 0.0, 0.0, 0.0],
            [0.0, 0.4, 0.4, 0.523_598_8, 0.0, 0.0],
        ],
        indirect_scale2: [0.15, 0.15],
        indirect_scale4: [0.25, 0.25],
        const_color2: [0.0, 0.0, 1.0, 0.0],
        const_color3: [0.08, 0.65, 0.9, 6.0],
        const_color5: [0.3, 1.07, 0.86, 0.0],
        const_vector0: [0.5, 0.5, 0.5, 0.5],
        const_vector1: [60.0, 0.0, 0.0, 0.0],
        const_value2: 1.75,
        const_value3: 0.2,
        const_value6: -4.5,
    };

    // SI-WAT-04: texture SRT as Maya rotation without pivot.
    /// The shader's six texture spaces as 2×3 matrices (two vectors each,
    /// column-major): the base one from world x/z (`tex_srt3`), then from
    /// it the secondary normal map's (`tex_srt0`), primary normal map's
    /// (`tex_srt1`), foam's (`tex_srt2`), third normal map's (`tex_srt4`)
    /// and the foam distortion's (`tex_srt5`). The chain is read from the
    /// water vertex shader (Cemu GLSL matching program 9); how the CPU
    /// turns an SRT into a matrix is not traced: this uses the usual
    /// Maya-mode rotation and drops the pivot, a fixed shift (archived GAPS.md notes,
    /// RENDER-002).
    pub fn texture_spaces(&self) -> [[f32; 4]; 12] {
        let order = [3, 0, 1, 2, 4, 5];
        let mut out = [[0.0; 4]; 12];
        for (slot, &i) in order.iter().enumerate() {
            [out[2 * slot], out[2 * slot + 1]] = crate::xlu::srt_matrix(self.tex_srt[i]);
        }
        out
    }
}
