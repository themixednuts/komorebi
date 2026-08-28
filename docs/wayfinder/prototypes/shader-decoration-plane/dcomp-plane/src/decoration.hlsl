cbuffer Scene : register(b0)
{
    float2 resolution;
    float time_seconds;
    float border_width;
};

struct VertexOutput
{
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

VertexOutput vs_main(uint id : SV_VertexID)
{
    VertexOutput output;
    float2 uv = float2((id << 1) & 2, id & 2);
    output.uv = uv;
    output.position = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    return output;
}
float hash(float2 value)
{
    return frac(sin(dot(value, float2(12.9898, 78.233))) * 43758.5453);
}

float4 ps_main(VertexOutput input) : SV_Target
{
    float2 pixel = input.uv * resolution;
    float edge = min(min(pixel.x, resolution.x - pixel.x), min(pixel.y, resolution.y - pixel.y));
    float border = 1.0 - smoothstep(border_width - 1.5, border_width + 1.5, edge);

    float2 cell = floor(pixel / 56.0);
    float seed = hash(cell);
    float2 center = (cell + float2(seed, hash(cell + 3.7))) * 56.0;
    center.y += fmod(time_seconds * (22.0 + seed * 38.0), resolution.y + 112.0) - 56.0;
    center.y = fmod(center.y, resolution.y + 112.0) - 56.0;
    float particle = smoothstep(7.0, 0.0, length(pixel - center));

    float alpha = saturate(border * 0.92 + particle * 0.48);
    float3 color = lerp(float3(1.0, 0.08, 0.42), float3(0.38, 0.20, 1.0), input.uv.x);
    return float4(color * alpha, alpha);
}
