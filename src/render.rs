//! 240×240 circle gauge renderer (RGB565), ported from mon64/`_ref`.
//!
//! Renders in horizontal bands to keep RAM small on ESP32-C3.

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_10X20, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::*,
    text::{Baseline, Text},
};
use libm::{cosf, sinf, sqrtf};

pub const SIZE: usize = 240;
pub const BAND_HEIGHT: usize = 40;
const CENTER: i32 = SIZE as i32 / 2;

#[derive(Clone, Copy)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_rgb565(self) -> u16 {
        rgb565(self.r, self.g, self.b)
    }
}

const BG: Rgba = Rgba::new(11, 14, 18, 255);
const GRID: Rgba = Rgba::new(22, 26, 32, 255);
const CENTER_GRID: Rgba = Rgba::new(38, 43, 51, 255);
const TRACK: Rgba = Rgba::new(24, 28, 35, 255);
const LABEL: Rgba = Rgba::new(210, 215, 220, 255);
const MUTED: Rgba = Rgba::new(136, 140, 148, 255);
const ERROR: Rgba = Rgba::new(255, 59, 48, 255);

const LOAD_BLUE: Rgba = Rgba::new(0x33, 0x88, 0xff, 255);
const LOAD_GREEN: Rgba = Rgba::new(0x44, 0xcc, 0x66, 255);
const LOAD_ORANGE: Rgba = Rgba::new(0xff, 0xaa, 0x33, 255);
const LOAD_RED: Rgba = Rgba::new(0xff, 0x44, 0x44, 255);

/// One horizontal band of the 240×240 framebuffer.
pub struct BandBuffer {
    pub y0: i32,
    pub height: usize,
    pub pixels: [u16; SIZE * BAND_HEIGHT],
}

impl BandBuffer {
    pub fn new() -> Self {
        Self {
            y0: 0,
            height: BAND_HEIGHT,
            pixels: [0; SIZE * BAND_HEIGHT],
        }
    }

    pub fn prepare(&mut self, y0: i32, height: usize) {
        self.y0 = y0;
        self.height = height.min(BAND_HEIGHT);
        let c = BG.to_rgb565();
        let n = SIZE * self.height;
        self.pixels[..n].fill(c);
    }

    fn set_rgba(&mut self, x: i32, y: i32, col: Rgba) {
        if x < 0 || x >= SIZE as i32 {
            return;
        }
        let ly = y - self.y0;
        if ly < 0 || ly >= self.height as i32 {
            return;
        }
        let i = ly as usize * SIZE + x as usize;
        if col.a == 255 {
            self.pixels[i] = col.to_rgb565();
            return;
        }
        if col.a == 0 {
            return;
        }
        let dst = from_rgb565(self.pixels[i]);
        self.pixels[i] = blend(dst, col).to_rgb565();
    }

    pub fn row_slice(&self) -> &[u16] {
        &self.pixels[..SIZE * self.height]
    }
}

impl Default for BandBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl DrawTarget for BandBuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            let x = coord.x;
            let y = coord.y;
            let ly = y - self.y0;
            if (0..SIZE as i32).contains(&x) && ly >= 0 && ly < self.height as i32 {
                let i = ly as usize * SIZE + x as usize;
                self.pixels[i] = color.into_storage();
            }
        }
        Ok(())
    }
}

impl OriginDimensions for BandBuffer {
    fn size(&self) -> Size {
        Size::new(SIZE as u32, SIZE as u32)
    }
}

/// Callback receives each filled band ready to flush to the LCD.
pub fn render_gauge_bands(
    band: &mut BandBuffer,
    cpu: Option<f32>,
    mem: Option<f32>,
    hostname: &str,
    reachable: bool,
    mut flush: impl FnMut(&BandBuffer),
) {
    let (cpu_val, cpu_ok) = match (reachable, cpu) {
        (false, _) => (0.0, false),
        (true, Some(v)) => (clamp_percent(v), true),
        (true, None) => (0.0, false),
    };
    let (mem_val, mem_ok) = match (reachable, mem) {
        (false, _) => (0.0, false),
        (true, Some(v)) => (clamp_percent(v), true),
        (true, None) => (0.0, false),
    };

    let mut cpu_color = level_color(cpu_val);
    let mut mem_color = level_color(mem_val);
    if !cpu_ok {
        cpu_color = MUTED;
    }
    if !mem_ok {
        mem_color = MUTED;
    }

    let (host_text, host_col) = if !reachable {
        ("DOWN", ERROR)
    } else {
        (hostname, LABEL)
    };

    let cpu_text = format_percent(cpu_val, cpu_ok);
    let mem_text = format_percent(mem_val, mem_ok);

    let mut y0 = 0i32;
    while y0 < SIZE as i32 {
        let height = ((SIZE as i32) - y0).min(BAND_HEIGHT as i32) as usize;
        band.prepare(y0, height);

        draw_grid(band);
        draw_gauge_arc(band, cpu_val, cpu_color, deg(175.0), deg(5.0));
        draw_gauge_arc(band, mem_val, mem_color, deg(185.0), deg(355.0));

        draw_label(band, "CPU", cpu_color, CENTER, 46, false);
        draw_label(band, &cpu_text, cpu_color, CENTER, 78, true);
        draw_label(band, host_text, host_col, CENTER, 120, false);
        draw_label(band, &mem_text, mem_color, CENTER, 162, true);
        draw_label(band, "MEM", mem_color, CENTER, 194, false);

        flush(band);
        y0 += height as i32;
    }
}

fn format_percent(v: f32, ok: bool) -> heapless::String<8> {
    let mut s = heapless::String::new();
    if !ok {
        let _ = s.push_str("n/a");
        return s;
    }
    let n = (v + 0.5) as i32;
    let _ = write_int(&mut s, n);
    let _ = s.push('%');
    s
}

fn write_int(s: &mut heapless::String<8>, mut n: i32) -> Result<(), ()> {
    if n < 0 {
        s.push('-').map_err(|_| ())?;
        n = -n;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    if n == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while n > 0 {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            i += 1;
        }
    }
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char).map_err(|_| ())?;
    }
    Ok(())
}

fn draw_label(fb: &mut BandBuffer, text: &str, col: Rgba, cx: i32, cy: i32, big: bool) {
    let color = Rgb565::new(col.r >> 3, col.g >> 2, col.b >> 3);
    if big {
        let style = MonoTextStyle::new(&FONT_10X20, color);
        let width = text.len() as i32 * 10;
        let x = cx - width / 2;
        let y = cy - 10;
        let _ = Text::with_baseline(text, Point::new(x, y), style, Baseline::Top).draw(fb);
    } else {
        let style = MonoTextStyle::new(&FONT_6X10, color);
        let width = text.len() as i32 * 6;
        let x = cx - width / 2;
        let y = cy - 5;
        let _ = Text::with_baseline(text, Point::new(x, y), style, Baseline::Top).draw(fb);
    }
}

fn level_color(percent: f32) -> Rgba {
    let percent = clamp_percent(percent);
    let stops: [(f32, Rgba); 4] = [
        (0.0, LOAD_BLUE),
        (33.0, LOAD_GREEN),
        (66.0, LOAD_ORANGE),
        (100.0, LOAD_RED),
    ];
    for i in 1..stops.len() {
        if percent <= stops[i].0 {
            let span = stops[i].0 - stops[i - 1].0;
            let t = (percent - stops[i - 1].0) / span;
            return lerp(stops[i - 1].1, stops[i].1, t);
        }
    }
    LOAD_RED
}

fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba {
        r: (a.r as f32 + (b.r as f32 - a.r as f32) * t + 0.5) as u8,
        g: (a.g as f32 + (b.g as f32 - a.g as f32) * t + 0.5) as u8,
        b: (a.b as f32 + (b.b as f32 - a.b as f32) * t + 0.5) as u8,
        a: 255,
    }
}

fn clamp_percent(v: f32) -> f32 {
    v.clamp(0.0, 100.0)
}

fn deg(d: f32) -> f32 {
    d * core::f32::consts::PI / 180.0
}

fn draw_grid(fb: &mut BandBuffer) {
    let step = 20;
    for x in (0..SIZE as i32).step_by(step) {
        draw_vline(fb, x, 0, SIZE as i32 - 1, GRID);
    }
    for y in (0..SIZE as i32).step_by(step) {
        draw_hline(fb, 0, SIZE as i32 - 1, y, GRID);
    }
    draw_vline(fb, CENTER, 0, SIZE as i32 - 1, CENTER_GRID);
    draw_hline(fb, 0, SIZE as i32 - 1, CENTER, CENTER_GRID);

    let mut x = CENTER - 110;
    while x <= CENTER + 110 {
        let tick_len = if (x - CENTER) % 20 == 0 { 2 } else { 1 };
        draw_vline(fb, x, CENTER - tick_len, CENTER + tick_len, CENTER_GRID);
        x += 5;
    }
}

fn draw_gauge_arc(fb: &mut BandBuffer, value: f32, accent: Rgba, start: f32, end: f32) {
    const RADIUS: f32 = 102.0;
    const TRACK_W: f32 = 4.0;
    const ACTIVE_W: f32 = 7.0;

    draw_arc(
        fb,
        CENTER as f32,
        CENTER as f32,
        RADIUS,
        TRACK_W,
        start,
        end,
        TRACK,
    );

    let t = clamp_percent(value) / 100.0;
    let value_angle = start + (end - start) * t;
    draw_arc(
        fb,
        CENTER as f32,
        CENTER as f32,
        RADIUS,
        ACTIVE_W,
        start,
        value_angle,
        accent,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_arc(
    fb: &mut BandBuffer,
    cx: f32,
    cy: f32,
    radius: f32,
    width: f32,
    start: f32,
    end: f32,
    col: Rgba,
) {
    let span = libm::fabsf(end - start);
    let steps = ((radius * span) as i32).max(16);
    let r = ((width / 2.0) as i32).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let angle = start + t * (end - start);
        let x = (cx + radius * cosf(angle)) as i32;
        let y = (cy - radius * sinf(angle)) as i32;
        fill_circle(fb, x, y, r, col);
    }
}

fn fill_circle(fb: &mut BandBuffer, cx: i32, cy: i32, radius: i32, col: Rgba) {
    let r_f = radius as f32;
    let y_lo = (cy - radius - 1).max(fb.y0);
    let y_hi = (cy + radius + 1).min(fb.y0 + fb.height as i32 - 1);
    for y in y_lo..=y_hi {
        for x in (cx - radius - 1)..=(cx + radius + 1) {
            if x < 0 || x >= SIZE as i32 {
                continue;
            }
            let dx = (x - cx) as f32;
            let dy = (y - cy) as f32;
            let dist = sqrtf(dx * dx + dy * dy);
            if dist >= r_f + 0.5 {
                continue;
            }
            if dist <= r_f - 0.5 {
                fb.set_rgba(x, y, col);
                continue;
            }
            let alpha = r_f + 0.5 - dist;
            let mut blend_col = col;
            blend_col.a = (col.a as f32 * alpha) as u8;
            fb.set_rgba(x, y, blend_col);
        }
    }
}

fn blend(dst: Rgba, src: Rgba) -> Rgba {
    if src.a == 255 {
        return src;
    }
    if src.a == 0 {
        return dst;
    }
    let sa = src.a as f32 / 255.0;
    let da = dst.a as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a == 0.0 {
        return Rgba::new(0, 0, 0, 0);
    }
    Rgba {
        r: ((src.r as f32 * sa + dst.r as f32 * da * (1.0 - sa)) / out_a) as u8,
        g: ((src.g as f32 * sa + dst.g as f32 * da * (1.0 - sa)) / out_a) as u8,
        b: ((src.b as f32 * sa + dst.b as f32 * da * (1.0 - sa)) / out_a) as u8,
        a: (out_a * 255.0) as u8,
    }
}

fn draw_vline(fb: &mut BandBuffer, x: i32, mut y0: i32, mut y1: i32, col: Rgba) {
    if y0 > y1 {
        core::mem::swap(&mut y0, &mut y1);
    }
    y0 = y0.max(fb.y0);
    y1 = y1.min(fb.y0 + fb.height as i32 - 1);
    for y in y0..=y1 {
        if in_circle(x, y) {
            fb.set_rgba(x, y, col);
        }
    }
}

fn draw_hline(fb: &mut BandBuffer, mut x0: i32, mut x1: i32, y: i32, col: Rgba) {
    if y < fb.y0 || y >= fb.y0 + fb.height as i32 {
        return;
    }
    if x0 > x1 {
        core::mem::swap(&mut x0, &mut x1);
    }
    for x in x0..=x1 {
        if in_circle(x, y) {
            fb.set_rgba(x, y, col);
        }
    }
}

fn in_circle(x: i32, y: i32) -> bool {
    let dx = (x - CENTER) as f32 + 0.5;
    let dy = (y - CENTER) as f32 + 0.5;
    dx * dx + dy * dy <= 120.0 * 120.0
}

fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | ((b as u16) >> 3)
}

fn from_rgb565(c: u16) -> Rgba {
    let r = ((c >> 11) & 0x1F) as u8;
    let g = ((c >> 5) & 0x3F) as u8;
    let b = (c & 0x1F) as u8;
    Rgba {
        r: (r << 3) | (r >> 2),
        g: (g << 2) | (g >> 4),
        b: (b << 3) | (b >> 2),
        a: 255,
    }
}
