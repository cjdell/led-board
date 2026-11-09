// Smoke test for the festival animations: every new variant must draw
// non-trivial content into the 60x40 buffer without panicking (out-of-bounds
// direct indexing would panic), and the default playlist must serialise.
// Run with `cargo test -p sim -- --nocapture` to see the ASCII previews.
use animations::{Animation, AnimationEnum, AnimationParams, BufferTargetRgb888};

const WIDTH: usize = 60;
const HEIGHT: usize = 40;

fn render(anim: &mut AnimationEnum, ms: u32) -> BufferTargetRgb888 {
    let mut buf = BufferTargetRgb888::new();
    anim.draw(ms, &AnimationParams::default(), &mut buf);
    buf
}

fn luminance(p: &embedded_graphics::pixelcolor::Rgb888) -> u32 {
    use embedded_graphics::pixelcolor::RgbColor;
    p.r() as u32 + p.g() as u32 + p.b() as u32
}

fn ascii(buf: &BufferTargetRgb888) -> String {
    const RAMP: [char; 9] = [' ', '.', ':', '-', '=', '+', '*', '#', '%'];
    let mut out = String::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let l = luminance(&buf.buffer[y * WIDTH + x]);
            out.push(RAMP[((l / 57).min(8) as usize)]);
        }
        out.push('\n');
    }
    out
}

fn coverage(buf: &BufferTargetRgb888) -> usize {
    buf.buffer
        .iter()
        .filter(|p| luminance(p) > 30)
        .count()
}

fn festival_anims() -> Vec<(&'static str, AnimationEnum)> {
    use AnimationEnum::*;
    vec![
        ("CenteredTitle", CenteredTitle("Northern".into(), "Lights".into())),
        ("Aurora", Aurora),
        ("Starfield", Starfield),
        ("Snowfall", Snowfall),
        ("FallingLeaves", FallingLeaves),
        ("Fireflies", Fireflies),
        ("VinylSpin", VinylSpin),
        ("CanalBoats", CanalBoats),
        ("ScrollingMessage", ScrollingMessage("Light Up Leigh with Leigh Hackspace".into())),
    ]
}

#[test]
fn new_animations_render_content() {
    for (name, anim) in festival_anims() {
        for ms in [0u32, 3_700, 9_000, 22_000] {
            let mut a = anim.clone();
            let buf = render(&mut a, ms);
            let lit = coverage(&buf);
            println!("{name} @ {ms}ms: {lit} lit pixels\n{}", ascii(&buf));
            // Fireworks-style sparse effects still cover a handful of pixels;
            // titles cover far more. Anything under 20 px means a broken draw.
            assert!(lit >= 20, "{name} drew nothing at {ms}ms");
        }
    }
}

#[test]
fn default_playlist_serialises_and_loops() {
    let playlist = animations::Playlist::get_default_playlist_data();
    let json = serde_json::to_string(&playlist).unwrap();
    let round: Vec<(AnimationEnum, u32)> = serde_json::from_str(&json).unwrap();
    assert_eq!(round.len(), playlist.len());

    let total_s: u32 = playlist.iter().map(|(_, ms)| ms / 1000).sum();
    println!("festival loop: {} items, {total_s}s", playlist.len());
    println!("{json}");
    assert!((180..300).contains(&total_s), "loop should stay near 4 minutes");
}

#[test]
fn animation_library_advertises_new_variants() {
    let json = serde_json::to_string(&animations::Playlist::get_available()).unwrap();
    for name in ["Aurora", "Starfield", "Snowfall", "FallingLeaves", "Fireflies", "VinylSpin", "CanalBoats", "CenteredTitle"] {
        assert!(json.contains(name), "{name} missing from /api/info/animations");
    }
}
