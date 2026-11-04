# Nebulosa developer libraries: common API contract

Every language binding in this repository implements the same model, with
names adapted to the language's conventions. The wire definition is
[`spec/nbp.h`](https://github.com/Instrix/Instrix-Nebulosa-Libs/blob/main/spec/nbp.h) (NBP, copied verbatim from
Instrix-Nebulosa-Firmware `common/nbp/nbp.h`) and
[`spec/neb_stream.h`](https://github.com/Instrix/Instrix-Nebulosa-Libs/blob/main/spec/neb_stream.h) (FPGA sample stream, format 5).
When the firmware changes, re-copy both and update the bindings.

## 1. Transports

NBP needs one ordered byte stream in each direction.

| Transport | How |
|-----------|-----|
| TCP | connect to `host:20034`. One connection carries responses and DATA interleaved. One client at a time: the device refuses a second connection (so close the Companion first). Closing stops a stream this connection started. Set `TCP_NODELAY`. |
| UDP discovery | send an 8-byte GET_INFO header (`4E 42 02 00 00 00 00 00`) to `255.255.255.255:20035` (broadcast) or a unicast address. Each device answers with one datagram: a GET_INFO response frame. The source address of the answer is the host to connect to. Default listen time 600 ms. |
| USB | VID `0x1A86`, PID `0xFE0D` (High-Speed / SuperSpeed, Type-A port) or `0xFE0C` (Full-Speed, USB-C port; that device also has a CDC log interface, do not touch it). Find the interface with class `0xFF`, subclass `0x4E`, protocol `0x01`. Claim only that interface. Its bulk OUT endpoint is host->device. Read **every** bulk IN endpoint (HS: two, one carries DATA and one responses; FS: one carries both). Each IN endpoint is its own byte stream: give each its own frame parser; a message never spans endpoints. Transfer boundaries mean nothing. Keep reads outstanding on every IN endpoint all the time (async transfer queue, or one reader thread per endpoint): reading endpoints in turn with sync timeouts lets the idle response endpoint block the DATA one, and at 2 MSPS the device ring stalls (measured: 95 % of samples lost; with queued transfers, 0). Linux needs `tools/99-nebulosa.rules` for non-root access. |

## 2. Framing

8-byte header, little-endian: `u16 magic = 0x424E` (bytes `'N' 'B'`), `u8 type`,
`u8 status`, `u16 seq`, `u16 len`, then `len` payload bytes.

- Commands: `type` = command code, `status` = 0, `seq` chosen by the host
  (increment a u16 per request, wrapping), payload <= 504 bytes.
- Responses: `type = cmd | 0x40`, `status` = `NBP_ST_*`, `seq` echoes the command.
- DATA: `type = 0x80`, `len` = 2040, payload = `nbp_block_t`.
- Parser: accumulate bytes; if the buffer does not start with `'N' 'B'`,
  discard up to the next `"NB"` (keep a trailing `'N'`) and count a resync;
  if `len > 2040`, drop one byte and resync; wait for `8 + len` bytes.

## 3. Request / response

One command in flight at a time. `request(cmd, payload, timeout)`:
send, then read until a frame with `type == cmd|0x40` and the same `seq`
arrives. DATA frames that arrive meanwhile are **kept** (queued for the
stream reader), never dropped. Other responses (stale seq) are discarded.
On timeout, raise/return a timeout error. If `status != 0`, raise/return a
status error carrying the status code and its name.

Status names: 0 OK, 1 UNKNOWN, 2 BAD_LEN, 3 BAD_ARG, 4 FPGA, 5 BUSY, 6 HW,
7 NO_POWER, 8 PROTECTED.

Default timeouts: 1.5 s; `board_test` 15 s; `fpga_config` 10 s;
`flash_erase` 5 s (per 64 KB chunk); `update`: INFO 2-5 s, GATEWARE 15 s,
FIRMWARE 10 s (the Companion's values). A refused UPDATE (status != 0)
still carries `nbp_update_info_t`; its `msg` says why: surface it.

## 4. Commands (all bindings)

Payload layouts are the packed structs in `nbp.h`. Sizes:
`nbp_info_t` 76, `nbp_stats_t` 64, `nbp_range_t` 16, `nbp_range_timing_t` 8,
`nbp_decim_t` 8, `nbp_board_status_t` 200, `nbp_update_info_t` 136.
Parse responses tolerantly: accept a payload longer than the struct (newer
firmware may append), reject a shorter one.

| Method | Command | Request payload | Returns |
|--------|---------|-----------------|---------|
| `ping(data)` | 0x01 | any | echoed bytes |
| `get_info()` | 0x02 | none | `Info` |
| `reg_read(index)` | 0x03 | `u8 index, u8 pad[3], u32 0` | `u32 value` (payload offset 4) |
| `reg_write(index, value)` | 0x04 | `u8 index, pad[3], u32 value` | - |
| `reset()` | 0x05 | none | - (device restarts ~50 ms later; connection drops) |
| `start_stream(flush=true, link_test=false)` | 0x10 | `u32 flags` (bit0 FLUSH, bit1 LINK_TEST) | - |
| `stop_stream()` | 0x11 | none | - |
| `get_stats()` | 0x12 | none | `Stats` |
| `set_source(src)` | 0x20 | `u8 src, pad[3]` (0 stub random, 1 stub pattern, 2 AD7387) | - |
| `set_range_auto(min=0, max=5)` | 0x21 | `u8 automatic=1, u8 manual=0, u8 min, u8 max` | - |
| `set_range_manual(range)` | 0x21 | `u8 automatic=0, u8 manual=range, u8 0, u8 5` | - |
| `get_range()` | 0x22 | none | `RangeState {status, count, ctrl, timing}` |
| `set_range_timing(up_filt, down_filt, overlap, hold)` | 0x23 | `u8,u8,u8,pad,u16 hold,u16 pad` | - |
| `set_range_force(enable, value)` | 0x24 | `u8 enable, u8 value, pad[2]` | - |
| `set_decimation(log2n, mode=AVERAGE)` | 0x30 | `u8 log2n (0..8), u8 mode (0 avg, 1 subsample), pad[2], u32 0` | `Decim {log2n, mode, rate_hz}` |
| `board_status()` | 0x38 | none | `BoardStatus` |
| `board_set(item, value)` | 0x39 | `u8 item, pad[3], u32 value` | `BoardStatus` |
| `board_test(mask=ALL)` | 0x3A | `u32 mask` | `BoardStatus` |
| `flash_read(addr, len<=256)` | 0x3B | `u32 addr, u32 len` | `len` bytes (response payload offset 8) |
| `flash_write(addr, data)` | 0x3C | `u32 addr, u32 len, data` (within one 256-byte page) | - |
| `flash_erase(addr, len)` | 0x3D | `u32 addr, u32 len` (4 KB aligned) | - |
| `fpga_config(addr, len)` | 0x3E | `u32 addr, u32 len, u32 flags=0` | `BoardStatus` |
| `update(op, slot=0, len=0, crc=0)` | 0x3F | `u8 op, u8 slot, pad[2], u32 len, u32 crc` | `UpdateInfo` |
| `update_info()` | 0x3F | op 0 | `UpdateInfo` |

Convenience helpers every binding provides: `crc32(bytes)` (zlib),
`set_leds(mask)` / `leds_auto()` (BSET_LEDS: value = mask \| 0x100 / 0),
`ui_key(key)` (BSET_UI_KEY).

### Records (fields, all little-endian)

`Info`: proto_version u32 @0, fw_version u32 @4 (major<<16|minor<<8|patch),
fpga_id u32 @8, base_rate_hz u32 @12, link u32 @16, link_unit u32 @20,
block_words u32 @24, caps u32 @28, build char[32] @32 (NUL-terminated),
mac u8[6] @64, ip u8[4] @72 (network order: a.b.c.d = bytes 0..3).

`Stats`: 16 x u32: uptime_ms, stream_link, blocks, words, stalls,
fpga_drops, fifo_level, range_status, decim, adc_ctrl, eth, eth_ip, reserved[4].

`BoardStatus` (200 bytes): board_id u32 @0, uptime_ms @4, core_hz @8,
hclk_hz @12, flags @16, rtc_s @20, v_bj_mv u16 @24, v_vbus_mv u16 @26,
v_main_mv u16 @28, leds u8 @32, led_manual u8 @33, pwr_sel u8 @34, cc u8 @35,
fpga_mode u8 @36, lcd_mode u8 @37, sd_type u8 @38, fpga_cfg_ms u32 @40,
fpga_cfg_bytes @44, fpga_id @48, flash_jedec @52, flash_bytes @56,
sd_blocks @60, sd_cid u32[4] @64, test_results u32 @80,
test_detail u32[12] @84, pd_state u8 @132, pd_cc_line u8 @133,
pd_spec_rev u8 @134, pd_npdo u8 @135, pd_mv u16 @136, pd_ma u16 @138,
pd_target_mv u16 @140, pd_pdo_sel u8 @142, pd_pdo u32[7] @144,
pd_contracts u32 @172, pd_hard_resets @176, pd_errors @180.
Test result of test t: `(test_results >> 2t) & 3` (0 none, 1 pass, 2 fail, 3 skip).

`UpdateInfo` (136 bytes): fw_version u32 @0, fw_build_id @4, fw_len @8,
fw_crc @12, fw_date char[24] @16, boot_version u32 @40, active u8 @44,
update_slot u8 @45, fw_pending u8 @46, result u8 @47, slot_size u32 @48,
slot[2] @52 (each 16 bytes: u8 type, pad[3], u32 len, u32 crc, u32 id),
installs u32 @84, msg char[48] @88.

## 5. Sample stream

`start_stream()` makes the device send DATA frames on this link. Each DATA
payload is `nbp_block_t`: `u32 seq, u32 t_us, u16 nwords, u16 flags` (12
bytes), then `nwords` FPGA words, each **big-endian**.

Per DATA block, in order:
1. `flags & FIRST (1)`: new stream: reset the decoder and counters.
2. If an expected seq is known and `seq != expected`: `lost_blocks += seq - expected` (u32 wrapping), `decoder.desync()`. Then `expected = seq + 1`.
3. `flags & TEST (8)`: link-test block, count it, skip its words.
4. `flags & STALL (2)` or `flags & GAP (4)`: count, `decoder.desync()`.
5. Feed each word (big-endian -> u32) to the stream decoder.

Stream decoder (format 5, FPGA ID >= `0x4E42000B`): port `StreamDecoder`
from `spec/neb_stream.h` / Companion `crates/nbp-core/src/stream.rs`
exactly (same state machine, same counters: samples, dirty, blocks, empty,
late, ovf_blocks, range_changes, seq_errors, malformed, unframed,
gap_periods). Older FPGAs (format 4) are not decoded: bindings expose the
raw words for those.

A decoded `Sample`:

| field | meaning |
|-------|---------|
| `ts` | u64, ADC periods of 500 ns since FPGA reset (`time_s = ts * 500e-9`) |
| `step` | u32, 2^log2N: ts distance to the next sample |
| `current_code`, `voltage_code` | u16, 14-bit codes |
| `range` | u8, 0 = highest current range (2 A) .. 5 (20 uA) |
| `dirty` | bool, sample may be unreliable (range switching) |
| `current` | f64 amps = `code * i_fs[range] / 16384 + i_offset[range]` |
| `voltage` | f64 volts = `code * v_scale + v_offset` |

Default `Calibration`: `i_fs = [2.0, 0.2, 20e-3, 2e-3, 200e-6, 20e-6, 0, 0]`,
`i_offset = 0`, `v_scale = 1e-3`, `v_offset = 0`. User-replaceable.

Reading API: a blocking `read_samples(max, timeout)` (or iterator /
callback) that returns whatever samples are available, pulling from the
queued DATA frames first, then the transport. `stream_counters()` returns
the decoder counters plus `data_blocks`, `lost_blocks`, `stall_flags`,
`gap_flags`, `test_blocks`, `resyncs`.

Rate: 2 MSPS at log2n = 0 is 8 MB/s of words. Scripting languages should
decimate (`set_decimation(log2n)`) unless they decode in native code.

## 6. Testing without hardware

`tools/nebulosa_sim.py` is a fake device: TCP on 20034 (or `--port`), UDP
discovery on 20035, all commands above, and format-5 streams (a sine on the
current, ramp on the voltage, auto-ranging, honouring decimation). Every
binding's tests/examples run against it:

    python3 tools/nebulosa_sim.py --port 20034 &
