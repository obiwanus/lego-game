use glam::{Vec2, Vec3};

#[inline(always)]
pub fn vec3_infinity() -> Vec3 {
    Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY)
}

#[inline(always)]
pub fn vec2_infinity() -> Vec2 {
    Vec2::new(f32::INFINITY, f32::INFINITY)
}

#[inline(always)]
pub fn size_of_slice<T>(slice: &[T]) -> usize {
    std::mem::size_of::<T>() * slice.len()
}
