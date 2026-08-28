struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
fn hash(value: vec2<f32>) -> f32 {
    return fract(sin(dot(value, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

@fragment
fn ps_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let resolution = vec2<f32>(900.0, 560.0);
    let pixel = input.uv * resolution;
    let edge = min(min(pixel.x, resolution.x - pixel.x), min(pixel.y, resolution.y - pixel.y));
    let border = 1.0 - smoothstep(4.5, 7.5, edge);
    let cell = floor(pixel / 56.0);
    let center = (cell + vec2<f32>(hash(cell), hash(cell + vec2<f32>(3.7, 3.7)))) * 56.0;
    let particle = smoothstep(7.0, 0.0, length(pixel - center));
    let alpha = clamp(border * 0.92 + particle * 0.48, 0.0, 1.0);
    let color = mix(vec3<f32>(1.0, 0.08, 0.42), vec3<f32>(0.38, 0.20, 1.0), input.uv.x);
    return vec4<f32>(color * alpha, alpha);
}
