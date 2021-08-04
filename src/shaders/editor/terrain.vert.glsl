#version 410 core

uniform mat4 mvp;

// uniform vec2 terrain_origin;
const vec2 terrain_origin = vec2(0.0);

const float PATCH_SIZE = 16.0;  // so that one terrain tile is 1000x1000 units
const vec2 VERTICES[] = vec2[](vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(-0.5, 0.5), vec2(0.5, 0.5));

out VS_OUT { vec2 uv; }
vs_out;

void main() {
    vec2 vertex = VERTICES[gl_VertexID];

    // One terrain tile will always have 64x64 patches
    int x = gl_InstanceID & 63;
    int y = gl_InstanceID >> 6;
    vec2 offset = vec2(x, y);

    // Texture coords
    vs_out.uv = (vertex + offset + vec2(0.5)) / 64.0;

    // Position
    vec2 position = (vertex + vec2(offset.x - 32.0, offset.y - 32.0)) * PATCH_SIZE;

    // TODO: displace height here?
    float height = 0.0;
    gl_Position = mvp * vec4(position.x, height, position.y, 1.0);
}