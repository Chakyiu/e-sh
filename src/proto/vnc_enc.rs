//! RFB rectangle decoders (Raw, Hextile, Zlib, ZRLE, Tight).
//!
//! Output is tightly packed RGBA (4 bytes/pixel). Pixel format on the wire is
//! little-endian 32bpp / 24-bit true-colour (RGBX) or Tight/ZRLE CPIXEL (RGB).

use std::io::Read;

use anyhow::{Context, Result, anyhow, bail};
use flate2::{Decompress, FlushDecompress, Status};

pub const ENCODING_RAW: i32 = 0;
pub const ENCODING_COPYRECT: i32 = 1;
pub const ENCODING_HEXTILE: i32 = 5;
pub const ENCODING_ZLIB: i32 = 6;
pub const ENCODING_TIGHT: i32 = 7;
pub const ENCODING_ZRLE: i32 = 16;

const HEXTILE_RAW: u8 = 1;
const HEXTILE_BACKGROUND: u8 = 2;
const HEXTILE_FOREGROUND: u8 = 4;
const HEXTILE_ANY_SUBRECTS: u8 = 8;
const HEXTILE_COLOURED: u8 = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TightZlib {
    Unknown,
    Raw,
    Zlib,
}

pub struct TightStreams {
    streams: [Decompress; 4],
    zlib: [TightZlib; 4],
}

impl Default for TightStreams {
    fn default() -> Self {
        Self {
            streams: [
                Decompress::new(false),
                Decompress::new(false),
                Decompress::new(false),
                Decompress::new(false),
            ],
            zlib: [TightZlib::Unknown; 4],
        }
    }
}

impl TightStreams {
    fn reset(&mut self, mask: u8) {
        for i in 0..4 {
            if mask & (1 << i) != 0 {
                self.streams[i] = Decompress::new(false);
                self.zlib[i] = TightZlib::Unknown;
            }
        }
    }
}

pub fn rgbx_to_rgba_inplace(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

pub fn read_raw_rgba(stream: &mut impl Read, w: u16, h: u16) -> Result<Vec<u8>> {
    let n = w as usize * h as usize * 4;
    let mut data = vec![0u8; n];
    stream.read_exact(&mut data)?;
    rgbx_to_rgba_inplace(&mut data);
    Ok(data)
}

pub fn read_hextile(stream: &mut impl Read, w: u16, h: u16) -> Result<Vec<u8>> {
    let rw = w as usize;
    let rh = h as usize;
    let mut out = vec![0u8; rw * rh * 4];
    let mut bg = [0u8, 0, 0, 255];
    let mut fg = [255u8, 255, 255, 255];

    let mut ty = 0usize;
    while ty < rh {
        let th = 16.min(rh - ty);
        let mut tx = 0usize;
        while tx < rw {
            let tw = 16.min(rw - tx);
            let mut mask = [0u8; 1];
            stream.read_exact(&mut mask)?;
            let mask = mask[0];

            if mask & HEXTILE_RAW != 0 {
                let mut raw = vec![0u8; tw * th * 4];
                stream.read_exact(&mut raw)?;
                rgbx_to_rgba_inplace(&mut raw);
                blit_tile(&mut out, rw, tx, ty, tw, th, &raw);
            } else {
                if mask & HEXTILE_BACKGROUND != 0 {
                    bg = read_pixel32(stream)?;
                }
                if mask & HEXTILE_FOREGROUND != 0 {
                    fg = read_pixel32(stream)?;
                }
                fill_rect(&mut out, rw, tx, ty, tw, th, bg);
                if mask & HEXTILE_ANY_SUBRECTS != 0 {
                    let mut n = [0u8; 1];
                    stream.read_exact(&mut n)?;
                    let coloured = mask & HEXTILE_COLOURED != 0;
                    for _ in 0..n[0] {
                        let color = if coloured { read_pixel32(stream)? } else { fg };
                        let mut xywh = [0u8; 2];
                        stream.read_exact(&mut xywh)?;
                        let sx = (xywh[0] >> 4) as usize;
                        let sy = (xywh[0] & 0x0F) as usize;
                        let sw = ((xywh[1] >> 4) as usize) + 1;
                        let sh = ((xywh[1] & 0x0F) as usize) + 1;
                        fill_rect(&mut out, rw, tx + sx, ty + sy, sw, sh, color);
                    }
                }
            }
            tx += 16;
        }
        ty += 16;
    }
    Ok(out)
}

pub fn read_zlib(stream: &mut impl Read, w: u16, h: u16) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut comp = vec![0u8; len];
    stream.read_exact(&mut comp)?;
    let raw = inflate_zlib_or_raw(&comp, w as usize * h as usize * 4)?;
    let mut rgba = raw;
    if rgba.len() < w as usize * h as usize * 4 {
        bail!("zlib rectangle too small");
    }
    rgba.truncate(w as usize * h as usize * 4);
    rgbx_to_rgba_inplace(&mut rgba);
    Ok(rgba)
}

pub fn read_zrle(stream: &mut impl Read, w: u16, h: u16) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut comp = vec![0u8; len];
    stream.read_exact(&mut comp)?;
    // Worst case: raw CPIXEL tiles (3 bytes) plus subencoding bytes.
    let estimate = w as usize * h as usize * 4 + 4096;
    let data = inflate_zlib_or_raw(&comp, estimate)?;
    decode_zrle_tiles(&data, w, h)
}

pub fn read_tight(
    stream: &mut impl Read,
    w: u16,
    h: u16,
    tight: &mut TightStreams,
) -> Result<Vec<u8>> {
    let mut ctrl = [0u8; 1];
    stream.read_exact(&mut ctrl)?;
    let ctrl = ctrl[0];
    tight.reset(ctrl & 0x0F);

    let comp_type = ctrl >> 4;
    if comp_type == 0x08 {
        let px = read_tpixel(stream)?;
        let mut out = vec![0u8; w as usize * h as usize * 4];
        for chunk in out.chunks_exact_mut(4) {
            chunk.copy_from_slice(&px);
        }
        return Ok(out);
    }
    if comp_type == 0x09 {
        let len = read_compact_len(stream)?;
        let mut jpeg = vec![0u8; len];
        stream.read_exact(&mut jpeg)?;
        return decode_image(&jpeg, image::ImageFormat::Jpeg, w, h);
    }
    if comp_type == 0x0A {
        let len = read_compact_len(stream)?;
        let mut png = vec![0u8; len];
        stream.read_exact(&mut png)?;
        return decode_image(&png, image::ImageFormat::Png, w, h);
    }

    let stream_id = ((ctrl >> 4) & 0x03) as usize;
    let mut filter = 0u8;
    if ctrl & 0x40 != 0 {
        let mut f = [0u8; 1];
        stream.read_exact(&mut f)?;
        filter = f[0];
    }

    match filter {
        0 => tight_copy(stream, w, h, tight, stream_id),
        1 => tight_palette(stream, w, h, tight, stream_id),
        2 => tight_gradient(stream, w, h, tight, stream_id),
        other => bail!("unsupported Tight filter {other}"),
    }
}

fn tight_copy(
    stream: &mut impl Read,
    w: u16,
    h: u16,
    tight: &mut TightStreams,
    id: usize,
) -> Result<Vec<u8>> {
    let count = w as usize * h as usize;
    let data = read_tight_data(stream, count * 3, tight, id)?;
    rgba_from_tpixels(&data, count)
}

fn tight_palette(
    stream: &mut impl Read,
    w: u16,
    h: u16,
    tight: &mut TightStreams,
    id: usize,
) -> Result<Vec<u8>> {
    let mut n = [0u8; 1];
    stream.read_exact(&mut n)?;
    let colors = n[0] as usize + 1;
    let mut palette = vec![[0u8; 4]; colors];
    for slot in &mut palette {
        *slot = read_tpixel(stream)?;
    }
    let bits = if colors <= 2 { 1 } else { 8 };
    let row_bytes = if bits == 1 {
        w.div_ceil(8) as usize
    } else {
        w as usize
    };
    let raw_len = row_bytes * h as usize;
    let data = read_tight_data(stream, raw_len, tight, id)?;
    if data.len() < raw_len {
        bail!("Tight palette data short");
    }
    let mut out = vec![0u8; w as usize * h as usize * 4];
    if bits == 1 {
        for y in 0..h as usize {
            for x in 0..w as usize {
                let byte = data[y * row_bytes + x / 8];
                let bit = 7 - (x % 8);
                let idx = ((byte >> bit) & 1) as usize;
                let color = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
                let o = (y * w as usize + x) * 4;
                out[o..o + 4].copy_from_slice(&color);
            }
        }
    } else {
        for (i, &idx) in data[..w as usize * h as usize].iter().enumerate() {
            let color = palette
                .get(idx as usize)
                .copied()
                .unwrap_or([0, 0, 0, 255]);
            out[i * 4..i * 4 + 4].copy_from_slice(&color);
        }
    }
    Ok(out)
}

fn tight_gradient(
    stream: &mut impl Read,
    w: u16,
    h: u16,
    tight: &mut TightStreams,
    id: usize,
) -> Result<Vec<u8>> {
    let raw_len = w as usize * h as usize * 3;
    let data = read_tight_data(stream, raw_len, tight, id)?;
    if data.len() < raw_len {
        bail!("Tight gradient data short");
    }
    let rw = w as usize;
    let rh = h as usize;
    let mut rgb = vec![0u8; raw_len];
    for y in 0..rh {
        for x in 0..rw {
            for c in 0..3 {
                let left = if x > 0 {
                    rgb[(y * rw + x - 1) * 3 + c] as i16
                } else {
                    0
                };
                let up = if y > 0 {
                    rgb[((y - 1) * rw + x) * 3 + c] as i16
                } else {
                    0
                };
                let ul = if x > 0 && y > 0 {
                    rgb[((y - 1) * rw + x - 1) * 3 + c] as i16
                } else {
                    0
                };
                let pred = (left + up - ul).clamp(0, 255) as u8;
                let residual = data[(y * rw + x) * 3 + c];
                rgb[(y * rw + x) * 3 + c] = pred.wrapping_add(residual);
            }
        }
    }
    Ok(tpixels_to_rgba(&rgb, rw * rh))
}

fn read_tight_data(
    stream: &mut impl Read,
    uncompressed: usize,
    tight: &mut TightStreams,
    id: usize,
) -> Result<Vec<u8>> {
    if uncompressed < 12 {
        let mut raw = vec![0u8; uncompressed];
        stream.read_exact(&mut raw)?;
        return Ok(raw);
    }
    let len = read_compact_len(stream)?;
    if len == 0 {
        bail!("Tight compressed length is 0");
    }
    let mut comp = vec![0u8; len];
    stream.read_exact(&mut comp)?;
    inflate_tight(tight, id, &comp, uncompressed)
}

fn inflate_tight(
    tight: &mut TightStreams,
    id: usize,
    comp: &[u8],
    hint: usize,
) -> Result<Vec<u8>> {
    match tight.zlib[id] {
        TightZlib::Raw => inflate_into(&mut tight.streams[id], comp, hint)
            .with_context(|| "Tight raw deflate"),
        TightZlib::Zlib => inflate_into(&mut tight.streams[id], comp, hint)
            .with_context(|| "Tight zlib"),
        TightZlib::Unknown => {
            let mut raw = Decompress::new(false);
            if let Ok(v) = inflate_into(&mut raw, comp, hint) {
                if tight_output_plausible(v.len(), hint) {
                    tight.streams[id] = raw;
                    tight.zlib[id] = TightZlib::Raw;
                    return Ok(v);
                }
            }
            let mut zlib = Decompress::new(true);
            let v = inflate_into(&mut zlib, comp, hint).with_context(|| {
                "Tight inflate failed as raw Deflate and as zlib"
            })?;
            tight.streams[id] = zlib;
            tight.zlib[id] = TightZlib::Zlib;
            Ok(v)
        }
    }
}

fn rgba_from_tpixels(data: &[u8], count: usize) -> Result<Vec<u8>> {
    if data.len() >= count * 4 && data.len() != count * 3 {
        let mut rgba = data[..count * 4].to_vec();
        rgbx_to_rgba_inplace(&mut rgba);
        return Ok(rgba);
    }
    if data.len() >= count * 3 {
        return Ok(tpixels_to_rgba(&data[..count * 3], count));
    }
    bail!(
        "Tight pixel data short: got {} bytes for {count} pixels",
        data.len()
    );
}

fn looks_like_zlib(data: &[u8]) -> bool {
    if data.len() < 2 {
        return false;
    }
    let cmf = data[0];
    let flg = data[1];
    (cmf & 0x0F) == 8 && (((cmf as u16) << 8) | flg as u16) % 31 == 0
}

fn read_compact_len(stream: &mut impl Read) -> Result<usize> {
    let mut b = [0u8; 1];
    stream.read_exact(&mut b)?;
    let mut len = (b[0] & 0x7F) as usize;
    if b[0] & 0x80 != 0 {
        stream.read_exact(&mut b)?;
        len |= ((b[0] & 0x7F) as usize) << 7;
        if b[0] & 0x80 != 0 {
            stream.read_exact(&mut b)?;
            len |= (b[0] as usize) << 14;
        }
    }
    Ok(len)
}

fn decode_zrle_tiles(data: &[u8], w: u16, h: u16) -> Result<Vec<u8>> {
    let rw = w as usize;
    let rh = h as usize;
    let mut out = vec![0u8; rw * rh * 4];
    let mut cur = data;
    let mut ty = 0usize;
    while ty < rh {
        let th = 64.min(rh - ty);
        let mut tx = 0usize;
        while tx < rw {
            let tw = 64.min(rw - tx);
            if cur.is_empty() {
                bail!("ZRLE truncated");
            }
            let sub = cur[0];
            cur = &cur[1..];
            match sub {
                0 => {
                    let n = tw * th * 3;
                    if cur.len() < n {
                        bail!("ZRLE raw short");
                    }
                    blit_cpixels(&mut out, rw, tx, ty, tw, th, &cur[..n]);
                    cur = &cur[n..];
                }
                1 => {
                    if cur.len() < 3 {
                        bail!("ZRLE solid short");
                    }
                    let px = [cur[0], cur[1], cur[2], 255];
                    fill_rect(&mut out, rw, tx, ty, tw, th, px);
                    cur = &cur[3..];
                }
                2..=16 => {
                    let pal_len = sub as usize;
                    let pal_bytes = pal_len * 3;
                    if cur.len() < pal_bytes {
                        bail!("ZRLE palette short");
                    }
                    let mut palette = vec![[0u8; 4]; pal_len];
                    for (i, slot) in palette.iter_mut().enumerate() {
                        let o = i * 3;
                        *slot = [cur[o], cur[o + 1], cur[o + 2], 255];
                    }
                    cur = &cur[pal_bytes..];
                    let bits = if pal_len <= 2 {
                        1
                    } else if pal_len <= 4 {
                        2
                    } else {
                        4
                    };
                    let row_bytes = (tw * bits).div_ceil(8);
                    let packed = row_bytes * th;
                    if cur.len() < packed {
                        bail!("ZRLE packed short");
                    }
                    unpack_palette(
                        &mut out, rw, tx, ty, tw, th, bits, &palette, &cur[..packed],
                    );
                    cur = &cur[packed..];
                }
                128 => {
                    cur = zrle_plain_rle(&mut out, rw, tx, ty, tw, th, cur)?;
                }
                130..=255 => {
                    let pal_len = (sub - 128) as usize;
                    let pal_bytes = pal_len * 3;
                    if cur.len() < pal_bytes {
                        bail!("ZRLE palette-RLE short");
                    }
                    let mut palette = vec![[0u8; 4]; pal_len];
                    for (i, slot) in palette.iter_mut().enumerate() {
                        let o = i * 3;
                        *slot = [cur[o], cur[o + 1], cur[o + 2], 255];
                    }
                    cur = &cur[pal_bytes..];
                    cur = zrle_palette_rle(&mut out, rw, tx, ty, tw, th, &palette, cur)?;
                }
                other => bail!("unsupported ZRLE subencoding {other}"),
            }
            tx += 64;
        }
        ty += 64;
    }
    Ok(out)
}

fn zrle_plain_rle<'a>(
    out: &mut [u8],
    stride: usize,
    tx: usize,
    ty: usize,
    tw: usize,
    th: usize,
    mut cur: &'a [u8],
) -> Result<&'a [u8]> {
    let mut remaining = tw * th;
    let mut i = 0usize;
    while remaining > 0 {
        if cur.len() < 3 {
            bail!("ZRLE RLE pixel short");
        }
        let px = [cur[0], cur[1], cur[2], 255];
        cur = &cur[3..];
        let (run, rest) = read_rle_len(cur, remaining)?;
        cur = rest;
        for _ in 0..run {
            let x = tx + (i % tw);
            let y = ty + (i / tw);
            put_px(out, stride, x, y, px);
            i += 1;
        }
        remaining -= run;
    }
    Ok(cur)
}

fn zrle_palette_rle<'a>(
    out: &mut [u8],
    stride: usize,
    tx: usize,
    ty: usize,
    tw: usize,
    th: usize,
    palette: &[[u8; 4]],
    mut cur: &'a [u8],
) -> Result<&'a [u8]> {
    let mut remaining = tw * th;
    let mut i = 0usize;
    while remaining > 0 {
        if cur.is_empty() {
            bail!("ZRLE palette-RLE short");
        }
        let b = cur[0];
        cur = &cur[1..];
        if b & 0x80 != 0 {
            let idx = (b & 0x7F) as usize;
            let px = *palette.get(idx).ok_or_else(|| anyhow!("ZRLE palette idx"))?;
            let (run, rest) = read_rle_len(cur, remaining)?;
            cur = rest;
            for _ in 0..run {
                let x = tx + (i % tw);
                let y = ty + (i / tw);
                put_px(out, stride, x, y, px);
                i += 1;
            }
            remaining -= run;
        } else {
            let px = *palette
                .get(b as usize)
                .ok_or_else(|| anyhow!("ZRLE palette idx"))?;
            let x = tx + (i % tw);
            let y = ty + (i / tw);
            put_px(out, stride, x, y, px);
            i += 1;
            remaining -= 1;
        }
    }
    Ok(cur)
}

fn read_rle_len(mut cur: &[u8], remaining: usize) -> Result<(usize, &[u8])> {
    let mut run = 1usize;
    loop {
        if cur.is_empty() {
            bail!("ZRLE run length short");
        }
        let b = cur[0] as usize;
        cur = &cur[1..];
        run += b;
        if b != 255 {
            break;
        }
    }
    if run > remaining {
        bail!("ZRLE run overflows tile");
    }
    Ok((run, cur))
}

fn unpack_palette(
    out: &mut [u8],
    stride: usize,
    tx: usize,
    ty: usize,
    tw: usize,
    th: usize,
    bits: usize,
    palette: &[[u8; 4]],
    packed: &[u8],
) {
    let row_bytes = (tw * bits).div_ceil(8);
    for y in 0..th {
        for x in 0..tw {
            let bit_index = x * bits;
            let byte = packed[y * row_bytes + bit_index / 8];
            let shift = 8 - bits - (bit_index % 8);
            let idx = (byte >> shift) as usize & ((1 << bits) - 1);
            let px = palette.get(idx).copied().unwrap_or([0, 0, 0, 255]);
            put_px(out, stride, tx + x, ty + y, px);
        }
    }
}

fn decode_image(bytes: &[u8], format: image::ImageFormat, w: u16, h: u16) -> Result<Vec<u8>> {
    let img = image::load_from_memory_with_format(bytes, format)
        .with_context(|| format!("Tight {format:?}"))?;
    let rgba = img.to_rgba8();
    if rgba.width() != w as u32 || rgba.height() != h as u32 {
        let mut out = vec![0u8; w as usize * h as usize * 4];
        let cw = (rgba.width() as usize).min(w as usize);
        let ch = (rgba.height() as usize).min(h as usize);
        for y in 0..ch {
            let src = y * rgba.width() as usize * 4;
            let dst = y * w as usize * 4;
            out[dst..dst + cw * 4].copy_from_slice(&rgba.as_raw()[src..src + cw * 4]);
        }
        return Ok(out);
    }
    Ok(rgba.into_raw())
}

fn tight_output_plausible(len: usize, hint: usize) -> bool {
    if hint == 0 {
        return len == 0;
    }
    // 3-byte or 4-byte TPIXEL, or a bit of zlib slack.
    len == hint || len == hint * 4 / 3 || (len >= hint && len <= hint + hint / 2 + 32)
}

fn inflate_zlib(comp: &[u8], hint: usize) -> Result<Vec<u8>> {
    let mut z = Decompress::new(true);
    inflate_into(&mut z, comp, hint).with_context(|| "ZRLE/Zlib encoding")
}

/// Standard ZRLE/Zlib is zlib-wrapped; a few servers send raw Deflate instead.
fn inflate_zlib_or_raw(comp: &[u8], hint: usize) -> Result<Vec<u8>> {
    match inflate_zlib(comp, hint) {
        Ok(v) => Ok(v),
        Err(zlib_err) => {
            let mut raw = Decompress::new(false);
            inflate_into(&mut raw, comp, hint).map_err(|_| zlib_err)
        }
    }
}

fn inflate_into(z: &mut Decompress, comp: &[u8], hint: usize) -> Result<Vec<u8>> {
    let mut out = vec![0u8; hint.max(64)];
    let mut written = 0usize;
    let mut in_off = 0usize;
    loop {
        if in_off > comp.len() {
            bail!("zlib inflate: input offset past buffer");
        }
        let before_in = z.total_in();
        let before_out = z.total_out();
        let status = z
            .decompress(&comp[in_off..], &mut out[written..], FlushDecompress::Sync)
            .map_err(|e| anyhow!("zlib inflate: {e}"))?;
        in_off += (z.total_in() - before_in) as usize;
        written += (z.total_out() - before_out) as usize;
        if status == Status::StreamEnd || in_off >= comp.len() {
            break;
        }
        if z.total_in() == before_in && z.total_out() == before_out {
            if written == out.len() {
                out.resize(out.len() * 2, 0);
                continue;
            }
            break;
        }
        if written == out.len() {
            out.resize(out.len() * 2, 0);
        }
    }
    out.truncate(written);
    Ok(out)
}

fn read_pixel32(stream: &mut impl Read) -> Result<[u8; 4]> {
    let mut p = [0u8; 4];
    stream.read_exact(&mut p)?;
    Ok([p[0], p[1], p[2], 255])
}

fn read_tpixel(stream: &mut impl Read) -> Result<[u8; 4]> {
    let mut p = [0u8; 3];
    stream.read_exact(&mut p)?;
    Ok([p[0], p[1], p[2], 255])
}

fn tpixels_to_rgba(rgb: &[u8], count: usize) -> Vec<u8> {
    let mut out = vec![0u8; count * 4];
    for i in 0..count {
        let s = i * 3;
        let d = i * 4;
        out[d] = rgb[s];
        out[d + 1] = rgb[s + 1];
        out[d + 2] = rgb[s + 2];
        out[d + 3] = 255;
    }
    out
}

fn blit_cpixels(out: &mut [u8], stride: usize, tx: usize, ty: usize, tw: usize, th: usize, rgb: &[u8]) {
    for y in 0..th {
        for x in 0..tw {
            let s = (y * tw + x) * 3;
            put_px(out, stride, tx + x, ty + y, [rgb[s], rgb[s + 1], rgb[s + 2], 255]);
        }
    }
}

fn blit_tile(out: &mut [u8], stride: usize, tx: usize, ty: usize, tw: usize, th: usize, rgba: &[u8]) {
    for y in 0..th {
        let src = y * tw * 4;
        let dst = ((ty + y) * stride + tx) * 4;
        let n = tw * 4;
        if dst + n <= out.len() && src + n <= rgba.len() {
            out[dst..dst + n].copy_from_slice(&rgba[src..src + n]);
        }
    }
}

fn fill_rect(out: &mut [u8], stride: usize, x: usize, y: usize, w: usize, h: usize, px: [u8; 4]) {
    for row in 0..h {
        for col in 0..w {
            put_px(out, stride, x + col, y + row, px);
        }
    }
}

fn put_px(out: &mut [u8], stride: usize, x: usize, y: usize, px: [u8; 4]) {
    let i = (y * stride + x) * 4;
    if i + 4 <= out.len() {
        out[i..i + 4].copy_from_slice(&px);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn rgbx_sets_alpha() {
        let mut buf = vec![1, 2, 3, 0, 4, 5, 6, 0];
        rgbx_to_rgba_inplace(&mut buf);
        assert_eq!(buf, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn zlib_roundtrip_raw() {
        let raw: Vec<u8> = (0..16).map(|i| if i % 4 == 3 { 0 } else { i as u8 }).collect();
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        let comp = enc.finish().unwrap();
        let mut msg = (comp.len() as u32).to_be_bytes().to_vec();
        msg.extend_from_slice(&comp);
        let out = read_zlib(&mut msg.as_slice(), 2, 2).unwrap();
        assert_eq!(out.len(), 16);
        assert_eq!(out[3], 255);
    }

    #[test]
    fn tight_raw_deflate_sync_flush_two_rects() {
        use flate2::{Compress, FlushCompress};
        let mut enc = Compress::new(Compression::default(), false);
        let mut deflate = |plain: &[u8]| {
            let mut out = vec![0u8; 128];
            let before = enc.total_out();
            enc.compress(plain, &mut out, FlushCompress::Sync).unwrap();
            let n = (enc.total_out() - before) as usize;
            out.truncate(n);
            out
        };
        let a = deflate(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        let b = deflate(&[9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2]);
        assert!(!looks_like_zlib(&a));
        let mut z = Decompress::new(false);
        let out_a = inflate_into(&mut z, &a, 12).unwrap();
        let out_b = inflate_into(&mut z, &b, 12).unwrap();
        assert_eq!(out_a, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(out_b, vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2]);
    }

    #[test]
    fn zlib_header_is_detected() {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&[1, 2, 3, 4]).unwrap();
        let comp = enc.finish().unwrap();
        assert!(looks_like_zlib(&comp));
    }
}
