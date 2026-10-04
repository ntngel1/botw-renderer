// Bevy 0.19 vertex interfaces with independent lash/brow coordinates.
#define_import_path botw::face_io
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::VertexOutput
struct FaceVertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,

#ifdef VERTEX_UVS_A
    @location(1) uv: vec2<f32>,
#endif

#ifdef VERTEX_UVS_B
    @location(2) uv_b: vec2<f32>,
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    @location(3) normal: vec3<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) tangent: vec4<f32>,
#endif
#endif // NORMAL_PREPASS_OR_DEFERRED_PREPASS

#ifdef SKINNED
    @location(5) joint_indices: vec4<u32>,
    @location(6) joint_weights: vec4<f32>,
#endif

#ifdef VERTEX_COLORS
    @location(7) color: vec4<f32>,
#endif

#ifdef MORPH_TARGETS
    @builtin(vertex_index) index: u32,
#endif // MORPH_TARGETS
    @location(9) lash_uv: vec2<f32>,
    @location(10) brow_uv: vec2<f32>,
}
struct FaceVertexOutput {
    // This is `clip position` when the struct is used as a vertex stage output
    // and `frag coord` when used as a fragment stage input
    @builtin(position) position: vec4<f32>,

#ifdef VERTEX_UVS_A
    @location(0) uv: vec2<f32>,
#endif

#ifdef VERTEX_UVS_B
    @location(1) uv_b: vec2<f32>,
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    @location(2) world_normal: vec3<f32>,
#ifdef VERTEX_TANGENTS
    @location(3) world_tangent: vec4<f32>,
#endif
#endif // NORMAL_PREPASS_OR_DEFERRED_PREPASS

    @location(4) world_position: vec4<f32>,
#ifdef MOTION_VECTOR_PREPASS
    @location(5) previous_world_position: vec4<f32>,
#endif

#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    @location(6) unclipped_depth: f32,
#endif // UNCLIPPED_DEPTH_ORTHO_EMULATION
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(7) instance_index: u32,
#endif

#ifdef VERTEX_COLORS
    @location(8) color: vec4<f32>,
#endif

#ifdef VISIBILITY_RANGE_DITHER
    @location(9) @interpolate(flat) visibility_range_dither: i32,
#endif  // VISIBILITY_RANGE_DITHER
    @location(10) face_uvs: vec4<f32>,
}
fn standard_vertex(face: FaceVertexOutput) -> VertexOutput {
    var out: VertexOutput;
    out.position = face.position;

#ifdef VERTEX_UVS_A
    out.uv = face.uv;
#endif

#ifdef VERTEX_UVS_B
    out.uv_b = face.uv_b;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
    out.world_normal = face.world_normal;
#ifdef VERTEX_TANGENTS
    out.world_tangent = face.world_tangent;
#endif
#endif

    out.world_position = face.world_position;
#ifdef MOTION_VECTOR_PREPASS
    out.previous_world_position = face.previous_world_position;
#endif

#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = face.unclipped_depth;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = face.instance_index;
#endif

#ifdef VERTEX_COLORS
    out.color = face.color;
#endif

#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = face.visibility_range_dither;
#endif
    return out;
}
#else
#import bevy_pbr::forward_io::VertexOutput
struct FaceVertex {
    @builtin(instance_index) instance_index: u32,
#ifdef VERTEX_POSITIONS
    @location(0) position: vec3<f32>,
#endif
#ifdef VERTEX_NORMALS
    @location(1) normal: vec3<f32>,
#endif
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef SKINNED
    @location(6) joint_indices: vec4<u32>,
    @location(7) joint_weights: vec4<f32>,
#endif
#ifdef MORPH_TARGETS
    @builtin(vertex_index) index: u32,
#endif
    @location(9) lash_uv: vec2<f32>,
    @location(10) brow_uv: vec2<f32>,
}
struct FaceVertexOutput {
    // This is `clip position` when the struct is used as a vertex stage output
    // and `frag coord` when used as a fragment stage input
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
#ifdef VERTEX_UVS_A
    @location(2) uv: vec2<f32>,
#endif
#ifdef VERTEX_UVS_B
    @location(3) uv_b: vec2<f32>,
#endif
#ifdef VERTEX_TANGENTS
    @location(4) world_tangent: vec4<f32>,
#endif
#ifdef VERTEX_COLORS
    @location(5) color: vec4<f32>,
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    @location(6) @interpolate(flat) instance_index: u32,
#endif
#ifdef VISIBILITY_RANGE_DITHER
    @location(7) @interpolate(flat) visibility_range_dither: i32,
#endif
    @location(10) face_uvs: vec4<f32>,
}
fn standard_vertex(face: FaceVertexOutput) -> VertexOutput {
    var out: VertexOutput;
    out.position = face.position;
    out.world_position = face.world_position;
    out.world_normal = face.world_normal;
#ifdef VERTEX_UVS_A
    out.uv = face.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = face.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = face.world_tangent;
#endif
#ifdef VERTEX_COLORS
    out.color = face.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = face.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = face.visibility_range_dither;
#endif
    return out;
}
#endif
