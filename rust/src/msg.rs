//! Command codes, status codes, constants and payload structs (`nbp.h`).
//!
//! Every `parse` is tolerant: a payload longer than the struct is accepted
//! (newer firmware may append fields), a shorter one gives `None`.

use crate::{le16, le32};
use std::net::Ipv4Addr;

/// NBP protocol version (`NBP_VERSION`).
pub const VERSION: u32 = 1;

/// USB vendor ID.
pub const USB_VID: u16 = 0x1A86;
/// USB product ID: High-Speed / SuperSpeed (Type-A port).
pub const USB_PID_HS: u16 = 0xFE0D;
/// USB product ID: Full-Speed (USB-C port, next to a CDC log interface).
pub const USB_PID_FS: u16 = 0xFE0C;
/// Class of the NBP vendor interface.
pub const USB_CLASS: u8 = 0xFF;
/// Subclass of the NBP vendor interface (`'N'`).
pub const USB_SUBCLASS: u8 = 0x4E;
/// Protocol of the NBP vendor interface.
pub const USB_PROTOCOL: u8 = 0x01;
/// TCP port the device listens on.
pub const TCP_PORT: u16 = 20034;
/// UDP port for discovery.
pub const UDP_DISCOVERY_PORT: u16 = 20035;

/// Message type of DATA blocks.
pub const DATA: u8 = 0x80;

/// The response type of a command.
pub const fn rsp(cmd: u8) -> u8 {
    cmd | 0x40
}

/// FPGA words per DATA block (`NBP_BLOCK_WORDS`).
pub const BLOCK_WORDS: usize = 507;
/// First FPGA ID with stream format 5 (decoded by [`crate::StreamDecoder`]).
pub const FPGA_ID_FORMAT5: u32 = 0x4E42_000B;
/// Board ID of INSTRIX-Nebulosa-Digital (`NBP_BOARD_DIGITAL`).
pub const BOARD_DIGITAL: u32 = 0x4E44_0001;
/// The H417 SPI flash transfer unit: max FLASH_READ length, one page.
pub const FLASH_CHUNK: usize = 256;
/// The SPI flash erase unit.
pub const FLASH_SECTOR: usize = 4096;

/// Command codes (`NBP_CMD_*`).
pub mod cmd {
    /// Any payload, echoed.
    pub const PING: u8 = 0x01;
    /// -> `nbp_info_t`.
    pub const GET_INFO: u8 = 0x02;
    /// FPGA register read.
    pub const REG_READ: u8 = 0x03;
    /// FPGA register write.
    pub const REG_WRITE: u8 = 0x04;
    /// Restart the device (~50 ms later).
    pub const RESET: u8 = 0x05;
    /// Start the sample stream on this link.
    pub const STREAM_START: u8 = 0x10;
    /// Stop the sample stream.
    pub const STREAM_STOP: u8 = 0x11;
    /// -> `nbp_stats_t`.
    pub const GET_STATS: u8 = 0x12;
    /// Select the ADC source.
    pub const SET_SOURCE: u8 = 0x20;
    /// Auto / manual range.
    pub const SET_RANGE: u8 = 0x21;
    /// -> `nbp_range_t`.
    pub const GET_RANGE: u8 = 0x22;
    /// Range switching timing.
    pub const SET_RANGE_TIMING: u8 = 0x23;
    /// Force the range comparators.
    pub const SET_RANGE_FORCE: u8 = 0x24;
    /// Decimation -> `nbp_decim_t` as applied.
    pub const SET_DECIMATION: u8 = 0x30;
    /// -> `nbp_board_status_t`.
    pub const BOARD_STATUS: u8 = 0x38;
    /// Board setting -> `nbp_board_status_t`.
    pub const BOARD_SET: u8 = 0x39;
    /// Board self-tests -> `nbp_board_status_t`.
    pub const BOARD_TEST: u8 = 0x3A;
    /// SPI flash read.
    pub const FLASH_READ: u8 = 0x3B;
    /// SPI flash write (within one 256-byte page).
    pub const FLASH_WRITE: u8 = 0x3C;
    /// SPI flash erase (4 KB aligned).
    pub const FLASH_ERASE: u8 = 0x3D;
    /// Configure the FPGA from flash -> `nbp_board_status_t`.
    pub const FPGA_CONFIG: u8 = 0x3E;
    /// Software update -> `nbp_update_info_t`.
    pub const UPDATE: u8 = 0x3F;
}

/// Response status codes (`NBP_ST_*`).
pub mod status {
    /// Success.
    pub const OK: u8 = 0;
    /// Unknown command.
    pub const UNKNOWN: u8 = 1;
    /// Payload length wrong for the command.
    pub const BAD_LEN: u8 = 2;
    /// Argument out of range.
    pub const BAD_ARG: u8 = 3;
    /// FPGA not answering.
    pub const FPGA: u8 = 4;
    /// Not possible now (stream running, ...).
    pub const BUSY: u8 = 5;
    /// The hardware didn't respond / the operation failed.
    pub const HW: u8 = 6;
    /// The analog supplies need a 12 V source.
    pub const NO_POWER: u8 = 7;
    /// That flash range is the running gateware or the boot record.
    pub const PROTECTED: u8 = 8;

    /// The name of a status code (`"BAD_ARG"`, ...), `"?"` if unknown.
    pub fn name(s: u8) -> &'static str {
        match s {
            OK => "OK",
            UNKNOWN => "UNKNOWN",
            BAD_LEN => "BAD_LEN",
            BAD_ARG => "BAD_ARG",
            FPGA => "FPGA",
            BUSY => "BUSY",
            HW => "HW",
            NO_POWER => "NO_POWER",
            PROTECTED => "PROTECTED",
            _ => "?",
        }
    }
}

/// Link types (`NBP_LINK_*`).
pub mod link {
    /// No link (stream stopped).
    pub const NONE: u32 = 0;
    /// USB Full-Speed.
    pub const USB_FS: u32 = 1;
    /// USB High-Speed.
    pub const USB_HS: u32 = 2;
    /// TCP.
    pub const TCP: u32 = 3;
    /// USB 3.0 SuperSpeed.
    pub const USB_SS: u32 = 4;

    /// A readable name of a link type.
    pub fn name(l: u32) -> &'static str {
        match l {
            USB_FS => "USB Full-Speed",
            USB_HS => "USB High-Speed",
            TCP => "TCP",
            USB_SS => "USB SuperSpeed",
            _ => "none",
        }
    }
}

/// Firmware capability bits (`NBP_CAP_*`, [`Info::caps`]).
pub mod caps {
    /// USB High-Speed link.
    pub const USB_HS: u32 = 1 << 0;
    /// Range control.
    pub const RANGE: u32 = 1 << 1;
    /// Decimation.
    pub const DECIM: u32 = 1 << 2;
    /// Ethernet (TCP) link.
    pub const ETH: u32 = 1 << 3;
    /// Board commands.
    pub const BOARD: u32 = 1 << 4;
    /// USB 3.0 SuperSpeed link.
    pub const USB_SS: u32 = 1 << 5;
}

/// STREAM_START flags (`NBP_START_*`).
pub mod start {
    /// Discard what the FPGA FIFO holds first.
    pub const FLUSH: u32 = 1 << 0;
    /// Link test blocks instead of FPGA data.
    pub const LINK_TEST: u32 = 1 << 1;
}

/// ADC sources (`NBP_SRC_*`).
pub mod source {
    /// FPGA stub, random data.
    pub const STUB_RANDOM: u8 = 0;
    /// FPGA stub, a pattern.
    pub const STUB_PATTERN: u8 = 1;
    /// The AD7387 ADC.
    pub const AD7387: u8 = 2;
}

/// Decimation modes (`NBP_DECIM_*`).
pub mod decim {
    /// Average N conversions.
    pub const AVERAGE: u8 = 0;
    /// Keep one of N.
    pub const SUBSAMPLE: u8 = 1;
}

/// DATA block flags (`NBP_BLK_*`, [`BlockHdr::flags`]).
pub mod blk {
    /// First block after STREAM_START.
    pub const FIRST: u16 = 1 << 0;
    /// The device's block ring was full since the previous block.
    pub const STALL: u16 = 1 << 1;
    /// The QSPI link reported a sequence gap before this block's words.
    pub const GAP: u16 = 1 << 2;
    /// A link test block.
    pub const TEST: u16 = 1 << 3;
}

/// Ethernet flags (`NBP_ETH_*`, [`Stats::eth`]).
pub mod eth {
    /// Cable in, link negotiated.
    pub const PHY: u32 = 1 << 0;
    /// Has an IP address.
    pub const IP: u32 = 1 << 1;
    /// A TCP client is connected.
    pub const CLIENT: u32 = 1 << 2;
}

/// Word `i` of a link test block (`NBP_LINK_TEST_WORD`).
pub const fn link_test_word(i: u32) -> u32 {
    i.wrapping_mul(0x9E37_79B1) ^ 0xA5A5_A5A5
}

/// Board commands: settings, flags, tests, and their values.
pub mod board {
    /// BOARD_SET items (`NBP_BSET_*`).
    pub mod set {
        /// Bits 4:0 LEDs, bit 8 manual (0: firmware status pattern).
        pub const LEDS: u8 = 0;
        /// Isolated analog supply H, 0 / 1.
        pub const APWR_H: u8 = 1;
        /// Isolated analog supply L, 0 / 1.
        pub const APWR_L: u8 = 2;
        /// USB3 mux enable: 0 / 1, 2 = while a USB-C source is attached.
        pub const USB3_EN: u8 = 3;
        /// USB3 mux direction: 0 / 1, 2 = follow the CC lines.
        pub const USB3_DIR: u8 = 4;
        /// Power input selection (`pwr_sel::*`).
        pub const PWR_SEL: u8 = 5;
        /// FPGA PROGRAM_B: 0 release, 1 hold low, 2 pulse.
        pub const FPGA_PROGRAM: u8 = 6;
        /// LCD mode (`lcd::*`).
        pub const LCD: u8 = 7;
        /// RTC counter, seconds.
        pub const RTC: u8 = 8;
        /// USB PD target voltage, mV.
        pub const PD_MV: u8 = 9;
        /// A key for the on-board UI (`key::*`).
        pub const UI_KEY: u8 = 10;
    }

    /// UI keys (`NBP_KEY_*`) for [`set::UI_KEY`].
    pub mod key {
        /// Up / previous item.
        pub const UP: u32 = 1;
        /// Down / next item.
        pub const DOWN: u32 = 2;
        /// Left / previous item.
        pub const LEFT: u32 = 3;
        /// Right / next item.
        pub const RIGHT: u32 = 4;
        /// Select (the button's hold).
        pub const ENTER: u32 = 5;
        /// The page above.
        pub const BACK: u32 = 6;
    }

    /// Power input selection (`NBP_PWR_SEL_*`).
    pub mod pwr_sel {
        /// The power mux picks.
        pub const AUTO: u32 = 0;
        /// Barrel jack (12 V) only.
        pub const BJ: u32 = 1;
        /// USB only.
        pub const USB: u32 = 2;
    }

    /// LCD modes (`NBP_LCD_*`).
    pub mod lcd {
        /// Off.
        pub const OFF: u32 = 0;
        /// The board UI.
        pub const STATUS: u32 = 1;
        /// Colour bars.
        pub const PATTERN: u32 = 2;
    }

    /// [`super::BoardStatus::flags`] bits (`NBP_BOARD_*`).
    pub mod flag {
        /// HSE ready, system clock from it.
        pub const HSE: u32 = 1 << 0;
        /// LSE ready.
        pub const LSE: u32 = 1 << 1;
        /// RTC running on the LSE.
        pub const RTC: u32 = 1 << 2;
        /// DFU button pressed.
        pub const DFU: u32 = 1 << 3;
        /// SD card inserted.
        pub const SD_CARD: u32 = 1 << 4;
        /// Power mux ST pin high.
        pub const PWR_ST: u32 = 1 << 5;
        /// Analog supply H enabled.
        pub const APWR_H: u32 = 1 << 6;
        /// Analog supply L enabled.
        pub const APWR_L: u32 = 1 << 7;
        /// USB3 mux enabled.
        pub const USB3_EN: u32 = 1 << 8;
        /// USB3 mux SEL high.
        pub const USB3_DIR: u32 = 1 << 9;
        /// FPGA DONE.
        pub const FPGA_DONE: u32 = 1 << 10;
        /// FPGA INIT_B high.
        pub const FPGA_INIT_B: u32 = 1 << 11;
        /// PROGRAM_B held low.
        pub const FPGA_PROG_HELD: u32 = 1 << 12;
        /// USB High-Speed link configured.
        pub const USB_HS: u32 = 1 << 13;
        /// Ethernet link up.
        pub const ETH_LINK: u32 = 1 << 14;
        /// LCD initialised.
        pub const LCD: u32 = 1 << 15;
        /// SWD still on.
        pub const SWD: u32 = 1 << 16;
        /// USB3 mux SEL follows the CC lines.
        pub const USB3_AUTO: u32 = 1 << 17;
        /// USB3 mux on while a USB-C source is attached.
        pub const USB3_EN_AUTO: u32 = 1 << 18;
        /// A USB-C source is attached.
        pub const USB_C: u32 = 1 << 19;
        /// A 12 V source feeds V_MAIN.
        pub const APWR_OK: u32 = 1 << 20;
        /// USB SuperSpeed link configured.
        pub const USB_SS: u32 = 1 << 21;
    }

    /// Board self-tests (`NBP_TEST_*`): bit numbers of the test mask.
    pub mod test {
        /// 25 MHz crystal; detail HCLK Hz.
        pub const HSE: u32 = 0;
        /// 32.768 kHz crystal; detail start-up ms.
        pub const LSE: u32 = 1;
        /// RTC on the LSE; detail LSE Hz.
        pub const RTC: u32 = 2;
        /// V_MAIN in range; detail mV.
        pub const RAILS: u32 = 3;
        /// SPI flash JEDEC ID; detail the ID.
        pub const FLASH_ID: u32 = 4;
        /// Flash erase / program / read back; detail bad bytes.
        pub const FLASH_RW: u32 = 5;
        /// SD card; detail error step.
        pub const SD: u32 = 6;
        /// LCD initialised.
        pub const LCD: u32 = 7;
        /// FPGA DONE.
        pub const FPGA_DONE: u32 = 8;
        /// FPGA ID over QSPI; detail the ID.
        pub const FPGA_QSPI: u32 = 9;
        /// Ethernet PHY; detail its ID.
        pub const ETH_PHY: u32 = 10;
        /// USB PD contract; detail mV.
        pub const USB_PD: u32 = 11;
        /// Number of tests.
        pub const COUNT: u32 = 12;
        /// Every test.
        pub const ALL: u32 = (1 << COUNT) - 1;
        /// Short names, by test number.
        pub const NAMES: [&str; COUNT as usize] = [
            "HSE", "LSE", "RTC", "Rails", "Flash ID", "Flash R/W", "SD card", "LCD", "FPGA DONE",
            "FPGA QSPI", "Ethernet PHY", "USB PD",
        ];
    }
}

/// Software update operations and slot types (`NBP_UPD_*`).
pub mod upd {
    /// Report the update state.
    pub const INFO: u8 = 0;
    /// Make a slot's gateware active (tested first).
    pub const GATEWARE: u8 = 1;
    /// Install a slot's firmware image at a restart.
    pub const FIRMWARE: u8 = 2;
    /// Make the previous gateware active again.
    pub const REVERT: u8 = 3;
    /// Restart into the ROM USB bootloader.
    pub const RECOVERY: u8 = 4;
    /// Slot type: empty.
    pub const SLOT_EMPTY: u8 = 0;
    /// Slot type: gateware.
    pub const SLOT_GATEWARE: u8 = 1;
    /// Slot type: firmware.
    pub const SLOT_FIRMWARE: u8 = 2;
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// `major.minor.patch` of a `major << 16 | minor << 8 | patch` version.
pub fn version_string(v: u32) -> String {
    format!("{}.{}.{}", v >> 16, (v >> 8) & 0xFF, v & 0xFF)
}

/// `nbp_info_t`: GET_INFO response (76 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// NBP version (1).
    pub proto_version: u32,
    /// `major << 16 | minor << 8 | patch`.
    pub fw_version: u32,
    /// FPGA ID register.
    pub fpga_id: u32,
    /// Conversions/s before decimation (2,000,000).
    pub base_rate_hz: u32,
    /// The link this response came on (`link::*`).
    pub link: u32,
    /// Transfer unit of the link in bytes (informational).
    pub link_unit: u32,
    /// FPGA words per DATA block.
    pub block_words: u32,
    /// `caps::*` bits.
    pub caps: u32,
    /// Firmware build date / time.
    pub build: String,
    /// Ethernet MAC (zero without Ethernet).
    pub mac: [u8; 6],
    /// IPv4 address (0.0.0.0 without one).
    pub ip: Ipv4Addr,
}

impl Info {
    /// Wire size.
    pub const LEN: usize = 76;

    /// Parses a GET_INFO payload.
    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < Self::LEN {
            return None;
        }
        let mut mac = [0; 6];
        mac.copy_from_slice(&p[64..70]);
        Some(Self {
            proto_version: le32(p, 0),
            fw_version: le32(p, 4),
            fpga_id: le32(p, 8),
            base_rate_hz: le32(p, 12),
            link: le32(p, 16),
            link_unit: le32(p, 20),
            block_words: le32(p, 24),
            caps: le32(p, 28),
            build: cstr(&p[32..64]),
            mac,
            ip: Ipv4Addr::new(p[72], p[73], p[74], p[75]),
        })
    }

    /// The firmware version as `major.minor.patch`.
    pub fn fw_version_string(&self) -> String {
        version_string(self.fw_version)
    }

    /// The link name ("TCP", "USB High-Speed", ...).
    pub fn link_name(&self) -> &'static str {
        link::name(self.link)
    }

    /// The firmware has all of `caps` (`caps::*` bits).
    pub fn has_caps(&self, caps: u32) -> bool {
        self.caps & caps == caps
    }

    /// The FPGA streams format 5 (decoded into samples); older ones only
    /// give raw words ([`crate::Device::read_blocks`]).
    pub fn is_format5(&self) -> bool {
        self.fpga_id >= FPGA_ID_FORMAT5
    }

    /// The MAC as `aa:bb:cc:dd:ee:ff`.
    pub fn mac_string(&self) -> String {
        self.mac.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(":")
    }
}

/// `nbp_stats_t`: GET_STATS response (64 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    /// Device uptime, ms.
    pub uptime_ms: u32,
    /// Link the stream goes to (`link::*`), NONE when stopped.
    pub stream_link: u32,
    /// DATA blocks sent since START.
    pub blocks: u32,
    /// FPGA words sent since START.
    pub words: u32,
    /// Times the device's block ring was full (link too slow).
    pub stalls: u32,
    /// FPGA DROP_COUNT.
    pub fpga_drops: u32,
    /// FPGA FIFO level, last read.
    pub fifo_level: u32,
    /// FPGA RANGE_STATUS.
    pub range_status: u32,
    /// FPGA DECIM.
    pub decim: u32,
    /// FPGA register 31 (source / stub mode).
    pub adc_ctrl: u32,
    /// `eth::*` flags.
    pub eth: u32,
    /// IPv4 address (0.0.0.0: none yet).
    pub eth_ip: Ipv4Addr,
    /// Reserved words.
    pub reserved: [u32; 4],
}

impl Stats {
    /// Wire size.
    pub const LEN: usize = 64;

    /// Parses a GET_STATS payload.
    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < Self::LEN {
            return None;
        }
        let w = |i: usize| le32(p, i * 4);
        Some(Self {
            uptime_ms: w(0),
            stream_link: w(1),
            blocks: w(2),
            words: w(3),
            stalls: w(4),
            fpga_drops: w(5),
            fifo_level: w(6),
            range_status: w(7),
            decim: w(8),
            adc_ctrl: w(9),
            eth: w(10),
            eth_ip: Ipv4Addr::new(p[44], p[45], p[46], p[47]),
            reserved: [w(12), w(13), w(14), w(15)],
        })
    }
}

/// `nbp_decim_t`: decimation as applied (8 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decim {
    /// N = 2^log2n.
    pub log2n: u8,
    /// `decim::*`.
    pub mode: u8,
    /// Conversions/s after decimation.
    pub rate_hz: u32,
}

impl Decim {
    /// Wire size.
    pub const LEN: usize = 8;

    /// Parses a SET_DECIMATION response.
    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= Self::LEN).then(|| Self { log2n: p[0], mode: p[1], rate_hz: le32(p, 4) })
    }
}

/// `nbp_range_t`: GET_RANGE response (16 bytes), FPGA registers 35..38.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeState {
    /// RANGE_STATUS.
    pub status: u32,
    /// RANGE_COUNT.
    pub count: u32,
    /// RANGE_CTRL.
    pub ctrl: u32,
    /// RANGE_TIMING.
    pub timing: u32,
}

impl RangeState {
    /// Wire size.
    pub const LEN: usize = 16;

    /// Parses a GET_RANGE response.
    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= Self::LEN).then(|| Self {
            status: le32(p, 0),
            count: le32(p, 4),
            ctrl: le32(p, 8),
            timing: le32(p, 12),
        })
    }
}

/// `nbp_range_timing_t` (8 bytes), FPGA clock cycles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RangeTiming {
    /// 0..63.
    pub up_filt: u8,
    /// 0..63.
    pub down_filt: u8,
    /// 0..255: both switches closed overlap + 1 cycles.
    pub overlap: u8,
    /// 0..4095.
    pub hold: u16,
}

impl RangeTiming {
    /// Wire size.
    pub const LEN: usize = 8;

    /// Parses the struct.
    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= Self::LEN).then(|| Self { up_filt: p[0], down_filt: p[1], overlap: p[2], hold: le16(p, 4) })
    }

    /// Encodes the struct.
    pub fn encode(&self) -> [u8; 8] {
        let h = self.hold.to_le_bytes();
        [self.up_filt, self.down_filt, self.overlap, 0, h[0], h[1], 0, 0]
    }
}

/// `nbp_block_t` header: the first 12 bytes of a DATA payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockHdr {
    /// Block number since STREAM_START.
    pub seq: u32,
    /// Device time (us, wraps) of the block's burst.
    pub t_us: u32,
    /// FPGA words that follow.
    pub nwords: u16,
    /// `blk::*` flags.
    pub flags: u16,
}

impl BlockHdr {
    /// Header size.
    pub const LEN: usize = 12;

    /// Parses a DATA payload's header.
    pub fn parse(p: &[u8]) -> Option<Self> {
        (p.len() >= Self::LEN).then(|| Self { seq: le32(p, 0), t_us: le32(p, 4), nwords: le16(p, 8), flags: le16(p, 10) })
    }
}

/// Result of one board self-test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestResult {
    /// Not run.
    None,
    /// Passed.
    Pass,
    /// Failed.
    Fail,
    /// Not applicable.
    Skip,
}

/// A USB PD source capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pdo {
    /// The raw PDO.
    pub raw: u32,
    /// A fixed supply (`mv` / `ma` valid); others have them 0.
    pub fixed: bool,
    /// Voltage, mV.
    pub mv: u32,
    /// Maximum current, mA.
    pub ma: u32,
}

/// `nbp_board_status_t` (200 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardStatus {
    /// `BOARD_DIGITAL`.
    pub board_id: u32,
    /// Uptime, ms.
    pub uptime_ms: u32,
    /// Core clock, Hz.
    pub core_hz: u32,
    /// HCLK, Hz.
    pub hclk_hz: u32,
    /// `board::flag::*`.
    pub flags: u32,
    /// RTC counter, seconds.
    pub rtc_s: u32,
    /// Barrel jack input, mV.
    pub v_bj_mv: u16,
    /// USB-C VBUS, mV.
    pub v_vbus_mv: u16,
    /// V_MAIN, mV.
    pub v_main_mv: u16,
    /// LED0..4, bit n = LEDn on.
    pub leds: u8,
    /// 1: LEDs as set; 0: the firmware's status pattern.
    pub led_manual: u8,
    /// `board::pwr_sel::*`.
    pub pwr_sel: u8,
    /// USB-C CC lines at attach, 2 bits each.
    pub cc: u8,
    /// FPGA configuration mode: 0 unknown, 1 master, 2 slave.
    pub fpga_mode: u8,
    /// `board::lcd::*`.
    pub lcd_mode: u8,
    /// 0 none, 1 SDSC v1, 2 SDSC v2, 3 SDHC / SDXC.
    pub sd_type: u8,
    /// Last FPGA configuration time, ms.
    pub fpga_cfg_ms: u32,
    /// Bytes of the last load from the H417 (0: FPGA's own flash).
    pub fpga_cfg_bytes: u32,
    /// FPGA ID register (0: no answer).
    pub fpga_id: u32,
    /// SPI flash JEDEC ID.
    pub flash_jedec: u32,
    /// SPI flash size, bytes.
    pub flash_bytes: u32,
    /// SD card size, 512-byte blocks.
    pub sd_blocks: u32,
    /// SD card CID words 1..4.
    pub sd_cid: [u32; 4],
    /// 2 bits per test: see [`BoardStatus::test_result`].
    pub test_results: u32,
    /// Per test detail (see `board::test`).
    pub test_detail: [u32; 12],
    /// USB PD state: 0 detached, 1..3 negotiating, 4 contract, 5 no PD.
    pub pd_state: u8,
    /// Attached CC line: 0 none, 1 CC1, 2 CC2.
    pub pd_cc_line: u8,
    /// Source PD revision: 1 = 2.0, 2 = 3.0.
    pub pd_spec_rev: u8,
    /// Source capabilities received.
    pub pd_npdo: u8,
    /// Contract voltage, mV.
    pub pd_mv: u16,
    /// Contract current, mA.
    pub pd_ma: u16,
    /// Requested voltage, mV.
    pub pd_target_mv: u16,
    /// Requested object position, 1-based.
    pub pd_pdo_sel: u8,
    /// The source's PDOs, raw.
    pub pd_pdo: [u32; 7],
    /// PS_RDYs since boot.
    pub pd_contracts: u32,
    /// Hard resets received since boot.
    pub pd_hard_resets: u32,
    /// PD errors since boot.
    pub pd_errors: u32,
}

impl BoardStatus {
    /// Wire size.
    pub const LEN: usize = 200;

    /// Parses a board status payload.
    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < Self::LEN {
            return None;
        }
        let arr = |o: usize, out: &mut [u32]| {
            for (k, v) in out.iter_mut().enumerate() {
                *v = le32(p, o + 4 * k);
            }
        };
        let (mut sd_cid, mut test_detail, mut pd_pdo) = ([0; 4], [0; 12], [0; 7]);
        arr(64, &mut sd_cid);
        arr(84, &mut test_detail);
        arr(144, &mut pd_pdo);
        Some(Self {
            board_id: le32(p, 0),
            uptime_ms: le32(p, 4),
            core_hz: le32(p, 8),
            hclk_hz: le32(p, 12),
            flags: le32(p, 16),
            rtc_s: le32(p, 20),
            v_bj_mv: le16(p, 24),
            v_vbus_mv: le16(p, 26),
            v_main_mv: le16(p, 28),
            leds: p[32],
            led_manual: p[33],
            pwr_sel: p[34],
            cc: p[35],
            fpga_mode: p[36],
            lcd_mode: p[37],
            sd_type: p[38],
            fpga_cfg_ms: le32(p, 40),
            fpga_cfg_bytes: le32(p, 44),
            fpga_id: le32(p, 48),
            flash_jedec: le32(p, 52),
            flash_bytes: le32(p, 56),
            sd_blocks: le32(p, 60),
            sd_cid,
            test_results: le32(p, 80),
            test_detail,
            pd_state: p[132],
            pd_cc_line: p[133],
            pd_spec_rev: p[134],
            pd_npdo: p[135],
            pd_mv: le16(p, 136),
            pd_ma: le16(p, 138),
            pd_target_mv: le16(p, 140),
            pd_pdo_sel: p[142],
            pd_pdo,
            pd_contracts: le32(p, 172),
            pd_hard_resets: le32(p, 176),
            pd_errors: le32(p, 180),
        })
    }

    /// Result of test `t` (`board::test::*`) in the last run.
    pub fn test_result(&self, t: u32) -> TestResult {
        if t >= board::test::COUNT {
            return TestResult::None;
        }
        match (self.test_results >> (2 * t)) & 3 {
            1 => TestResult::Pass,
            2 => TestResult::Fail,
            3 => TestResult::Skip,
            _ => TestResult::None,
        }
    }

    /// All of `flags` (`board::flag::*`) are set.
    pub fn has_flags(&self, flags: u32) -> bool {
        self.flags & flags == flags
    }

    /// The source's PDOs received (`pd_npdo` of them), decoded.
    pub fn pdos(&self) -> Vec<Pdo> {
        self.pd_pdo[..(self.pd_npdo as usize).min(7)]
            .iter()
            .map(|&w| {
                let fixed = w >> 30 == 0;
                Pdo {
                    raw: w,
                    fixed,
                    mv: if fixed { ((w >> 10) & 0x3FF) * 50 } else { 0 },
                    ma: if fixed { (w & 0x3FF) * 10 } else { 0 },
                }
            })
            .collect()
    }
}

/// One flash slot of [`UpdateInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// `upd::SLOT_*`.
    pub kind: u8,
    /// Image length.
    pub len: u32,
    /// Image CRC-32.
    pub crc: u32,
    /// Gateware: IP core ID; firmware: version.
    pub id: u32,
}

/// `nbp_update_info_t` (136 bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// Running firmware version.
    pub fw_version: u32,
    /// Git commit.
    pub fw_build_id: u32,
    /// Running image length.
    pub fw_len: u32,
    /// Its body CRC.
    pub fw_crc: u32,
    /// Build date.
    pub fw_date: String,
    /// Bootloader version, 0 = none.
    pub boot_version: u32,
    /// The active gateware slot.
    pub active: u8,
    /// Where an update goes.
    pub update_slot: u8,
    /// A firmware install waits for the restart.
    pub fw_pending: u8,
    /// `status::*` of the operation.
    pub result: u8,
    /// Slot size, bytes (slot n starts at n * slot_size).
    pub slot_size: u32,
    /// The two slots.
    pub slots: [Slot; 2],
    /// Firmware installs the bootloader did.
    pub installs: u32,
    /// What happened (text).
    pub msg: String,
}

impl UpdateInfo {
    /// Wire size.
    pub const LEN: usize = 136;

    /// Parses an UPDATE response.
    pub fn parse(p: &[u8]) -> Option<Self> {
        if p.len() < Self::LEN {
            return None;
        }
        let slot = |o: usize| Slot { kind: p[o], len: le32(p, o + 4), crc: le32(p, o + 8), id: le32(p, o + 12) };
        Some(Self {
            fw_version: le32(p, 0),
            fw_build_id: le32(p, 4),
            fw_len: le32(p, 8),
            fw_crc: le32(p, 12),
            fw_date: cstr(&p[16..40]),
            boot_version: le32(p, 40),
            active: p[44],
            update_slot: p[45],
            fw_pending: p[46],
            result: p[47],
            slot_size: le32(p, 48),
            slots: [slot(52), slot(68)],
            installs: le32(p, 84),
            msg: cstr(&p[88..136]),
        })
    }

    /// The firmware version as `major.minor.patch`.
    pub fn fw_version_string(&self) -> String {
        version_string(self.fw_version)
    }

    /// Flash address of the update slot.
    pub fn update_addr(&self) -> u32 {
        self.update_slot as u32 * self.slot_size
    }
}

// ---- payload encoders -------------------------------------------------------

/// `nbp_reg_t`.
pub fn enc_reg(index: u8, value: u32) -> [u8; 8] {
    let v = value.to_le_bytes();
    [index, 0, 0, 0, v[0], v[1], v[2], v[3]]
}

/// `nbp_range_set_t`.
pub fn enc_range_set(automatic: bool, manual: u8, min: u8, max: u8) -> [u8; 4] {
    [automatic as u8, manual, min, max]
}

/// `nbp_decim_t` request.
pub fn enc_decim(log2n: u8, mode: u8) -> [u8; 8] {
    [log2n, mode, 0, 0, 0, 0, 0, 0]
}

/// `nbp_board_set_t`.
pub fn enc_board_set(item: u8, value: u32) -> [u8; 8] {
    enc_reg(item, value)
}

/// `nbp_flash_t`, followed by `data` (FLASH_WRITE).
pub fn enc_flash(addr: u32, len: u32, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + data.len());
    v.extend_from_slice(&addr.to_le_bytes());
    v.extend_from_slice(&len.to_le_bytes());
    v.extend_from_slice(data);
    v
}

/// `nbp_fpga_config_t`.
pub fn enc_fpga_config(addr: u32, len: u32, flags: u32) -> [u8; 12] {
    let mut v = [0; 12];
    v[0..4].copy_from_slice(&addr.to_le_bytes());
    v[4..8].copy_from_slice(&len.to_le_bytes());
    v[8..12].copy_from_slice(&flags.to_le_bytes());
    v
}

/// `nbp_update_t`.
pub fn enc_update(op: u8, slot: u8, len: u32, crc: u32) -> [u8; 12] {
    let mut v = [op, slot, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    v[4..8].copy_from_slice(&len.to_le_bytes());
    v[8..12].copy_from_slice(&crc.to_le_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put32(p: &mut [u8], o: usize, v: u32) {
        p[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn sizes() {
        assert_eq!(Info::LEN, 76);
        assert_eq!(Stats::LEN, 64);
        assert_eq!(RangeState::LEN, 16);
        assert_eq!(RangeTiming::LEN, 8);
        assert_eq!(Decim::LEN, 8);
        assert_eq!(BoardStatus::LEN, 200);
        assert_eq!(UpdateInfo::LEN, 136);
        assert_eq!(BlockHdr::LEN + 4 * BLOCK_WORDS + 8, 2048);
        // every parser rejects one byte short, accepts longer
        let z = [0u8; 300];
        assert!(Info::parse(&z[..75]).is_none() && Info::parse(&z[..80]).is_some());
        assert!(Stats::parse(&z[..63]).is_none() && Stats::parse(&z[..70]).is_some());
        assert!(RangeState::parse(&z[..15]).is_none() && RangeState::parse(&z[..16]).is_some());
        assert!(Decim::parse(&z[..7]).is_none() && Decim::parse(&z[..8]).is_some());
        assert!(BoardStatus::parse(&z[..199]).is_none() && BoardStatus::parse(&z[..250]).is_some());
        assert!(UpdateInfo::parse(&z[..135]).is_none() && UpdateInfo::parse(&z[..136]).is_some());
    }

    #[test]
    fn info_layout() {
        let mut p = vec![0u8; Info::LEN];
        put32(&mut p, 4, 0x01_04_02);
        put32(&mut p, 8, 0x4E42_000B);
        put32(&mut p, 16, link::TCP);
        put32(&mut p, 28, caps::ETH | caps::BOARD);
        p[32..37].copy_from_slice(b"build");
        p[64..70].copy_from_slice(&[2, 0x4e, 0x42, 0, 0, 1]);
        p[72..76].copy_from_slice(&[192, 168, 1, 50]);
        let i = Info::parse(&p).unwrap();
        assert_eq!(i.fw_version_string(), "1.4.2");
        assert!(i.is_format5() && i.has_caps(caps::ETH) && !i.has_caps(caps::USB_HS));
        assert_eq!(i.build, "build");
        assert_eq!(i.mac_string(), "02:4e:42:00:00:01");
        assert_eq!(i.ip, Ipv4Addr::new(192, 168, 1, 50));
        assert_eq!(i.link_name(), "TCP");
    }

    #[test]
    fn board_status_layout() {
        let mut p = vec![0u8; BoardStatus::LEN];
        put32(&mut p, 0, BOARD_DIGITAL);
        put32(&mut p, 16, board::flag::APWR_OK | board::flag::USB_SS);
        p[24..26].copy_from_slice(&12_050u16.to_le_bytes());
        p[28..30].copy_from_slice(&11_900u16.to_le_bytes());
        p[32] = 0x15;
        p[38] = 3;
        put32(&mut p, 48, 0x4E42_000B);
        put32(&mut p, 76, 0xC1D);
        put32(&mut p, 80, 1 | 2 << 2 | 3 << 12);
        put32(&mut p, 84, 150_000_000);
        put32(&mut p, 128, 9000);
        p[132] = 4;
        p[135] = 2;
        p[136..138].copy_from_slice(&12_000u16.to_le_bytes());
        p[142] = 2;
        put32(&mut p, 144, (100 << 10) | 300);
        put32(&mut p, 148, (240 << 10) | 225);
        put32(&mut p, 180, 7);
        let s = BoardStatus::parse(&p).unwrap();
        assert!(s.has_flags(board::flag::APWR_OK | board::flag::USB_SS) && !s.has_flags(board::flag::APWR_H));
        assert_eq!((s.v_bj_mv, s.v_main_mv, s.leds, s.sd_type), (12_050, 11_900, 0x15, 3));
        assert_eq!((s.fpga_id, s.sd_cid[3]), (0x4E42_000B, 0xC1D));
        assert_eq!(s.test_result(board::test::HSE), TestResult::Pass);
        assert_eq!(s.test_result(board::test::LSE), TestResult::Fail);
        assert_eq!(s.test_result(board::test::RTC), TestResult::None);
        assert_eq!(s.test_result(board::test::SD), TestResult::Skip);
        assert_eq!((s.test_detail[0], s.test_detail[11]), (150_000_000, 9000));
        assert_eq!((s.pd_state, s.pd_mv, s.pd_pdo_sel, s.pd_errors), (4, 12_000, 2, 7));
        let pd = s.pdos();
        assert_eq!((pd[0].mv, pd[0].ma, pd[1].mv, pd[1].ma), (5000, 3000, 12_000, 2250));
    }

    #[test]
    fn update_info_layout() {
        let mut p = vec![0u8; UpdateInfo::LEN];
        put32(&mut p, 0, 0x0002_0000);
        p[16..21].copy_from_slice(b"Nov 3");
        p[45] = 1;
        p[47] = status::HW;
        put32(&mut p, 48, 448 * 1024);
        p[68] = upd::SLOT_FIRMWARE;
        put32(&mut p, 72, 1234);
        put32(&mut p, 84, 3);
        p[88..92].copy_from_slice(b"done");
        let i = UpdateInfo::parse(&p).unwrap();
        assert_eq!((i.fw_version_string().as_str(), i.update_slot, i.slot_size), ("2.0.0", 1, 448 * 1024));
        assert_eq!(i.update_addr(), 448 * 1024);
        assert_eq!((i.slots[1].kind, i.slots[1].len, i.installs, i.result), (upd::SLOT_FIRMWARE, 1234, 3, 6));
        assert_eq!((i.fw_date.as_str(), i.msg.as_str()), ("Nov 3", "done"));
    }

    #[test]
    fn encodings() {
        assert_eq!(enc_reg(3, 0x0102_0304), [3, 0, 0, 0, 4, 3, 2, 1]);
        assert_eq!(enc_range_set(false, 2, 0, 5), [0, 2, 0, 5]);
        assert_eq!(enc_decim(4, 1), [4, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(enc_update(1, 1, 0x10, 0xAABBCCDD), [1, 1, 0, 0, 0x10, 0, 0, 0, 0xDD, 0xCC, 0xBB, 0xAA]);
        assert_eq!(enc_flash(0x1000, 2, &[9, 8]), vec![0, 0x10, 0, 0, 2, 0, 0, 0, 9, 8]);
        assert_eq!(enc_fpga_config(1, 2, 0)[..8], [1, 0, 0, 0, 2, 0, 0, 0]);
        let t = RangeTiming { up_filt: 1, down_filt: 2, overlap: 3, hold: 0x0405 };
        assert_eq!(t.encode(), [1, 2, 3, 0, 5, 4, 0, 0]);
        assert_eq!(RangeTiming::parse(&t.encode()), Some(t));
        assert_eq!(link_test_word(0), 0xA5A5_A5A5);
    }
}
