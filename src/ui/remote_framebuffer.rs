//! Tiled RGBA framebuffer for remote desktop tabs (RDP / VNC).
//!
//! Uploads only dirty tiles to the GPU instead of re-uploading the full screen
//! on every small update (cursor blink, partial repaint, etc.).

use egui::{Color32, ColorImage, Painter, Rect, TextureHandle, TextureOptions, pos2, vec2};

const TILE_SIZE: usize = 256;

pub struct RemoteFramebuffer {
    width: u16,
    height: u16,
    pixels: Vec<u8>,
    tiles_x: usize,
    tiles_y: usize,
    textures: Vec<Option<TextureHandle>>,
    dirty_tiles: Vec<bool>,
    texture_prefix: String,
    tile_scratch: Vec<Color32>,
}

impl RemoteFramebuffer {
    pub fn new(texture_prefix: impl Into<String>) -> Self {
        Self {
            width: 0,
            height: 0,
            pixels: Vec::new(),
            tiles_x: 0,
            tiles_y: 0,
            textures: Vec::new(),
            dirty_tiles: Vec::new(),
            texture_prefix: texture_prefix.into(),
            tile_scratch: Vec::new(),
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn is_empty(&self) -> bool {
        self.pixels.is_empty()
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn resize(&mut self, width: u16, height: u16, ctx: &egui::Context) {
        let w = width as usize;
        let h = height as usize;
        self.width = width;
        self.height = height;
        self.pixels = vec![0u8; w * h * 4];
        self.tiles_x = w.div_ceil(TILE_SIZE);
        self.tiles_y = h.div_ceil(TILE_SIZE);
        let tile_count = self.tiles_x * self.tiles_y;
        self.textures = vec![None; tile_count];
        self.dirty_tiles = vec![true; tile_count];
        self.upload_dirty(ctx);
    }

    pub fn fill_rgb(&mut self, r: u8, g: u8, b: u8) {
        for px in self.pixels.chunks_exact_mut(4) {
            px[0] = r;
            px[1] = g;
            px[2] = b;
            px[3] = 255;
        }
    }

    pub fn mark_all_dirty(&mut self) {
        self.dirty_tiles.fill(true);
    }

    pub fn mark_dirty_rect(&mut self, x: u32, y: u32, w: u32, h: u32) {
        if self.tiles_x == 0 || self.tiles_y == 0 {
            return;
        }
        let x0 = x as usize;
        let y0 = y as usize;
        let x1 = (x0 + w as usize).min(self.width as usize);
        let y1 = (y0 + h as usize).min(self.height as usize);
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let tx0 = x0 / TILE_SIZE;
        let ty0 = y0 / TILE_SIZE;
        let tx1 = (x1.saturating_sub(1)) / TILE_SIZE;
        let ty1 = (y1.saturating_sub(1)) / TILE_SIZE;

        for ty in ty0..=ty1.min(self.tiles_y - 1) {
            for tx in tx0..=tx1.min(self.tiles_x - 1) {
                self.dirty_tiles[ty * self.tiles_x + tx] = true;
            }
        }
    }

    /// Copy an RGBA rectangle into the framebuffer (row memcpy).
    pub fn blit_rgba(&mut self, x: u16, y: u16, w: u16, h: u16, data: &[u8]) {
        let fb_w = self.width as usize;
        let fb_h = self.height as usize;
        let rw = w as usize;
        let rh = h as usize;
        let rx = x as usize;
        let ry = y as usize;
        if self.pixels.len() != fb_w * fb_h * 4 || rw == 0 || rh == 0 {
            return;
        }
        let copy_w = rw.min(fb_w.saturating_sub(rx));
        if copy_w == 0 {
            return;
        }
        let row_bytes = copy_w * 4;
        for row in 0..rh {
            let dst_y = ry + row;
            if dst_y >= fb_h {
                break;
            }
            let src = row * rw * 4;
            let dst = (dst_y * fb_w + rx) * 4;
            if src + row_bytes > data.len() || dst + row_bytes > self.pixels.len() {
                break;
            }
            self.pixels[dst..dst + row_bytes].copy_from_slice(&data[src..src + row_bytes]);
        }
        self.mark_dirty_rect(x as u32, y as u32, w as u32, h as u32);
    }

    /// Upload dirty tiles to the GPU. Returns true if anything was uploaded.
    pub fn upload_dirty(&mut self, ctx: &egui::Context) -> bool {
        if self.pixels.is_empty() {
            return false;
        }

        let mut uploaded = false;
        let fb_w = self.width as usize;
        let fb_h = self.height as usize;

        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let idx = ty * self.tiles_x + tx;
                if !self.dirty_tiles[idx] {
                    continue;
                }
                self.dirty_tiles[idx] = false;
                uploaded = true;

                let x0 = tx * TILE_SIZE;
                let y0 = ty * TILE_SIZE;
                let tw = TILE_SIZE.min(fb_w - x0);
                let th = TILE_SIZE.min(fb_h - y0);

                self.tile_scratch.clear();
                self.tile_scratch.resize(tw * th, Color32::BLACK);
                for row in 0..th {
                    let src = ((y0 + row) * fb_w + x0) * 4;
                    let dst = &mut self.tile_scratch[row * tw..row * tw + tw];
                    // Color32 is packed RGBA; our framebuffer is already RGBA with A=255.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            self.pixels[src..].as_ptr(),
                            dst.as_mut_ptr() as *mut u8,
                            tw * 4,
                        );
                    }
                }

                let image = ColorImage {
                    size: [tw, th],
                    source_size: vec2(tw as f32, th as f32),
                    pixels: self.tile_scratch.clone(),
                };
                let name = format!("{}_{}_{}", self.texture_prefix, tx, ty);
                match &mut self.textures[idx] {
                    Some(tex) => tex.set(image, TextureOptions::NEAREST),
                    None => {
                        self.textures[idx] =
                            Some(ctx.load_texture(name, image, TextureOptions::NEAREST));
                    }
                }
            }
        }

        uploaded
    }

    pub fn paint(&self, painter: &Painter, img_rect: Rect) {
        if self.pixels.is_empty() || img_rect.width() <= 0.0 || img_rect.height() <= 0.0 {
            return;
        }

        let fb_w = self.width as f32;
        let fb_h = self.height as f32;

        for ty in 0..self.tiles_y {
            for tx in 0..self.tiles_x {
                let idx = ty * self.tiles_x + tx;
                let Some(tex) = &self.textures[idx] else {
                    continue;
                };

                let x0 = (tx * TILE_SIZE) as f32;
                let y0 = (ty * TILE_SIZE) as f32;
                let tw = TILE_SIZE.min(self.width as usize - tx * TILE_SIZE) as f32;
                let th = TILE_SIZE.min(self.height as usize - ty * TILE_SIZE) as f32;

                let screen_rect = Rect::from_min_size(
                    pos2(
                        img_rect.min.x + (x0 / fb_w) * img_rect.width(),
                        img_rect.min.y + (y0 / fb_h) * img_rect.height(),
                    ),
                    vec2(
                        (tw / fb_w) * img_rect.width(),
                        (th / fb_h) * img_rect.height(),
                    ),
                );

                let uv_rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(tex.id(), screen_rect, uv_rect, Color32::WHITE);
            }
        }
    }
}
