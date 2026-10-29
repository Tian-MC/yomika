//! Manual check: `cargo run -p vnl-graphics --example fullscreen_assist -- :3`
//! against a game running inside gamescope.
fn main() {
    let display = std::env::args().nth(1).expect("usage: fullscreen_assist <display>");
    let outcome = vnl_graphics::fullscreen::ensure_game_fullscreen(&display, std::time::Duration::from_secs(60));
    println!("{outcome:?}");
}
