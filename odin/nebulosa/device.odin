package nebulosa

import "base:intrinsics"
import "core:mem"
import "core:net"
import "core:time"

MAX_IN_ENDPOINTS :: 4 // byte streams from the device (one parser each)
@(private)
RX_SIZE :: 64 * 1024

Transport :: enum u8 {
	None,
	TCP,
	USB,
}

@(private)
Queued_Block :: struct {
	buf: []u8, // BLOCK_BYTES, owned
	n:   int, // payload bytes in buf
}

// A connection to one device. Create with connect_tcp / connect_usb, free
// with close. Not thread-safe: one command in flight at a time, from one
// thread.
Device :: struct {
	allocator:      mem.Allocator,
	transport:      Transport,
	tcp:            net.TCP_Socket,
	usb:            Usb_Link,
	parsers:        [MAX_IN_ENDPOINTS]Frame_Parser,
	n_parsers:      int,
	seq:            u16,
	tx:             [HDR_LEN + MAX_PAYLOAD]u8,
	rx:             []u8,

	// GET_INFO at connect time (refreshed by get_info)
	info:           Info,
	// code -> A / V for read_samples; replace freely
	calibration:    Calibration,
	streaming:      bool,

	// DATA frames that arrived during requests, oldest first
	queue:          [dynamic]Queued_Block,
	queue_head:     int,
	free_blocks:    [dynamic][]u8,

	// the current DATA block, words in host order
	blk_seq:        u32,
	blk_t_us:       u32,
	blk_flags:      Block_Flags,
	blk_nwords:     int,
	blk_pos:        int,
	blk_decode:     bool,
	words:          [BLOCK_WORDS]u32,

	decoder:        Stream_Decoder,
	counters:       Stream_Counters, // link part; stream_counters() adds the rest
	expected_seq:   u32,
	have_expected:  bool,
	resync_base:    u64,

	// details of the last .Network / .Usb error
	last_net_error: net.Network_Error,
	last_usb_error: i32,
}

// ---- net error helpers (robust to core:net returning a union or a
// specific error enum, which differs between Odin versions) ---------------

@(private)
net_failed :: proc(e: $E) -> bool {
	when intrinsics.type_is_union(E) {
		return e != nil
	} else {
		return e != E(0)
	}
}

@(private)
net_is_timeout :: proc(e: $E) -> bool {
	when E == net.TCP_Recv_Error || E == net.UDP_Recv_Error {
		return e == .Timeout
	} else when intrinsics.type_is_union(E) {
		#partial switch v in e {
		case net.TCP_Recv_Error:
			return v == .Timeout
		case net.UDP_Recv_Error:
			return v == .Timeout
		}
		return false
	} else {
		return false
	}
}

// ---- lifecycle ------------------------------------------------------------

@(private)
device_new :: proc(transport: Transport, n_parsers: int, allocator: mem.Allocator) -> ^Device {
	dev := new(Device, allocator)
	if dev == nil {
		return nil
	}
	dev.allocator = allocator
	dev.transport = transport
	dev.n_parsers = n_parsers
	dev.rx = make([]u8, RX_SIZE, allocator)
	for i in 0 ..< MAX_IN_ENDPOINTS {
		parser_init(&dev.parsers[i], allocator)
	}
	dev.queue = make([dynamic]Queued_Block, 0, 16, allocator)
	dev.free_blocks = make([dynamic][]u8, 0, 16, allocator)
	dev.calibration = default_calibration()
	return dev
}

@(private)
device_free :: proc(dev: ^Device) {
	allocator := dev.allocator
	for i in 0 ..< MAX_IN_ENDPOINTS {
		parser_destroy(&dev.parsers[i])
	}
	for qb in dev.queue {
		delete(qb.buf, allocator)
	}
	delete(dev.queue)
	for b in dev.free_blocks {
		delete(b, allocator)
	}
	delete(dev.free_blocks)
	delete(dev.rx, allocator)
	free(dev, allocator)
}

// Connects to a device over TCP (`host`: an IPv4 address or a host name).
// One client at a time: the device refuses a second connection, so close
// the Companion first. Does GET_INFO (Device.info).
connect_tcp_host :: proc(host: string, port: int = TCP_PORT, allocator := context.allocator) -> (dev: ^Device, err: Error) {
	ep: net.Endpoint
	addr := net.parse_address(host)
	if addr != nil {
		ep = net.Endpoint{address = addr, port = port}
	} else {
		ep4, rerr := net.resolve_ip4(host)
		if net_failed(rerr) {
			return nil, General_Error.Resolve
		}
		ep = ep4
		ep.port = port
	}
	return connect_tcp_endpoint(ep, allocator)
}

connect_tcp_endpoint :: proc(ep: net.Endpoint, allocator := context.allocator) -> (dev: ^Device, err: Error) {
	sock, derr := net.dial_tcp_from_endpoint(ep)
	if net_failed(derr) {
		return nil, General_Error.Connect_Failed
	}
	_ = net.set_option(sock, .TCP_Nodelay, true)
	dev = device_new(.TCP, 1, allocator)
	if dev == nil {
		net.close(sock)
		return nil, General_Error.Out_Of_Memory
	}
	dev.tcp = sock
	_, err = get_info(dev)
	if err != nil {
		close(dev)
		return nil, err
	}
	return dev, nil
}

connect_tcp :: proc {
	connect_tcp_host,
	connect_tcp_endpoint,
}

// Connects to a device over USB (needs -define:NEBULOSA_USB=true, else
// .Usb_Disabled). bus / address select one device (see list_usb); -1 takes
// the first one found. Does GET_INFO (Device.info).
connect_usb :: proc(bus: int = -1, address: int = -1, allocator := context.allocator) -> (dev: ^Device, err: Error) {
	dev = device_new(.USB, 0, allocator)
	if dev == nil {
		return nil, General_Error.Out_Of_Memory
	}
	err = usb_open(dev, bus, address)
	if err != nil {
		device_free(dev)
		return nil, err
	}
	_, err = get_info(dev)
	if err != nil {
		close(dev)
		return nil, err
	}
	return dev, nil
}

// Stops a stream this connection started (best effort), closes the link
// and frees the device.
close :: proc(dev: ^Device) {
	if dev == nil {
		return
	}
	if dev.streaming && dev.transport != .None {
		_ = stop_stream(dev)
	}
	switch dev.transport {
	case .TCP:
		net.close(dev.tcp)
	case .USB:
		usb_close(dev)
	case .None:
	}
	dev.transport = .None
	device_free(dev)
}

// ---- I/O ------------------------------------------------------------------

@(private)
send_all :: proc(dev: ^Device, data: []u8) -> Error {
	switch dev.transport {
	case .TCP:
		sent := 0
		for sent < len(data) {
			n, serr := net.send_tcp(dev.tcp, data[sent:])
			if net_failed(serr) {
				dev.last_net_error = serr
				return General_Error.Network
			}
			if n <= 0 {
				return General_Error.Closed
			}
			sent += n
		}
		return nil
	case .USB:
		return usb_write(dev, data)
	case .None:
		return General_Error.Closed
	}
	return General_Error.Closed
}

// Receives what arrives within `timeout` into the parsers. nil also when
// nothing came in time (callers watch their own deadline).
@(private)
pump :: proc(dev: ^Device, timeout: time.Duration) -> Error {
	switch dev.transport {
	case .TCP:
		return tcp_pump(dev, timeout)
	case .USB:
		return usb_pump(dev, timeout)
	case .None:
		return General_Error.Closed
	}
	return General_Error.Closed
}

@(private)
tcp_pump :: proc(dev: ^Device, timeout: time.Duration) -> Error {
	t := max(timeout, time.Millisecond) // 0 would mean "block forever"
	serr := net.set_option(dev.tcp, .Receive_Timeout, t)
	if net_failed(serr) {
		dev.last_net_error = serr
		return General_Error.Network
	}
	n, rerr := net.recv_tcp(dev.tcp, dev.rx)
	if net_failed(rerr) {
		if net_is_timeout(rerr) {
			return nil
		}
		dev.last_net_error = rerr
		return General_Error.Network
	}
	if n <= 0 {
		return General_Error.Closed
	}
	parser_push(&dev.parsers[0], dev.rx[:n])
	return nil
}

// Keeps a DATA payload for the stream reader.
@(private)
queue_data :: proc(dev: ^Device, p: []u8) {
	buf: []u8
	if len(dev.free_blocks) > 0 {
		buf = pop(&dev.free_blocks)
	} else {
		buf = make([]u8, BLOCK_BYTES, dev.allocator)
	}
	n := min(len(p), len(buf))
	copy(buf[:n], p[:n])
	append(&dev.queue, Queued_Block{buf = buf, n = n})
}

// ---- request / response ---------------------------------------------------

// Sends one command and waits for its response (type cmd | 0x40, same
// seq). DATA frames arriving meanwhile are queued for the stream reader;
// stale responses are discarded. A non-zero status gives that `Status`.
// The returned payload points into a parser buffer: valid until the next
// call on `dev` (copy it to keep it).
request :: proc(dev: ^Device, cmd: Cmd, payload: []u8 = nil, timeout: time.Duration = DEFAULT_TIMEOUT) -> (resp: []u8, err: Error) {
	if dev == nil || dev.transport == .None {
		return nil, General_Error.Closed
	}
	if len(payload) > MAX_PAYLOAD {
		return nil, General_Error.Payload_Too_Long
	}
	dev.seq += 1
	seq := dev.seq
	n, _ := frame_encode(dev.tx[:], u8(cmd), seq, payload)
	err = send_all(dev, dev.tx[:n])
	if err != nil {
		return nil, err
	}
	want := u8(cmd) | RSP_FLAG
	start := time.tick_now()
	for {
		for i in 0 ..< dev.n_parsers {
			for {
				f, ok := parser_next(&dev.parsers[i])
				if !ok {
					break
				}
				if f.ty == DATA_TYPE {
					queue_data(dev, f.payload)
					continue
				}
				if f.ty == want && f.seq == seq {
					if f.status != 0 {
						return nil, Status(f.status)
					}
					return f.payload, nil
				}
				// a stale response: discarded
			}
		}
		remaining := timeout - time.tick_since(start)
		if remaining <= 0 {
			return nil, General_Error.Timeout
		}
		err = pump(dev, remaining)
		if err != nil {
			return nil, err
		}
	}
	return nil, General_Error.Timeout // not reached
}
