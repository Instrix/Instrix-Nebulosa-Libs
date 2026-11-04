"""Protocol constants from ``spec/nbp.h`` and ``spec/neb_stream.h``."""

from __future__ import annotations

from enum import IntEnum, IntFlag

NBP_VERSION = 1

MAGIC = 0x424E  # bytes 'N' 'B'
HDR_LEN = 8
MAX_PAYLOAD = 504  # commands and responses
BLOCK_BYTES = 2048  # a DATA message, header included
MAX_LEN = BLOCK_BYTES - HDR_LEN  # largest payload the parser accepts (DATA)
BLOCK_WORDS = 507
BLOCK_HDR_LEN = 12
DATA = 0x80

TCP_PORT = 20034
UDP_DISCOVERY_PORT = 20035

USB_VID = 0x1A86
USB_PID_HS = 0xFE0D  # High-Speed / SuperSpeed (Type-A port)
USB_PID_FS = 0xFE0C  # Full-Speed (USB-C port, next to the CDC log interface)
USB_CLASS = 0xFF
USB_SUBCLASS = 0x4E
USB_PROTOCOL = 0x01

FLASH_CHUNK = 256  # flash_read / flash_write page
FLASH_SECTOR = 4096  # erase granularity
FLASH_ERASE_CHUNK = 65536  # upload_flash erases this much per command

BOARD_DIGITAL = 0x4E440001  # "ND": INSTRIX-Nebulosa-Digital

FPGA_ID_FORMAT5 = 0x4E42000B  # first FPGA ID with stream format 5
FPGA_ID_CLK_HZ = 0x4E420006
FPGA_CLK_HZ_OLD = 108_000_000
FPGA_REGS = 45
REG_RANGE_CFG = 40
REG_CLK_HZ = 41

TS_PERIOD_S = 500e-9  # one ADC sample period: the timestamp unit


def rsp(cmd: int) -> int:
    """Response type for a command code."""
    return (cmd | 0x40) & 0xFF


class Cmd(IntEnum):
    """NBP command codes (``NBP_CMD_*``)."""

    PING = 0x01
    GET_INFO = 0x02
    REG_READ = 0x03
    REG_WRITE = 0x04
    RESET = 0x05
    STREAM_START = 0x10
    STREAM_STOP = 0x11
    GET_STATS = 0x12
    SET_SOURCE = 0x20
    SET_RANGE = 0x21
    GET_RANGE = 0x22
    SET_RANGE_TIMING = 0x23
    SET_RANGE_FORCE = 0x24
    SET_DECIMATION = 0x30
    BOARD_STATUS = 0x38
    BOARD_SET = 0x39
    BOARD_TEST = 0x3A
    FLASH_READ = 0x3B
    FLASH_WRITE = 0x3C
    FLASH_ERASE = 0x3D
    FPGA_CONFIG = 0x3E
    UPDATE = 0x3F


class Status(IntEnum):
    """Response status (``NBP_ST_*``)."""

    OK = 0
    UNKNOWN = 1
    BAD_LEN = 2
    BAD_ARG = 3
    FPGA = 4
    BUSY = 5
    HW = 6
    NO_POWER = 7
    PROTECTED = 8


def status_name(status: int) -> str:
    """Name of a status code, ``"?"`` when unknown."""
    try:
        return Status(status).name
    except ValueError:
        return "?"


class Link(IntEnum):
    """Link a response came on / a stream goes to (``NBP_LINK_*``)."""

    NONE = 0
    USB_FS = 1
    USB_HS = 2
    TCP = 3
    USB_SS = 4


class Cap(IntFlag):
    """Firmware capabilities (``NBP_CAP_*``)."""

    USB_HS = 1 << 0
    RANGE = 1 << 1
    DECIM = 1 << 2
    ETH = 1 << 3
    BOARD = 1 << 4
    USB_SS = 1 << 5


class StartFlag(IntFlag):
    """``start_stream`` flags (``NBP_START_*``)."""

    FLUSH = 1 << 0
    LINK_TEST = 1 << 1


class EthFlag(IntFlag):
    """``Stats.eth`` flags (``NBP_ETH_*``)."""

    PHY = 1 << 0
    IP = 1 << 1
    CLIENT = 1 << 2


class Source(IntEnum):
    """Sample source (``NBP_SRC_*``)."""

    STUB_RANDOM = 0
    STUB_PATTERN = 1
    AD7387 = 2


class DecimMode(IntEnum):
    """Decimation mode (``NBP_DECIM_*``)."""

    AVERAGE = 0
    SUBSAMPLE = 1


class BlockFlag(IntFlag):
    """DATA block flags (``NBP_BLK_*``)."""

    FIRST = 1 << 0
    STALL = 1 << 1
    GAP = 1 << 2
    TEST = 1 << 3


def link_test_word(i: int) -> int:
    """``NBP_LINK_TEST_WORD(i)``: word ``i`` of a link-test block."""
    return ((i * 0x9E3779B1) & 0xFFFFFFFF) ^ 0xA5A5A5A5


class Test(IntEnum):
    """Board self-tests: bit numbers of the ``board_test`` mask (``NBP_TEST_*``)."""

    HSE = 0
    LSE = 1
    RTC = 2
    RAILS = 3
    FLASH_ID = 4
    FLASH_RW = 5
    SD = 6
    LCD = 7
    FPGA_DONE = 8
    FPGA_QSPI = 9
    ETH_PHY = 10
    USB_PD = 11


TEST_COUNT = 12
TEST_ALL = (1 << TEST_COUNT) - 1


class TestResult(IntEnum):
    """Result of one board test (``NBP_TR_*``)."""

    NONE = 0
    PASS = 1
    FAIL = 2
    SKIP = 3


class BoardFlag(IntFlag):
    """``BoardStatus.flags`` (``NBP_BOARD_*``)."""

    HSE = 1 << 0
    LSE = 1 << 1
    RTC = 1 << 2
    DFU = 1 << 3
    SD_CARD = 1 << 4
    PWR_ST = 1 << 5
    APWR_H = 1 << 6
    APWR_L = 1 << 7
    USB3_EN = 1 << 8
    USB3_DIR = 1 << 9
    FPGA_DONE = 1 << 10
    FPGA_INIT_B = 1 << 11
    FPGA_PROG_HELD = 1 << 12
    USB_HS = 1 << 13
    ETH_LINK = 1 << 14
    LCD = 1 << 15
    SWD = 1 << 16
    USB3_AUTO = 1 << 17
    USB3_EN_AUTO = 1 << 18
    USB_C = 1 << 19
    APWR_OK = 1 << 20
    USB_SS = 1 << 21


class BoardSet(IntEnum):
    """``board_set`` items (``NBP_BSET_*``)."""

    LEDS = 0
    APWR_H = 1
    APWR_L = 2
    USB3_EN = 3
    USB3_DIR = 4
    PWR_SEL = 5
    FPGA_PROGRAM = 6
    LCD = 7
    RTC = 8
    PD_MV = 9
    UI_KEY = 10


class Key(IntEnum):
    """Keys for the on-board UI (``NBP_KEY_*``, ``BoardSet.UI_KEY``)."""

    UP = 1
    DOWN = 2
    LEFT = 3
    RIGHT = 4
    ENTER = 5
    BACK = 6


class PwrSel(IntEnum):
    """Power input selection (``NBP_PWR_SEL_*``)."""

    AUTO = 0
    BJ = 1
    USB = 2


class Lcd(IntEnum):
    """LCD mode (``NBP_LCD_*``)."""

    OFF = 0
    STATUS = 1
    PATTERN = 2


class PdState(IntEnum):
    """USB PD sink state (``NBP_PD_*``)."""

    DETACHED = 0
    WAIT_CAPS = 1
    WAIT_ACCEPT = 2
    WAIT_PS_RDY = 3
    READY = 4
    NO_PD = 5


class CC(IntEnum):
    """USB-C CC line state, 2 bits per line (``NBP_CC_*``)."""

    OPEN = 0
    DEFAULT = 1
    A1_5 = 2
    A3_0 = 3


class FpgaMode(IntEnum):
    """FPGA configuration mode (``NBP_FPGA_MODE_*``)."""

    UNKNOWN = 0
    MASTER = 1
    SLAVE = 2


class FpgaProgram(IntEnum):
    """Values for ``BoardSet.FPGA_PROGRAM``."""

    RELEASE = 0
    HOLD = 1
    PULSE = 2


class UpdateOp(IntEnum):
    """``update`` operations (``NBP_UPD_*``)."""

    INFO = 0
    GATEWARE = 1
    FIRMWARE = 2
    REVERT = 3
    RECOVERY = 4


class SlotType(IntEnum):
    """Contents of an update slot (``NBP_UPD_SLOT_*``)."""

    EMPTY = 0
    GATEWARE = 1
    FIRMWARE = 2
