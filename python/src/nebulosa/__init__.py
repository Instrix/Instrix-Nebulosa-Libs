"""Nebulosa: script the INSTRIX Nebulosa power profiler over TCP or USB.

    import nebulosa

    with nebulosa.Nebulosa.tcp("192.168.1.50") as dev:
        print(dev.get_info())
        dev.set_decimation(6)
        dev.start_stream()
        for s in dev.read_samples(timeout=1.0):
            print(s.time_s, s.current, s.voltage)

The wire definition is spec/nbp.h and spec/neb_stream.h; docs/API.md is the
contract every binding implements.
"""

from __future__ import annotations

from .constants import (
    BOARD_DIGITAL,
    FPGA_ID_FORMAT5,
    REG_CLK_HZ,
    REG_RANGE_CFG,
    TCP_PORT,
    TEST_ALL,
    TEST_COUNT,
    TS_PERIOD_S,
    UDP_DISCOVERY_PORT,
    USB_PID_FS,
    USB_PID_HS,
    USB_VID,
    CC,
    BlockFlag,
    BoardFlag,
    BoardSet,
    Cap,
    Cmd,
    DecimMode,
    EthFlag,
    FpgaMode,
    FpgaProgram,
    Key,
    Lcd,
    Link,
    PdState,
    PwrSel,
    SlotType,
    Source,
    StartFlag,
    Status,
    Test,
    TestResult,
    UpdateOp,
    link_test_word,
    status_name,
)
from .device import Nebulosa, Progress
from .discovery import discover
from .errors import ImageError, LinkClosed, NebulosaError, TimeoutError, VerifyError
from .frame import Frame, FrameParser, encode
from .images import bitstream, crc32, firmware_check
from .records import (
    Block,
    BlockHeader,
    BoardStatus,
    Decim,
    Discovered,
    Info,
    Pdo,
    RangeState,
    Slot,
    Stats,
    UpdateInfo,
    UsbDevice,
)
from .stream import (
    Calibration,
    DecoderCounters,
    RawSample,
    Sample,
    SampleArrays,
    StreamCounters,
    StreamDecoder,
    StreamProcessor,
    decode_words,
)
from .transport import list_usb

__version__ = "0.1.0"

__all__ = [name for name in dir() if not name.startswith("_") and name != "annotations"]
