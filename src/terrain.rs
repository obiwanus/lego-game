use std::ffi::c_void;

use gl::types::*;
use glam::Vec3Swizzles;
use glam::{Vec2, Vec3};
use image::GenericImageView;

use crate::texture::{calculate_mip_levels, get_max_anisotropy, unit_to_gl_const};
use crate::{
    opengl::shader::Program,
    ray::{Ray, AABB},
    utils::vec2_infinity,
    Result,
};
use crate::{WINDOW_HEIGHT, WINDOW_WIDTH};

struct Heightmap {
    texture: GLuint,
    texture_size: usize,

    // For drawing on heightmap
    fbo: GLuint,
    shader: Program,
}

impl Heightmap {
    pub fn flat(texture_size: usize) -> Result<Self> {
        Heightmap::new(None, Some(texture_size))
    }

    pub fn from_image(path: &str) -> Result<Self> {
        Heightmap::new(Some(path), None)
    }

    fn new(path: Option<&str>, texture_size: Option<usize>) -> Result<Self> {
        debug_assert!(!(path.is_some() && texture_size.is_some()));
        debug_assert!(!(path.is_none() && texture_size.is_none()));

        let (pixels, texture_size) = if let Some(path) = path {
            let img = image::open(path)?;
            let (width, height) = img.dimensions();
            assert_eq!(width, height, "Only square heightmaps are supported");
            assert!(
                width == 1024 || width == 2048 || width == 4096,
                "Only heightmaps with sizes 1024, 2048 and 4096 are supported"
            );

            (img.into_luma16().into_raw(), width as usize)
        } else {
            let size = texture_size.unwrap();

            (vec![0u16; size * size], size)
        };

        let mut texture: GLuint = 0;
        unsafe {
            gl::CreateTextures(gl::TEXTURE_2D, 1, &mut texture);
            gl::BindTexture(gl::TEXTURE_2D, texture);
            gl::TextureParameteri(texture, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as GLint);
            gl::TextureParameteri(texture, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as GLint);
            gl::TextureParameteri(texture, gl::TEXTURE_MIN_FILTER, gl::LINEAR as GLint);
            gl::TextureParameteri(texture, gl::TEXTURE_MAG_FILTER, gl::LINEAR as GLint);
            gl::TextureStorage2D(
                texture,
                1,
                gl::R16,
                texture_size as i32,
                texture_size as i32,
            );

            gl::TextureSubImage2D(
                texture,
                0,
                0,
                0,
                texture_size as i32,
                texture_size as i32,
                gl::RED,
                gl::UNSIGNED_SHORT,
                pixels.as_ptr() as *const _,
            );
        }

        // Framebuffer object for rendering to heightmap
        let mut fbo: GLuint = 0;
        unsafe {
            gl::CreateFramebuffers(1, &mut fbo);
            gl::NamedFramebufferTexture(fbo, gl::COLOR_ATTACHMENT0, texture, 0);
            let draw_buffers = [gl::COLOR_ATTACHMENT0];
            gl::NamedFramebufferDrawBuffers(fbo, 1, draw_buffers.as_ptr() as *const _);
            assert_eq!(
                gl::CheckNamedFramebufferStatus(fbo, gl::FRAMEBUFFER),
                gl::FRAMEBUFFER_COMPLETE,
                "Heightmap texture framebuffer is incomplete",
            );
        }

        let shader = Program::new()
            .vertex_shader(include_str!("shaders/editor/terrain/heightmap.vert"))?
            .fragment_shader(include_str!("shaders/editor/terrain/heightmap.frag"))?
            .link()?;

        Ok(Heightmap {
            texture,
            texture_size,

            fbo,
            shader,
        })
    }

    fn draw_on_heightmap(
        &self,
        cursor: Vec2,
        brush: &Brush,
        terrain_size: f32,
        delta_time: f32,
        raise: bool,
    ) {
        self.shader.set_used();
        debug_assert!(cursor.x <= 1.0 && cursor.x >= 0.0);
        debug_assert!(cursor.y <= 1.0 && cursor.y >= 0.0);
        self.shader.set_vec2("cursor", &cursor).unwrap();
        let brush_size = brush.size as f32 / terrain_size;
        self.shader.set_f32("brush_size", brush_size).unwrap();
        self.shader.set_f32("delta_time", delta_time).unwrap();

        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo);
            gl::Disable(gl::FRAMEBUFFER_SRGB);
            gl::Viewport(0, 0, self.texture_size as i32, self.texture_size as i32);

            gl::ActiveTexture(unit_to_gl_const(0));
            gl::BindTexture(gl::TEXTURE_2D, brush.texture);

            gl::Enable(gl::BLEND);
            gl::Disable(gl::DEPTH_TEST);

            gl::BlendFunc(gl::ONE, gl::ONE);
            gl::BlendEquation(if raise {
                gl::FUNC_ADD
            } else {
                gl::FUNC_REVERSE_SUBTRACT
            });

            gl::DrawArrays(gl::TRIANGLE_FAN, 0, 4);
            gl::MemoryBarrier(gl::FRAMEBUFFER_BARRIER_BIT); // not critical

            // Reset everything back
            gl::Disable(gl::BLEND);
            gl::Enable(gl::DEPTH_TEST);
            gl::BlendEquation(gl::FUNC_ADD);
            gl::Enable(gl::FRAMEBUFFER_SRGB);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::Viewport(0, 0, WINDOW_WIDTH as i32, WINDOW_HEIGHT as i32);
        }
    }
}

pub struct Brush {
    texture: GLuint,
    texture_size: usize,
    pub size: f32,
}

impl Brush {
    pub fn new(path: &str, size: f32) -> Self {
        let img = image::open(path)
            .expect("Can't load brush image")
            .into_luma16();
        let (width, height) = img.dimensions();
        assert_eq!(width, height, "Only square brushes are supported");
        let texture_size = width as usize;

        let mut texture: GLuint = 0;
        unsafe {
            gl::CreateTextures(gl::TEXTURE_2D, 1, &mut texture);
            gl::TextureParameteri(texture, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_BORDER as GLint);
            gl::TextureParameteri(texture, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_BORDER as GLint);
            gl::TextureParameteri(
                texture,
                gl::TEXTURE_MIN_FILTER,
                gl::LINEAR_MIPMAP_LINEAR as GLint,
            );
            gl::TextureParameteri(texture, gl::TEXTURE_MAG_FILTER, gl::LINEAR as GLint);
            gl::TextureStorage2D(
                texture,
                calculate_mip_levels(texture_size, texture_size),
                gl::R16,
                texture_size as i32,
                texture_size as i32,
            );
            gl::TextureSubImage2D(
                texture,
                0,
                0,
                0,
                texture_size as i32,
                texture_size as i32,
                gl::RED,
                gl::UNSIGNED_SHORT,
                img.as_raw().as_ptr() as *const _,
            );
            gl::GenerateTextureMipmap(texture);
        }

        Brush {
            texture,
            size,
            texture_size,
        }
    }
}

pub struct Terrain {
    pub aabb: AABB,

    vao: GLuint,
    shader: Program,
    pub tess_level: f32,

    texture: GLuint,
    heightmap: Heightmap,

    pub cursor: Vec2,
    pub brush: Brush,

    shadow_map_fbo: GLuint,
    shadow_map: GLuint,
    shadow_map_size: i32,
    shadow_map_shader: Program,

    debug: TerrainDebug,

    // Main parameters
    center: Vec2,
    max_height: f32,
    num_patches: i32,
    patch_size: f32,
}

struct TerrainDebug {
    aabb_shader: Program,
    normal_shader: Program,
}

impl Terrain {
    pub fn new(center: Vec2, start_flat: bool, heightmap_path: &str) -> Result<Self> {
        // TODO: support centers other than 0, 0
        // (currently hard-coded in terrain.vert.glsl)
        assert_eq!(center, Vec2::new(0.0, 0.0));

        let max_height = 200.0;
        let num_patches = 64;
        let patch_size = 16.0;

        let terrain_size = patch_size * num_patches as f32;
        let aabb = {
            let half_size = terrain_size / 2.0;
            let min = Vec3::new(-half_size, 0.0, -half_size);
            let max = Vec3::new(half_size, max_height, half_size);
            AABB::new(min, max)
        };

        let mut vao: GLuint = 0;
        unsafe {
            gl::CreateVertexArrays(1, &mut vao);
        }

        let texture = {
            let img = image::open("textures/checkerboard.png")
                .unwrap()
                .flipv()
                .into_rgb8();
            let (width, height) = img.dimensions();
            assert_eq!(width, height);
            let size = width as usize;

            let mut texture: GLuint = 0;
            unsafe {
                gl::CreateTextures(gl::TEXTURE_2D, 1, &mut texture);
                gl::TextureParameteri(texture, gl::TEXTURE_WRAP_S, gl::REPEAT as GLint);
                gl::TextureParameteri(texture, gl::TEXTURE_WRAP_T, gl::REPEAT as GLint);
                gl::TextureParameteri(
                    texture,
                    gl::TEXTURE_MIN_FILTER,
                    gl::LINEAR_MIPMAP_LINEAR as GLint,
                );
                gl::TextureParameteri(texture, gl::TEXTURE_MAG_FILTER, gl::NEAREST as GLint);
                gl::TextureParameterf(texture, gl::TEXTURE_MAX_ANISOTROPY, get_max_anisotropy());
                gl::TextureStorage2D(
                    texture,
                    calculate_mip_levels(size, size),
                    gl::SRGB8,
                    size as i32,
                    size as i32,
                );
                gl::TextureSubImage2D(
                    texture,
                    0,
                    0,
                    0,
                    size as i32,
                    size as i32,
                    gl::RGB,
                    gl::UNSIGNED_BYTE,
                    img.as_raw().as_ptr() as *const _,
                );
                gl::GenerateTextureMipmap(texture);
            }

            texture
        };

        let cursor = vec2_infinity();
        let heightmap = if start_flat {
            Heightmap::flat(1024)?
        } else {
            Heightmap::from_image(heightmap_path)?
        };
        let brush = Brush::new("textures/brushes/mountain05.tga", 100.0);

        let shader = Program::new()
            .vertex_shader(include_str!("shaders/editor/terrain/terrain.vert.glsl"))?
            .tess_control_shader(include_str!("shaders/editor/terrain/terrain.tc.glsl"))?
            .tess_evaluation_shader(include_str!("shaders/editor/terrain/terrain.te.glsl"))?
            .fragment_shader(include_str!("shaders/editor/terrain/terrain.frag.glsl"))?
            .link()?;
        shader.set_used();
        shader.set_vec2("terrain_center", &center)?;
        shader.set_f32("terrain_max_height", max_height)?;
        shader.set_f32("terrain_size", terrain_size)?;
        shader.set_i32("num_patches", num_patches)?;
        shader.set_f32("patch_size", patch_size)?;

        // Shadow map
        let mut shadow_map_fbo: GLuint = 0;
        let mut shadow_map: GLuint = 0;
        let shadow_map_size = 2048;
        unsafe {
            gl::CreateFramebuffers(1, &mut shadow_map_fbo);
            gl::CreateTextures(gl::TEXTURE_2D, 1, &mut shadow_map);
            gl::TextureParameteri(shadow_map, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TextureParameteri(shadow_map, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
            gl::TextureParameteri(shadow_map, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
            gl::TextureParameteri(shadow_map, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
            gl::TextureStorage2D(
                shadow_map,
                1,
                gl::DEPTH_COMPONENT16,
                shadow_map_size,
                shadow_map_size,
            );
            gl::NamedFramebufferTexture(shadow_map_fbo, gl::DEPTH_ATTACHMENT, shadow_map, 0);
            gl::NamedFramebufferDrawBuffer(shadow_map_fbo, gl::NONE);
            gl::NamedFramebufferReadBuffer(shadow_map_fbo, gl::NONE);

            assert_eq!(
                gl::CheckNamedFramebufferStatus(shadow_map_fbo, gl::FRAMEBUFFER),
                gl::FRAMEBUFFER_COMPLETE,
                "Shadow map framebuffer is incomplete",
            );
        }
        let shadow_map_shader = Program::new()
            .vertex_shader(include_str!("shaders/editor/terrain/terrain.vert.glsl"))?
            .tess_control_shader(include_str!("shaders/editor/terrain/terrain.tc.glsl"))?
            .tess_evaluation_shader(include_str!("shaders/editor/terrain/shadow.te.glsl"))?
            .fragment_shader(include_str!("shaders/editor/terrain/shadow.frag.glsl"))?
            .link()?;
        shadow_map_shader.set_used();
        shadow_map_shader.set_vec2("terrain_center", &center)?;
        shadow_map_shader.set_f32("terrain_max_height", max_height)?;
        shadow_map_shader.set_i32("num_patches", num_patches)?;
        shadow_map_shader.set_f32("patch_size", patch_size)?;

        let debug = {
            let aabb_shader = Program::new()
                .vertex_shader(include_str!("shaders/debug/aabb.vert"))?
                .fragment_shader(include_str!("shaders/debug/aabb.frag"))?
                .link()?;
            aabb_shader.set_used();
            aabb_shader.set_vec3("aabb_min", &aabb.min)?;
            aabb_shader.set_vec3("aabb_max", &aabb.max)?;

            let normal_shader = Program::new()
                .vertex_shader(include_str!("shaders/editor/terrain/terrain.vert.glsl"))?
                .tess_control_shader(include_str!("shaders/editor/terrain/terrain.tc.glsl"))?
                .tess_evaluation_shader(include_str!("shaders/editor/terrain/terrain.te.glsl"))?
                .geometry_shader(include_str!("shaders/debug/terrain/normals.geometry.glsl"))?
                .fragment_shader(include_str!("shaders/debug/terrain/normals.frag.glsl"))?
                .link()?;
            normal_shader.set_used();

            TerrainDebug {
                aabb_shader,
                normal_shader,
            }
        };

        Ok(Terrain {
            aabb,

            vao,
            shader,
            tess_level: 11.0,

            texture,
            heightmap,

            cursor,
            brush,

            shadow_map_fbo,
            shadow_map,
            shadow_map_size,
            shadow_map_shader,

            debug,

            center,
            max_height,
            num_patches,
            patch_size,
        })
    }

    // TODO: use a renderer
    pub fn draw(&mut self, time: f32) -> Result<()> {
        // Set common stuff for shadow pass / render pass
        unsafe {
            gl::PatchParameteri(gl::PATCH_VERTICES, 4);
            gl::BindVertexArray(self.vao);

            // Default texture
            gl::ActiveTexture(unit_to_gl_const(0));
            gl::BindTexture(gl::TEXTURE_2D, self.texture);

            // Heightmap
            gl::ActiveTexture(unit_to_gl_const(1));
            gl::BindTexture(gl::TEXTURE_2D, self.heightmap.texture);

            // Brush
            gl::ActiveTexture(unit_to_gl_const(2));
            gl::BindTexture(gl::TEXTURE_2D, self.brush.texture);

            // Shadow map
            gl::ActiveTexture(unit_to_gl_const(3));
            gl::BindTexture(gl::TEXTURE_2D, self.shadow_map);
        }

        // Draw into shadow map
        self.shadow_map_shader.set_used();
        self.shadow_map_shader
            .set_f32("tess_level", self.tess_level)?;
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.shadow_map_fbo);
            gl::Viewport(0, 0, self.shadow_map_size, self.shadow_map_size);
            gl::Clear(gl::DEPTH_BUFFER_BIT);

            gl::DrawArraysInstanced(gl::PATCHES, 0, 4, 64 * 64);

            gl::Viewport(0, 0, WINDOW_WIDTH as i32, WINDOW_HEIGHT as i32);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }

        // Draw the scene
        self.shader.set_used();
        self.shader.set_vec2("cursor", &self.cursor)?;
        self.shader.set_f32("brush_size", self.brush.size)?;
        self.shader.set_f32("tess_level", self.tess_level)?;

        unsafe {
            // gl::PolygonMode(gl::FRONT_AND_BACK, gl::LINE);
            gl::DrawArraysInstanced(gl::PATCHES, 0, 4, 64 * 64);
            // gl::PolygonMode(gl::FRONT_AND_BACK, gl::FILL);
        }

        // // Draw debug stuff
        // {
        //     // Draw AABB
        //     let debug = &mut self.debug;
        //     debug.aabb_shader.set_used();
        //     debug.aabb_shader.set_f32("time", time)?;
        //     unsafe {
        //         gl::DrawArrays(gl::LINE_STRIP, 0, 16);
        //     }

        //     // Draw normals
        //     debug.normal_shader.set_used();
        //     debug.normal_shader.set_f32("tess_level", self.tess_level)?;
        //     unsafe {
        //         gl::DrawArraysInstanced(gl::PATCHES, 0, 4, 64 * 64);
        //     }
        // }

        Ok(())
    }

    pub fn get_heightmap_pixels(&self) -> (Vec<u8>, usize) {
        let buffer_size = self.heightmap.texture_size * self.heightmap.texture_size * 2;
        let mut pixels = Vec::<u8>::with_capacity(buffer_size);
        unsafe {
            pixels.set_len(buffer_size);
            gl::GetTextureImage(
                self.heightmap.texture,
                0,
                gl::RED,
                gl::UNSIGNED_SHORT,
                buffer_size as i32,
                pixels.as_mut_ptr() as *mut c_void,
            );
        }
        (pixels, self.heightmap.texture_size)
    }

    pub fn size(&self) -> f32 {
        self.aabb.max.x - self.aabb.min.x
    }

    pub fn shape_terrain(&mut self, delta_time: f32, raise: bool) {
        let terrain_size = self.size();
        let cursor = (self.cursor - self.aabb.min.xz()) / terrain_size;
        self.heightmap
            .draw_on_heightmap(cursor, &self.brush, terrain_size, delta_time, raise);
    }

    /// Currently only intersects with the bottom plane of the AABB
    pub fn intersect_with_ray(&self, ray: &Ray) -> Option<Vec3> {
        let hit = ray.hits_aabb(&self.aabb)?;
        let point = ray.get_point_at(hit.t_max);

        const EPSILON: f32 = 0.001;
        if (point.y - self.aabb.min.y) > EPSILON {
            None // not hitting the bottom plane
        } else {
            Some(point)
        }
    }

    pub fn move_cursor(&mut self, ray: &Ray) -> bool {
        if let Some(point) = self.intersect_with_ray(ray) {
            self.cursor = Vec2::new(point.x, point.z).clamp(self.aabb.min.xz(), self.aabb.max.xz());
            true
        } else {
            self.hide_cursor();
            false
        }
    }

    pub fn hide_cursor(&mut self) {
        self.cursor = vec2_infinity();
    }
}

impl Drop for Terrain {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteVertexArrays(1, &self.vao);
            gl::DeleteTextures(1, &self.texture);
        }
    }
}
