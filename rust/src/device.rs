//! A Nebulosa device: request / response, every NBP command, the sample
//! stream, and the flash / update helpers.

use crate::calibration::Calibration;
use crate::error::{Error, Result};
use crate::frame::{encode, Frame, FrameParser, MAX_PAYLOAD};
use crate::msg::{
    self, board, cmd, enc_board_set, enc_decim, enc_flash, enc_fpga_config, enc_range_set, enc_reg, enc_update, rsp,
    upd, BlockHdr, BoardStatus, Decim, Info, RangeState, RangeTiming, Stats, UpdateInfo, FLASH_CHUNK, FLASH_SECTOR,
};
use crate::stream::{Sample, StreamDecoder, Word};
use crate::transport::{TcpTransport, Transport};
use crate::update::{bitstream, crc32, firmware_check};
use std::collections::VecDeque;
use std::fmt;
use std::time::{Duration, Instant};

/// Bytes asked of the transport per read.
const READ_BUF: usize = 64 * 1024;
/// Stale responses kept at most (they are discarded at the next request).
const RSP_QUEUE_MAX: usize = 64;
/// Flash erase request size in [`Device::upload_flash`].
const ERASE_CHUNK: usize = 64 * 1024;

/// Response timeouts, per kind of command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Every command not listed below (1.5 s).
    pub default: Duration,
    /// BOARD_TEST (15 s).
    pub board_test: Duration,
    /// FPGA_CONFIG (10 s).
    pub fpga_config: Duration,
    /// FLASH_ERASE (5 s, per request).
    pub flash_erase: Duration,
    /// UPDATE, ops other than the two below (30 s).
    pub update: Duration,
    /// UPDATE GATEWARE: the device test-loads the FPGA (15 s).
    pub update_gateware: Duration,
    /// UPDATE FIRMWARE: the device checks the image (10 s).
    pub update_firmware: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            default: Duration::from_millis(1500),
            board_test: Duration::from_secs(15),
            fpga_config: Duration::from_secs(10),
            flash_erase: Duration::from_secs(5),
            update: Duration::from_secs(30),
            update_gateware: Duration::from_secs(15),
            update_firmware: Duration::from_secs(10),
        }
    }
}

/// How to start the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamOptions {
    /// Discard what the FPGA FIFO holds first (default true).
    pub flush: bool,
    /// Link test: blocks of a known pattern, no FPGA data (default false).
    pub link_test: bool,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self { flush: true, link_test: false }
    }
}

impl StreamOptions {
    fn flags(&self) -> u32 {
        (if self.flush { msg::start::FLUSH } else { 0 }) | (if self.link_test { msg::start::LINK_TEST } else { 0 })
    }
}

/// Stream counters: the decoder's, and the DATA block level ones. Reset by
/// [`Device::start_stream`] and by a block flagged FIRST.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamCounters {
    /// Samples decoded.
    pub samples: u64,
    /// Of them, dirty.
    pub dirty: u64,
    /// FPGA blocks (headers completed).
    pub blocks: u64,
    /// Empty words.
    pub empty: u64,
    /// LATE words.
    pub late: u64,
    /// FPGA blocks carrying OVF.
    pub ovf_blocks: u64,
    /// Range changes between FPGA blocks.
    pub range_changes: u64,
    /// FPGA block sequence errors.
    pub seq_errors: u64,
    /// Words out of place.
    pub malformed: u64,
    /// Samples before the first header.
    pub unframed: u64,
    /// Sample periods between FPGA blocks without samples.
    pub gap_periods: u64,
    /// DATA blocks received.
    pub data_blocks: u64,
    /// DATA blocks lost (jumps in `nbp_block_t.seq`).
    pub lost_blocks: u64,
    /// DATA blocks flagged STALL.
    pub stall_flags: u64,
    /// DATA blocks flagged GAP.
    pub gap_flags: u64,
    /// Link test blocks.
    pub test_blocks: u64,
    /// Frame parser resyncs (garbage skipped).
    pub resyncs: u64,
    /// DATA payloads too short to hold a block header.
    pub bad_blocks: u64,
}

/// A raw DATA block ([`Device::read_blocks`]), for FPGAs whose stream
/// format is not decoded (format 4), or for link tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Block number since STREAM_START.
    pub seq: u32,
    /// Device time, us (wraps).
    pub t_us: u32,
    /// `msg::blk::*` flags.
    pub flags: u16,
    /// FPGA words, converted from big-endian to host order.
    pub words: Vec<u32>,
}

/// Where [`Device::upload_flash`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadStage {
    /// Erasing sectors.
    Erasing,
    /// Writing pages.
    Writing,
    /// Reading back.
    Verifying,
}

impl fmt::Display for UploadStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UploadStage::Erasing => "erasing",
            UploadStage::Writing => "writing",
            UploadStage::Verifying => "verifying",
        })
    }
}

/// Progress of [`Device::upload_flash`]: `done` of `total` bytes in `stage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadProgress {
    /// The stage.
    pub stage: UploadStage,
    /// Bytes done in this stage.
    pub done: usize,
    /// Bytes in this stage.
    pub total: usize,
}

#[derive(Default)]
struct StreamState {
    dec: StreamDecoder,
    expected: Option<u32>,
    data_blocks: u64,
    lost_blocks: u64,
    stall_flags: u64,
    gap_flags: u64,
    test_blocks: u64,
    bad_blocks: u64,
    resync_base: u64,
    /// After START: drop blocks from an earlier stream until this one's
    /// first block (FIRST, or seq 0).
    wait_first: bool,
    /// The DATA payload being decoded, and the word cursor in it.
    cur: Vec<u8>,
    pos: usize,
    end: usize,
}

/// A connection to one Nebulosa.
///
/// One command is in flight at a time. DATA frames that arrive while a
/// command waits for its response are queued for the stream reader, never
/// dropped.
pub struct Device {
    t: Box<dyn Transport>,
    parsers: Vec<FrameParser>,
    rbuf: Vec<u8>,
    seq: u16,
    data_q: VecDeque<Frame>,
    rsp_q: VecDeque<Frame>,
    timeouts: Timeouts,
    cal: Calibration,
    st: StreamState,
}

impl fmt::Debug for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Device").field("transport", &self.t.describe()).finish_non_exhaustive()
    }
}

impl Device {
    /// Connects over TCP: `"192.168.1.50"`, `"host:port"`, ... (port 20034
    /// by default). The device takes one client at a time: close the
    /// Companion first.
    pub fn tcp(addr: &str) -> Result<Self> {
        Ok(Self::with_transport(TcpTransport::connect(addr)?))
    }

    /// Opens a device over USB.
    #[cfg(feature = "usb")]
    pub fn usb(sel: crate::usb::UsbSelector) -> Result<Self> {
        Ok(Self::with_transport(crate::usb::UsbTransport::open(&sel)?))
    }

    /// Runs NBP over any [`Transport`].
    pub fn with_transport(t: impl Transport + 'static) -> Self {
        Self {
            t: Box::new(t),
            parsers: vec![FrameParser::new()],
            rbuf: vec![0; READ_BUF],
            seq: 0,
            data_q: VecDeque::new(),
            rsp_q: VecDeque::new(),
            timeouts: Timeouts::default(),
            cal: Calibration::default(),
            st: StreamState::default(),
        }
    }

    /// A description of the transport ("TCP 1.2.3.4:20034", ...).
    pub fn describe(&self) -> String {
        self.t.describe()
    }

    /// The response timeouts.
    pub fn timeouts(&self) -> &Timeouts {
        &self.timeouts
    }

    /// The response timeouts, to change them.
    pub fn timeouts_mut(&mut self) -> &mut Timeouts {
        &mut self.timeouts
    }

    /// The calibration samples are converted with.
    pub fn calibration(&self) -> &Calibration {
        &self.cal
    }

    /// Replaces the calibration (applies to samples decoded from now on).
    pub fn set_calibration(&mut self, cal: Calibration) {
        self.cal = cal;
    }

    // ---- request / response -------------------------------------------------

    /// One read from the transport, sorted into the DATA and response
    /// queues. `Ok(false)`: nothing came within `timeout`.
    fn pump(&mut self, timeout: Duration) -> Result<bool> {
        let Some((src, n)) = self.t.read(&mut self.rbuf, timeout)? else {
            return Ok(false);
        };
        if self.parsers.len() <= src {
            self.parsers.resize_with(src + 1, FrameParser::new);
        }
        let p = &mut self.parsers[src];
        p.push(&self.rbuf[..n]);
        while let Some(f) = p.next_frame() {
            if f.ty == msg::DATA {
                self.data_q.push_back(f);
            } else {
                if self.rsp_q.len() >= RSP_QUEUE_MAX {
                    self.rsp_q.pop_front();
                }
                self.rsp_q.push_back(f);
            }
        }
        Ok(true)
    }

    /// Sends a command and waits for its response, whatever its status.
    pub fn transact(&mut self, command: u8, payload: &[u8], timeout: Duration) -> Result<Frame> {
        if payload.len() > MAX_PAYLOAD {
            return Err(Error::Invalid(format!("payload of {} bytes (max {MAX_PAYLOAD})", payload.len())));
        }
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        self.rsp_q.clear();
        self.t.write_all(&encode(command, seq, payload))?;
        let deadline = Instant::now() + timeout;
        loop {
            while let Some(f) = self.rsp_q.pop_front() {
                if f.ty == rsp(command) && f.seq == seq {
                    return Ok(f);
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout { cmd: command });
            }
            self.pump(deadline - now)?;
        }
    }

    /// Sends a command and returns its response payload; a status other
    /// than OK is [`Error::Status`].
    pub fn request(&mut self, command: u8, payload: &[u8], timeout: Duration) -> Result<Vec<u8>> {
        let f = self.transact(command, payload, timeout)?;
        if f.status != msg::status::OK {
            return Err(Error::Status { cmd: command, status: f.status });
        }
        Ok(f.payload)
    }

    fn req(&mut self, command: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let t = self.timeouts.default;
        self.request(command, payload, t)
    }

    fn parsed<T>(command: u8, p: &[u8], what: &str, f: fn(&[u8]) -> Option<T>) -> Result<T> {
        f(p).ok_or_else(|| Error::BadResponse(format!("command 0x{command:02X}: {what} of {} bytes", p.len())))
    }

    // ---- commands -----------------------------------------------------------

    /// PING: the device echoes `data` (at most 504 bytes).
    pub fn ping(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        self.req(cmd::PING, data)
    }

    /// GET_INFO: firmware, FPGA, link, capabilities, network.
    pub fn get_info(&mut self) -> Result<Info> {
        let p = self.req(cmd::GET_INFO, &[])?;
        Self::parsed(cmd::GET_INFO, &p, "info", Info::parse)
    }

    /// REG_READ: an FPGA register.
    pub fn reg_read(&mut self, index: u8) -> Result<u32> {
        let p = self.req(cmd::REG_READ, &enc_reg(index, 0))?;
        if p.len() < 8 {
            return Err(Error::BadResponse(format!("REG_READ response of {} bytes", p.len())));
        }
        Ok(crate::le32(&p, 4))
    }

    /// REG_WRITE: an FPGA register.
    pub fn reg_write(&mut self, index: u8, value: u32) -> Result<()> {
        self.req(cmd::REG_WRITE, &enc_reg(index, value)).map(drop)
    }

    /// RESET: the device restarts ~50 ms later and the connection drops;
    /// open a new [`Device`] afterwards.
    pub fn reset(&mut self) -> Result<()> {
        match self.req(cmd::RESET, &[]) {
            Ok(_) | Err(Error::Closed) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// STREAM_START: DATA blocks follow on this link. Resets the decoder,
    /// the stream counters, and drops DATA still queued from an earlier
    /// stream.
    pub fn start_stream(&mut self, opts: StreamOptions) -> Result<()> {
        self.data_q.clear();
        self.reset_stream_state();
        self.st.wait_first = true;
        self.req(cmd::STREAM_START, &opts.flags().to_le_bytes()).map(drop)
    }

    /// STREAM_STOP. DATA already received stays readable.
    pub fn stop_stream(&mut self) -> Result<()> {
        self.req(cmd::STREAM_STOP, &[]).map(drop)
    }

    /// GET_STATS: device side stream statistics.
    pub fn get_stats(&mut self) -> Result<Stats> {
        let p = self.req(cmd::GET_STATS, &[])?;
        Self::parsed(cmd::GET_STATS, &p, "stats", Stats::parse)
    }

    /// SET_SOURCE: `msg::source::*` (0 stub random, 1 stub pattern, 2 AD7387).
    pub fn set_source(&mut self, src: u8) -> Result<()> {
        self.req(cmd::SET_SOURCE, &[src, 0, 0, 0]).map(drop)
    }

    /// SET_RANGE: automatic ranging between ranges `min` and `max` (0..5;
    /// 0 is the highest current range).
    pub fn set_range_auto(&mut self, min: u8, max: u8) -> Result<()> {
        self.req(cmd::SET_RANGE, &enc_range_set(true, 0, min, max)).map(drop)
    }

    /// SET_RANGE: a fixed range (0 = 2 A .. 5 = 20 uA).
    pub fn set_range_manual(&mut self, range: u8) -> Result<()> {
        self.req(cmd::SET_RANGE, &enc_range_set(false, range, 0, 5)).map(drop)
    }

    /// GET_RANGE: the FPGA range registers.
    pub fn get_range(&mut self) -> Result<RangeState> {
        let p = self.req(cmd::GET_RANGE, &[])?;
        Self::parsed(cmd::GET_RANGE, &p, "range", RangeState::parse)
    }

    /// SET_RANGE_TIMING, in FPGA clock cycles.
    pub fn set_range_timing(&mut self, up_filt: u8, down_filt: u8, overlap: u8, hold: u16) -> Result<()> {
        let t = RangeTiming { up_filt, down_filt, overlap, hold };
        self.req(cmd::SET_RANGE_TIMING, &t.encode()).map(drop)
    }

    /// SET_RANGE_FORCE: comparators from `value` when `enable`.
    pub fn set_range_force(&mut self, enable: bool, value: u8) -> Result<()> {
        self.req(cmd::SET_RANGE_FORCE, &[enable as u8, value, 0, 0]).map(drop)
    }

    /// SET_DECIMATION: N = 2^`log2n` (0..8), `mode` `msg::decim::*`.
    /// Returns what was applied.
    pub fn set_decimation(&mut self, log2n: u8, mode: u8) -> Result<Decim> {
        let p = self.req(cmd::SET_DECIMATION, &enc_decim(log2n, mode))?;
        Self::parsed(cmd::SET_DECIMATION, &p, "decimation", Decim::parse)
    }

    /// BOARD_STATUS.
    pub fn board_status(&mut self) -> Result<BoardStatus> {
        let p = self.req(cmd::BOARD_STATUS, &[])?;
        Self::parsed(cmd::BOARD_STATUS, &p, "board status", BoardStatus::parse)
    }

    /// BOARD_SET: `item` `msg::board::set::*`. Returns the status after.
    pub fn board_set(&mut self, item: u8, value: u32) -> Result<BoardStatus> {
        let p = self.req(cmd::BOARD_SET, &enc_board_set(item, value))?;
        Self::parsed(cmd::BOARD_SET, &p, "board status", BoardStatus::parse)
    }

    /// BOARD_TEST: runs the tests in `mask` (`msg::board::test::ALL`).
    pub fn board_test(&mut self, mask: u32) -> Result<BoardStatus> {
        let t = self.timeouts.board_test;
        let p = self.request(cmd::BOARD_TEST, &mask.to_le_bytes(), t)?;
        Self::parsed(cmd::BOARD_TEST, &p, "board status", BoardStatus::parse)
    }

    /// FLASH_READ: `len` (<= 256) bytes of the SPI flash.
    pub fn flash_read(&mut self, addr: u32, len: usize) -> Result<Vec<u8>> {
        if len > FLASH_CHUNK {
            return Err(Error::Invalid(format!("flash_read of {len} bytes (max {FLASH_CHUNK})")));
        }
        let mut p = self.req(cmd::FLASH_READ, &enc_flash(addr, len as u32, &[]))?;
        if p.len() < 8 + len {
            return Err(Error::BadResponse(format!("FLASH_READ: {} bytes for {len}", p.len())));
        }
        p.truncate(8 + len);
        p.drain(..8);
        Ok(p)
    }

    /// FLASH_WRITE: `data` within one 256-byte page (flash must be erased).
    pub fn flash_write(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        if data.len() > FLASH_CHUNK || (addr as usize % FLASH_CHUNK) + data.len() > FLASH_CHUNK {
            return Err(Error::Invalid(format!("flash_write of {} bytes at 0x{addr:X} crosses a page", data.len())));
        }
        self.req(cmd::FLASH_WRITE, &enc_flash(addr, data.len() as u32, data)).map(drop)
    }

    /// FLASH_ERASE: `addr` and `len` 4 KB aligned.
    pub fn flash_erase(&mut self, addr: u32, len: u32) -> Result<()> {
        if addr as usize % FLASH_SECTOR != 0 || len as usize % FLASH_SECTOR != 0 {
            return Err(Error::Invalid(format!("flash_erase 0x{addr:X}+0x{len:X}: not 4 KB aligned")));
        }
        let t = self.timeouts.flash_erase;
        self.request(cmd::FLASH_ERASE, &enc_flash(addr, len, &[]), t).map(drop)
    }

    /// FPGA_CONFIG: loads the FPGA from a raw bitstream in the SPI flash.
    pub fn fpga_config(&mut self, addr: u32, len: u32) -> Result<BoardStatus> {
        let t = self.timeouts.fpga_config;
        let p = self.request(cmd::FPGA_CONFIG, &enc_fpga_config(addr, len, 0), t)?;
        Self::parsed(cmd::FPGA_CONFIG, &p, "board status", BoardStatus::parse)
    }

    /// UPDATE with an operation (`msg::upd::*`). A status other than OK is
    /// [`Error::Update`] when the device sent its update info along (its
    /// `msg` says why), else [`Error::Status`].
    pub fn update(&mut self, op: u8, slot: u8, len: u32, crc: u32) -> Result<UpdateInfo> {
        let t = match op {
            upd::GATEWARE => self.timeouts.update_gateware,
            upd::FIRMWARE => self.timeouts.update_firmware,
            upd::INFO => self.timeouts.default,
            _ => self.timeouts.update,
        };
        let f = self.transact(cmd::UPDATE, &enc_update(op, slot, len, crc), t)?;
        let info = UpdateInfo::parse(&f.payload);
        if f.status != msg::status::OK {
            return Err(match info {
                Some(info) => Error::Update { status: f.status, info: Box::new(info) },
                None => Error::Status { cmd: cmd::UPDATE, status: f.status },
            });
        }
        info.ok_or_else(|| Error::BadResponse(format!("UPDATE: {} bytes", f.payload.len())))
    }

    /// UPDATE INFO: firmware, bootloader and update slot state.
    pub fn update_info(&mut self) -> Result<UpdateInfo> {
        self.update(upd::INFO, 0, 0, 0)
    }

    // ---- helpers --------------------------------------------------------------

    /// LEDs by hand: bit n of `mask` = LEDn (0..4) on.
    pub fn set_leds(&mut self, mask: u8) -> Result<BoardStatus> {
        self.board_set(board::set::LEDS, (mask as u32 & 0x1F) | 0x100)
    }

    /// LEDs back to the firmware's status pattern.
    pub fn leds_auto(&mut self) -> Result<BoardStatus> {
        self.board_set(board::set::LEDS, 0)
    }

    /// A key for the on-board UI (`msg::board::key::*`), as if pressed.
    pub fn ui_key(&mut self, key: u32) -> Result<BoardStatus> {
        self.board_set(board::set::UI_KEY, key)
    }

    /// Writes `data` to the SPI flash at `addr` (4 KB aligned): erases the
    /// sectors it covers (64 KB per request), writes 256-byte pages
    /// (skipping blank ones), then reads everything back and compares.
    pub fn upload_flash(&mut self, addr: u32, data: &[u8], mut progress: impl FnMut(UploadProgress)) -> Result<()> {
        if addr as usize % FLASH_SECTOR != 0 {
            return Err(Error::Invalid(format!("upload_flash at 0x{addr:X}: not 4 KB aligned")));
        }
        let mut prog = |stage, done, total| progress(UploadProgress { stage, done, total });
        let span = data.len().div_ceil(FLASH_SECTOR) * FLASH_SECTOR;
        let mut off = 0;
        while off < span {
            let n = ERASE_CHUNK.min(span - off);
            self.flash_erase(addr + off as u32, n as u32)?;
            off += n;
            prog(UploadStage::Erasing, off, span);
        }
        for (k, page) in data.chunks(FLASH_CHUNK).enumerate() {
            if page.iter().any(|&b| b != 0xFF) {
                self.flash_write(addr + (k * FLASH_CHUNK) as u32, page)?;
            }
            prog(UploadStage::Writing, k * FLASH_CHUNK + page.len(), data.len());
        }
        for (k, page) in data.chunks(FLASH_CHUNK).enumerate() {
            let a = addr + (k * FLASH_CHUNK) as u32;
            let got = self.flash_read(a, page.len())?;
            if let Some(i) = (0..page.len()).find(|&i| got[i] != page[i]) {
                return Err(Error::Verify { addr: a + i as u32, expected: page[i], got: got[i] });
            }
            prog(UploadStage::Verifying, k * FLASH_CHUNK + page.len(), data.len());
        }
        Ok(())
    }

    fn install(&mut self, op: u8, image: &[u8], progress: impl FnMut(UploadProgress)) -> Result<UpdateInfo> {
        let i = self.update_info()?;
        if image.is_empty() || image.len() > i.slot_size as usize {
            return Err(Error::Invalid(format!("the image is {} bytes; the slot holds {}", image.len(), i.slot_size)));
        }
        self.upload_flash(i.update_addr(), image, progress)?;
        let info = self.update(op, i.update_slot, image.len() as u32, crc32(image))?;
        if info.result != msg::status::OK {
            return Err(Error::Update { status: info.result, info: Box::new(info) });
        }
        Ok(info)
    }

    /// Installs a gateware (`.bit` or raw `.bin`): writes it to the update
    /// slot, then the device test-loads it and makes it active only if it
    /// works (else the old one stays, [`Error::Update`]).
    pub fn update_gateware(&mut self, file: &[u8]) -> Result<UpdateInfo> {
        self.update_gateware_with_progress(file, |_| {})
    }

    /// [`Device::update_gateware`] with upload progress.
    pub fn update_gateware_with_progress(
        &mut self,
        file: &[u8],
        progress: impl FnMut(UploadProgress),
    ) -> Result<UpdateInfo> {
        let image = bitstream(file).map_err(Error::Invalid)?;
        self.install(upd::GATEWARE, &image, progress)
    }

    /// Installs a firmware image (`.nbfw`, checked first): writes it to the
    /// update slot and marks it pending; the device restarts ~0.5 s after
    /// answering and its bootloader installs it. The connection drops:
    /// reconnect and check [`Device::update_info`] (`fw_version` new,
    /// `fw_pending` 0).
    pub fn update_firmware(&mut self, image: &[u8]) -> Result<UpdateInfo> {
        self.update_firmware_with_progress(image, |_| {})
    }

    /// [`Device::update_firmware`] with upload progress.
    pub fn update_firmware_with_progress(
        &mut self,
        image: &[u8],
        progress: impl FnMut(UploadProgress),
    ) -> Result<UpdateInfo> {
        firmware_check(image).map_err(Error::Invalid)?;
        self.install(upd::FIRMWARE, image, progress)
    }

    /// Makes the previous gateware (the other slot) active again.
    pub fn update_revert(&mut self) -> Result<UpdateInfo> {
        self.update(upd::REVERT, 0, 0, 0)
    }

    // ---- stream -----------------------------------------------------------------

    fn total_resyncs(&self) -> u64 {
        self.parsers.iter().map(|p| p.resyncs).sum()
    }

    fn reset_stream_state(&mut self) {
        let resync_base = self.total_resyncs();
        self.st = StreamState { resync_base, cur: std::mem::take(&mut self.st.cur), ..Default::default() };
        self.st.pos = 0;
        self.st.end = 0;
    }

    /// Block level accounting (API.md section 5, steps 1..4). Returns the
    /// header when the block's words are to be used.
    fn account(&mut self, payload: &[u8]) -> Option<BlockHdr> {
        let Some(h) = BlockHdr::parse(payload) else {
            self.st.bad_blocks += 1;
            return None;
        };
        if self.st.wait_first {
            if h.flags & msg::blk::FIRST == 0 && h.seq != 0 {
                return None; // from a stream before this START
            }
            self.st.wait_first = false;
        }
        if h.flags & msg::blk::FIRST != 0 {
            self.reset_stream_state();
        }
        let st = &mut self.st;
        if let Some(e) = st.expected {
            if h.seq != e {
                st.lost_blocks += h.seq.wrapping_sub(e) as u64;
                st.dec.desync();
            }
        }
        st.expected = Some(h.seq.wrapping_add(1));
        st.data_blocks += 1;
        if h.flags & msg::blk::TEST != 0 {
            st.test_blocks += 1;
            return Some(h);
        }
        if h.flags & msg::blk::STALL != 0 {
            st.stall_flags += 1;
            st.dec.desync();
        }
        if h.flags & msg::blk::GAP != 0 {
            st.gap_flags += 1;
            st.dec.desync();
        }
        Some(h)
    }

    fn begin_block(&mut self, f: Frame) {
        let Some(h) = self.account(&f.payload) else { return };
        if h.flags & msg::blk::TEST != 0 {
            return;
        }
        let nw = (h.nwords as usize).min((f.payload.len() - BlockHdr::LEN) / 4);
        self.st.cur = f.payload;
        self.st.pos = BlockHdr::LEN;
        self.st.end = BlockHdr::LEN + 4 * nw;
    }

    fn decode_into(&mut self, out: &mut Vec<Sample>, max: usize) -> usize {
        let cal = self.cal;
        let st = &mut self.st;
        let mut n = 0;
        while st.pos < st.end && n < max {
            let b = &st.cur[st.pos..st.pos + 4];
            st.pos += 4;
            if let Word::Sample(s) = st.dec.feed(u32::from_be_bytes([b[0], b[1], b[2], b[3]])) {
                out.push(s.calibrate(&cal));
                n += 1;
            }
        }
        n
    }

    /// Appends up to `max` decoded samples to `out` and returns how many.
    ///
    /// Takes what is already queued first; waits up to `timeout` only when
    /// nothing is. Returns 0 when nothing arrived in time. Needs a format-5
    /// FPGA ([`Info::is_format5`]); use [`Device::read_blocks`] otherwise.
    pub fn read_samples(&mut self, out: &mut Vec<Sample>, max: usize, timeout: Duration) -> Result<usize> {
        let start = out.len();
        let deadline = Instant::now() + timeout;
        let mut polled = false;
        out.reserve(max.min(1 << 16));
        loop {
            let got = out.len() - start;
            if got >= max {
                break;
            }
            if self.st.pos < self.st.end {
                self.decode_into(out, max - got);
                continue;
            }
            if let Some(f) = self.data_q.pop_front() {
                self.begin_block(f);
                continue;
            }
            if got > 0 {
                break;
            }
            let now = Instant::now();
            if now >= deadline && polled {
                break;
            }
            polled = true;
            self.pump(deadline.saturating_duration_since(now))?;
        }
        Ok(out.len() - start)
    }

    /// An iterator over samples; it ends when none arrives within
    /// `timeout`, or after yielding an error.
    pub fn samples(&mut self, timeout: Duration) -> Samples<'_> {
        Samples { dev: self, buf: Vec::with_capacity(4096), pos: 0, timeout, done: false }
    }

    /// Appends up to `max` raw DATA blocks to `out` and returns how many,
    /// waiting up to `timeout` when none is queued. The block counters are
    /// kept as for [`Device::read_samples`]; the words are not decoded. Do
    /// not mix with `read_samples` in one stream.
    pub fn read_blocks(&mut self, out: &mut Vec<Block>, max: usize, timeout: Duration) -> Result<usize> {
        let start = out.len();
        let deadline = Instant::now() + timeout;
        let mut polled = false;
        while out.len() - start < max {
            if let Some(f) = self.data_q.pop_front() {
                if let Some(h) = self.account(&f.payload) {
                    let nw = (h.nwords as usize).min((f.payload.len() - BlockHdr::LEN) / 4);
                    let words = f.payload[BlockHdr::LEN..BlockHdr::LEN + 4 * nw]
                        .chunks_exact(4)
                        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    out.push(Block { seq: h.seq, t_us: h.t_us, flags: h.flags, words });
                }
                continue;
            }
            if out.len() > start {
                break;
            }
            let now = Instant::now();
            if now >= deadline && polled {
                break;
            }
            polled = true;
            self.pump(deadline.saturating_duration_since(now))?;
        }
        Ok(out.len() - start)
    }

    /// The stream counters since the stream started.
    pub fn stream_counters(&self) -> StreamCounters {
        let st = &self.st;
        let c = &st.dec.c;
        StreamCounters {
            samples: c.samples,
            dirty: c.dirty,
            blocks: c.blocks,
            empty: c.empty,
            late: c.late,
            ovf_blocks: c.ovf_blocks,
            range_changes: c.range_changes,
            seq_errors: c.seq_errors,
            malformed: c.malformed,
            unframed: c.unframed,
            gap_periods: c.gap_periods,
            data_blocks: st.data_blocks,
            lost_blocks: st.lost_blocks,
            stall_flags: st.stall_flags,
            gap_flags: st.gap_flags,
            test_blocks: st.test_blocks,
            resyncs: self.total_resyncs() - st.resync_base,
            bad_blocks: st.bad_blocks,
        }
    }

    /// DATA blocks received and not yet read.
    pub fn queued_blocks(&self) -> usize {
        self.data_q.len()
    }
}

/// Iterator over samples ([`Device::samples`]).
pub struct Samples<'a> {
    dev: &'a mut Device,
    buf: Vec<Sample>,
    pos: usize,
    timeout: Duration,
    done: bool,
}

impl Iterator for Samples<'_> {
    type Item = Result<Sample>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos < self.buf.len() {
            self.pos += 1;
            return Some(Ok(self.buf[self.pos - 1]));
        }
        if self.done {
            return None;
        }
        self.buf.clear();
        self.pos = 0;
        match self.dev.read_samples(&mut self.buf, 4096, self.timeout) {
            Ok(0) => {
                self.done = true;
                None
            }
            Ok(_) => {
                self.pos = 1;
                Some(Ok(self.buf[0]))
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;
    use crate::stream::tests::block_words;
    use std::io;
    use std::sync::{Arc, Mutex};

    /// A scripted device: canned bytes to read, a log of what was written,
    /// and a responder that answers each command.
    type Inbox = Arc<Mutex<VecDeque<(usize, Vec<u8>)>>>;
    type Responder = Box<dyn FnMut(&Frame) -> Vec<(usize, Vec<u8>)> + Send>;

    struct Fake {
        inbox: Inbox,
        written: Arc<Mutex<Vec<Frame>>>,
        respond: Responder,
    }

    impl Transport for Fake {
        fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
            let mut p = FrameParser::new();
            p.push(data);
            let f = p.next_frame().unwrap();
            let r = (self.respond)(&f);
            self.inbox.lock().unwrap().extend(r);
            self.written.lock().unwrap().push(f);
            Ok(())
        }
        fn read(&mut self, buf: &mut [u8], _t: Duration) -> io::Result<Option<(usize, usize)>> {
            let mut q = self.inbox.lock().unwrap();
            let Some((src, mut v)) = q.pop_front() else { return Ok(None) };
            let n = v.len().min(buf.len());
            buf[..n].copy_from_slice(&v[..n]);
            if n < v.len() {
                q.push_front((src, v.split_off(n)));
            }
            Ok(Some((src, n)))
        }
    }

    fn response(f: &Frame, status: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = frame::encode(f.ty | 0x40, f.seq, payload);
        v[3] = status;
        v
    }

    fn data_frame(seq: u32, flags: u16, words: &[u32]) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&seq.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&(words.len() as u16).to_le_bytes());
        p.extend_from_slice(&flags.to_le_bytes());
        for w in words {
            p.extend_from_slice(&w.to_be_bytes());
        }
        p.resize(2040, 0);
        let mut v = vec![b'N', b'B', 0x80, 0];
        v.extend_from_slice(&(seq as u16).to_le_bytes());
        v.extend_from_slice(&2040u16.to_le_bytes());
        v.extend_from_slice(&p);
        v
    }

    #[test]
    fn data_during_request_is_queued_and_stale_responses_skipped() {
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let w1 = block_words(0, 2, 0, 1000, &[(1, 2, false), (3, 4, false)]);
        let w2 = block_words(1, 2, 0, 1002, &[(5, 6, false)]);
        let (d1, d2) = (data_frame(0, msg::blk::FIRST, &w1), data_frame(1, 0, &w2));
        let fake = Fake {
            inbox: inbox.clone(),
            written: Default::default(),
            respond: Box::new(move |f| {
                if f.ty == cmd::GET_STATS {
                    // a stale response (wrong seq), DATA, split, then the answer on source 1
                    let mut stale = f.clone();
                    stale.seq = f.seq.wrapping_sub(1);
                    let mut both = d1.clone();
                    both.extend_from_slice(&d2);
                    let (a, b) = both.split_at(1000);
                    vec![
                        (0, response(&stale, 0, &[0; 64])),
                        (0, a.to_vec()),
                        (0, b.to_vec()),
                        (1, response(f, 0, &[7; 64])),
                    ]
                } else {
                    vec![(0, response(f, 0, &[]))]
                }
            }),
        };
        let mut d = Device::with_transport(fake);
        d.start_stream(StreamOptions::default()).unwrap();
        let s = d.get_stats().unwrap();
        assert_eq!(s.uptime_ms, 0x0707_0707);
        assert_eq!(d.queued_blocks(), 2);
        let mut out = vec![];
        assert_eq!(d.read_samples(&mut out, 2, Duration::ZERO).unwrap(), 2);
        assert_eq!(d.read_samples(&mut out, 100, Duration::ZERO).unwrap(), 1);
        assert_eq!(out.iter().map(|s| s.ts).collect::<Vec<_>>(), [1000, 1001, 1002]);
        let c = d.stream_counters();
        assert_eq!((c.data_blocks, c.lost_blocks, c.samples, c.seq_errors, c.malformed), (2, 0, 3, 0, 0));
        assert_eq!(d.read_samples(&mut out, 100, Duration::ZERO).unwrap(), 0);
    }

    #[test]
    fn lost_blocks_and_flags() {
        let inbox = Arc::new(Mutex::new(VecDeque::new()));
        let fake = Fake { inbox: inbox.clone(), written: Default::default(), respond: Box::new(|_| vec![]) };
        let mut d = Device::with_transport(fake);
        {
            let mut q = inbox.lock().unwrap();
            q.push_back((0, data_frame(0, msg::blk::FIRST, &block_words(0, 0, 0, 0, &[(1, 1, false)]))));
            q.push_back((0, data_frame(3, msg::blk::STALL, &block_words(9, 0, 0, 50, &[(1, 1, false)]))));
            q.push_back((0, data_frame(4, msg::blk::TEST | msg::blk::GAP, &[1, 2, 3])));
        }
        let mut out = vec![];
        while d.read_samples(&mut out, 100, Duration::ZERO).unwrap() > 0 {}
        let c = d.stream_counters();
        assert_eq!(out.len(), 2);
        assert_eq!((c.data_blocks, c.lost_blocks, c.stall_flags, c.test_blocks, c.gap_flags), (3, 2, 1, 1, 0));
        // desync: the FPGA seq jump 0 -> 9 isn't a seq error
        assert_eq!(c.seq_errors, 0);
    }

    #[test]
    fn status_and_timeout_errors() {
        let fake = Fake {
            inbox: Default::default(),
            written: Default::default(),
            respond: Box::new(|f| if f.ty == cmd::SET_RANGE { vec![(0, response(f, 3, &[]))] } else { vec![] }),
        };
        let mut d = Device::with_transport(fake);
        let e = d.set_range_manual(9).unwrap_err();
        assert!(matches!(e, Error::Status { cmd: 0x21, status: 3 }));
        assert!(e.to_string().contains("BAD_ARG"));
        d.timeouts_mut().default = Duration::from_millis(5);
        assert!(matches!(d.get_info(), Err(Error::Timeout { cmd: 0x02 })));
        assert!(matches!(d.ping(&[0; 505]), Err(Error::Invalid(_))));
        assert!(matches!(d.flash_read(0, 257), Err(Error::Invalid(_))));
        assert!(matches!(d.flash_write(250, &[0; 10]), Err(Error::Invalid(_))));
        assert!(matches!(d.flash_erase(100, 4096), Err(Error::Invalid(_))));
    }

    #[test]
    fn request_encodings() {
        let written = Arc::new(Mutex::new(Vec::new()));
        let fake = Fake {
            inbox: Default::default(),
            written: written.clone(),
            respond: Box::new(|f| {
                let p: Vec<u8> = match f.ty {
                    cmd::SET_DECIMATION => vec![3, 0, 0, 0, 0x50, 0xC3, 0, 0],
                    cmd::BOARD_SET => vec![0; 200],
                    _ => vec![],
                };
                vec![(0, response(f, 0, &p))]
            }),
        };
        let mut d = Device::with_transport(fake);
        d.set_range_auto(1, 4).unwrap();
        d.set_range_timing(1, 2, 3, 0x0405).unwrap();
        d.set_range_force(true, 0x21).unwrap();
        assert_eq!(d.set_decimation(3, 0).unwrap().rate_hz, 50_000);
        d.set_leds(0x15).unwrap();
        d.leds_auto().unwrap();
        d.ui_key(board::key::ENTER).unwrap();
        d.start_stream(StreamOptions { flush: true, link_test: true }).unwrap();
        let w = written.lock().unwrap();
        let p: Vec<(u8, &[u8])> = w.iter().map(|f| (f.ty, &f.payload[..])).collect();
        assert_eq!(p[0], (0x21, &[1, 0, 1, 4][..]));
        assert_eq!(p[1], (0x23, &[1, 2, 3, 0, 5, 4, 0, 0][..]));
        assert_eq!(p[2], (0x24, &[1, 0x21, 0, 0][..]));
        assert_eq!(p[3], (0x30, &[3, 0, 0, 0, 0, 0, 0, 0][..]));
        assert_eq!(p[4], (0x39, &[0, 0, 0, 0, 0x15, 1, 0, 0][..]));
        assert_eq!(p[5], (0x39, &[0, 0, 0, 0, 0, 0, 0, 0][..]));
        assert_eq!(p[6], (0x39, &[10, 0, 0, 0, 5, 0, 0, 0][..]));
        assert_eq!(p[7], (0x10, &[3, 0, 0, 0][..]));
        // sequence numbers increment
        assert!(w.windows(2).all(|x| x[1].seq == x[0].seq.wrapping_add(1)));
    }
}
