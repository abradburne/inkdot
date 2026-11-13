//! The two screens, on OpenGL ES 3 (desktop GL 3.3 when tried out on a computer).
//!
//! The top screen shows the game at 5×, 800×720, centred: the largest whole scale that fits
//! the 1024×768 panel, so every Game Boy pixel is 5×5 panel pixels. Each frame is uploaded as a
//! 160×144 texture (converted to RGBA on the CPU, where palettes and colour models are a table
//! lookup) and drawn by one fragment shader, which also does the screen texture
//! ([`crate::lcd`]): one multiply by a mask built once per texture and size, with the gain that
//! keeps mid-tones bright, the reflective screens' pixel shadow, and the slow response of the
//! original screens as a blend with the previous frame. The bottom screen is the emulator's own
//! interface, drawn in software ([`crate::ui`]) and uploaded only when it changes.
//!
//! Each screen is a window of its own where nibOS has a display per panel; on the stock OS,
//! whose Weston makes the two panels one display (the bottom on the right), one window covers
//! both and each screen is half of it; on a computer, one window shows both, stacked, at half
//! size.

use crate::lcd::{self, Pattern, Shading};
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
out vec4 colour;
uniform sampler2D now;
uniform sampler2D before;
uniform sampler2D mask;     // per screen pixel: what passes of each channel, how much backing shows
uniform float persistence;  // the share of the last frame left in this one
uniform float gain;         // the brightening that gives back what the mask takes
uniform vec3 paper;         // the screen's backing, linear
uniform float shadow;       // how dark a black pixel's shadow is; 0: none
uniform int shadow_step;    // how far it falls, down and right, in window pixels
uniform ivec2 size;         // the picture's size in window pixels (the mask's)
uniform vec2 origin;        // the picture's bottom-left corner, in window pixels
uniform float scale;        // window pixels per panel pixel (1 on the handheld)

// Linear light, with a gamma of 2.
vec3 lin(vec3 c) { return c * c; }

float luma(vec3 c) { return dot(c, vec3(0.2126, 0.7152, 0.0722)); }

// The game's colour at window pixel q of the picture, linear, with the last frame's ghost.
vec3 at(ivec2 q) {
    q = clamp(q, ivec2(0), size - 1);
    // Which panel pixel this is, then which Game Boy pixel (5x5 panel pixels each). Integers
    // throughout: with interpolated texture coordinates at mediump (16-bit on the DS+'s GPU)
    // the right of the picture rounded to the wrong column, and the grid showed as stray lines.
    ivec2 p = ivec2(floor(vec2(q) / scale));
    ivec2 gb = ivec2(p.x / 5, 143 - p.y / 5);
    return mix(lin(texelFetch(now, gb, 0).rgb), lin(texelFetch(before, gb, 0).rgb), persistence);
}

void main() {
    // GL's rows go up; the mask's go down.
    ivec2 q = ivec2(floor(gl_FragCoord.xy - origin));
    vec3 c = at(q);
    // Brightened alike in all channels, to where the brightest is full: the hue is kept.
    float top = max(max(c.r, c.g), max(c.b, 1e-6));
    vec4 m = texelFetch(mask, ivec2(q.x, size.y - 1 - q.y), 0);
    vec3 o = c * min(gain, 1.0 / top) * m.rgb + paper * m.a;
    if (shadow > 0.0) {
        // The shadow of the pixel up and to the left, relative to this pixel's own.
        float lp = max(luma(paper), 1e-4);
        float caster = clamp(1.0 - luma(at(q + ivec2(-shadow_step, shadow_step))) / lp, 0.0, 1.0);
        float own = clamp(1.0 - luma(c) / lp, 0.0, 1.0);
        o *= (1.0 - shadow * caster) / (1.0 - shadow * own);
    }
    colour = vec4(sqrt(clamp(o, 0.0, 1.0)), 1.0);
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
    /// The screen texture.
    pub pattern: Pattern,
    /// The last frame's ghost, when the texture keeps it, and the screen's backing.
    pub shading: Shading,
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
    /// Where the bottom screen is in the top's window, when they share one (GL's viewport).
    shared_ui: Option<(i32, i32, i32, i32)>,
    desktop: bool,
    game: glow::Program,
    plain: glow::Program,
    quad: glow::VertexArray,
    /// Game pictures in a ring of three: the newest, the one before (for the LCD's blur), and
    /// the one being uploaded, which no draw still in flight reads (an upload into a texture
    /// the GPU is still reading makes the driver wait).
    frames: [glow::Texture; 3],
    newest: usize,
    ui: glow::Texture,
    ui_ready: bool,
    /// Over the game on the top screen while the menu is open (premultiplied RGBA), when shown.
    overlay: glow::Texture,
    overlay_on: bool,
    /// The screen texture's mask, and what it was built for (pattern and size).
    mask: glow::Texture,
    mask_for: Option<(Pattern, i32, i32)>,
    gain: f32,
    last_swap: std::time::Duration,
}

fn compile(gl: &glow::Context, es: bool, vs: &str, fs: &str) -> Result<glow::Program, String> {
    let header = if es {
        "#version 300 es\nprecision highp float;\nprecision highp int;\n"
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

/// What SDL has to know before its video subsystem starts: call it before `sdl2::init()`.
///
/// SDL 3.4's KMSDRM driver has two page-flip paths. The atomic one, its default, sets up a DRM
/// plane only for the display of the first window made, and dereferences the missing plane of
/// every other window when that window's surfaces are swapped or torn down: the bottom
/// screen's window crashed inkdot at its first present, and again at exit (SDL 3.4.18,
/// `add_plane_property` on a null plane from `drm_atomic_set_plane_props`). The legacy path
/// drives both displays, so this picks it, as DSlot does for its own two windows. An
/// `SDL_KMSDRM_ATOMIC` in the environment wins over this setting, for trying a fixed SDL.
pub fn prepare_sdl() {
    sdl2::hint::set("SDL_KMSDRM_ATOMIC", "0");
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
        let (top, bottom, shared_ui, desktop) = match placement {
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
                    None,
                    false,
                )
            }
            Some(p) => {
                let (t, b) = (p.region(Screen::Top).rect, p.region(Screen::Bottom).rect);
                let (x0, y0) = (t.x.min(b.x), t.y.min(b.y));
                let x1 = (t.x + t.w as i32).max(b.x + b.w as i32);
                let y1 = (t.y + t.h as i32).max(b.y + b.h as i32);
                let union = screens::Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32);
                let window = window(union)?;
                let (dw, dh) = window.drawable_size();
                let (sx, sy) = (dw as f32 / union.w as f32, dh as f32 / union.h as f32);
                // GL counts rows from the bottom of the window.
                let place = |r: screens::Rect| {
                    let (x, y) = (r.x - union.x, r.y - union.y);
                    let (w, h) = ((r.w as f32 * sx) as i32, (r.h as f32 * sy) as i32);
                    (
                        (x as f32 * sx) as i32,
                        dh as i32 - (y as f32 * sy) as i32 - h,
                        w,
                        h,
                    )
                };
                (
                    Target {
                        window,
                        viewport: place(t),
                    },
                    None,
                    Some(place(b)),
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
                    Some((0, 0, dw as i32, dh as i32 / 2)),
                    true,
                )
            }
        };
        let context = top.window.gl_create_context()?;
        // The bottom screen's swaps don't wait for its refresh: it changes rarely, and a wait
        // there held up the game's next frame (a 48 ms stall each time the clock changed).
        if let Some(b) = &bottom {
            b.window.gl_make_current(&context)?;
            let _ = video.gl_set_swap_interval(SwapInterval::Immediate);
        }
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
        let frames = [
            texture(&gl, 160, 144)?,
            texture(&gl, 160, 144)?,
            texture(&gl, 160, 144)?,
        ];
        let ui = texture(&gl, screens::WIDTH as i32, screens::HEIGHT as i32)?;
        let overlay = texture(&gl, screens::WIDTH as i32, screens::HEIGHT as i32)?;
        let mask = texture(&gl, 1, 1)?;
        Ok(Video {
            gl,
            _context: context,
            top,
            bottom,
            shared_ui,
            desktop,
            game,
            plain,
            quad,
            frames,
            newest: 0,
            ui,
            ui_ready: false,
            overlay,
            overlay_on: false,
            mask,
            mask_for: None,
            gain: 1.0,
            last_swap: std::time::Duration::ZERO,
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
        self.newest = (self.newest + 1) % 3;
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

    /// Puts a picture over the game on the top screen (1024×768, premultiplied RGBA), or
    /// takes it away.
    pub fn set_overlay(&mut self, rgba: Option<&[u8]>) {
        self.overlay_on = rgba.is_some();
        if let Some(rgba) = rgba {
            self.upload(self.overlay, rgba);
        }
    }

    /// Uploads a rectangle of the top screen's overlay (`rgba` is the whole screen, as for
    /// [`Video::set_overlay`]), the rest as it was: a challenge's ring moving on.
    pub fn set_overlay_part(&mut self, rgba: &[u8], (x, y, w, h): (usize, usize, usize, usize)) {
        let (sw, sh) = (screens::WIDTH as usize, screens::HEIGHT as usize);
        if !self.overlay_on
            || w == 0
            || h == 0
            || x + w > sw
            || y + h > sh
            || rgba.len() < sw * sh * 4
        {
            self.set_overlay(Some(rgba));
            return;
        }
        let part: Vec<u8> = (y..y + h)
            .flat_map(|row| &rgba[(row * sw + x) * 4..(row * sw + x + w) * 4])
            .copied()
            .collect();
        let gl = &self.gl;
        // SAFETY: as above; `part` is `w` x `h` pixels inside the texture.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.overlay));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x as i32,
                y as i32,
                w as i32,
                h as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(&part),
            );
        }
    }

    /// Uploads the bottom screen's picture (1024×768 RGBA).
    pub fn upload_ui(&mut self, rgba: &[u8]) {
        self.upload(self.ui, rgba);
        self.ui_ready = true;
    }

    fn upload(&self, texture: glow::Texture, rgba: &[u8]) {
        let gl = &self.gl;
        // SAFETY: as above; `rgba` is the full screen.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
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
        let rgb = |c: [u8; 3]| c.map(|v| v as f32 / 255.0);
        let surround = rgb(look.surround);
        // The picture's size in window pixels, and a Game Boy pixel's.
        let scale = self.top.viewport.2 as f32 / screens::WIDTH as f32;
        let (w, h) = (
            (GAME_W as f32 * scale).round() as i32,
            (GAME_H as f32 * scale).round() as i32,
        );
        let cell = 5.0 * scale;
        self.prepare_mask(look.pattern, w, h, cell);
        let gl = &self.gl;
        // SAFETY: GL calls on the current context with this module's objects.
        unsafe {
            if self.shared_ui.is_some() {
                let (w, h) = self.top.window.drawable_size();
                gl.viewport(0, 0, w as i32, h as i32);
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
            gl.bind_texture(glow::TEXTURE_2D, Some(self.frames[(self.newest + 2) % 3]));
            gl.active_texture(glow::TEXTURE2);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.mask));
            let u = |n: &str| gl.get_uniform_location(self.game, n);
            gl.uniform_1_i32(u("now").as_ref(), 0);
            gl.uniform_1_i32(u("before").as_ref(), 1);
            gl.uniform_1_i32(u("mask").as_ref(), 2);
            let s = look.shading;
            gl.uniform_1_f32(u("persistence").as_ref(), s.persistence);
            gl.uniform_1_f32(u("gain").as_ref(), self.gain);
            let p = rgb(s.paper).map(|v| v * v);
            gl.uniform_3_f32(u("paper").as_ref(), p[0], p[1], p[2]);
            gl.uniform_1_f32(u("shadow").as_ref(), s.shadow);
            gl.uniform_1_i32(u("shadow_step").as_ref(), lcd::shadow_step(cell));
            gl.uniform_2_i32(u("size").as_ref(), w, h);
            let v = self.top.viewport;
            let bottom = screens::HEIGHT as i32 - GAME_Y - GAME_H;
            gl.uniform_2_f32(
                u("origin").as_ref(),
                v.0 as f32 + GAME_X as f32 * scale,
                v.1 as f32 + bottom as f32 * scale,
            );
            gl.uniform_1_f32(u("scale").as_ref(), scale);
        }
        self.draw_quad(self.game, Self::clip((GAME_X, GAME_Y, GAME_W, GAME_H)));
        if self.overlay_on {
            let gl = &self.gl;
            // SAFETY: GL calls on the current context with this module's objects.
            unsafe {
                gl.enable(glow::BLEND);
                gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                gl.use_program(Some(self.plain));
                gl.active_texture(glow::TEXTURE0);
                gl.bind_texture(glow::TEXTURE_2D, Some(self.overlay));
                gl.uniform_1_i32(gl.get_uniform_location(self.plain, "image").as_ref(), 0);
            }
            self.draw_quad(self.plain, [-1.0, -1.0, 2.0, 2.0]);
            // SAFETY: as above.
            unsafe { self.gl.disable(glow::BLEND) };
        }
        let shot = capture.then(|| self.read_back(self.top.viewport));
        if let Some(v) = self.shared_ui {
            // The bottom screen is in the same window.
            self.draw_ui_at(v);
        }
        let t = std::time::Instant::now();
        self.top.window.gl_swap_window();
        self.last_swap = t.elapsed();
        shot
    }

    /// Builds the screen texture's mask for this pattern and picture size, when it isn't already
    /// (once per change of texture or window size: about 2 ms for 800×720 on a computer).
    fn prepare_mask(&mut self, pattern: Pattern, w: i32, h: i32, cell: f32) {
        if self.mask_for == Some((pattern, w, h)) {
            return;
        }
        let mask = pattern.build(w as usize, h as usize, cell);
        self.gain = mask.gain;
        self.mask_for = Some((pattern, w, h));
        let gl = &self.gl;
        // SAFETY: GL calls on the current context; `texels` is w*h*4 bytes.
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.mask));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(&mask.texels),
            );
        }
    }

    /// How long the last swap of the top screen took (for INKDOT_DEBUG=stats).
    pub fn last_swap(&self) -> std::time::Duration {
        self.last_swap
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
