// Prints a device's info, statistics and board status.
//
//     odin run . -collection:nebulosa=../.. -- 192.168.1.50
//     odin run . -collection:nebulosa=../..            (UDP discovery)
//     odin run . -collection:nebulosa=../.. -define:NEBULOSA_USB=true -- usb
package main

import "core:fmt"
import "core:os"
import nb "nebulosa:nebulosa"

main :: proc() {
	dev: ^nb.Device
	err: nb.Error
	if len(os.args) > 1 && os.args[1] == "usb" {
		dev, err = nb.connect_usb()
	} else if len(os.args) > 1 {
		dev, err = nb.connect_tcp(os.args[1])
	} else {
		found, derr := nb.discover()
		if derr != nil || len(found) == 0 {
			fmt.eprintln("no device answered UDP discovery; give a host or 'usb'")
			os.exit(1)
		}
		for d in found {
			fmt.printf("found %d.%d.%d.%d\n", d.address[0], d.address[1], d.address[2], d.address[3])
		}
		a := found[0].address
		host := fmt.tprintf("%d.%d.%d.%d", a[0], a[1], a[2], a[3])
		delete(found)
		dev, err = nb.connect_tcp(host)
	}
	if err != nil {
		fmt.eprintln("connect:", nb.error_string(err))
		os.exit(1)
	}
	defer nb.close(dev)

	info := dev.info
	maj, mnr, pat := nb.version_parts(info.fw_version)
	fmt.printf("firmware   %d.%d.%d (%s)\n", maj, mnr, pat, nb.info_build(&info))
	fmt.printf("protocol   %d\n", info.proto_version)
	fmt.printf("FPGA ID    0x%08X%s\n", info.fpga_id, nb.info_is_format5(info) ? "" : " (format 4: raw blocks only)")
	fmt.printf("base rate  %d Hz\n", info.base_rate_hz)
	fmt.printf("link       %v (unit %d bytes)\n", info.link, info.link_unit)
	fmt.printf("caps       %v\n", info.caps)
	fmt.printf("MAC        %02x:%02x:%02x:%02x:%02x:%02x\n", info.mac[0], info.mac[1], info.mac[2], info.mac[3], info.mac[4], info.mac[5])
	fmt.printf("IP         %d.%d.%d.%d\n", info.ip[0], info.ip[1], info.ip[2], info.ip[3])

	if s, serr := nb.get_stats(dev); serr == nil {
		fmt.printf("uptime     %.1f s, stream %v, FPGA drops %d\n", f64(s.uptime_ms) / 1000, s.stream_link, s.fpga_drops)
	}
	if .Board in info.caps {
		b, berr := nb.board_status(dev)
		if berr != nil {
			fmt.eprintln("board_status:", nb.error_string(berr))
			return
		}
		fmt.printf("board      0x%08X, inputs: jack %d mV, VBUS %d mV, V_MAIN %d mV\n", b.board_id, b.v_bj_mv, b.v_vbus_mv, b.v_main_mv)
		fmt.printf("flags      %v\n", b.flags)
		fmt.printf("USB PD     state %d, %d mV %d mA\n", b.pd_state, b.pd_mv, b.pd_ma)
	}
}
