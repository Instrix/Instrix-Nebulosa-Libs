// Average current and voltage over a time window, with charge and energy.
//
//     odin run . -collection:nebulosa=../.. -- <host|usb> [seconds=5]
package main

import "core:fmt"
import "core:os"
import "core:strconv"
import "core:time"
import nb "nebulosa:nebulosa"

main :: proc() {
	if len(os.args) < 2 {
		fmt.eprintln("usage: average_current <host|usb> [seconds=5]")
		os.exit(2)
	}
	seconds := 5.0
	if len(os.args) > 2 {
		if v, ok := strconv.parse_f64(os.args[2]); ok {
			seconds = v
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

	// 2 MSPS / 2^6 = 31.25 kS/s, averaged in the FPGA
	if _, derr := nb.set_decimation(dev, 6, .Average); derr != nil {
		fmt.eprintln("set_decimation:", nb.error_string(derr))
		os.exit(1)
	}
	if serr := nb.start_stream(dev); serr != nil {
		fmt.eprintln("start_stream:", nb.error_string(serr))
		os.exit(1)
	}

	buf := make([]nb.Sample, 4096)
	defer delete(buf)
	sum_i, sum_v, sum_p: f64
	min_i, max_i := 1e300, -1e300
	count, dirty := 0, 0
	first_ts, last_ts: u64
	start := time.tick_now()
	for time.duration_seconds(time.tick_since(start)) < seconds {
		n, rerr := nb.read_samples(dev, buf, 500 * time.Millisecond)
		if rerr != nil {
			fmt.eprintln("read_samples:", nb.error_string(rerr))
			break
		}
		for s in buf[:n] {
			if count == 0 {
				first_ts = s.ts
			}
			last_ts = s.ts + u64(s.step)
			sum_i += s.current
			sum_v += s.voltage
			sum_p += s.current * s.voltage
			min_i = min(min_i, s.current)
			max_i = max(max_i, s.current)
			if s.dirty {
				dirty += 1
			}
			count += 1
		}
	}
	_ = nb.stop_stream(dev)
	if count == 0 {
		fmt.eprintln("no samples")
		os.exit(1)
	}
	span := f64(last_ts - first_ts) * nb.TS_PERIOD_S
	avg_i := sum_i / f64(count)
	avg_p := sum_p / f64(count)
	fmt.printf("samples   %d (%d dirty) over %.3f s\n", count, dirty, span)
	fmt.printf("current   avg %.6f mA, min %.6f mA, max %.6f mA\n", avg_i * 1e3, min_i * 1e3, max_i * 1e3)
	fmt.printf("voltage   avg %.4f V\n", sum_v / f64(count))
	fmt.printf("power     avg %.6f mW\n", avg_p * 1e3)
	fmt.printf("charge    %.6f mC, energy %.6f mJ\n", avg_i * span * 1e3, avg_p * span * 1e3)
	c := nb.stream_counters(dev)
	if c.lost_blocks != 0 || c.seq_errors != 0 {
		fmt.printf("warning: %d DATA blocks lost, %d FPGA sequence errors\n", c.lost_blocks, c.seq_errors)
	}
}
