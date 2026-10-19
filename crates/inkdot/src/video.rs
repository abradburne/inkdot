//! The two screens, on OpenGL ES 3 (desktop GL 3.3 when tried out on a computer).
//!
//! The top screen shows the game at 5×, 800×720, centred: the largest whole scale that fits
//! the 1024×768 panel, so every Game Boy pixel is 5×5 panel pixels. Each frame is uploaded as a
//! 160×144 texture (converted to RGBA on the CPU, where palettes and colour models are a table
//! lookup) and drawn by one fragment shader, which also does the LCD texture: a faint grid
//! between pixels and the slow response of the original screens, as a blend with the previous
//! frame. The bottom screen is the emulator's own interface, drawn in software
//! ([`crate::ui`]) and uploaded only when it changes.
//!
//! Each screen is a window of its own where nibOS has a display per panel; on a computer, one
//! window shows both, stacked, at half size.

use glow::HasContext;
use nibkit::screens::{self, Screen};
use sdl2::video::{GLContext, GLProfile, SwapInterval, Window};

/// The game's place on the top screen.
pub const GAME_X: i32 = 112;
pub const GAME_Y: i32 = 24;
pub const GAME_W: i32 = 800;
pub const GAME_H: i32 = 720;

const VERTEX: &str = r#"
in vec2 corner;
uniform vec4 rect;      // x, y, w, h in clip space
out vec2 uv;
void main() {
    uv = vec2(corner.x, 1.0 - corner.y);
    gl_Position = vec4(rect.xy + corner * rect.zw, 0.0, 1.0);
}
"#;

const GAME: &str = r#"
in vec2 uv;
out vec4 colour;
uniform sampler2D now;
uniform sampler2D before;
uniform float persistence;  // the share of the last frame left in this one
uniform float grid;         // 0: no grid; 1: the grid at full strength
uniform vec3 gap;           // the colour between pixels
void main() {
    vec3 c = mix(texture(now, uv).rgb, texture(before, uv).rgb, persistence);
    // Where in its 5x5 square this panel pixel is: the last row and column are the gap.
    vec2 f = fract(uv * vec2(160.0, 144.0));
    float edge = step(0.8, max(f.x, f.y));
    c = mix(c, mix(c, gap, 0.35), edge * grid);
    colour = vec4(c, 1.0);
}
"#;

const PLAIN: &str = r#"
in vec2 uv;
out vec4 colour;
uniform sampler2D image;
void main() {
    colour = texture(image, uv);
}
"#;

/// How the game is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LcdLook {
    pub persistence: f32,
    pub grid: bool,
    pub gap: [u8; 3],
    /// Behind the game: the panel around it.
    pub surround: [u8; 3],
}

struct Target {
    window: Window,
    /// Where the screen is in the window: the whole of it, or half on a computer.
    viewport: (i32, i32, i32, i32),
}

/// The screens and what draws on them.
pub struct Video {
    gl: glow::Context,
    _context: GLContext,
    top: Target,
    /// `None` on a computer, where the top window shows both screens.
    bottom: Option<Target>,
    desktop: bool,
    game: glow::Program,
    plain: glow::Program,
    quad: glow::VertexArray,
    frames: [glow::Texture; 2],
    newest: usize,
    ui: glow::Texture,
    ui_ready: bool,
}

fn compile(gl: &glow::Context, es: bool, vs: &str, fs: &str) -> Result<glow::Program, String> {
    let header = if es {
        "#version 300 es\nprecision mediump float;\n"
    } else {
        "#version 330 core\n"
    };
    // SAFETY: plain GL calls on the current context, with sources this module owns.
    unsafe {
        let program = gl.create_program()?;
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, &format!("{header}{src}"));
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            gl.attach_shader(program, shader);
        }
        gl.bind_attrib_location(program, 0, "corner");
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        Ok(program)
    }
}

fn texture(gl: &glow::Context, w: i32, h: i32) -> Result<glow::Texture, String> {
    // SAFETY: GL calls on the current context; the texture is filled later.
    unsafe {
        let t = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::NEAREST as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            w,
            h,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            None,
        );
        Ok(t)
    }
}

impl Video {
    /// Opens the screens: a window on each panel on the handheld, one window on a computer.
    pub fn open(video: &sdl2::VideoSubsystem) -> Result<Video, String> {
        let bounds: Vec<screens::Rect> = (0..video.num_video_displays()?)
            .filter_map(|i| video.display_bounds(i).ok())
            .map(|r| screens::Rect::new(r.x(), r.y(), r.width(), r.height()))
            .collect();
        let placement = screens::place(&bounds, false).filter(|_| nibkit::on_device());
        let es = placement.is_some();
        {
            let attr = video.gl_attr();
            if es {
                attr.set_context_profile(GLProfile::GLES);
                attr.set_context_version(3, 0);
            } else {
                attr.set_context_profile(GLProfile::Core);
                attr.set_context_version(3, 3);
            }
            attr.set_double_buffer(true);
        }
        let window = |r: screens::Rect| {
            video
                .window("inkdot", r.w, r.h)
                .position(r.x, r.y)
                .fullscreen_desktop()
                .opengl()
                .build()
                .map_err(|e| e.to_string())
        };
        let (top, bottom, desktop) = match placement {
            Some(p) if !p.shared => {
                let top = window(p.region(Screen::Top).rect)?;
                let bottom = window(p.region(Screen::Bottom).rect)?;
                let full = (0, 0, screens::WIDTH as i32, screens::HEIGHT as i32);
                (
                    Target {
                        window: top,
                        viewport: full,
                    },
                    Some(Target {
                        window: bottom,
                        viewport: full,
                    }),
                    false,
                )
            }
            _ => {
                let (w, h) = (screens::WIDTH / 2, screens::HEIGHT / 2);
                let window = video
                    .window("inkdot", w, h * 2)
                    .opengl()
                    .allow_highdpi()
                    .build()
                    .map_err(|e| e.to_string())?;
                let (dw, dh) = window.drawable_size();
                (
                    Target {
                        window,
                        viewport: (0, dh as i32 / 2, dw as i32, dh as i32 / 2),
                    },
                    None,
                    true,
                )
            }
        };
        let context = top.window.gl_create_context()?;
        top.window.gl_make_current(&context)?;
        video.gl_set_swap_interval(SwapInterval::VSync)?;
        // SAFETY: the loader returns GL entry points for the context just made current.
        let gl = unsafe {
            glow::Context::from_loader_function(|s| video.gl_get_proc_address(s) as *const _)
        };
        let game = compile(&gl, es, VERTEX, GAME)?;
        let plain = compile(&gl, es, VERTEX, PLAIN)?;
        // SAFETY: GL calls on the current context.
        let quad = unsafe {
            let vao = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(vao));
            let vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let corners: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            let bytes: Vec<u8> = corners.iter().flat_map(|f| f.to_ne_bytes()).collect();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            vao
        };
        let frames = [texture(&gl, 160, 144)?, texture(&gl, 160, 144)?];
        let ui = texture(&gl, screens::WIDTH as i32, screens::HEIGHT as i32)?;
        Ok(Video {
            gl,
            _context: context,
            top,
            bottom,
            desktop,
            game,
            plain,
            quad,
            frames,
            newest: 0,
            ui,
            ui_ready: false,
        })
    }

    /// Where a point in the bottom screen's window is on the bottom screen, as 0..1 (for mouse
    /// clicks on a computer, where the bottom screen is the window's lower half).
    pub fn bottom_point(&self, x: i32, y: i32) -> Option<(f32, f32)> {
        if !self.desktop {
            return None;
        }
        let (w, h) = self.top.window.size();
        let y = y as f32 - h as f32 / 2.0;
        (y >= 0.0).then(|| (x as f32 / w as f32, y / (h as f32 / 2.0)))
    }

    /// Uploads a new game picture (160×144 RGBA).
    pub fn upload_frame(&mut self, rgba: &[u8]) {
        self.newest ^= 1;
        let gl = &self.gl;
        // SAFETY: GL calls on the current context; `rgba` is 160*144*4 bytes.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.frames[self.newest]));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                160,
                144,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(rgba),
            );
        }
    }

    /// Uploads the bottom screen's picture (1024×768 RGBA).
    pub fn upload_ui(&mut self, rgba: &[u8]) {
        let gl = &self.gl;
        // SAFETY: as above; `rgba` is the full screen.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.ui));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                screens::WIDTH as i32,
                screens::HEIGHT as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(rgba),
            );
        }
        self.ui_ready = true;
    }

    /// A rectangle in panel pixels as clip-space x, y, w, h.
    fn clip(px: (i32, i32, i32, i32)) -> [f32; 4] {
        let (w, h) = (screens::WIDTH as f32, screens::HEIGHT as f32);
        let x = px.0 as f32 / w * 2.0 - 1.0;
        let y = 1.0 - (px.1 + px.3) as f32 / h * 2.0;
        [x, y, px.2 as f32 / w * 2.0, px.3 as f32 / h * 2.0]
    }

    fn draw_quad(&self, program: glow::Program, rect: [f32; 4]) {
        let gl = &self.gl;
        // SAFETY: GL calls on the current context with this module's objects.
        unsafe {
            let at = gl.get_uniform_location(program, "rect");
            gl.uniform_4_f32(at.as_ref(), rect[0], rect[1], rect[2], rect[3]);
            gl.bind_vertex_array(Some(self.quad));
            gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
        }
    }

    fn viewport(&self, v: (i32, i32, i32, i32)) {
        // SAFETY: a GL state change.
        unsafe { self.gl.viewport(v.0, v.1, v.2, v.3) };
    }

    /// Draws the game on the top screen and shows it at the next refresh; with `capture`, also
    /// returns what was drawn (RGBA, rows top first), for INKDOT_DEBUG=shot.
    pub fn present_top(&mut self, look: LcdLook, capture: bool) -> Option<Vec<u8>> {
        self.top.window.gl_make_current(&self._context).ok();
        self.viewport(self.top.viewport);
        let gl = &self.gl;
        let rgb = |c: [u8; 3]| c.map(|v| v as f32 / 255.0);
        let surround = rgb(look.surround);
        // SAFETY: GL calls on the current context with this module's objects.
        unsafe {
            if self.desktop {
                gl.viewport(0, 0, self.top.viewport.2, self.top.viewport.3 * 2);
                gl.clear_color(0.05, 0.05, 0.06, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT);
                gl.viewport(
                    self.top.viewport.0,
                    self.top.viewport.1,
                    self.top.viewport.2,
                    self.top.viewport.3,
                );
                gl.enable(glow::SCISSOR_TEST);
                gl.scissor(
                    self.top.viewport.0,
                    self.top.viewport.1,
                    self.top.viewport.2,
                    self.top.viewport.3,
                );
            }
            gl.clear_color(surround[0], surround[1], surround[2], 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::SCISSOR_TEST);
            gl.use_program(Some(self.game));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.frames[self.newest]));
            gl.active_texture(glow::TEXTURE1);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.frames[self.newest ^ 1]));
            let u = |n: &str| gl.get_uniform_location(self.game, n);
            gl.uniform_1_i32(u("now").as_ref(), 0);
            gl.uniform_1_i32(u("before").as_ref(), 1);
            gl.uniform_1_f32(u("persistence").as_ref(), look.persistence);
            gl.uniform_1_f32(u("grid").as_ref(), if look.grid { 1.0 } else { 0.0 });
            let g = rgb(look.gap);
            gl.uniform_3_f32(u("gap").as_ref(), g[0], g[1], g[2]);
        }
        self.draw_quad(self.game, Self::clip((GAME_X, GAME_Y, GAME_W, GAME_H)));
        let shot = capture.then(|| self.read_back(self.top.viewport));
        if self.desktop {
            // The bottom screen goes in the same window, below.
            self.draw_ui_at((0, 0, self.top.viewport.2, self.top.viewport.3));
        }
        self.top.window.gl_swap_window();
        shot
    }

    /// The pixels of a viewport, top row first.
    fn read_back(&self, v: (i32, i32, i32, i32)) -> Vec<u8> {
        let mut px = vec![0u8; (v.2 * v.3 * 4) as usize];
        // SAFETY: reads the current framebuffer into a buffer of exactly w*h*4 bytes.
        unsafe {
            self.gl.read_pixels(
                v.0,
                v.1,
                v.2,
                v.3,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(&mut px),
            );
        }
        // GL's rows go bottom up.
        let row = (v.2 * 4) as usize;
        let mut flipped = Vec::with_capacity(px.len());
        for r in px.chunks(row).rev() {
            flipped.extend_from_slice(r);
        }
        flipped
    }

    /// The size of the top screen's drawing area, for a capture.
    pub fn top_size(&self) -> (u32, u32) {
        (self.top.viewport.2 as u32, self.top.viewport.3 as u32)
    }

    fn draw_ui_at(&self, viewport: (i32, i32, i32, i32)) {
        if !self.ui_ready {
            return;
        }
        self.viewport(viewport);
        let gl = &self.gl;
        // SAFETY: GL calls on the current context with this module's objects.
        unsafe {
            gl.use_program(Some(self.plain));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.ui));
            gl.uniform_1_i32(gl.get_uniform_location(self.plain, "image").as_ref(), 0);
        }
        self.draw_quad(self.plain, [-1.0, -1.0, 2.0, 2.0]);
    }

    /// Shows the bottom screen's current picture (on the handheld; on a computer it is drawn
    /// with the top screen).
    pub fn present_bottom(&mut self) {
        let Some(bottom) = &self.bottom else { return };
        bottom.window.gl_make_current(&self._context).ok();
        self.draw_ui_at(bottom.viewport);
        bottom.window.gl_swap_window();
        self.top.window.gl_make_current(&self._context).ok();
    }
}
