//! Rolling Blue Ridge ridges and pines. Shapes are fixed (seeded); only colours change.
//!
//! The ridges are rounded, forested and hazy-blue in the distance. Their foliage colour is a
//! seeded per-pixel mix of tree colours whose proportions follow the day of the year, so the
//! shape of a ridge never changes but a hillside goes from green to red, orange and yellow in
//! October and to bare brown in winter.

use std::collections::HashMap;
use std::rc::Rc;

use super::cache::Memo;
use super::palette::*;
use crate::gfx::{line_points, pyround, Img, Mask, Rgb};
use crate::scene_rng::SceneRng;
use crate::state::Condition;

const SEED: u64 = 64281;

const BLUES: [Rgb; 5] = [[90, 128, 200], [68, 106, 178], [52, 84, 144], [42, 66, 108], [32, 50, 78]];
const NIGHTS: [Rgb; 5] = [[36, 46, 88], [29, 38, 72], [22, 31, 58], [16, 24, 44], [11, 17, 32]];

/// Each ridge is (baseline y, [(peak x, height, width)], foliage 0..1), back to front. Wide, low
/// domes keep the slopes gentle like old Appalachian ridges; far ridges are plain atmospheric
/// blue and only the near ones carry autumn colour.
type Peaks = &'static [(i32, i32, i32)];
const RIDGES: [(i32, Peaks, f64); 4] = [
    (21, &[(20, 8, 28), (60, 9, 24)], 0.0),
    (24, &[(2, 9, 26), (40, 10, 24)], 0.0),
    (27, &[(26, 9, 26), (62, 10, 24)], 0.5),
    (30, &[(8, 8, 24), (46, 9, 24)], 0.8),
];
const PINES_X: [i32; 10] = [2, 7, 12, 16, 21, 44, 49, 53, 58, 62];
/// Indices into `PINES_X` that stay evergreen (x = 16, 21, 49).
const PINES_KEPT: [usize; 3] = [3, 4, 6];
/// Large foreground trees: (trunk x, crown width, crown height, crown bottom y, colour pick).
const BIG_TREES: [(i32, i32, i32, i32, f64); 3] =
    [(6, 22, 20, 32, 0.66), (57, 19, 18, 33, 0.93), (41, 13, 12, 34, 0.4)];
const PINE_WIDTHS: [i32; 11] = [1, 3, 3, 5, 5, 5, 7, 7, 7, 9, 9];
const PINE_BASE: i32 = 34;

// Foliage colours, in the order a hillside's weights are applied.
const CONIFER: usize = 0;
const GREEN: usize = 1;
const SPRING: usize = 2;
const YELLOW: usize = 3;
const ORANGE: usize = 4;
const RED: usize = 5;
const BROWN: usize = 6;
const BARE: usize = 7;
const BLOSSOM: usize = 8;
const TREES: [Rgb; 9] = [
    [24, 62, 40], [48, 108, 48], [116, 168, 68], [226, 184, 44], [222, 110, 30],
    [182, 44, 32], [112, 80, 52], [112, 94, 86], [206, 150, 168],
];

type Weights = [f64; 9];

fn season(entries: &[(usize, f64)]) -> Weights {
    let mut w = [0.0; 9];
    for &(k, v) in entries {
        w[k] = v;
    }
    w
}

/// (day of year, weights); linear in between, wrapping through the new year.
fn seasons() -> [(i32, Weights); 9] {
    [
        (15, season(&[(CONIFER, 0.2), (BARE, 0.55), (BROWN, 0.25)])),
        (75, season(&[(CONIFER, 0.2), (BARE, 0.5), (BROWN, 0.2), (SPRING, 0.1)])),
        (110, season(&[(CONIFER, 0.12), (SPRING, 0.45), (GREEN, 0.2), (BLOSSOM, 0.13), (BARE, 0.1)])),
        (150, season(&[(CONIFER, 0.15), (GREEN, 0.65), (SPRING, 0.2)])),
        (200, season(&[(CONIFER, 0.25), (GREEN, 0.75)])),
        (260, season(&[(CONIFER, 0.2), (GREEN, 0.6), (YELLOW, 0.12), (ORANGE, 0.08)])),
        (290, season(&[(CONIFER, 0.12), (GREEN, 0.2), (YELLOW, 0.24), (ORANGE, 0.24), (RED, 0.2)])),
        (320, season(&[(CONIFER, 0.15), (BROWN, 0.4), (ORANGE, 0.2), (BARE, 0.15), (YELLOW, 0.1)])),
        (350, season(&[(CONIFER, 0.2), (BARE, 0.5), (BROWN, 0.3)])),
    ]
}

const SNOW: [Rgb; 2] = [[236, 242, 252], [62, 76, 112]];
const PINE: [Rgb; 2] = [[14, 44, 32], [4, 10, 12]];
const FOG_AMOUNT: [f64; 5] = [0.6, 0.5, 0.4, 0.3, 0.2];
const PINE_NIGHT: Rgb = [7, 12, 20];
/// Days per terrain cache entry.
pub const SEASON_STEP: i32 = 4;

fn leaf_shade(c: char) -> f64 {
    match c {
        'h' => 1.4,
        'm' => 1.0,
        'd' => 0.6,
        'x' => 0.4,
        _ => 1.14, // 's'
    }
}

pub fn season_key(doy: u32) -> i32 {
    doy as i32 / SEASON_STEP
}

/// (colour index, weight) for the foliage on `doy`, weights summing to 1.
pub fn tree_weights(doy: i32) -> Vec<(usize, f64)> {
    let table = seasons();
    let mut pts: Vec<(i32, Weights)> = table.to_vec();
    pts.push((table[0].0 + 365, table[0].1));
    let d = if doy >= table[0].0 { doy } else { doy + 365 };
    let (mut w0, mut w1, mut f) = (pts[0].1, pts[1].1, 0.0);
    for pair in pts.windows(2) {
        let ((d0, a), (d1, b)) = (pair[0], pair[1]);
        w0 = a;
        w1 = b;
        f = (d - d0) as f64 / (d1 - d0) as f64;
        if d <= d1 {
            break;
        }
    }
    let mut mix = [0.0; 9];
    for k in 0..9 {
        mix[k] = w0[k] * (1.0 - f) + w1[k] * f;
    }
    let total: f64 = mix.iter().sum();
    (0..9).filter(|&k| mix[k] > 0.0).map(|k| (k, mix[k] / total)).collect()
}

fn foliage(u: f64, weights: &[(Rgb, f64)]) -> Rgb {
    let mut acc = 0.0;
    for &(col, w) in weights {
        acc += w;
        if u < acc {
            return col;
        }
    }
    weights[weights.len() - 1].0
}

fn profile(rng: &mut SceneRng, base: i32, peaks: Peaks) -> Vec<i32> {
    (0..W as i32)
        .map(|x| {
            let mut h = f64::NEG_INFINITY;
            for &(px, ph, pw) in peaks {
                let z = (x - px) as f64 / pw as f64;
                h = h.max(ph as f64 * (-(z * z)).exp());
            }
            h += 0.5 * (x as f64 * 0.4 + base as f64).sin(); // faint knolls on the ridgeline
            let jitter = if rng.random() < 0.06 { 1 } else { 0 };
            (base + 3).min(pyround(base as f64 - h) as i32 + jitter)
        })
        .collect()
}

struct Shapes {
    ridges: Vec<(Vec<i32>, f64)>,
    pines: Vec<(i32, i32, i32)>,
}

thread_local! {
    static SHAPES: Memo<(), Shapes> = Memo::new(1);
    static BIG_TREE: Memo<(i32, i32, u64, u64), Tree> = Memo::new(8);
    static TERRAIN: Memo<(i32, Option<Condition>, i32), (Img, Mask)> = Memo::new(256);
}

fn shapes() -> Rc<Shapes> {
    SHAPES.with(|m| {
        m.get((), || {
            let mut rng = SceneRng::new(SEED);
            let ridges = RIDGES
                .iter()
                .map(|&(base, peaks, fol)| (profile(&mut rng, base, peaks), fol))
                .collect();
            let all: Vec<(i32, i32, i32)> = PINES_X
                .iter()
                .map(|&x| {
                    let h = rng.randint(9, 12);
                    let b = PINE_BASE + rng.randint(0, 1);
                    (x, h, b)
                })
                .collect();
            Shapes { ridges, pines: PINES_KEPT.iter().map(|&k| all[k]).collect() }
        })
    })
}

struct Tree {
    /// ((dx, dy), (shade, colour pick)) for each leaf pixel.
    leaves: Vec<((i32, i32), (char, f64))>,
    /// Limbs in crown coordinates, the first starting below the crown.
    branches: Vec<((f64, f64), (f64, f64))>,
}

/// A deciduous tree as pixels: a crown of overlapping leaf clusters (each lit from the upper
/// left, each a slightly different fall colour near `v`), a ragged edge, gaps that show the
/// branches, and a trunk that forks up into the crown.
fn big_tree(w: i32, h: i32, seed: u64, v: f64) -> Rc<Tree> {
    BIG_TREE.with(|m| m.get((w, h, seed, v.to_bits()), || build_tree(w, h, seed, v)))
}

fn build_tree(w: i32, h: i32, seed: u64, v: f64) -> Tree {
    let mut rng = SceneRng::new(seed);
    let (wf, hf) = (w as f64, h as f64);
    let mut clusters: Vec<(f64, f64, f64, f64, f64)> = Vec::new();
    for _ in 0..(w * h / 9) {
        // clumps fill an irregular dome
        let (mut cx, mut cy) = (0.0, 0.0);
        for _ in 0..20 {
            cx = rng.uniform(0.0, wf);
            cy = rng.uniform(0.0, hf);
            let (nx, ny) = ((cx - wf / 2.0) / (wf / 2.0), (cy - hf * 0.55) / (hf * 0.55));
            let limit = 0.7 + rng.uniform(-0.1, 0.2);
            if nx * nx + ny * ny <= limit {
                break;
            }
        }
        let rx = rng.uniform(2.2, 4.2);
        let ry = rng.uniform(1.8, 3.2);
        let pick = clamp(v + rng.uniform(-0.2, 0.2), 0.0, 0.999);
        clusters.push((cx, cy, rx, ry, pick));
    }
    clusters.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap()); // lower clumps overlap the ones above (stable)

    // Insertion-ordered, like the dict the rim pass walks: a later cluster overwriting a pixel
    // keeps that pixel's original place in the order.
    let mut order: Vec<(i32, i32)> = Vec::new();
    let mut leaves: HashMap<(i32, i32), (char, f64)> = HashMap::new();
    for &(cx, cy, rx, ry, pick) in &clusters {
        for y in ((cy - ry) as i32 - 1)..((cy + ry) as i32 + 2) {
            for x in ((cx - rx) as i32 - 1)..((cx + rx) as i32 + 2) {
                let (nx, ny) = ((x as f64 + 0.5 - cx) / rx, (y as f64 + 0.5 - cy) / ry);
                let d = nx * nx + ny * ny;
                if d > 1.0 + rng.uniform(-0.25, 0.2) || !(0..w).contains(&x) || !(0..h).contains(&y) {
                    continue;
                }
                let light = nx * 0.45 + ny * 0.85;
                let mut ch = if light < -0.35 {
                    'h'
                } else if light < 0.3 {
                    'm'
                } else if light > 0.62 {
                    'x'
                } else {
                    'd'
                };
                if ch == 'm' && rng.random() < 0.1 {
                    ch = 's';
                }
                if leaves.insert((x, y), (ch, pick)).is_none() {
                    order.push((x, y));
                }
            }
        }
    }
    // ragged rim, gaps in the shade
    for &(x, y) in &order {
        let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .any(|(dx, dy)| !leaves.contains_key(&(x + dx, y + dy)));
        let mut drop = edge && rng.random() < 0.22;
        if !drop {
            let shade = leaves[&(x, y)].0;
            drop = (shade == 'd' || shade == 'x') && rng.random() < 0.1;
        }
        if drop {
            leaves.remove(&(x, y));
        }
    }
    let leaves = order.into_iter().filter_map(|p| leaves.get(&p).map(|l| (p, *l))).collect();

    let top = hf * 0.25;
    let mid = (w / 2) as f64;
    let branches = vec![
        ((mid, hf + 3.0), (mid, hf * 0.62)),
        ((mid, hf * 0.7), (wf * 0.25, top + hf * 0.2)),
        ((mid, hf * 0.66), (wf * 0.78, top + hf * 0.1)),
        ((mid, hf * 0.6), (wf * 0.5, top)),
    ];
    Tree { leaves, branches }
}

fn shade_mix(day: Rgb, night: Rgb, light: f64, tw: f64, warm: f64) -> Rgb {
    lerp(lerp(night, day, light), [255, 150, 110], warm * tw)
}

/// An RGB image with a coverage mask that drawing calls fill in together.
struct Layer {
    rgb: Img,
    alpha: Mask,
}

impl Layer {
    fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
            self.rgb.put(x, y, c);
            self.alpha.set(x, y, 255);
        }
    }

    fn line(&mut self, a: (i32, i32), b: (i32, i32), c: Rgb) {
        for (x, y) in line_points(a.0, a.1, b.0, b.1) {
            self.set(x, y, c);
        }
    }

    fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        for y in y0..=y1.min(H as i32 - 1) {
            for x in x0..=x1 {
                self.set(x, y, c);
            }
        }
    }
}

/// (RGB, mask) of the ridges and trees at 64x35 for elevation eq/40, a condition and a
/// `season_key()`. Foliage colour fades out with the light: at night the hills are plain
/// blue silhouettes, stepped so each ridge stays distinct from the sky and its neighbours.
pub fn terrain(eq: i32, cond: Option<Condition>, season_k: i32) -> Rc<(Img, Mask)> {
    TERRAIN.with(|m| m.get((eq, cond, season_k), || build_terrain(eq, cond, season_k)))
}

fn build_terrain(eq: i32, cond: Option<Condition>, season_k: i32) -> (Img, Mask) {
    let elev = eq as f64 / 40.0;
    let (light, tw) = (light_level(elev), twilight(elev));
    let g = gray_mix(cond) * 0.7;
    let snowing = cond == Some(Condition::Snow);
    let shapes = shapes();
    let weights = tree_weights(season_k * SEASON_STEP + SEASON_STEP / 2);
    let lively: Vec<(Rgb, f64)> = weights
        .iter()
        .filter(|(k, _)| ![BROWN, BARE, CONIFER].contains(k))
        .map(|&(k, w)| (TREES[k], w))
        .collect();
    let total: f64 = lively.iter().map(|(_, w)| w).sum();
    let leaves_pal: Vec<(Rgb, f64)> = if total > 0.3 {
        lively.iter().map(|&(c, w)| (c, w / total)).collect()
    } else {
        weights.iter().map(|&(k, w)| (TREES[k], w)).collect() // bright leaves in season
    };
    let fog_col = lerp([26, 30, 44], [190, 196, 206], light);
    let snow = shade_mix(SNOW[0], SNOW[1], light, tw, 0.4);
    let n = shapes.ridges.len();
    let sum_of = |ks: &[usize]| -> f64 { weights.iter().filter(|(k, _)| ks.contains(k)).map(|(_, w)| w).sum() };
    let warm = sum_of(&[YELLOW, ORANGE, RED]);
    let mut avg = [0u8; 3];
    for (k, out) in avg.iter_mut().enumerate() {
        let s: f64 = weights.iter().map(|&(c, w)| TREES[c][k] as f64 * w).sum();
        *out = pyround(s).clamp(0, 255) as u8;
    }
    // hillsides glow russet only in the fall
    let mean = lerp(avg, [204, 82, 30], f64::min(0.5, warm * 0.7));
    let bare = sum_of(&[BROWN, BARE]);
    let leaf_loss = clamp01((bare - 0.3) / 0.6) * 0.85; // how many leaves have dropped
    let ramp = if n > 1 { (n - 1) as f64 } else { 1.0 };

    let tone = |i: usize, foliage: Option<Rgb>, fol: f64| -> Rgb {
        let k = ((BLUES.len() - 1) as i64).min(pyround(i as f64 * (BLUES.len() - 1) as f64 / ramp)) as usize;
        let mut day = BLUES[k];
        if let Some(f) = foliage {
            if fol != 0.0 {
                // far hills darker, near ones bright
                day = lerp(day, scale(f, 0.6 + 0.4 * fol), fol);
            }
        }
        let mut col = shade_mix(day, NIGHTS[k], light, tw, 0.15 + 0.1 * (i as f64 / ramp));
        col = lerp(col, gray_of(col), g);
        if cond == Some(Condition::Fog) {
            col = lerp(col, fog_col, FOG_AMOUNT[i.min(4)]);
        }
        col
    };

    let mut layer = Layer { rgb: Img::black(W, H), alpha: Mask::new(W, H, 0) };

    // Colours are worked out once per ridge and once per leaf colour, not per pixel: a cache
    // miss here (the sun moving a step, a new season) stalls the frame it happens in.
    for (i, (tops, fol)) in shapes.ridges.iter().enumerate() {
        let base_col = if *fol != 0.0 { tone(i, Some(mean), *fol) } else { tone(i, None, 0.0) };
        for x in 0..W {
            let top = tops[x];
            let ahead = tops[(W - 1).min(x + 3)];
            let face = if ahead < top - 1 { 0.88 } else { 1.0 }; // slopes rising to the right sit in shade
            for y in top.max(0)..H as i32 {
                let depth = clamp01((y - top) as f64 / 12.0);
                let mut col = base_col;
                if *fol != 0.0 && y - top < 1 {
                    col = scale(col, 1.12); // lit crest
                }
                col = scale(col, face * (1.0 - 0.22 * depth));
                if snowing && y - top < 2 + (x as i32 + y) % 2 {
                    // a dusting on the crests
                    col = lerp(col, scale(snow, face * 0.92 + 0.08), 0.85);
                }
                layer.set(x as i32, y, col);
            }
        }
    }

    let mut pine = lerp(PINE_NIGHT, PINE[0], light);
    pine = lerp(pine, gray_of(pine), g);
    let tip = shade_mix(SNOW[0], SNOW[1], light, tw, 0.3);
    for &(x, h, base) in &shapes.pines {
        for r in 0..h {
            let half = PINE_WIDTHS[(r as usize).min(PINE_WIDTHS.len() - 1)] / 2;
            let y = base - h + r + 1;
            let col = if snowing && r < 2 { tip } else { pine };
            layer.line((x - half, y), (x + half, y), col);
        }
        layer.set(x, base + 1, scale(pine, 0.7));
    }

    let dull = tone(n, Some(TREES[BROWN]), 0.95);
    let mute = clamp01(leaf_loss / 0.85) * 0.85;
    let mut leaf_cols: HashMap<Rgb, Rgb> = HashMap::new();
    for (k, &(bx, cw, ch, cb, v)) in BIG_TREES.iter().enumerate() {
        // foreground trees, drawn last
        let tree = big_tree(cw, ch, 100 + k as u64, v);
        let (ox, oy) = (bx - cw / 2, cb - ch);
        let bark = lerp([14, 11, 12], [92, 66, 48], light);
        for &((x0, y0), (x1, y1)) in &tree.branches {
            // trunk and the limbs in the crown
            layer.line(
                ((ox as f64 + x0) as i32, (oy as f64 + y0) as i32),
                ((ox as f64 + x1) as i32, (oy as f64 + y1) as i32),
                bark,
            );
        }
        layer.rect(bx - 1, cb - 2, bx, H as i32, bark); // 2 px trunk
        layer.line((bx - 1, cb - 2), (bx - 1, H as i32), scale(bark, 1.3));
        for y in (cb + 2)..H as i32 {
            layer.set(bx + 1, y, scale(bark, 0.7));
        }
        for &((dx, dy), (c, pick)) in &tree.leaves {
            let h = ((dx as i64 * 73_856_093) ^ (dy as i64 * 19_349_663) ^ (k as i64 * 83_492_791)).rem_euclid(100);
            if (h as f64) / 100.0 < leaf_loss {
                continue; // bare branches show through
            }
            let fol = foliage(pick, &leaves_pal);
            let leaf = *leaf_cols.entry(fol).or_insert_with(|| {
                // late leaves fade to brown
                lerp(tone(n, Some(fol), 0.95), dull, mute)
            });
            layer.set(ox + dx, oy + dy, scale(leaf, leaf_shade(c)));
        }
    }
    (layer.rgb, layer.alpha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one_all_year() {
        for doy in 0..366 {
            let total: f64 = tree_weights(doy).iter().map(|(_, w)| w).sum();
            assert!((total - 1.0).abs() < 1e-9, "doy {doy}");
        }
    }

    #[test]
    fn autumn_is_warmer_than_summer() {
        let warm = |doy| -> f64 {
            tree_weights(doy).iter().filter(|(k, _)| [YELLOW, ORANGE, RED].contains(k)).map(|(_, w)| w).sum()
        };
        assert!(warm(295) > warm(180) + 0.3);
    }

    #[test]
    fn terrain_has_ridges_and_open_sky() {
        let t = terrain(40, Some(Condition::Clear), season_key(100));
        let (_, mask) = &*t;
        assert_eq!(mask.get(0, 0), 0);
        assert_eq!(mask.get(32, 34), 255);
    }
}
