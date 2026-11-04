// Streams samples to stdout as CSV: time_s,current_A,voltage_V,range,dirty
//
//     odin run . -collection:nebulosa=../.. -- <host|usb> [seconds=2] [log2n=4]
package main

import "core:fmt"
import "core:os"
import "core:strconv"
import "core:time"
import nb "nebulosa:nebulosa"

main :: proc() {
	if len(os.args) < 2 {
		fmt.eprintln("usage: stream_csv <host|usb> [seconds=2] [log2n=4]")
		os.exit(2)
	}
	seconds := 2.0
	log2n := 4
	if len(os.args) > 2 {
		if v, ok := strconv.parse_f64(os.args[2]); ok {
			seconds = v
		}
	}
	if len(os.args) > 3 {
		if v, ok := strconv.parse_int(os.args[3]); ok {
			log2n = v
		}
	}

	dev: ^nb.Device
	err: nb.Error
	if os.args[1] == "usb" {
		dev, err = nb.connect_usb()
	} else {
		dev, err = nb.connect_tcp(os.args[1])
	}
	if err != nil {
		fmt.eprintln("connect:", nb.error_string(err))
		os.exit(1)
	}
	defer nb.close(dev)

	d, derr := nb.set_decimation(dev, u8(clamp(log2n, 0, 8)))
	if derr != nil {
		fmt.eprintln("set_decimation:", nb.error_string(derr))
		os.exit(1)
	}
	fmt.eprintf("%d samples/s\n", d.rate_hz)
	if serr := nb.start_stream(dev); serr != nil {
		fmt.eprintln("start_stream:", nb.error_string(serr))
		os.exit(1)
	}

	buf := make([]nb.Sample, 4096)
	defer delete(buf)
	fmt.println("time_s,current_A,voltage_V,range,dirty")
	start := time.tick_now()
	for time.duration_seconds(time.tick_since(start)) < seconds {
		n, rerr := nb.read_samples(dev, buf, 500 * time.Millisecond)
		if rerr != nil {
			fmt.eprintln("read_samples:", nb.error_string(rerr))
			break
		}
		for s in buf[:n] {
			fmt.printf("%.7f,%.9g,%.5f,%d,%d\n", nb.sample_time_s(s), s.current, s.voltage, s.range, s.dirty ? 1 : 0)
		}
	}
	_ = nb.stop_stream(dev)
	c := nb.stream_counters(dev)
	fmt.eprintf("samples %d, blocks %d, lost %d, seq errors %d, malformed %d\n", c.samples, c.data_blocks, c.lost_blocks, c.seq_errors, c.malformed)
}
