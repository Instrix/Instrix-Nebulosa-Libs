/*
 * NBP - Nebulosa Bulk Protocol, version 1.
 *
 * Wire format between the Nebulosa firmware (CH32H417) and a host. The
 * protocol is independent of the interface: it needs one ordered,
 * reliable byte stream in each direction, and nothing else. Transport
 * bindings (below) map those streams onto USB bulk endpoints or a TCP
 * connection.
 *
 * This header is the definition. It is shared as-is by the firmware
 * (CH32H417-Playground/common/nbp/nbp.h) and the host
 * (Instrix-Nebulosa-Debugger/protocol/nbp.h); keep the copies identical.
 * docs/protocol.md explains it.
 *
 * All multi-byte header and payload fields are little-endian, except the
 * FPGA words inside DATA blocks, which are big-endian (the FPGA's wire
 * order, passed through untouched).
 */
#ifndef NBP_H
#define NBP_H

#include <stdint.h>

#define NBP_VERSION             1

/* ---- transport bindings ------------------------------------------------
 *
 * USB: one vendor interface, found by class / subclass / protocol. Its
 * OUT endpoint is the host->device stream. Its IN endpoint(s) are the
 * device->host stream: with two IN endpoints (High-Speed), DATA blocks go
 * on one and responses on the other; a message never spans two
 * endpoints. The host reads every IN endpoint and sorts messages by type.
 * The streams have no relation to USB transfer boundaries.              */
#define NBP_USB_CLASS           0xFF
#define NBP_USB_SUBCLASS        0x4E    /* 'N' */
#define NBP_USB_PROTOCOL        0x01    /* NBP_VERSION */

#define NBP_USB_VID             0x1A86
#define NBP_USB_PID_HS          0xFE0D  /* USB High-Speed (USBHS, Type-A port), and SuperSpeed (USBSS) */
#define NBP_USB_PID_FS          0xFE0C  /* USB Full-Speed (USBFS, USB-C), next to the CDC log port */

/* TCP: the device listens on NBP_TCP_PORT; one connection carries both
 * streams (responses and DATA interleaved, whole messages). One client
 * at a time: a new connection is refused while one is open. Closing the
 * connection stops a stream it started.
 *
 * UDP discovery: a host sends a GET_INFO command (header only) as a UDP
 * datagram, broadcast or unicast, to NBP_UDP_DISCOVERY_PORT. Every device
 * answers with its GET_INFO response in one datagram to the sender's
 * address and port. Nothing else is carried over UDP.                   */
#define NBP_TCP_PORT            20034   /* 0x4E42, "NB" */
#define NBP_UDP_DISCOVERY_PORT  20035

/* ---- messages ---------------------------------------------------------
 * Every message starts with this header. Each direction is a byte stream
 * of messages: parse by `len`. A receiver that finds no magic where a
 * header should be discards bytes up to the next magic.                 */
#define NBP_MAGIC               0x424Eu /* bytes 'N' 'B' */
#define NBP_MAX_PAYLOAD         504     /* commands and responses: header + payload <= 512 */

typedef struct __attribute__((packed))
{
    uint16_t magic;             /* NBP_MAGIC */
    uint8_t  type;              /* NBP_CMD_*, NBP_RSP(cmd), NBP_DATA */
    uint8_t  status;            /* responses: NBP_ST_*; 0 otherwise */
    uint16_t seq;               /* commands: set by the host, echoed in the response;
                                   DATA: low 16 bits of the block sequence */
    uint16_t len;               /* payload bytes that follow */
} nbp_hdr_t;

#define NBP_RSP(cmd)            ((uint8_t)((cmd) | 0x40))
#define NBP_DATA                0x80

/* response status */
#define NBP_ST_OK               0
#define NBP_ST_UNKNOWN          1       /* unknown command */
#define NBP_ST_BAD_LEN          2       /* payload length wrong for the command */
#define NBP_ST_BAD_ARG          3       /* argument out of range */
#define NBP_ST_FPGA             4       /* FPGA not answering (QSPI error / not configured) */

/* ---- commands (host -> device) and their payloads ------------------------ */

#define NBP_CMD_PING            0x01    /* any payload -> the same payload */
#define NBP_CMD_GET_INFO        0x02    /* -> nbp_info_t */
#define NBP_CMD_REG_READ        0x03    /* nbp_reg_t (value ignored) -> nbp_reg_t */
#define NBP_CMD_REG_WRITE       0x04    /* nbp_reg_t -> nothing */
#define NBP_CMD_RESET           0x05    /* -> nothing; the device resets ~50 ms later */

#define NBP_CMD_STREAM_START    0x10    /* nbp_stream_start_t -> nothing; DATA follows on this link */
#define NBP_CMD_STREAM_STOP     0x11    /* -> nothing */
#define NBP_CMD_GET_STATS       0x12    /* -> nbp_stats_t */

#define NBP_CMD_SET_SOURCE      0x20    /* nbp_source_t -> nothing */
#define NBP_CMD_SET_RANGE       0x21    /* nbp_range_set_t -> nothing */
#define NBP_CMD_GET_RANGE       0x22    /* -> nbp_range_t */
#define NBP_CMD_SET_RANGE_TIMING 0x23   /* nbp_range_timing_t -> nothing */
#define NBP_CMD_SET_RANGE_FORCE 0x24    /* nbp_range_force_t -> nothing */
#define NBP_CMD_SET_DECIMATION  0x30    /* nbp_decim_t -> nbp_decim_t (as applied) */

typedef struct __attribute__((packed))
{
    uint32_t proto_version;     /* NBP_VERSION */
    uint32_t fw_version;        /* major << 16 | minor << 8 | patch */
    uint32_t fpga_id;           /* FPGA ID register, e.g. 0x4E420006 */
    uint32_t base_rate_hz;      /* conversions/s before decimation (2,000,000) */
    uint32_t link;              /* NBP_LINK_* this response came on */
    uint32_t link_unit;         /* transfer unit of this link in bytes: USB packet size (512 / 64),
                                   TCP maximum segment size; informational */
    uint32_t block_words;       /* FPGA words per DATA block (NBP_BLOCK_WORDS) */
    uint32_t caps;              /* NBP_CAP_* */
    char     build[32];         /* firmware build date/time, NUL-terminated */
    uint8_t  mac[6];            /* Ethernet MAC address (all zero without NBP_CAP_ETH) */
    uint8_t  pad[2];
    uint32_t ip;                /* IPv4 address, network byte order (0 without Ethernet) */
} nbp_info_t;

#define NBP_LINK_NONE           0
#define NBP_LINK_USB_FS         1
#define NBP_LINK_USB_HS         2
#define NBP_LINK_TCP            3
#define NBP_LINK_USB_SS         4       /* USB 3.0 SuperSpeed (same device and endpoints as USB_HS) */

#define NBP_CAP_USB_HS          (1u << 0)   /* firmware has the USB High-Speed link */
#define NBP_CAP_RANGE           (1u << 1)   /* range control */
#define NBP_CAP_DECIM           (1u << 2)   /* decimation */
#define NBP_CAP_ETH             (1u << 3)   /* firmware has the Ethernet (TCP) link */
#define NBP_CAP_BOARD           (1u << 4)   /* board commands (NBP_CMD_BOARD_*, below) */
#define NBP_CAP_USB_SS          (1u << 5)   /* firmware has the USB 3.0 SuperSpeed link (USB 2.0 fallback) */

typedef struct __attribute__((packed))
{
    uint8_t  index;             /* FPGA register index (0..40, see the QSPI driver manual) */
    uint8_t  pad[3];
    uint32_t value;
} nbp_reg_t;

#define NBP_START_FLUSH         (1u << 0)   /* discard what the FPGA FIFO holds first */
#define NBP_START_LINK_TEST     (1u << 1)   /* no FPGA: blocks of NBP_LINK_TEST_WORD() as fast as the link
                                               takes them (flagged NBP_BLK_TEST), to measure the link.
                                               Older firmware ignores the flag and streams the FPGA's data */

typedef struct __attribute__((packed))
{
    uint32_t flags;             /* NBP_START_* */
} nbp_stream_start_t;

typedef struct __attribute__((packed))
{
    uint32_t uptime_ms;
    uint32_t stream_link;       /* NBP_LINK_* the stream goes to, NONE when stopped */
    uint32_t blocks;            /* DATA blocks sent since START */
    uint32_t words;             /* FPGA words sent since START */
    uint32_t stalls;            /* times the H417 block ring was full (link too slow) */
    uint32_t fpga_drops;        /* FPGA DROP_COUNT (conversions lost to FIFO overflow, since FPGA reset) */
    uint32_t fifo_level;        /* FPGA FIFO_LEVEL, last read */
    uint32_t range_status;      /* FPGA RANGE_STATUS */
    uint32_t decim;             /* FPGA DECIM */
    uint32_t adc_ctrl;          /* FPGA register 31 (source / stub mode) */
    uint32_t eth;               /* NBP_ETH_* flags (0 without NBP_CAP_ETH) */
    uint32_t eth_ip;            /* IPv4 address, network byte order, 0 = none yet */
    uint32_t reserved[4];
} nbp_stats_t;

#define NBP_ETH_PHY             (1u << 0)   /* cable in, link negotiated */
#define NBP_ETH_IP              (1u << 1)   /* has an address (DHCP done, or static) */
#define NBP_ETH_CLIENT          (1u << 2)   /* a TCP client is connected */

#define NBP_SRC_STUB_RANDOM     0
#define NBP_SRC_STUB_PATTERN    1
#define NBP_SRC_AD7387          2

typedef struct __attribute__((packed))
{
    uint8_t  source;            /* NBP_SRC_* */
    uint8_t  pad[3];
} nbp_source_t;

typedef struct __attribute__((packed))
{
    uint8_t  automatic;         /* 0 = manual, 1 = automatic */
    uint8_t  manual;            /* range for manual mode, 0..5 */
    uint8_t  min;               /* auto limits, 0..5 */
    uint8_t  max;
} nbp_range_set_t;

typedef struct __attribute__((packed))
{
    uint32_t status;            /* FPGA RANGE_STATUS (35) */
    uint32_t count;             /* FPGA RANGE_COUNT (36) */
    uint32_t ctrl;              /* FPGA RANGE_CTRL (37) */
    uint32_t timing;            /* FPGA RANGE_TIMING (38) */
} nbp_range_t;

typedef struct __attribute__((packed))
{
    uint8_t  up_filt;           /* FPGA clk cycles (see NBP_REG_CLK_HZ), 0..63 */
    uint8_t  down_filt;         /* 0..63 */
    uint8_t  overlap;           /* 0..255: both switches closed overlap + 1 cycles */
    uint8_t  pad;
    uint16_t hold;              /* 0..4095 */
    uint16_t pad2;
} nbp_range_timing_t;

typedef struct __attribute__((packed))
{
    uint8_t  enable;            /* 1 = comparators from `value` */
    uint8_t  value;             /* {down_c, up_c, down_b, up_b, down_a, up_a} */
    uint8_t  pad[2];
} nbp_range_force_t;

#define NBP_DECIM_AVERAGE       0
#define NBP_DECIM_SUBSAMPLE     1

typedef struct __attribute__((packed))
{
    uint8_t  log2n;             /* N = 2^log2n, 0..8 */
    uint8_t  mode;              /* NBP_DECIM_* */
    uint8_t  pad[2];
    uint32_t rate_hz;           /* response: conversions/s after decimation */
} nbp_decim_t;

/* ---- board commands (NBP_CAP_BOARD) -------------------------------------
 * The microcontroller side of a board: its I/O, supplies, clocks, the FPGA
 * configuration pins and the peripherals on the board, for bring-up and
 * test. Firmware without NBP_CAP_BOARD answers them NBP_ST_UNKNOWN.
 * INSTRIX-Nebulosa-Digital: CH32H417-Playground/nebulosa_digital.        */
#define NBP_CMD_BOARD_STATUS    0x38    /* -> nbp_board_status_t */
#define NBP_CMD_BOARD_SET       0x39    /* nbp_board_set_t -> nbp_board_status_t (after the change) */
#define NBP_CMD_BOARD_TEST      0x3A    /* nbp_board_test_t -> nbp_board_status_t (after the tests) */
#define NBP_CMD_FLASH_READ      0x3B    /* nbp_flash_t (len <= NBP_FLASH_CHUNK) -> nbp_flash_t + data */
#define NBP_CMD_FLASH_WRITE     0x3C    /* nbp_flash_t + len data bytes (within one 256-byte page) -> nothing */
#define NBP_CMD_FLASH_ERASE     0x3D    /* nbp_flash_t (addr, len: 4 KB aligned) -> nothing */
#define NBP_CMD_FPGA_CONFIG     0x3E    /* nbp_fpga_config_t -> nbp_board_status_t (after configuring) */
#define NBP_CMD_UPDATE          0x3F    /* nbp_update_t -> nbp_update_info_t (see "software update" below) */

#define NBP_ST_BUSY             5       /* board: not possible now (stream running, ...) */
#define NBP_ST_HW               6       /* board: the hardware didn't respond / the operation failed */
#define NBP_ST_NO_POWER         7       /* board: the analog supplies need a 12 V source (jack, or a 12 V PD contract) */
#define NBP_ST_PROTECTED        8       /* board: that flash range is the running gateware or the boot record:
                                           write the update slot, then NBP_CMD_UPDATE */

#define NBP_BOARD_DIGITAL       0x4E440001u /* "ND": INSTRIX-Nebulosa-Digital */

/* board tests: bit numbers of nbp_board_test_t.mask, and 2-bit result
 * fields of nbp_board_status_t.test_results */
enum
{
    NBP_TEST_HSE = 0,           /* 25 MHz crystal running, system clock from it; detail: HCLK Hz */
    NBP_TEST_LSE,               /* 32.768 kHz crystal starts; detail: start-up ms */
    NBP_TEST_RTC,               /* RTC counts on the LSE; detail: LSE Hz measured against the HSE */
    NBP_TEST_RAILS,             /* V_MAIN in range and one input present; detail: V_MAIN mV */
    NBP_TEST_FLASH_ID,          /* H417 SPI flash answers its JEDEC ID; detail: the ID */
    NBP_TEST_FLASH_RW,          /* erase / program / read back of the last 4 KB sector; detail: bad bytes */
    NBP_TEST_SD,                /* SD card identifies, block 0 reads (skipped: no card); detail: error step */
    NBP_TEST_LCD,               /* LCD initialised (write only: look at it) */
    NBP_TEST_FPGA_DONE,         /* FPGA configured (DONE high) */
    NBP_TEST_FPGA_QSPI,         /* FPGA ID register over QSPI; detail: the ID */
    NBP_TEST_ETH_PHY,           /* Ethernet PHY answers; detail: its ID */
    NBP_TEST_USB_PD,            /* USB PD contract at the target (skipped: no USB-C source, or not PD);
                                   detail: contract mV */
    NBP_TEST_COUNT
};
#define NBP_TEST_ALL            ((1u << NBP_TEST_COUNT) - 1)
#define NBP_TR_NONE             0       /* not run */
#define NBP_TR_PASS             1
#define NBP_TR_FAIL             2
#define NBP_TR_SKIP             3       /* not applicable (no SD card, ...) */
#define NBP_TEST_RESULT(results, t) (((results) >> (2 * (t))) & 3)

/* nbp_board_status_t.flags */
#define NBP_BOARD_HSE           (1u << 0)   /* HSE ready, system clock from it */
#define NBP_BOARD_LSE           (1u << 1)   /* LSE ready */
#define NBP_BOARD_RTC           (1u << 2)   /* RTC running on the LSE */
#define NBP_BOARD_DFU           (1u << 3)   /* DFU button (SW3) pressed */
#define NBP_BOARD_SD_CARD       (1u << 4)   /* SD card inserted */
#define NBP_BOARD_PWR_ST        (1u << 5)   /* power mux ST pin high */
#define NBP_BOARD_APWR_H        (1u << 6)   /* isolated analog supply H enabled (EN_APWR_H) */
#define NBP_BOARD_APWR_L        (1u << 7)   /* isolated analog supply L enabled */
#define NBP_BOARD_USB3_EN       (1u << 8)   /* USB3 mux (CH482X) enabled */
#define NBP_BOARD_USB3_DIR      (1u << 9)   /* USB3 mux SEL high */
#define NBP_BOARD_FPGA_DONE     (1u << 10)
#define NBP_BOARD_FPGA_INIT_B   (1u << 11)  /* INIT_B high (low: clearing, or a CRC error) */
#define NBP_BOARD_FPGA_PROG_HELD (1u << 12) /* PROGRAM_B held low by the H417 */
#define NBP_BOARD_USB_HS        (1u << 13)  /* USB High-Speed link configured */
#define NBP_BOARD_ETH_LINK      (1u << 14)  /* Ethernet cable in, link negotiated */
#define NBP_BOARD_LCD           (1u << 15)  /* LCD initialised */
#define NBP_BOARD_SWD           (1u << 16)  /* SWD still on (USB 2.0 not started: SuperSpeed leaves it alone) */
#define NBP_BOARD_USB3_AUTO     (1u << 17)  /* USB3 mux SEL follows the CC lines (orientation) */
#define NBP_BOARD_USB3_EN_AUTO  (1u << 18)  /* USB3 mux on while a USB-C source is attached */
#define NBP_BOARD_USB_C         (1u << 19)  /* a USB-C source is attached (pd_cc_line) */
#define NBP_BOARD_APWR_OK       (1u << 20)  /* a 12 V source feeds V_MAIN: the analog supplies may be switched on */
#define NBP_BOARD_USB_SS        (1u << 21)  /* USB 3.0 SuperSpeed link configured */

/* nbp_board_status_t.pd_state: the USB PD sink */
#define NBP_PD_DETACHED         0       /* nothing on the USB-C port */
#define NBP_PD_WAIT_CAPS        1       /* attached, waiting for Source_Capabilities */
#define NBP_PD_WAIT_ACCEPT      2       /* Request sent */
#define NBP_PD_WAIT_PS_RDY      3       /* accepted, the source is switching */
#define NBP_PD_READY            4       /* contract: pd_mv / pd_ma */
#define NBP_PD_NO_PD            5       /* attached, the source doesn't do PD: 5 V at the Rp current */

/* nbp_board_status_t.cc: USB-C CC lines at attach, 2 bits each (CC1 bits 1:0, CC2 bits 3:2) */
#define NBP_CC_OPEN             0       /* below 0.22 V: nothing there */
#define NBP_CC_DEFAULT          1       /* source Rp, default USB power (0.22..0.66 V) */
#define NBP_CC_1A5              2       /* 1.5 A (0.66..1.23 V) */
#define NBP_CC_3A0              3       /* 3.0 A (above 1.23 V) */

/* FPGA configuration mode, seen after a PROGRAM_B pulse (jumper J9, M1) */
#define NBP_FPGA_MODE_UNKNOWN   0
#define NBP_FPGA_MODE_MASTER    1       /* the FPGA drives CCLK: it loads from its own flash */
#define NBP_FPGA_MODE_SLAVE     2       /* CCLK idle: slave serial, the H417 can load it */

typedef struct __attribute__((packed))
{
    uint32_t board_id;          /* NBP_BOARD_* */
    uint32_t uptime_ms;
    uint32_t core_hz;           /* the core running this firmware (V3F) */
    uint32_t hclk_hz;
    uint32_t flags;             /* NBP_BOARD_* */
    uint32_t rtc_s;             /* RTC counter, seconds (NBP_BSET_RTC sets it) */
    uint16_t v_bj_mv;           /* 12 V barrel jack input (VSENSE_BJ) */
    uint16_t v_vbus_mv;         /* USB-C VBUS (VSENSE_VBUS) */
    uint16_t v_main_mv;         /* V_MAIN, the power mux output (VSENSE_MAIN) */
    uint16_t pad0;
    uint8_t  leds;              /* LED0..4, bit n = LEDn on */
    uint8_t  led_manual;        /* 1: as NBP_BSET_LEDS set them; 0: the firmware's status pattern */
    uint8_t  pwr_sel;           /* NBP_PWR_SEL_* */
    uint8_t  cc;                /* NBP_CC_* per line */
    uint8_t  fpga_mode;         /* NBP_FPGA_MODE_* */
    uint8_t  lcd_mode;          /* NBP_LCD_* */
    uint8_t  sd_type;           /* 0 none, 1 SDSC v1, 2 SDSC v2, 3 SDHC / SDXC */
    uint8_t  pad1;
    uint32_t fpga_cfg_ms;       /* last configuration seen: PROGRAM_B release to DONE, ms (0 = none) */
    uint32_t fpga_cfg_bytes;    /* last load from the H417 (slave serial), bytes; 0 = from its own flash */
    uint32_t fpga_id;           /* FPGA ID register (0 = no answer) */
    uint32_t flash_jedec;       /* H417 SPI flash JEDEC ID (0xEF4014 = W25Q80) */
    uint32_t flash_bytes;       /* its size */
    uint32_t sd_blocks;         /* SD card size in 512-byte blocks (0 = unknown) */
    uint32_t sd_cid[4];         /* SD card CID, response words 1..4 */
    uint32_t test_results;      /* 2 bits per NBP_TEST_*: NBP_TR_*, last run */
    uint32_t test_detail[NBP_TEST_COUNT]; /* per test: see the NBP_TEST_* comments */
    /* USB PD sink */
    uint8_t  pd_state;          /* NBP_PD_* */
    uint8_t  pd_cc_line;        /* attached line: 0 none, 1 CC1, 2 CC2 (plug orientation) */
    uint8_t  pd_spec_rev;       /* source's PD revision: 1 = 2.0, 2 = 3.0 */
    uint8_t  pd_npdo;           /* source capabilities received */
    uint16_t pd_mv;             /* contract voltage (5000 without PD) */
    uint16_t pd_ma;             /* contract current (from Rp without PD) */
    uint16_t pd_target_mv;      /* what the sink asks for (NBP_BSET_PD_MV) */
    uint8_t  pd_pdo_sel;        /* requested object position, 1-based */
    uint8_t  pad2;
    uint32_t pd_pdo[7];         /* the source's PDOs, as received (fixed: [19:10] 50 mV, [9:0] 10 mA) */
    uint32_t pd_contracts;      /* PS_RDYs since boot */
    uint32_t pd_hard_resets;    /* received since boot */
    uint32_t pd_errors;         /* messages without GoodCRC, timeouts */
    uint32_t reserved[4];
} nbp_board_status_t;

/* NBP_CMD_BOARD_SET items */
enum
{
    NBP_BSET_LEDS = 0,          /* value: bits 4:0 LEDs, bit 8 = manual (0: back to the status pattern) */
    NBP_BSET_APWR_H,            /* value 0 / 1. On only with NBP_BOARD_APWR_OK, else NO_POWER; the */
    NBP_BSET_APWR_L,            /* firmware switches both off when the 12 V source goes away */
    NBP_BSET_USB3_EN,           /* value 0 / 1, 2 = on while a USB-C source is attached (default) */
    NBP_BSET_USB3_DIR,          /* value 0 / 1, 2 = follow the CC lines (default) */
    NBP_BSET_PWR_SEL,           /* value NBP_PWR_SEL_*: refused (BAD_ARG) when the input left on is absent */
    NBP_BSET_FPGA_PROGRAM,      /* value: 0 release PROGRAM_B, 1 hold it low, 2 pulse (reconfigure) */
    NBP_BSET_LCD,               /* value NBP_LCD_* */
    NBP_BSET_RTC,               /* value: RTC counter, seconds */
    NBP_BSET_PD_MV,             /* value: USB PD target voltage, mV (5000 .. the firmware's maximum, BAD_ARG
                                   above); renegotiates now if a PD source is attached */
    NBP_BSET_UI_KEY,            /* value NBP_KEY_*: a key for the on-board UI, as if from its button
                                   (remote control from the PC); wakes the screen if it is off */
};

/* NBP_BSET_UI_KEY: up / left the previous item, down / right the next,
 * enter selects (the button's hold), back the page above. While a value
 * is edited (the clock, an IP address): up / down step it, enter / right
 * go on (the button's click / hold). */
#define NBP_KEY_UP              1
#define NBP_KEY_DOWN            2
#define NBP_KEY_LEFT            3
#define NBP_KEY_RIGHT           4
#define NBP_KEY_ENTER           5
#define NBP_KEY_BACK            6

#define NBP_PWR_SEL_AUTO        0       /* OV1 / OV2 released: the TPS2121 picks */
#define NBP_PWR_SEL_BJ          1       /* USB input disabled (OV2 driven high): 12 V only */
#define NBP_PWR_SEL_USB         2       /* barrel jack input disabled (OV1 driven high): USB only */

#define NBP_LCD_OFF             0
#define NBP_LCD_STATUS          1       /* on: the board UI (menus; a status page) */
#define NBP_LCD_PATTERN         2       /* colour bars */

typedef struct __attribute__((packed))
{
    uint8_t  item;              /* NBP_BSET_* */
    uint8_t  pad[3];
    uint32_t value;
} nbp_board_set_t;

typedef struct __attribute__((packed))
{
    uint32_t mask;              /* tests to run: bits NBP_TEST_* */
} nbp_board_test_t;

/* The H417's SPI flash (W25Q80, 1 MB): a bitstream store for
 * NBP_CMD_FPGA_CONFIG, or anything else. Its last 4 KB sector is scratch
 * for NBP_TEST_FLASH_RW. */
#define NBP_FLASH_CHUNK         256

typedef struct __attribute__((packed))
{
    uint32_t addr;
    uint32_t len;
    /* NBP_CMD_FLASH_WRITE: len data bytes follow; NBP_CMD_FLASH_READ
     * response: this header, then len bytes */
} nbp_flash_t;

typedef struct __attribute__((packed))
{
    uint32_t addr;              /* bitstream in the H417 flash: a raw .bin (sync word inside) */
    uint32_t len;               /* bytes */
    uint32_t flags;             /* reserved, 0 */
} nbp_fpga_config_t;

/* ---- software update (NBP_CMD_UPDATE) ------------------------------------
 * The H417 flash holds two 448 KB slots (UPD_SLOT_SIZE apart, from 0): the
 * active gateware in one, the other is the update slot. Over USB or TCP:
 *   1. NBP_UPD_INFO: where the update slot is (update_slot, slot_size).
 *   2. NBP_CMD_FLASH_ERASE / FLASH_WRITE the image into the update slot
 *      (the active slot and the record sectors answer NBP_ST_PROTECTED).
 *   3. NBP_UPD_GATEWARE (slot, len, crc): the device checks the CRC, loads
 *      the FPGA from it, checks DONE and the IP core's ID, and only then
 *      makes it active; else it restores the previous gateware and answers
 *      NBP_ST_HW. The previous one stays in the other slot (NBP_UPD_REVERT).
 *   or NBP_UPD_FIRMWARE (slot, len, crc): an .nbfw image (header + body,
 *      both CRC'd); checked, marked pending, the device restarts ~0.5 s
 *      after the response, its bootloader installs and checks it. Then
 *      NBP_UPD_INFO again: fw_version is the new one, fw_pending 0.
 * crc: CRC-32 (zlib's) of the len bytes, what the host sent. msg: text. */
#define NBP_UPD_INFO            0
#define NBP_UPD_GATEWARE        1       /* make slot's gateware active (tested first) */
#define NBP_UPD_FIRMWARE        2       /* install slot's firmware image at a restart */
#define NBP_UPD_REVERT          3       /* the other slot's gateware (the previous) active again */
#define NBP_UPD_RECOVERY        4       /* restart into the H417's ROM USB bootloader (WCH ISP) */

#define NBP_UPD_SLOT_EMPTY      0
#define NBP_UPD_SLOT_GATEWARE   1
#define NBP_UPD_SLOT_FIRMWARE   2

typedef struct __attribute__((packed))
{
    uint8_t  op;                /* NBP_UPD_* */
    uint8_t  slot;
    uint8_t  pad[2];
    uint32_t len;
    uint32_t crc;
} nbp_update_t;

typedef struct __attribute__((packed))
{
    uint32_t fw_version;        /* major << 16 | minor << 8 | patch */
    uint32_t fw_build_id;       /* git commit, 8 hex digits */
    uint32_t fw_len;            /* the running image */
    uint32_t fw_crc;            /* its body CRC (header) */
    char     fw_date[24];
    uint32_t boot_version;      /* the bootloader's, 0 = none */
    uint8_t  active;            /* the active gateware slot */
    uint8_t  update_slot;       /* where an update goes */
    uint8_t  fw_pending;        /* a firmware install waits for the restart */
    uint8_t  result;            /* NBP_ST_* of the operation */
    uint32_t slot_size;
    struct __attribute__((packed))
    {
        uint8_t  type;          /* NBP_UPD_SLOT_* */
        uint8_t  pad[3];
        uint32_t len, crc;
        uint32_t id;            /* gateware: IP core ID; firmware: version */
    } slot[2];
    uint32_t installs;          /* firmware installs the bootloader did */
    char     msg[48];           /* what happened */
} nbp_update_info_t;

/* ---- DATA blocks (device -> host) ---------------------------------------
 * header (type NBP_DATA, len NBP_BLOCK_BYTES - 8) + nbp_block_t. A whole
 * block is 2048 bytes (four USB High-Speed packets).                     */
#define NBP_BLOCK_BYTES         2048
#define NBP_BLOCK_WORDS         507     /* (2048 - 8 - 12) / 4 */

#define NBP_BLK_FIRST           (1u << 0)   /* first block after STREAM_START */
#define NBP_BLK_STALL           (1u << 1)   /* the H417 ring was full since the previous block:
                                               the FPGA FIFO may have overflowed (see DROPPED) */
#define NBP_BLK_GAP             (1u << 2)   /* the QSPI link reported a sequence gap before this
                                               block's words (NEB_GAP): entries were lost there */
#define NBP_BLK_TEST            (1u << 3)   /* a link test block (NBP_START_LINK_TEST): word i is
                                               NBP_LINK_TEST_WORD(i), in the block's byte order as sent */
#define NBP_LINK_TEST_WORD(i)   ((uint32_t)(i) * 0x9E3779B1u ^ 0xA5A5A5A5u)

typedef struct __attribute__((packed))
{
    uint32_t seq;               /* block number since STREAM_START */
    uint32_t t_us;              /* H417 time (us, wraps) when the block's QSPI burst started */
    uint16_t nwords;            /* FPGA words that follow (<= NBP_BLOCK_WORDS) */
    uint16_t flags;             /* NBP_BLK_* */
    uint32_t words[NBP_BLOCK_WORDS];  /* big-endian FPGA FIFO words */
} nbp_block_t;

/* FPGA stream format 5 (fpga_id 0x4E42000B): the words are blocks, decoded
 * by neb_stream.h (copy it, or see the gateware's docs/qspi-driver-manual.md
 * "Byte layouts"): a BLOCK / TS_HI / TS_LO header (sequence number, OVF,
 * decimation, range, source, 48-bit timestamp of the first sample in 500 ns
 * periods), then up to 256 samples {1, DIRTY, 00, current[13:0],
 * voltage[13:0]}; empty words (0) and LATE words (0x20000000) can fall
 * anywhere and carry nothing. Blocks span NBP blocks: parse the words of
 * consecutive NBP blocks as one stream (NBP_BLK_STALL or a jump in
 * nbp_block_t.seq means words were lost: resync at the next BLOCK word; the
 * FPGA's block sequence number shows the same).
 *
 * Older bitstreams (fpga_id up to 0x4E42000A), format 4:
 * FPGA word (FPGA ID 0x4E420005, format 4), after byte-swapping to host order:
 *   conversion (VALID = 1): [31] VALID [30] DROPPED [29] DIRTY [28] MUX
 *                           [27:14] A (14 bits) [13:0] B (14 bits)
 *   range event (VALID = 0, EVENT = 1): [28] EVENT [27:24] type (2 RANGE)
 *                           [10:8] range before [2:0] range after
 *   late (VALID = 0, [29] LATE): a QSPI read missed its deadline, no data
 * A RANGE event comes right before the first conversion sampled in the new
 * range (and before the first conversion of a stream): every conversion
 * after it was sampled in "after". DIRTY: the sample may be unreliable (a
 * comparator of the active pair was asserted when it was taken, or a
 * range switch was in progress or settling); a host may drop and
 * interpolate it.
 * Channels: MUX 0 -> A = AINA0, B = AINB0; MUX 1 -> A = AINA1, B = AINB1. */
#define NBP_FPGA_REGS           45          /* registers 0..44 (42..44: RS1473 link status, ID 0x4E42000A on) */
#define NBP_REG_RANGE_CFG       40          /* [11:0] SETTLE, [16] ALIGN, [17] DIRTY_UP, [18] DIRTY_DOWN */
#define NBP_REG_CLK_HZ          41          /* RO: FPGA fabric clock in Hz: the unit of the range
                                               timing cycle counts (108 MHz Tang Nano, 100 MHz Spartan-6) */
#define NBP_FPGA_ID_CLK_HZ      0x4E420006u /* first FPGA ID with NBP_REG_CLK_HZ; older ones: 108 MHz */
#define NBP_FPGA_CLK_HZ_OLD     108000000u
#define NBP_W_VALID             (1u << 31)
#define NBP_W_DROPPED           (1u << 30)
#define NBP_W_DIRTY             (1u << 29)  /* VALID = 1 */
#define NBP_W_LATE              (1u << 29)  /* VALID = 0 */
#define NBP_W_MUX               (1u << 28)
#define NBP_W_EVENT             (1u << 28)
#define NBP_EV_RANGE            2

#endif
