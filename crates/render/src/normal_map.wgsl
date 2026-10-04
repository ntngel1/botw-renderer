// botw::normal_map — the normal of a model's surface from its normal map,
// with the z of a two-component (BC5) map rebuilt the way the game's shaders
// do, `√(1 − sat(x² + y²))` (the water's and the grass's,
// docs/research/wiiu-water-variants.md, wiiu-field-shading.md). Bevy's
// `apply_normal_mapping` takes `√(1 − x² − y²)`: NaN on the texels whose xy
// reaches past the unit circle (Eldin's rocks have many), and the NaN the
// deferred shading makes of them reaches the cube map, which lights its own
// faces, and blackens the whole scene from then on.
//
// Otherwise it repeats Bevy's normal mapping (`pbr_input_from_standard_
// material`, Bevy 0.19): the same sample, mikktspace basis, flipped y and
// back faces. Forward pass only, for materials extending `StandardMaterial`.

#define_import_path botw::normal_map

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_bindings::mesh,
    mesh_view_bindings::view,
    pbr_bindings,
    pbr_functions::calculate_tbn_mikktspace,
    pbr_types::{PbrInput, STANDARD_MATERIAL_FLAGS_TWO_COMPONENT_NORMAL_MAP, STANDARD_MATERIAL_FLAGS_FLIP_NORMAL_MAP_Y, STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT},
}
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}

#ifdef BINDLESS
#import bevy_pbr::pbr_bindings::material_indices
#endif

// `pbr.N` (from `pbr_input_from_standard_material(in, is_front)`) with a
// two-component normal map's z kept real; other surfaces keep theirs.
fn mapped_normal(in: VertexOutput, is_front: bool, pbr: PbrInput) -> vec3<f32> {
    var N = pbr.N;
#ifdef VERTEX_UVS_A
#ifdef VERTEX_TANGENTS
#ifdef STANDARD_MATERIAL_NORMAL_MAP
    var uv = in.uv;
#ifdef STANDARD_MATERIAL_NORMAL_MAP_UV_B
#ifdef VERTEX_UVS_B
    uv = in.uv_b;
#endif
#endif
    uv = (pbr.material.uv_transform * vec3(uv, 1.0)).xy;
    // Sampled before any branch: derivatives need uniform control flow.
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let texel = textureSampleBias(
        bindless_textures_2d[material_indices[slot].normal_map_texture],
        bindless_samplers_filtering[material_indices[slot].normal_map_sampler],
        uv,
        view.mip_bias,
    ).rg;
#else
    let texel = textureSampleBias(pbr_bindings::normal_map_texture, pbr_bindings::normal_map_sampler, uv, view.mip_bias).rg;
#endif
    let flags = pbr.material.flags;
    if (flags & STANDARD_MATERIAL_FLAGS_TWO_COMPONENT_NORMAL_MAP) != 0u {
        // SI-LGT-22: normal xy as 2t - 1 for every model (foliage differs in the game).
        let xy = texel * 2.0 - 1.0;
        var Nt = vec3<f32>(xy, sqrt(1.0 - saturate(dot(xy, xy))));
        if (flags & STANDARD_MATERIAL_FLAGS_FLIP_NORMAL_MAP_Y) != 0u {
            Nt.y = -Nt.y;
        }
        if (flags & STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u && !is_front {
            Nt = -Nt;
        }
        let TBN = calculate_tbn_mikktspace(pbr.world_normal, in.world_tangent);
        N = normalize(Nt.x * TBN[0] + Nt.y * TBN[1] + Nt.z * TBN[2]);
    }
#endif
#endif
#endif
    return N;
}
