struct Particle {
    float2 position;
    float2 velocity;
};

cbuffer Step : register(b0) {
    float delta_seconds;
    float drag;
    float acceleration_x;
    float acceleration_y;
};

RWStructuredBuffer<Particle> particles : register(u0);

[numthreads(64, 1, 1)]
void cs_main(uint3 id : SV_DispatchThreadID) {
    uint count;
    uint stride;
    particles.GetDimensions(count, stride);
    if (id.x >= count) {
        return;
    }
    Particle particle = particles[id.x];
    particle.velocity.x = particle.velocity.x * drag + acceleration_x * delta_seconds;
    particle.velocity.y = particle.velocity.y * drag + acceleration_y * delta_seconds;
    particle.position.x += particle.velocity.x * delta_seconds;
    particle.position.y += particle.velocity.y * delta_seconds;
    particles[id.x] = particle;
}
