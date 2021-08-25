use std::mem::size_of;

use egui::{epaint::ClippedShape, ClippedMesh, CtxRef, Output, RawInput};
use epaint::Color32;
use gl::types::*;
use glam::Vec2;
use memoffset::offset_of;

use crate::{opengl::shader::Program, texture::unit_to_gl_const, utils::size_of_slice, Result};

pub struct Gui {
    screen_size: Vec2,

    ctx: CtxRef,
    egui_texture: GLuint,
    egui_texture_version: Option<u64>,

    shader: Program,

    // Trying without any abstractions
    vao: GLuint,
    vbo: GLuint,
    ebo: GLuint,

    name: String,
}

impl Gui {
    // Note: assuming non-resizable window for now
    pub fn new(screen_size: Vec2) -> Result<Gui> {
        let mut vao: GLuint = 0;
        let mut vbo: GLuint = 0;
        let mut ebo: GLuint = 0;
        unsafe {
            // Create objects
            gl::CreateVertexArrays(1, &mut vao);
            gl::CreateBuffers(1, &mut vbo);
            gl::CreateBuffers(1, &mut ebo);

            // Attach buffers to vao
            gl::VertexArrayVertexBuffer(vao, 0, vbo, 0, size_of::<Vertex>() as i32);
            gl::VertexArrayElementBuffer(vao, ebo);

            // Position
            gl::VertexArrayAttribFormat(
                vao,
                0,
                2,
                gl::FLOAT,
                gl::FALSE,
                offset_of!(Vertex, pos) as u32,
            );

            // UV
            gl::VertexArrayAttribFormat(
                vao,
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                offset_of!(Vertex, uv) as u32,
            );

            // Color
            gl::VertexArrayAttribFormat(
                vao,
                2,
                4,
                gl::UNSIGNED_BYTE,
                gl::FALSE,
                offset_of!(Vertex, srgba) as u32,
            );

            gl::EnableVertexArrayAttrib(vao, 0);
            gl::EnableVertexArrayAttrib(vao, 1);
            gl::EnableVertexArrayAttrib(vao, 2);

            gl::VertexArrayAttribBinding(vao, 0, 0);
            gl::VertexArrayAttribBinding(vao, 1, 0);
            gl::VertexArrayAttribBinding(vao, 2, 0);
        }

        let shader = Program::new()
            .vertex_shader(include_str!("../shaders/editor/gui.vert"))?
            .fragment_shader(include_str!("../shaders/editor/gui.frag"))?
            .link()?;

        Ok(Gui {
            screen_size,

            ctx: CtxRef::default(),
            egui_texture: 0, // will be created before draw
            egui_texture_version: None,

            shader,
            name: String::new(),

            vao,
            vbo,
            ebo,
        })
    }

    pub fn wants_input(&self) -> bool {
        self.ctx.wants_pointer_input() || self.ctx.wants_keyboard_input()
    }

    pub fn layout_and_interact(&mut self, input: RawInput) -> (Output, Vec<ClippedShape>) {
        self.ctx.begin_frame(input);

        let mut name = self.name.clone();

        // ================== GUI starts ========================
        egui::SidePanel::left("my_side_panel").show(&self.ctx, |ui| {
            ui.heading("Водка водка!");
            if ui.button("Quit").clicked() {
                println!("Quit clicked");
            }

            egui::ComboBox::from_label("Version")
                .width(350.0)
                .selected_text("foo")
                .show_ui(ui, |ui| {
                    egui::CollapsingHeader::new("Dev")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label("contents");
                        });
                });

            ui.horizontal(|ui| {
                ui.label("Your name: ");
                ui.text_edit_singleline(&mut name);
            });
        });

        self.name = name;
        // ================== GUI ends ===========================

        let (output, shapes) = self.ctx.end_frame();

        (output, shapes)
    }

    pub fn draw(&mut self, shapes: Vec<ClippedShape>) {
        let pixels_per_point = self.ctx.pixels_per_point();
        let screen_size_in_points = self.screen_size / pixels_per_point;

        self.upload_egui_texture();

        let clipped_meshes = self.ctx.tessellate(shapes);

        self.shader.set_used();
        self.shader
            .set_vec2("u_screen_size", &screen_size_in_points)
            .unwrap();
        unsafe {
            gl::ActiveTexture(unit_to_gl_const(0));
            gl::BindTexture(gl::TEXTURE_2D, self.egui_texture);
        }

        for ClippedMesh(clip_rect, mesh) in clipped_meshes {
            let vertices = mesh
                .vertices
                .iter()
                .map(|v| Vertex {
                    pos: [v.pos.x, v.pos.y],
                    uv: [v.uv.x, v.uv.y],
                    srgba: v.color.to_array(),
                })
                .collect::<Vec<Vertex>>();

            // Upload vertex and index data
            unsafe {
                gl::NamedBufferData(
                    self.vbo,
                    size_of_slice(&vertices) as isize,
                    vertices.as_ptr() as *const _,
                    gl::STREAM_DRAW,
                );
                gl::NamedBufferData(
                    self.ebo,
                    size_of_slice(&mesh.indices) as isize,
                    mesh.indices.as_ptr() as *const _,
                    gl::STREAM_DRAW,
                );
            }

            // Draw
            unsafe {
                gl::BindVertexArray(self.vao);
                gl::Disable(gl::DEPTH_TEST);
                gl::Disable(gl::CULL_FACE);
                gl::Enable(gl::BLEND);
                gl::BlendFuncSeparate(
                    gl::ONE,
                    gl::ONE_MINUS_SRC_ALPHA,
                    gl::ONE_MINUS_DST_ALPHA,
                    gl::ONE,
                );

                gl::DrawElements(
                    gl::TRIANGLES,
                    mesh.indices.len() as i32,
                    gl::UNSIGNED_INT,
                    std::ptr::null(),
                );

                gl::Disable(gl::BLEND);
                gl::Enable(gl::DEPTH_TEST);
                gl::Enable(gl::CULL_FACE);
            }
        }
    }

    fn upload_egui_texture(&mut self) {
        let texture = self.ctx.texture();
        if self.egui_texture_version == Some(texture.version) {
            return; // No change
        }
        self.egui_texture_version = Some(texture.version);

        let pixels: Vec<(u8, u8, u8, u8)> = texture
            .pixels
            .iter()
            .map(|&a| Color32::from_white_alpha(a).to_tuple())
            .collect();

        let mut new_texture: GLuint = 0;
        unsafe {
            if self.egui_texture != 0 {
                // Delete old texture
                gl::DeleteTextures(1, &self.egui_texture);
            }
            gl::CreateTextures(gl::TEXTURE_2D, 1, &mut new_texture);
            gl::TextureParameteri(new_texture, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as GLint);
            gl::TextureParameteri(new_texture, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as GLint);
            gl::TextureParameteri(new_texture, gl::TEXTURE_MIN_FILTER, gl::LINEAR as GLint);
            gl::TextureParameteri(new_texture, gl::TEXTURE_MAG_FILTER, gl::LINEAR as GLint);
            gl::TextureStorage2D(
                new_texture,
                1,
                gl::SRGB8_ALPHA8,
                texture.width as GLint,
                texture.height as GLint,
            );
            gl::TextureSubImage2D(
                new_texture,
                0,
                0,
                0,
                texture.width as i32,
                texture.height as i32,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr() as *const _,
            );
        }
        self.egui_texture = new_texture;
    }
}

impl Drop for Gui {
    fn drop(&mut self) {
        unsafe {
            let buffers = [self.vbo, self.ebo];
            gl::DeleteBuffers(1, buffers.as_ptr());

            let arrays = [self.vao];
            gl::DeleteVertexArrays(1, arrays.as_ptr());
        }
    }
}

#[derive(Debug)]
#[repr(C)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    srgba: [u8; 4],
}
