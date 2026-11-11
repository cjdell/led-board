//! Dev-only contact-sheet renderer: draws festival animations into PPM files
//! so the visuals can be inspected without the minifb window.
//!
//!     cargo run -p sim --example render_sheet -- <out_dir> [times_ms...]
//!
//! Each frame lands as `NN_Name_Tms.ppm` (60x40, P6); `python3` montage helper
//! is intentionally not checked in.
use animations::{Animation, AnimationEnum, AnimationParams, BufferTargetRgb888};
use embedded_graphics::pixelcolor::RgbColor as _;
use std::fs;
use std::io::Write as _;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().unwrap_or_else(|| "tmp/anim_sheet".to_string());
    let times: Vec<u32> = if args.len() == 0 {
        vec![2_000, 6_000, 11_000]
    } else {
        args.map(|a| a.parse().unwrap()).collect()
    };

    fs::create_dir_all(&out).unwrap();

    let mut list: Vec<(&str, AnimationEnum)> = vec![
        ("Aurora", AnimationEnum::Aurora),
        ("Starfield", AnimationEnum::Starfield),
        ("Snowfall", AnimationEnum::Snowfall),
        ("FallingLeaves", AnimationEnum::FallingLeaves),
        ("Fireflies", AnimationEnum::Fireflies),
        ("VinylSpin", AnimationEnum::VinylSpin),
        ("CanalBoats", AnimationEnum::CanalBoats),
        ("Robin", AnimationEnum::Robin),
        ("Owl", AnimationEnum::Owl),
        ("Deer", AnimationEnum::Deer),
        ("SnowLeopard", AnimationEnum::SnowLeopard),
        ("FatherChristmas", AnimationEnum::FatherChristmas),
        ("FrostFairies", AnimationEnum::FrostFairies),
        ("SnowGlobe", AnimationEnum::SnowGlobe),
        ("Penguins", AnimationEnum::Penguins),
        ("Reindeer", AnimationEnum::Reindeer),
        ("ElectricGuitar", AnimationEnum::ElectricGuitar),
        ("TrackChampion", AnimationEnum::TrackChampion),
        ("MadameRatcliffe", AnimationEnum::MadameRatcliffe),
        ("Parrot", AnimationEnum::Parrot),
        ("CircusTent", AnimationEnum::CircusTent),
        ("Elephants", AnimationEnum::Elephants),
        ("SpinningWheel", AnimationEnum::SpinningWheel),
        ("MillBuildings", AnimationEnum::MillBuildings),
    ];

    let mut n = 0;
    for (name, anim) in list.iter_mut() {
        for &ms in &times {
            let mut buf = BufferTargetRgb888::new();
            anim.draw(ms, &AnimationParams::default(), &mut buf);

            let path = format!("{out}/{n:02}_{name}_{ms}.ppm");
            let mut f = fs::File::create(&path).unwrap();
            write!(f, "P6\n60 40\n255\n").unwrap();
            let mut data = Vec::with_capacity(60 * 40 * 3);
            for p in buf.buffer {
                data.extend_from_slice(&[p.r(), p.g(), p.b()]);
            }
            f.write_all(&data).unwrap();
            n += 1;
        }
    }

    println!("wrote {n} frames to {out}");
}
