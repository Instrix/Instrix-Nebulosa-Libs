//! Runs a light across the board's five LEDs, then gives them back to the
//! firmware.
//!
//!     cargo run --example board_leds -- --host H [--rounds 3]

mod common;

use std::time::Duration;

fn main() -> nebulosa::Result<()> {
    let a = common::Args::new();
    let rounds: u32 = a.num("--rounds", 3);
    let mut dev = a.connect();
    let r = (|| -> nebulosa::Result<()> {
        for _ in 0..rounds {
            for k in (0..5).chain((1..4).rev()) {
                let s = dev.set_leds(1 << k)?;
                print!("\rLEDs {:05b}", s.leds);
                use std::io::Write;
                std::io::stdout().flush().ok();
                std::thread::sleep(Duration::from_millis(80));
            }
        }
        Ok(())
    })();
    let s = dev.leds_auto()?;
    println!("\rLEDs back to the status pattern (manual {})", s.led_manual);
    r
}
