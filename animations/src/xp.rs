//! Basic rendering example
//!
//! Demonstrates rendering simple 3D shapes in different modes:
//! - Points mode
//! - Lines mode
//! - Solid mode
//!
//! Press SPACE to cycle through render modes

use crate::BufferTargetRgb565;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::OnceCell;
use embedded_3dgfx::K3dengine;
use embedded_3dgfx::command_buffer::CommandBuffer;
use embedded_3dgfx::config::apply_default_caps;
use embedded_3dgfx::mesh::{Geometry, K3dMesh, RenderMode};
use embedded_3dgfx::renderer::FrameCtx;
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::prelude::*;
use micromath::num_traits::Float;
use nalgebra::{Point3, Vector3};
use once_cell::sync::Lazy;

fn calculate_face_normal(v0: &[f32; 3], v1: &[f32; 3], v2: &[f32; 3]) -> [f32; 3] {
    let edge1 = Vector3::new(v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]);
    let edge2 = Vector3::new(v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]);
    let normal = edge1.cross(&edge2).normalize();
    [normal.x, normal.y, normal.z]
}

fn make_cube() -> (Vec<[f32; 3]>, Vec<[usize; 3]>, Vec<[f32; 3]>) {
    let vertices = vec![
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
    ];

    let faces = vec![
        [0, 1, 2],
        [0, 2, 3], // Front
        [5, 4, 7],
        [5, 7, 6], // Back
        [3, 2, 6],
        [3, 6, 7], // Top
        [4, 5, 1],
        [4, 1, 0], // Bottom
        [1, 5, 6],
        [1, 6, 2], // Right
        [4, 0, 3],
        [4, 3, 7], // Left
    ];

    // Calculate per-face normals
    let mut normals = Vec::new();
    for face in &faces {
        let v0 = &vertices[face[0]];
        let v1 = &vertices[face[1]];
        let v2 = &vertices[face[2]];
        normals.push(calculate_face_normal(v0, v1, v2));
    }

    (vertices, faces, normals)
}

static GEOMETRY: Lazy<(Vec<[f32; 3]>, Vec<[usize; 3]>, Vec<[f32; 3]>)> = Lazy::new(|| make_cube());

const WIDTH: usize = 60;
const HEIGHT: usize = 40;

pub struct Scene {
    engine: K3dengine,
    zbuffer: Vec<u32>,
    commands: CommandBuffer<4096>,
    cube1: K3dMesh<'static>,
}

impl Scene {
    pub fn new() -> Self {
        let mut engine = K3dengine::new(WIDTH as u16, HEIGHT as u16);
        let zbuffer = vec![u32::MAX; WIDTH * HEIGHT];
        let commands = CommandBuffer::<4096>::new();

        apply_default_caps(&mut engine);

        engine.camera.set_position(Point3::new(0.0, 3.0, 10.0));
        engine.camera.set_target(Point3::new(0.0, 0.0, 0.0));

        let cube_geometry = Geometry {
            vertices: &GEOMETRY.0,
            faces: &GEOMETRY.1,
            colors: &[],
            lines: &[],
            normals: &GEOMETRY.2,
            vertex_normals: &[],
            uvs: &[],
            texture_id: None,
        };

        let mut cube1 = K3dMesh::new(cube_geometry);

        cube1.set_color(Rgb565::new(31, 0, 0));
        cube1.set_position(0.0, 0.0, 3.0);
        cube1.set_scale(2.);

        Self {
            engine,
            zbuffer,
            commands,
            cube1,
        }
    }

    pub fn render(&mut self, display: &mut BufferTargetRgb565, ms_since_start: u32) {
        let time = ms_since_start as f32 * 0.001;

        let light_angle_h = time * 1.0;
        let light_angle_v = (time * 0.5).sin() * 0.3;

        let light_dir = Vector3::new(light_angle_h.cos(), light_angle_v, light_angle_h.sin()).normalize();

        self.cube1.set_attitude(time * 1.3, time * 1.5, time * 1.2);

        self.cube1.set_render_mode(RenderMode::SolidLightDir(light_dir));

        self.zbuffer.fill(u32::MAX);

        self.engine
            .record(core::iter::once(&self.cube1), &mut self.commands, None)
            .unwrap();

        let mut frame = FrameCtx {
            zbuffer: &mut self.zbuffer,
            width: WIDTH,
            height: HEIGHT,
        };

        self.engine
            .execute::<_, 4096>(display, &mut frame, &self.commands, None)
            .unwrap();
    }
}
