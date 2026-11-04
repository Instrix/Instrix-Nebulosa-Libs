//! Prints what a device is: GET_INFO, GET_STATS, board status, update info.
//!
//!     cargo run --example info -- --host 192.168.1.50
//!     cargo run --example info --features usb -- --usb
//!     cargo run --example info -- --discover

mod common;

use nebulosa::msg::{board, caps, link};
use std::time::Duration;

fn main() -> nebulosa::Result<()> {
    let a = common::Args::new();
    if a.flag("--discover") {
        for d in nebulosa::discover(Duration::from_millis(600))? {
            println!("{}  firmware {}  {}", d.addr, d.info.fw_version_string(), d.info.mac_string());
        }
        #[cfg(feature = "usb")]
        for u in nebulosa::list_usb()? {
            println!("USB {}:{}  {:04x}  {}  {:?}", u.bus, u.address, u.product_id, u.speed, u.serial);
        }
        return Ok(());
    }
    let mut dev = a.connect();
    let i = dev.get_info()?;
    println!("connected  {}", dev.describe());
    println!("firmware   {} ({})", i.fw_version_string(), i.build);
    println!("FPGA ID    0x{:08X}{}", i.fpga_id, if i.is_format5() { "" } else { " (format 4: raw words only)" });
    println!("link       {} (unit {} B)", i.link_name(), i.link_unit);
    println!("base rate  {} Hz, {} words/block, caps 0x{:02X}", i.base_rate_hz, i.block_words, i.caps);
    if i.has_caps(caps::ETH) {
        println!("ethernet   {}  {}", i.mac_string(), i.ip);
    }
    let s = dev.get_stats()?;
    println!(
        "stats      uptime {:.1} s, stream {}, fpga drops {}",
        s.uptime_ms as f64 / 1e3,
        link::name(s.stream_link),
        s.fpga_drops
    );
    if i.has_caps(caps::BOARD) {
        let b = dev.board_status()?;
        println!(
            "board      0x{:08X}  jack {} mV  vbus {} mV  main {} mV  analog power {}",
            b.board_id,
            b.v_bj_mv,
            b.v_vbus_mv,
            b.v_main_mv,
            if b.has_flags(board::flag::APWR_OK) { "available" } else { "no 12 V" }
        );
        println!("flash      JEDEC 0x{:06X}, {} bytes; PD {} mV {} mA", b.flash_jedec, b.flash_bytes, b.pd_mv, b.pd_ma);
        let tests: Vec<String> = (0..board::test::COUNT)
            .map(|t| format!("{}:{:?}", board::test::NAMES[t as usize], b.test_result(t)))
            .collect();
        println!("last tests {}", tests.join(" "));
        match dev.update_info() {
            Ok(u) => println!(
                "update     firmware {} ({}), boot 0x{:X}, active slot {}, update slot {} ({} KB)",
                u.fw_version_string(),
                u.fw_date,
                u.boot_version,
                u.active,
                u.update_slot,
                u.slot_size / 1024
            ),
            Err(e) => println!("update     {e}"),
        }
    }
    Ok(())
}
