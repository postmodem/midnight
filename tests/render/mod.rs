//! The reference renderer: the pixels a list of draws must give, by "Which
//! pixels a shape covers" and "Colour" in the reference, in 32-bit
//! arithmetic. It states the rules apart from the runtime's shaders, so
//! that each is a check on the other.

/// The shape of a draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Circle,
    Line,
    Triangle,
}

impl Shape {
    fn named(name: &str) -> Option<(Shape, usize)> {
        Some(match name {
            "rect" => (Shape::Rect, 4),
            "circle" => (Shape::Circle, 3),
            "line" => (Shape::Line, 5),
            "triangle" => (Shape::Triangle, 6),
            _ => return None,
        })
    }
}

/// One draw: its shape, its numbers in the order of the statement's
/// arguments (unused ones zero) and its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub shape: Shape,
    pub n: [f32; 6],
    pub color: [u8; 4],
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
            for number in n.iter_mut().take(count) {
                let (text, after) = rest.split_once(", ")?;
                *number = text.parse().ok()?;
                rest = after;
            }
            let (color, rest) = color(rest)?;
            (rest == ")").then_some(Draw { shape, n, color })
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

/// Whether a centre exactly on an edge of direction `(dx, dy)`, with the
/// shape on its right, counts: a left edge, or a top edge, y down.
fn on_edge_counts(dx: f32, dy: f32) -> bool {
    dy < 0.0 || (dy == 0.0 && dx > 0.0)
}

/// The test of the edge from `a` to `b` at the centre `c`, computed from
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

/// Whether `draw` covers the pixel whose centre is `(px, py)` in points.
pub fn covers(draw: &Draw, px: f32, py: f32) -> bool {
    let n = draw.n;
    if n.iter().any(|number| number.is_nan()) {
        return false;
    }
    match draw.shape {
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
    }
}

/// The pixels, of `count` of `step` points each, whose centres may lie
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
        let ((x0, x1), (y0, y1)) = bounds(draw);
        let columns = pixel_span(x0, x1, step_x, pixel_width);
        let rows = pixel_span(y0, y1, step_y, pixel_height);
        for j in rows {
            let py = (j as f32 + 0.5) * step_y;
            for i in columns.clone() {
                let px = (i as f32 + 0.5) * step_x;
                if !covers(draw, px, py) {
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
        }
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
        // A fan around a centre, corners on a circle, and a square split on
        // a diagonal that runs through pixel centres.
        let centre = (64.3f32, 63.7f32);
        let corner = |k: u32| {
            let angle = k as f32 * std::f32::consts::TAU / 7.0;
            (centre.0 + 50.0 * angle.cos(), centre.1 + 47.0 * angle.sin())
        };
        let mut fan: Vec<Draw> = (0..7)
            .map(|k| {
                let (a, b) = (corner(k), corner(k + 1));
                draw(Shape::Triangle, &[centre.0, centre.1, a.0, a.1, b.0, b.1])
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
        // Each centre on the diagonal is in one of the two halves.
        let halves = &fan[7..];
        for i in 1..20u32 {
            let (px, py) = (i as f32 + 0.5, i as f32 + 0.5);
            let inside = halves.iter().filter(|t| covers(t, px, py)).count();
            assert_eq!(inside, 1, "the centre ({px}, {py})");
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
            let mut with_nan = whole;
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
                    draws: vec![*shape],
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
    fn the_colour_rule() {
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
        let line = frame.draws[1];
        assert!(line.n[0].is_nan() && line.n[1] == f32::NEG_INFINITY && line.n[2] == 1e30);
        assert!(line.n[3] == 0.0 && line.n[3].is_sign_negative() && line.n[4] == 1.0 / 3.0);
        assert!(parse("draws: 1 tick, 2x2, 1 draw\n  Window.background = Color(0, 0, 0, 255)\n  Draw.oval(1.0, Color(0, 0, 0, 255))\n").is_err());
    }

    impl Draw {
        fn with(self, color: [u8; 4]) -> Draw {
            Draw { color, ..self }
        }
    }
}
