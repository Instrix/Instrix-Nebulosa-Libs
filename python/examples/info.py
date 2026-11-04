#!/usr/bin/env python3
"""Print device information and board status (discovers a device if no --host / --usb)."""

from _connect import connect, nebulosa, parser


def names(flag_cls, value):
    return " ".join(f.name for f in flag_cls if value & f) or "-"


def main():
    args = parser(__doc__).parse_args()
    with connect(args) as dev:
        i = dev.get_info()
        print("link        %s (unit %d bytes)" % (i.link_name, i.link_unit))
        print("firmware    %s, built %s, protocol %d" % (i.fw_version_str, i.build, i.proto_version))
        print("FPGA ID     0x%08X (stream format %d)" % (i.fpga_id, i.stream_format))
        print("base rate   %d S/s, %d words/block" % (i.base_rate_hz, i.block_words))
        print("caps        %s" % names(nebulosa.Cap, i.caps))
        if i.mac_str:
            print("Ethernet    %s %s" % (i.mac_str, i.ip or "(no address)"))
        if not i.caps & nebulosa.Cap.BOARD:
            return
        b = dev.board_status()
        print("board       0x%08X, up %.1f s, core %d MHz" % (b.board_id, b.uptime_ms / 1000, b.core_hz // 1000000))
        print("supplies    jack %.2f V, VBUS %.2f V, V_MAIN %.2f V" % (b.v_bj_mv / 1e3, b.v_vbus_mv / 1e3, b.v_main_mv / 1e3))
        print("flags       %s" % names(nebulosa.BoardFlag, b.flags))
        print("flash       JEDEC 0x%06X, %d KB" % (b.flash_jedec, b.flash_bytes // 1024))
        try:
            pd = nebulosa.PdState(b.pd_state).name
        except ValueError:
            pd = str(b.pd_state)
        print("USB PD      %s, %d mV %d mA" % (pd, b.pd_mv, b.pd_ma))
        print("last tests  " + ", ".join("%s %s" % (t.name, b.test_result(t).name.lower()) for t in nebulosa.Test))


if __name__ == "__main__":
    main()
