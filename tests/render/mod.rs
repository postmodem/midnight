//! The reference renderer: the pixels a list of draws must give, by "Which
//! pixels a shape covers" and "Color" in the reference, in 32-bit
//! arithmetic. It states the rules apart from the runtime's shaders, so
//! that each is a check on the other.

/// The shape of a draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Circle,
    Line,
    Triangle,
    Text,
}

impl Shape {
    fn named(name: &str) -> Option<(Shape, usize)> {
        Some(match name {
            "rect" => (Shape::Rect, 4),
            "circle" => (Shape::Circle, 3),
            "line" => (Shape::Line, 5),
            "triangle" => (Shape::Triangle, 6),
            "text" => (Shape::Text, 3),
            _ => return None,
        })
    }
}

/// One draw: its shape, its numbers in the order of the statement's
/// arguments (unused ones zero; for text `x`, `y` and `size`), its color,
/// and the text of a draw of text.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub shape: Shape,
    pub n: [f32; 6],
    pub color: [u8; 4],
    pub text: String,
}

/// A frame as `draws` lists it: the window's size in points, its
/// background and its draws in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub background: [u8; 4],
    pub draws: Vec<Draw>,
}

/// The four numbers of `Color(r, g, b, a)` at the start of `text`, and
/// what follows it.
fn color(text: &str) -> Option<([u8; 4], &str)> {
    let rest = text.strip_prefix("Color(")?;
    let (inside, rest) = rest.split_once(')')?;
    let bytes: Vec<u8> = inside
        .split(", ")
        .map(|byte| byte.parse().ok())
        .collect::<Option<_>>()?;
    Some((bytes.try_into().ok()?, rest))
}

/// The string written in Midnight's quoted form at the start of `text`, as
/// `to_s` writes a `String` inside a struct, and what follows it.
fn quoted(text: &str) -> Option<(String, &str)> {
    let mut bytes = Vec::new();
    let mut rest = text.strip_prefix('"')?.as_bytes();
    loop {
        match *rest.first()? {
            b'"' => break,
            b'\\' => {
                let (byte, skip) = match *rest.get(1)? {
                    b'n' => (b'\n', 2),
                    b't' => (b'\t', 2),
                    b'r' => (b'\r', 2),
                    b'0' => (0, 2),
                    b'e' => (0x1b, 2),
                    b'\\' => (b'\\', 2),
                    b'"' => (b'"', 2),
                    b'#' => (b'#', 2),
                    b'x' => {
                        let hex = std::str::from_utf8(rest.get(2..4)?).ok()?;
                        (u8::from_str_radix(hex, 16).ok()?, 4)
                    }
                    _ => return None,
                };
                bytes.push(byte);
                rest = &rest[skip..];
            }
            byte => {
                bytes.push(byte);
                rest = &rest[1..];
            }
        }
    }
    let after = &text[text.len() - rest.len() + 1..];
    Some((String::from_utf8(bytes).ok()?, after))
}

/// Reads the lines of one `draws` answer, with or without
/// `midnight: drive: ` before each.
pub fn parse(answer: &str) -> Result<Frame, String> {
    let mut lines = answer
        .lines()
        .map(|line| line.strip_prefix("midnight: drive: ").unwrap_or(line));
    let head = lines.next().ok_or("an empty answer")?;
    let size = head
        .strip_prefix("draws: ")
        .and_then(|rest| rest.split(", ").nth(1))
        .and_then(|size| size.split_once('x'))
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        .ok_or_else(|| format!("not the first line of a `draws` answer: {head}"))?;
    let line = lines.next().unwrap_or_default();
    let background = line
        .strip_prefix("  Window.background = ")
        .and_then(color)
        .filter(|(_, rest)| rest.is_empty())
        .map(|(bytes, _)| bytes)
        .ok_or_else(|| format!("not a background: {line}"))?;
    let mut draws = Vec::new();
    for line in lines {
        let draw = (|| {
            let rest = line.strip_prefix("  Draw.")?;
            let (name, rest) = rest.split_once('(')?;
            let (shape, count) = Shape::named(name)?;
            let mut n = [0.0f32; 6];
            let mut rest = rest;
            let mut text = String::new();
            for (k, number) in n.iter_mut().take(count).enumerate() {
                if shape == Shape::Text && k == 2 {
                    let (string, after) = quoted(rest)?;
                    text = string;
                    rest = after.strip_prefix(", ")?;
                }
                let (digits, after) = rest.split_once(", ")?;
                *number = digits.parse().ok()?;
                rest = after;
            }
            let (color, rest) = color(rest)?;
            (rest == ")").then_some(Draw {
                shape,
                n,
                color,
                text,
            })
        })();
        draws.push(draw.ok_or_else(|| format!("not a draw: {line}"))?);
    }
    Ok(Frame {
        width: size.0,
        height: size.1,
        background,
        draws,
    })
}

/// Whether a center exactly on an edge of direction `(dx, dy)`, with the
/// shape on its right, counts: a left edge, or a top edge, y down.
fn on_edge_counts(dx: f32, dy: f32) -> bool {
    dy < 0.0 || (dy == 0.0 && dx > 0.0)
}

/// The test of the edge from `a` to `b` at the center `c`, computed from
/// the edge's ends in one order whichever way it runs.
fn edge(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let b_first = b.0 < a.0 || (b.0 == a.0 && b.1 < a.1);
    let (l, h) = if b_first { (b, a) } else { (a, b) };
    let mut e = (h.0 - l.0) * (c.1 - l.1) - (h.1 - l.1) * (c.0 - l.0);
    if b_first {
        e = -e;
    }
    e > 0.0 || (e == 0.0 && on_edge_counts(b.0 - a.0, b.1 - a.1))
}

/// The font of `runtime/font.txt`: each glyph's twelve rows of six
/// pixels, bit `j` the column `j` from the left, and the glyph of each
/// code point it has. The replacement glyph is the last.
pub struct Font {
    pub glyphs: Vec<[u8; 12]>,
    pub codes: Vec<Option<u32>>,
}

impl Font {
    /// Reads a font file. Says what is wrong with one that does not have
    /// the form of `runtime/font.txt`.
    pub fn read(file: &str) -> Result<Font, String> {
        let mut glyphs = Vec::new();
        let mut codes = Vec::new();
        let mut lines = file.lines().peekable();
        while lines.peek().is_some_and(|line| line.starts_with("# ")) {
            lines.next();
        }
        while let Some(name) = lines.next() {
            let code = if name == "replacement" {
                None
            } else {
                let hex = name
                    .strip_prefix("U+")
                    .filter(|hex| {
                        hex.len() == 4
                            && hex
                                .bytes()
                                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_lowercase())
                    })
                    .ok_or_else(|| format!("not a glyph's name: {name}"))?;
                Some(u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?)
            };
            let mut rows = [0u8; 12];
            for row in &mut rows {
                let line = lines
                    .next()
                    .ok_or_else(|| format!("{name}: fewer than 12 rows"))?;
                if line.len() != 6 || !line.bytes().all(|b| b == b'#' || b == b'.') {
                    return Err(format!("{name}: not a row of 6 pixels: {line}"));
                }
                for (j, pixel) in line.bytes().enumerate() {
                    if pixel == b'#' {
                        *row |= 1 << j;
                    }
                }
            }
            glyphs.push(rows);
            codes.push(code);
            if code.is_none() {
                break;
            }
        }
        if let Some(line) = lines.next() {
            return Err(format!("a line after the replacement glyph: {line}"));
        }
        if codes.last() != Some(&None) {
            return Err("no replacement glyph last".to_string());
        }
        if !file.ends_with('\n') {
            return Err("the file does not end with a newline".to_string());
        }
        Ok(Font { glyphs, codes })
    }

    /// The glyph that a character shows.
    pub fn glyph(&self, code: u32) -> usize {
        self.codes
            .iter()
            .position(|known| *known == Some(code))
            .unwrap_or(self.glyphs.len() - 1)
    }
}

/// The built-in font.
pub fn font() -> &'static Font {
    static FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        Font::read(include_str!("../../runtime/font.txt")).expect("runtime/font.txt reads")
    })
}

/// A character of a draw of text that shows a glyph: the text's corner
/// and the side of a pixel of the font, `u`, the glyph, and the first
/// column and row of its cell, counted in pixels of the font from the
/// text's corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub x: f32,
    pub y: f32,
    pub u: f32,
    pub glyph: usize,
    pub column: u32,
    pub row: u32,
}

impl Cell {
    /// The bound before column `c` of the font's pixels.
    fn left(&self, c: u32) -> f32 {
        self.x + c as f32 * self.u
    }

    /// The bound before row `r` of the font's pixels.
    fn top(&self, r: u32) -> f32 {
        self.y + r as f32 * self.u
    }

    /// Whether the cell covers the pixel whose center is `(px, py)`: the
    /// center lies in the rectangle of one of its glyph's set pixels.
    pub fn covers(&self, px: f32, py: f32) -> bool {
        let rows = &font().glyphs[self.glyph];
        (0..12).any(|r| {
            self.top(self.row + r) <= py
                && py < self.top(self.row + r + 1)
                && (0..6).any(|j| {
                    rows[r as usize] & (1 << j) != 0
                        && self.left(self.column + j) <= px
                        && px < self.left(self.column + j + 1)
                })
        })
    }

    /// The cell's rectangle in points, `(x0, x1)` and `(y0, y1)`.
    fn bounds(&self) -> ((f32, f32), (f32, f32)) {
        (
            (self.left(self.column), self.left(self.column + 6)),
            (self.top(self.row), self.top(self.row + 12)),
        )
    }
}

/// The characters of a draw of text that show a glyph, in order: every
/// character but a space, U+00A0 and `"\n"`, which begins a line.
pub fn cells(draw: &Draw) -> Vec<Cell> {
    let [x, y, size, ..] = draw.n;
    if x.is_nan() || y.is_nan() || size.is_nan() || size <= 0.0 {
        return Vec::new();
    }
    let u = size / 12.0;
    let mut out = Vec::new();
    let (mut column, mut row) = (0u32, 0u32);
    for c in draw.text.chars() {
        if c == '\n' {
            column = 0;
            row += 12;
            continue;
        }
        if c != ' ' && c != '\u{a0}' {
            out.push(Cell {
                x,
                y,
                u,
                glyph: font().glyph(u32::from(c)),
                column,
                row,
            });
        }
        column += 6;
    }
    out
}

/// Whether `draw` covers the pixel whose center is `(px, py)` in points.
pub fn covers(draw: &Draw, px: f32, py: f32) -> bool {
    let n = draw.n;
    if n.iter().any(|number| number.is_nan()) {
        return false;
    }
    match draw.shape {
        Shape::Text => cells(draw).iter().any(|cell| cell.covers(px, py)),
        Shape::Rect => {
            let [x, y, width, height, ..] = n;
            width > 0.0 && height > 0.0 && x <= px && px < x + width && y <= py && py < y + height
        }
        Shape::Circle => {
            let [x, y, radius, ..] = n;
            let (dx, dy) = (px - x, py - y);
            radius > 0.0 && dx * dx + dy * dy < radius * radius
        }
        Shape::Line => {
            let [x1, y1, x2, y2, width, _] = n;
            if width <= 0.0 {
                return false;
            }
            let (dx, dy) = (x2 - x1, y2 - y1);
            let (qx, qy) = (px - x1, py - y1);
            let t = qx * dx + qy * dy;
            let s = qx * dy - qy * dx;
            let l = dx * dx + dy * dy;
            let lhs = (4.0 * s) * s;
            let rhs = (width * width) * l;
            let body = l > 0.0
                && 0.0 <= t
                && t <= l
                && (lhs < rhs
                    || (lhs == rhs
                        && ((s > 0.0 && on_edge_counts(dx, dy))
                            || (s < 0.0 && on_edge_counts(-dx, -dy)))));
            let h = width * 0.5;
            let (ex, ey) = (px - x2, py - y2);
            body || qx * qx + qy * qy < h * h || ex * ex + ey * ey < h * h
        }
        Shape::Triangle => {
            let p1 = (n[0], n[1]);
            let mut p2 = (n[2], n[3]);
            let mut p3 = (n[4], n[5]);
            let w = (p2.0 - p1.0) * (p3.1 - p1.1) - (p2.1 - p1.1) * (p3.0 - p1.0);
            if w == 0.0 {
                return false;
            }
            if w < 0.0 {
                std::mem::swap(&mut p2, &mut p3);
            }
            let c = (px, py);
            edge(p1, p2, c) && edge(p2, p3, c) && edge(p3, p1, c)
        }
    }
}

/// A frame's pixels: four bytes each, rows from the top, and for each
/// pixel the number of draws with an alpha from 1 to 254 that cover it
/// since the last draw with an alpha of 255 did.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub layers: Vec<u32>,
}

/// `color` of alpha `a` laid over the byte `under`, rounded to the nearest.
pub fn over(a: u8, color: u8, under: u8) -> u8 {
    let (a, c, d) = (u32::from(a), u32::from(color), u32::from(under));
    ((a * c + (255 - a) * d + 127) / 255) as u8
}

/// An upright rectangle in points, `(x0, x1)` and `(y0, y1)`, that holds
/// every point `draw` can cover; infinite where a number is.
fn bounds(draw: &Draw) -> ((f32, f32), (f32, f32)) {
    let n = draw.n;
    let span = |values: &[f32], pad: f32| {
        let low = values.iter().copied().fold(f32::INFINITY, f32::min);
        let high = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (low - pad, high + pad)
    };
    match draw.shape {
        Shape::Rect => ((n[0], n[0] + n[2]), (n[1], n[1] + n[3])),
        Shape::Circle => ((n[0] - n[2], n[0] + n[2]), (n[1] - n[2], n[1] + n[2])),
        Shape::Line => (span(&[n[0], n[2]], n[4]), span(&[n[1], n[3]], n[4])),
        Shape::Triangle => (
            span(&[n[0], n[2], n[4]], 0.0),
            span(&[n[1], n[3], n[5]], 0.0),
        ),
        Shape::Text => unreachable!("text is rendered by its cells"),
    }
}

/// The pixels, of `count` of `step` points each, whose centers may lie
/// from `low` to `high`, with two more on each side.
fn pixel_span(low: f32, high: f32, step: f32, count: u32) -> std::ops::Range<u32> {
    if low.is_nan() || high.is_nan() {
        return 0..0;
    }
    let first = ((low / step).floor() as i64)
        .saturating_sub(2)
        .clamp(0, i64::from(count));
    let last = ((high / step).ceil() as i64)
        .saturating_add(2)
        .clamp(0, i64::from(count));
    first as u32..last.max(first) as u32
}

/// Renders `frame` at `pixel_width` by `pixel_height` pixels.
pub fn render(frame: &Frame, pixel_width: u32, pixel_height: u32) -> Picture {
    let count = pixel_width as usize * pixel_height as usize;
    let [r, g, b, _] = frame.background;
    let mut rgba = [r, g, b, 255].repeat(count);
    let mut layers = vec![0u32; count];
    let step_x = frame.width as f32 / pixel_width as f32;
    let step_y = frame.height as f32 / pixel_height as f32;
    for draw in &frame.draws {
        let a = draw.color[3];
        if a == 0 {
            continue;
        }
        // A draw of text is the cells of its characters, which never
        // overlap; any other draw is its shape.
        let parts: Vec<(_, Option<Cell>)> = if draw.shape == Shape::Text {
            cells(draw)
                .into_iter()
                .map(|cell| (cell.bounds(), Some(cell)))
                .collect()
        } else {
            vec![(bounds(draw), None)]
        };
        for (((x0, x1), (y0, y1)), cell) in parts {
            let columns = pixel_span(x0, x1, step_x, pixel_width);
            let rows = pixel_span(y0, y1, step_y, pixel_height);
            for j in rows {
                let py = (j as f32 + 0.5) * step_y;
                for i in columns.clone() {
                    let px = (i as f32 + 0.5) * step_x;
                    let hit = match &cell {
                        Some(cell) => cell.covers(px, py),
                        None => covers(draw, px, py),
                    };
                    if !hit {
                        continue;
                    }
                    let at = j as usize * pixel_width as usize + i as usize;
                    let pixel = &mut rgba[at * 4..at * 4 + 4];
                    if a == 255 {
                        pixel[..3].copy_from_slice(&draw.color[..3]);
                        layers[at] = 0;
                    } else {
                        for (byte, color) in pixel.iter_mut().zip(draw.color).take(3) {
                            *byte = over(a, color, *byte);
                        }
                        layers[at] += 1;
                    }
                }
            }
        }
    }
    Picture {
        width: pixel_width,
        height: pixel_height,
        rgba,
        layers,
    }
}

/// Whether `rgba`, a frame from a device, agrees with `picture`: every
/// pixel opaque, and each of its three bytes within the pixel's number of
/// translucent layers of the picture's. Says where it does not.
pub fn agrees(picture: &Picture, rgba: &[u8]) -> Result<(), String> {
    if rgba.len() != picture.rgba.len() {
        return Err(format!(
            "{} bytes, not the {} of {}x{} pixels",
            rgba.len(),
            picture.rgba.len(),
            picture.width,
            picture.height
        ));
    }
    let mut wrong = 0usize;
    let mut first = None;
    for (at, layers) in picture.layers.iter().enumerate() {
        let got = &rgba[at * 4..at * 4 + 4];
        let want = &picture.rgba[at * 4..at * 4 + 4];
        let off = got[3] != 255 || (0..3).any(|k| u32::from(got[k].abs_diff(want[k])) > *layers);
        if off {
            wrong += 1;
            first.get_or_insert((at, got.to_vec(), want.to_vec(), *layers));
        }
    }
    match first {
        None => Ok(()),
        Some((at, got, want, layers)) => Err(format!(
            "{wrong} of {} pixels disagree; the first, column {} row {}, is {got:?} \
             where the reference renderer has {want:?} under {layers} translucent draws",
            picture.layers.len(),
            at % picture.width as usize,
            at / picture.width as usize
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: [u8; 4] = [255, 255, 255, 255];

    fn draw(shape: Shape, numbers: &[f32]) -> Draw {
        let mut n = [0.0; 6];
        n[..numbers.len()].copy_from_slice(numbers);
        Draw {
            shape,
            n,
            color: WHITE,
            text: String::new(),
        }
    }

    fn text(x: f32, y: f32, string: &str, size: f32) -> Draw {
        Draw {
            text: string.to_string(),
            ..draw(Shape::Text, &[x, y, size])
        }
    }

    /// The glyph of `c` as the pixels it sets, (column, row) from the
    /// cell's top left.
    fn glyph_pixels(c: char) -> Vec<(u32, u32)> {
        let rows = font().glyphs[font().glyph(u32::from(c))];
        (0..12u32)
            .flat_map(|r| (0..6u32).map(move |j| (j, r)))
            .filter(|&(j, r)| rows[r as usize] & (1 << j) != 0)
            .collect()
    }

    /// The pixels of the font of `draw` (set or not) whose rectangles hold
    /// the point `(px, py)`, as (cell, column, row) in pixels of the font.
    fn font_pixels_at(draw: &Draw, px: f32, py: f32) -> Vec<(usize, u32, u32)> {
        let mut found = Vec::new();
        for (k, cell) in cells(draw).iter().enumerate() {
            for r in cell.row..cell.row + 12 {
                for c in cell.column..cell.column + 6 {
                    if cell.left(c) <= px
                        && px < cell.left(c + 1)
                        && cell.top(r) <= py
                        && py < cell.top(r + 1)
                    {
                        found.push((k, c, r));
                    }
                }
            }
        }
        found
    }

    /// The pixels `draw` covers in a frame of `width` by `height` points
    /// at `pixel_width` by `pixel_height` pixels, as (column, row).
    fn covered(draw: &Draw, size: (u32, u32), pixels: (u32, u32)) -> Vec<(u32, u32)> {
        let step_x = size.0 as f32 / pixels.0 as f32;
        let step_y = size.1 as f32 / pixels.1 as f32;
        let mut out = Vec::new();
        for j in 0..pixels.1 {
            for i in 0..pixels.0 {
                let (px, py) = ((i as f32 + 0.5) * step_x, (j as f32 + 0.5) * step_y);
                if covers(draw, px, py) {
                    out.push((i, j));
                }
            }
        }
        out
    }

    fn at_one(draw: &Draw) -> Vec<(u32, u32)> {
        covered(draw, (128, 128), (128, 128))
    }

    fn block(columns: std::ops::Range<u32>, rows: std::ops::Range<u32>) -> Vec<(u32, u32)> {
        rows.flat_map(|j| columns.clone().map(move |i| (i, j)))
            .collect()
    }

    const SCALES: [(u32, u32); 4] = [(320, 200), (400, 250), (480, 300), (640, 400)];

    #[test]
    fn the_examples_of_the_reference() {
        let rect = draw(Shape::Rect, &[10.0, 20.0, 30.0, 5.0]);
        assert_eq!(at_one(&rect), block(10..40, 20..25));
        let line = draw(Shape::Line, &[0.0, 10.0, 100.0, 10.0, 1.0]);
        assert_eq!(at_one(&line), block(0..100, 9..10));
    }

    #[test]
    fn rectangles_side_by_side_neither_overlap_nor_leave_a_gap() {
        for pixels in SCALES {
            for (x, width) in [(10.0f32, 30.0f32), (10.5, 7.25), (0.1, 33.3)] {
                let first = draw(Shape::Rect, &[x, 20.0, width, 40.0]);
                let second = draw(Shape::Rect, &[x + width, 20.0, 50.0, 40.0]);
                let (a, b) = (
                    covered(&first, (320, 200), pixels),
                    covered(&second, (320, 200), pixels),
                );
                assert!(
                    a.iter().all(|pixel| !b.contains(pixel)),
                    "{pixels:?} {x} {width}"
                );
                let both = [a, b].concat();
                for row in 0..pixels.1 {
                    let columns: Vec<u32> =
                        both.iter().filter(|p| p.1 == row).map(|p| p.0).collect();
                    if let (Some(lo), Some(hi)) = (columns.iter().min(), columns.iter().max()) {
                        assert_eq!(hi - lo + 1, columns.len() as u32, "{pixels:?} {x} {width}");
                    }
                }
            }
        }
    }

    #[test]
    fn triangles_that_share_edges_cover_each_pixel_once() {
        // A fan around a center, corners on a circle, and a square split on
        // a diagonal that runs through pixel centers.
        let center = (64.3f32, 63.7f32);
        let corner = |k: u32| {
            let angle = k as f32 * std::f32::consts::TAU / 7.0;
            (center.0 + 50.0 * angle.cos(), center.1 + 47.0 * angle.sin())
        };
        let mut fan: Vec<Draw> = (0..7)
            .map(|k| {
                let (a, b) = (corner(k), corner(k + 1));
                draw(Shape::Triangle, &[center.0, center.1, a.0, a.1, b.0, b.1])
            })
            .collect();
        fan.push(draw(Shape::Triangle, &[0.5, 0.5, 20.5, 0.5, 20.5, 20.5]));
        fan.push(draw(Shape::Triangle, &[20.5, 20.5, 0.5, 20.5, 0.5, 0.5]));
        for pixels in [(128, 128), (160, 160), (192, 192), (256, 256)] {
            let mut count = vec![0u32; (pixels.0 * pixels.1) as usize];
            for triangle in &fan {
                for (i, j) in covered(triangle, (128, 128), pixels) {
                    count[(j * pixels.0 + i) as usize] += 1;
                }
            }
            assert!(count.iter().all(|&n| n <= 1), "{pixels:?}");
        }
        // Each center on the diagonal is in one of the two halves.
        let halves = &fan[7..];
        for i in 1..20u32 {
            let (px, py) = (i as f32 + 0.5, i as f32 + 0.5);
            let inside = halves.iter().filter(|t| covers(t, px, py)).count();
            assert_eq!(inside, 1, "the center ({px}, {py})");
        }
    }

    #[test]
    fn a_line_one_point_wide_is_one_row_or_column() {
        for (ends, want) in [
            ([0.0, 10.0, 100.0, 10.0], block(0..100, 9..10)),
            ([100.0, 10.0, 0.0, 10.0], block(0..100, 9..10)),
            ([0.0, 10.5, 100.0, 10.5], block(0..100, 10..11)),
            ([100.0, 10.5, 0.0, 10.5], block(0..100, 10..11)),
            ([10.0, 0.0, 10.0, 100.0], block(9..10, 0..100)),
            ([10.0, 100.0, 10.0, 0.0], block(9..10, 0..100)),
            ([10.5, 0.0, 10.5, 100.0], block(10..11, 0..100)),
            ([10.5, 100.0, 10.5, 0.0], block(10..11, 0..100)),
        ] {
            let line = draw(Shape::Line, &[ends[0], ends[1], ends[2], ends[3], 1.0]);
            assert_eq!(at_one(&line), want, "{ends:?}");
        }
        let diagonal: Vec<(u32, u32)> = (0..10).map(|i| (i, i)).collect();
        for ends in [[0.0, 0.0, 10.0, 10.0], [10.0, 10.0, 0.0, 0.0]] {
            let line = draw(Shape::Line, &[ends[0], ends[1], ends[2], ends[3], 1.0]);
            assert_eq!(at_one(&line), diagonal, "{ends:?}");
        }
    }

    #[test]
    fn shapes_that_cover_nothing() {
        let nothing = [
            draw(Shape::Circle, &[10.0, 10.0, -5.0]),
            draw(Shape::Circle, &[10.0, 10.0, 0.0]),
            draw(Shape::Line, &[0.0, 0.0, 20.0, 20.0, -2.0]),
            draw(Shape::Rect, &[0.0, 0.0, -5.0, 10.0]),
            draw(Shape::Rect, &[0.0, 0.0, 10.0, 0.0]),
            draw(Shape::Triangle, &[0.0, 0.0, 10.0, 10.0, 20.0, 20.0]),
        ];
        for shape in &nothing {
            assert!(at_one(shape).is_empty(), "{shape:?}");
        }
        let whole = draw(Shape::Triangle, &[0.0, 0.0, 100.0, 0.0, 0.0, 100.0]);
        assert!(!at_one(&whole).is_empty());
        for k in 0..6 {
            let mut with_nan = whole.clone();
            with_nan.n[k] = f32::NAN;
            assert!(at_one(&with_nan).is_empty(), "a NaN at {k}");
        }
    }

    #[test]
    fn a_shape_covers_nothing_outside_the_pixels_render_visits() {
        let mut shapes = vec![
            draw(Shape::Rect, &[-1e6, -1e6, 2e6, 2e6]),
            draw(Shape::Rect, &[f32::NEG_INFINITY, 3.0, f32::INFINITY, 4.0]),
            draw(Shape::Rect, &[319.75, 199.75, 0.25, 0.25]),
            draw(Shape::Circle, &[64.0, 64.0, 1e6]),
            draw(Shape::Circle, &[-0.5, 200.5, 1.0]),
            draw(Shape::Circle, &[10.0, 10.0, f32::INFINITY]),
            draw(Shape::Line, &[-1e6, 64.0, 1e6, 64.0, 2.0]),
            draw(Shape::Line, &[5.0, 5.0, 5.0, 5.0, 9.0]),
            draw(Shape::Line, &[300.0, 10.0, 20.0, 190.0, 0.5]),
            draw(Shape::Triangle, &[1e30, 64.0, -10.0, -10.0, -10.0, 200.0]),
            draw(Shape::Triangle, &[0.0, 0.0, 100.0, 0.0, 0.0, f32::NAN]),
        ];
        for k in 0..40u16 {
            let f = f32::from(k);
            shapes.push(draw(
                Shape::Rect,
                &[f * 7.9 - 3.0, f * 4.7, 0.6 + f * 0.3, 1.0],
            ));
            shapes.push(draw(
                Shape::Circle,
                &[f * 8.1, 199.0 - f * 5.2, 0.3 + f * 0.1],
            ));
            shapes.push(draw(
                Shape::Line,
                &[f * 8.0, 0.0, 320.0 - f * 8.0, 200.0, f * 0.05],
            ));
            let (x, y) = (f * 7.7 + 0.3, f * 4.9 + 0.1);
            shapes.push(draw(
                Shape::Triangle,
                &[x, y, x + 2.5, y + 0.5, x + 0.75, y + 3.0],
            ));
        }
        for pixels in SCALES.into_iter().chain([(160, 100), (333, 211)]) {
            for shape in &shapes {
                let frame = Frame {
                    width: 320,
                    height: 200,
                    background: [0, 0, 0, 255],
                    draws: vec![shape.clone()],
                };
                let picture = render(&frame, pixels.0, pixels.1);
                let drawn: Vec<(u32, u32)> = (0..pixels.1)
                    .flat_map(|j| (0..pixels.0).map(move |i| (i, j)))
                    .filter(|(i, j)| {
                        let at = (*j * pixels.0 + *i) as usize * 4;
                        picture.rgba[at..at + 3] != [0, 0, 0]
                    })
                    .collect();
                assert_eq!(
                    drawn,
                    covered(shape, (320, 200), pixels),
                    "{shape:?} at {pixels:?}"
                );
            }
        }
    }

    #[test]
    fn shapes_far_larger_than_the_frame() {
        let all = block(0..128, 0..128);
        assert_eq!(at_one(&draw(Shape::Rect, &[-1e6, -1e6, 2e6, 2e6])), all);
        assert_eq!(at_one(&draw(Shape::Circle, &[64.0, 64.0, 1e6])), all);
        let wide = draw(Shape::Line, &[-1e6, 64.0, 1e6, 64.0, 2.0]);
        assert_eq!(at_one(&wide), block(0..128, 63..65));
        let far = draw(Shape::Triangle, &[1e30, 64.0, -10.0, -10.0, -10.0, 200.0]);
        assert_eq!(at_one(&far), all);
    }

    #[test]
    fn a_frame_of_every_size_is_covered_by_its_own_rectangle() {
        for pixels in SCALES {
            let rect = draw(Shape::Rect, &[0.0, 0.0, 320.0, 200.0]);
            assert_eq!(
                covered(&rect, (320, 200), pixels),
                block(0..pixels.0, 0..pixels.1)
            );
        }
        let rect = draw(Shape::Rect, &[10.0, 20.0, 30.0, 5.0]);
        assert_eq!(
            covered(&rect, (320, 200), (640, 400)),
            block(20..80, 40..50)
        );
    }

    #[test]
    fn the_color_rule() {
        for a in 0..=255u8 {
            for (c, d) in [
                (0u8, 255u8),
                (255, 0),
                (128, 64),
                (7, 200),
                (255, 255),
                (0, 0),
            ] {
                let exact =
                    (f64::from(a) * f64::from(c) + f64::from(255 - a) * f64::from(d)) / 255.0;
                assert_eq!(over(a, c, d), exact.round() as u8, "{a} {c} {d}");
            }
        }
        assert_eq!(over(128, 255, 0), 128);
        let frame = Frame {
            width: 2,
            height: 1,
            background: [0, 0, 0, 7],
            draws: vec![
                Draw {
                    color: [255, 255, 255, 128],
                    ..draw(Shape::Rect, &[0.0, 0.0, 2.0, 1.0])
                },
                Draw {
                    color: [255, 0, 0, 128],
                    ..draw(Shape::Rect, &[1.0, 0.0, 1.0, 1.0])
                },
                Draw {
                    color: [9, 9, 9, 0],
                    ..draw(Shape::Rect, &[0.0, 0.0, 2.0, 1.0])
                },
            ],
        };
        let picture = render(&frame, 2, 1);
        assert_eq!(picture.rgba, [128, 128, 128, 255, 192, 64, 64, 255]);
        assert_eq!(picture.layers, [1, 2]);
        assert!(agrees(&picture, &[129, 127, 128, 255, 194, 62, 64, 255]).is_ok());
        assert!(agrees(&picture, &[130, 128, 128, 255, 192, 64, 64, 255]).is_err());
        assert!(agrees(&picture, &[128, 128, 128, 254, 192, 64, 64, 255]).is_err());
    }

    #[test]
    fn answers_are_read() {
        let frame = parse(
            "midnight: drive: draws: 3 ticks, 320x200, 2 draws\n\
             midnight: drive:   Window.background = Color(16, 16, 32, 255)\n\
             midnight: drive:   Draw.rect(0.0, 190.0, 320.0, 10.0, Color(40, 40, 60, 255))\n\
             midnight: drive:   Draw.line(NaN, -Infinity, 1e+30, -0.0, 0.33333334, Color(1, 2, 3, 4))\n",
        )
        .unwrap();
        assert_eq!(
            (frame.width, frame.height, frame.background),
            (320, 200, [16, 16, 32, 255])
        );
        assert_eq!(
            frame.draws[0],
            draw(Shape::Rect, &[0.0, 190.0, 320.0, 10.0]).with([40, 40, 60, 255])
        );
        let line = &frame.draws[1];
        assert!(line.n[0].is_nan() && line.n[1] == f32::NEG_INFINITY && line.n[2] == 1e30);
        assert!(line.n[3] == 0.0 && line.n[3].is_sign_negative() && line.n[4] == 1.0 / 3.0);
        assert!(parse("draws: 1 tick, 2x2, 1 draw\n  Window.background = Color(0, 0, 0, 255)\n  Draw.oval(1.0, Color(0, 0, 0, 255))\n").is_err());
    }

    /// Every pixel whose center lies in a cell of `draw` lies in exactly one
    /// pixel of the font, and no other pixel lies in one.
    fn tiles(draw: &Draw, pixels: (u32, u32)) {
        let step_x = 320.0 / pixels.0 as f32;
        let step_y = 200.0 / pixels.1 as f32;
        let cells = cells(draw);
        let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for cell in &cells {
            let ((a, b), (c, d)) = cell.bounds();
            (x0, x1, y0, y1) = (x0.min(a), x1.max(b), y0.min(c), y1.max(d));
        }
        for j in pixel_span(y0, y1, step_y, pixels.1) {
            for i in pixel_span(x0, x1, step_x, pixels.0) {
                let (px, py) = ((i as f32 + 0.5) * step_x, (j as f32 + 0.5) * step_y);
                let found = font_pixels_at(draw, px, py);
                let inside = cells.iter().any(|cell| {
                    let ((x0, x1), (y0, y1)) = cell.bounds();
                    x0 <= px && px < x1 && y0 <= py && py < y1
                });
                assert_eq!(
                    found.len(),
                    usize::from(inside),
                    "{draw:?} at {pixels:?}, pixel {i} {j}: {found:?}"
                );
            }
        }
    }

    const TEXT_PLACES: [(f32, f32); 4] = [(10.0, 20.0), (10.5, 20.5), (0.3, 7.7), (101.25, 3.125)];
    const TEXT_SIZES: [f32; 7] = [12.0, 24.0, 48.0, 10.0, 17.0, 7.3, 13.000_001];

    #[test]
    fn characters_and_lines_share_their_bounds() {
        for pixels in SCALES {
            for (x, y) in TEXT_PLACES {
                for size in TEXT_SIZES {
                    for string in ["AB", "A\nB", "MW\nQ\u{f0}"] {
                        let draw = text(x, y, string, size);
                        let cells = cells(&draw);
                        if string == "AB" {
                            assert_eq!(cells[0].bounds().0.1, cells[1].bounds().0.0);
                        } else {
                            assert_eq!(cells[0].bounds().1.1, cells[cells.len() / 2].bounds().1.0);
                        }
                        tiles(&draw, pixels);
                    }
                }
            }
        }
    }

    #[test]
    fn a_size_of_twelve_at_a_scale_of_one_is_the_font() {
        // The reference's example: the cell covers columns 10 to 15 and rows
        // 20 to 31, the letter columns 10 to 14 and rows 23 to 29.
        let a = text(10.0, 20.0, "A", 12.0);
        let want: Vec<(u32, u32)> = glyph_pixels('A')
            .iter()
            .map(|&(j, r)| (10 + j, 20 + r))
            .collect();
        let mut got = at_one(&a);
        got.sort_by_key(|&(i, j)| (j, i));
        let mut want_sorted = want.clone();
        want_sorted.sort_by_key(|&(i, j)| (j, i));
        assert_eq!(got, want_sorted);
        assert!(
            got.iter()
                .all(|&(i, j)| (10..15).contains(&i) && (23..30).contains(&j))
        );

        // Every glyph of the font, one pixel of the font on one pixel.
        let all: String = (0x20u32..=0xff)
            .filter_map(char::from_u32)
            .filter(|c| !c.is_control())
            .collect();
        let line = text(0.0, 0.0, &all, 12.0);
        let size = (6 * all.chars().count() as u32, 12);
        let mut got = covered(&line, size, size);
        let mut want = Vec::new();
        for (k, c) in all.chars().enumerate() {
            if c == ' ' || c == '\u{a0}' {
                continue;
            }
            want.extend(glyph_pixels(c).iter().map(|&(j, r)| (6 * k as u32 + j, r)));
        }
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want);

        // At 24, and at 12 on a scale of 2, each pixel of the font is two by two.
        let doubled: Vec<(u32, u32)> = want_sorted
            .iter()
            .flat_map(|&(i, j)| {
                [
                    (2 * i, 2 * j),
                    (2 * i + 1, 2 * j),
                    (2 * i, 2 * j + 1),
                    (2 * i + 1, 2 * j + 1),
                ]
            })
            .collect();
        let mut doubled = doubled;
        doubled.sort_unstable();
        let mut big = covered(&text(20.0, 40.0, "A", 24.0), (128, 128), (128, 128));
        big.sort_unstable();
        assert_eq!(big, doubled);
        let mut scaled = covered(&a, (128, 128), (256, 256));
        scaled.sort_unstable();
        assert_eq!(scaled, doubled);
    }

    #[test]
    fn sizes_that_make_u_inexact() {
        let mut inexact = 0;
        for size in [10.0f32, 17.0, 7.3, 13.000_001, 100.0 / 3.0] {
            if f64::from(size / 12.0) != f64::from(size) / 12.0 {
                inexact += 1;
            }
            for pixels in SCALES.into_iter().chain([(333, 211)]) {
                for (x, y) in TEXT_PLACES {
                    let draw = text(x, y, "Hg\u{f0}\n\u{e9}#", size);
                    tiles(&draw, pixels);
                    let frame = Frame {
                        width: 320,
                        height: 200,
                        background: [0, 0, 0, 255],
                        draws: vec![draw.clone()],
                    };
                    let picture = render(&frame, pixels.0, pixels.1);
                    let drawn: Vec<(u32, u32)> = (0..pixels.1)
                        .flat_map(|j| (0..pixels.0).map(move |i| (i, j)))
                        .filter(|(i, j)| {
                            let at = (*j * pixels.0 + *i) as usize * 4;
                            picture.rgba[at..at + 3] != [0, 0, 0]
                        })
                        .collect();
                    let (step_x, step_y) = (320.0 / pixels.0 as f32, 200.0 / pixels.1 as f32);
                    let near = |low: f32, high: f32, step: f32, count: u32| {
                        pixel_span(low - 4.0, high + 4.0, step, count)
                    };
                    let last = cells(&draw).last().unwrap().bounds();
                    let mut want = Vec::new();
                    for j in near(y, last.1.1, step_y, pixels.1) {
                        for i in near(x, x + 6.0 * size, step_x, pixels.0) {
                            let (px, py) = ((i as f32 + 0.5) * step_x, (j as f32 + 0.5) * step_y);
                            if covers(&draw, px, py) {
                                want.push((i, j));
                            }
                        }
                    }
                    assert_eq!(drawn, want, "{size} {x} {y} {pixels:?}");
                }
            }
        }
        assert!(inexact >= 3);
        // A bound comes from the text's corner: at 10 points, the fourth
        // character's first column is x + 18 * u, which is not the sum of
        // the widths of the cells before it.
        let draw = text(0.1, 0.0, "ABCD", 10.0);
        let u = 10.0f32 / 12.0;
        assert_eq!(cells(&draw)[3].bounds().0.0, 0.1 + 18.0 * u);
    }

    #[test]
    fn the_replacement_glyph_and_eth() {
        let replacement = font().glyphs.len() - 1;
        let lacking = text(
            0.0,
            0.0,
            "\t\r\u{1b}\0\u{7f}\u{85}\u{ad}\u{20ac}\u{2192}\u{1f600}",
            12.0,
        );
        let cells = cells(&lacking);
        assert_eq!(cells.len(), 10);
        for (k, cell) in cells.iter().enumerate() {
            assert_eq!(
                (cell.glyph, cell.column),
                (replacement, 6 * k as u32),
                "{k}"
            );
        }
        let mut box_pixels = at_one(&text(10.0, 20.0, "\t", 12.0));
        box_pixels.sort_unstable();
        let mut want: Vec<(u32, u32)> = (10..15)
            .flat_map(|i| (23..30).map(move |j| (i, j)))
            .filter(|&(i, j)| i == 10 || i == 14 || j == 23 || j == 29)
            .collect();
        want.sort_unstable();
        assert_eq!(box_pixels, want);

        // `ð` reaches the sixth column of its cell; the next cell begins after it.
        let eth = at_one(&text(10.0, 20.0, "\u{f0}A", 12.0));
        assert!(eth.contains(&(15, 23)));
        let a: Vec<(u32, u32)> = glyph_pixels('A')
            .iter()
            .map(|&(j, r)| (16 + j, 20 + r))
            .collect();
        assert!(a.iter().all(|pixel| eth.contains(pixel)));
        assert_eq!(eth.len(), glyph_pixels('\u{f0}').len() + a.len());
    }

    #[test]
    fn line_breaks() {
        let at = |string: &str| -> Vec<(u32, u32)> {
            cells(&text(5.0, 5.0, string, 12.0))
                .iter()
                .map(|cell| (cell.column, cell.row))
                .collect()
        };
        assert_eq!(at("\nA"), [(0, 12)]);
        assert_eq!(at("A\n\nB"), [(0, 0), (0, 24)]);
        assert_eq!(at("A\n"), at("A"));
        assert_eq!(at("A\r\nB"), [(0, 0), (6, 0), (0, 12)]);
        assert_eq!(at(" A\u{a0}B"), [(6, 0), (18, 0)]);
        assert_eq!(
            at_one(&text(5.0, 5.0, "A\n", 12.0)),
            at_one(&text(5.0, 5.0, "A", 12.0))
        );
    }

    #[test]
    fn text_that_covers_nothing() {
        for draw in [
            text(f32::NAN, 0.0, "A", 12.0),
            text(0.0, f32::NAN, "A", 12.0),
            text(0.0, 0.0, "A", f32::NAN),
            text(0.0, 0.0, "A", 0.0),
            text(0.0, 0.0, "A", -0.0),
            text(0.0, 0.0, "A", -12.0),
            text(0.0, 0.0, "", 12.0),
            text(0.0, 0.0, "  \n\u{a0}", 12.0),
        ] {
            assert!(at_one(&draw).is_empty(), "{draw:?}");
            let frame = Frame {
                width: 128,
                height: 128,
                background: [0, 0, 0, 255],
                draws: vec![draw],
            };
            assert!(
                render(&frame, 128, 128)
                    .rgba
                    .chunks(4)
                    .all(|p| p == [0, 0, 0, 255])
            );
        }
        assert!(!at_one(&text(0.0, 0.0, "A", 12.0)).is_empty());
    }

    #[test]
    fn text_is_read() {
        let frame = parse(
            "draws: 1 tick, 320x200, 2 draws\n  Window.background = Color(0, 0, 0, 255)\n  \
             Draw.text(8.0, 8.5, \"A, \\\"B\\\"\\n\\t\\r\\0\\e\\\\\\#{x}\\x01\\x7F é\", 24.0, Color(1, 2, 3, 255))\n  \
             Draw.text(NaN, 0.0, \"\", -1.0, Color(0, 0, 0, 0))\n",
        )
        .unwrap();
        assert_eq!(frame.draws[0], {
            let mut d = text(8.0, 8.5, "A, \"B\"\n\t\r\0\u{1b}\\#{x}\u{1}\u{7f} é", 24.0);
            d.color = [1, 2, 3, 255];
            d
        });
        assert!(frame.draws[1].n[0].is_nan() && frame.draws[1].text.is_empty());
        assert!(parse("draws: 1 tick, 2x2, 1 draw\n  Window.background = Color(0, 0, 0, 255)\n  Draw.text(1.0, 1.0, \"open, 2.0, Color(0, 0, 0, 255))\n").is_err());
    }

    impl Draw {
        fn with(self, color: [u8; 4]) -> Draw {
            Draw { color, ..self }
        }
    }
}
