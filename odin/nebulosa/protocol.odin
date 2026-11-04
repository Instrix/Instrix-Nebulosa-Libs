// Nebulosa developer library for Odin.
//
// Talks NBP (Nebulosa Bulk Protocol, spec/nbp.h) to an INSTRIX Nebulosa
// power profiler over TCP or USB, without the Companion app. The model is
// the one of docs/API.md, shared by every language binding.
package nebulosa

import "core:time"

// ---- transports -----------------------------------------------------------

TCP_PORT :: 20034 // 0x4E42, "NB"
UDP_DISCOVERY_PORT :: 20035

USB_VID :: 0x1A86
USB_PID_HS :: 0xFE0D // High-Speed (Type-A port) and SuperSpeed
USB_PID_FS :: 0xFE0C // Full-Speed (USB-C port, next to a CDC log interface: not touched)
USB_CLASS :: 0xFF
USB_SUBCLASS :: 0x4E
USB_PROTOCOL :: 0x01

// ---- framing --------------------------------------------------------------

PROTO_VERSION :: 1
MAGIC :: 0x424E // bytes 'N' 'B'
HDR_LEN :: 8
MAX_PAYLOAD :: 504 // commands and responses
BLOCK_BYTES :: 2048 // a DATA message, header included
MAX_FRAME_LEN :: BLOCK_BYTES - HDR_LEN // 2040: the largest `len` a parser accepts
RSP_FLAG :: 0x40 // response type = command | RSP_FLAG
DATA_TYPE :: 0x80 // DATA message type

// ---- commands -------------------------------------------------------------

Cmd :: enum u8 {
	Ping             = 0x01,
	Get_Info         = 0x02,
	Reg_Read         = 0x03,
	Reg_Write        = 0x04,
	Reset            = 0x05,
	Stream_Start     = 0x10,
	Stream_Stop      = 0x11,
	Get_Stats        = 0x12,
	Set_Source       = 0x20,
	Set_Range        = 0x21,
	Get_Range        = 0x22,
	Set_Range_Timing = 0x23,
	Set_Range_Force  = 0x24,
	Set_Decimation   = 0x30,
	Board_Status     = 0x38,
	Board_Set        = 0x39,
	Board_Test       = 0x3A,
	Flash_Read       = 0x3B,
	Flash_Write      = 0x3C,
	Flash_Erase      = 0x3D,
	Fpga_Config      = 0x3E,
	Update           = 0x3F,
}

// Response status (NBP_ST_*). Values above Protected may come from newer
// firmware; they still carry their code.
Status :: enum u8 {
	OK        = 0,
	Unknown   = 1, // unknown command
	Bad_Len   = 2, // payload length wrong for the command
	Bad_Arg   = 3, // argument out of range
	FPGA      = 4, // FPGA not answering
	Busy      = 5, // not possible now (stream running, ...)
	HW        = 6, // the hardware didn't respond / the operation failed
	No_Power  = 7, // the analog supplies need a 12 V source
	Protected = 8, // that flash range is the running gateware or the boot record
}

status_name :: proc(s: Status) -> string {
	switch s {
	case .OK:        return "OK"
	case .Unknown:   return "UNKNOWN"
	case .Bad_Len:   return "BAD_LEN"
	case .Bad_Arg:   return "BAD_ARG"
	case .FPGA:      return "FPGA"
	case .Busy:      return "BUSY"
	case .HW:        return "HW"
	case .No_Power:  return "NO_POWER"
	case .Protected: return "PROTECTED"
	}
	return "?"
}

// NBP_LINK_*: the link a response came on / a stream goes to.
Link :: enum u32 {
	None   = 0,
	Usb_FS = 1,
	Usb_HS = 2,
	TCP    = 3,
	Usb_SS = 4,
}

// NBP_CAP_* bit numbers.
Cap :: enum u32 {
	Usb_HS = 0,
	Range  = 1,
	Decim  = 2,
	Eth    = 3,
	Board  = 4,
	Usb_SS = 5,
}
Caps :: bit_set[Cap;u32]

// NBP_ETH_* bit numbers (Stats.eth).
Eth_Flag :: enum u32 {
	Phy    = 0,
	IP     = 1,
	Client = 2,
}
Eth_Flags :: bit_set[Eth_Flag;u32]

START_FLUSH :: u32(1 << 0)
START_LINK_TEST :: u32(1 << 1)

Source :: enum u8 {
	Stub_Random  = 0,
	Stub_Pattern = 1,
	AD7387       = 2,
}

Decim_Mode :: enum u8 {
	Average   = 0,
	Subsample = 1,
}

// ---- board ----------------------------------------------------------------

BOARD_DIGITAL :: u32(0x4E440001) // "ND": INSTRIX-Nebulosa-Digital

// NBP_TEST_*: bit numbers of the board_test mask and 2-bit result fields.
Board_Test :: enum u32 {
	HSE       = 0,
	LSE       = 1,
	RTC       = 2,
	Rails     = 3,
	Flash_ID  = 4,
	Flash_RW  = 5,
	SD        = 6,
	LCD       = 7,
	FPGA_Done = 8,
	FPGA_QSPI = 9,
	Eth_PHY   = 10,
	USB_PD    = 11,
}
TEST_COUNT :: 12
TEST_ALL :: u32((1 << TEST_COUNT) - 1)

Test_Result :: enum u8 {
	None = 0, // not run
	Pass = 1,
	Fail = 2,
	Skip = 3, // not applicable
}

// nbp_board_status_t.flags bit numbers.
Board_Flag :: enum u32 {
	Hse            = 0,
	Lse            = 1,
	Rtc            = 2,
	Dfu            = 3,
	Sd_Card        = 4,
	Pwr_St         = 5,
	Apwr_H         = 6,
	Apwr_L         = 7,
	Usb3_En        = 8,
	Usb3_Dir       = 9,
	Fpga_Done      = 10,
	Fpga_Init_B    = 11,
	Fpga_Prog_Held = 12,
	Usb_HS         = 13,
	Eth_Link       = 14,
	Lcd            = 15,
	Swd            = 16,
	Usb3_Auto      = 17,
	Usb3_En_Auto   = 18,
	Usb_C          = 19,
	Apwr_Ok        = 20,
	Usb_SS         = 21,
}
Board_Flags :: bit_set[Board_Flag;u32]

// NBP_BSET_*: items of board_set.
Board_Set_Item :: enum u8 {
	Leds         = 0, // bits 4:0 LEDs, bit 8 = manual (0: back to the status pattern)
	Apwr_H       = 1,
	Apwr_L       = 2,
	Usb3_En      = 3,
	Usb3_Dir     = 4,
	Pwr_Sel      = 5,
	Fpga_Program = 6,
	Lcd          = 7,
	Rtc          = 8,
	Pd_Mv        = 9,
	Ui_Key       = 10,
}

// NBP_KEY_*: keys for the on-board UI.
Ui_Key :: enum u32 {
	Up    = 1,
	Down  = 2,
	Left  = 3,
	Right = 4,
	Enter = 5,
	Back  = 6,
}

FLASH_PAGE :: 256 // NBP_FLASH_CHUNK: flash_read / flash_write unit
FLASH_SECTOR :: 4096 // flash_erase unit

// ---- software update ------------------------------------------------------

Update_Op :: enum u8 {
	Info     = 0,
	Gateware = 1,
	Firmware = 2,
	Revert   = 3,
	Recovery = 4,
}

Update_Slot_Kind :: enum u8 {
	Empty    = 0,
	Gateware = 1,
	Firmware = 2,
}

// ---- DATA blocks ----------------------------------------------------------

BLOCK_WORDS :: 507 // FPGA words per DATA block
BLOCK_HDR_LEN :: 12 // nbp_block_t without the words

Block_Flag :: enum u16 {
	First = 0, // first block after STREAM_START
	Stall = 1, // the device ring was full since the previous block
	Gap   = 2, // the QSPI link reported a sequence gap before this block's words
	Test  = 3, // a link test block
}
Block_Flags :: bit_set[Block_Flag;u16]

// First FPGA ID with stream format 5 (decoded by Stream_Decoder).
FPGA_ID_FORMAT5 :: u32(0x4E42000B)

// ADC sample period: Sample.ts counts these.
TS_PERIOD_S :: 500e-9

// ---- timeouts (docs/API.md section 3) -------------------------------------

DEFAULT_TIMEOUT :: 1500 * time.Millisecond
BOARD_TEST_TIMEOUT :: 15 * time.Second
FPGA_CONFIG_TIMEOUT :: 10 * time.Second
FLASH_ERASE_TIMEOUT :: 5 * time.Second
UPDATE_TIMEOUT :: 30 * time.Second
DISCOVERY_TIME :: 600 * time.Millisecond
