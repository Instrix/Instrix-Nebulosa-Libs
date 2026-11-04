package nebulosa

// NBP over USB (libusb-1.0): the vendor interface (class FF / 4E / 01).
// Its bulk OUT endpoint takes commands (synchronous writes); every bulk IN
// endpoint is read, each as its own byte stream with its own frame parser
// (High-Speed: one carries DATA, one responses; Full-Speed: one both).
//
// IN uses libusb's asynchronous API: USB_QUEUE transfers of USB_XFER bytes
// stay queued on every IN endpoint, so the device can always send (a
// synchronous round-robin read leaves gaps: the idle endpoint's timeout
// blocks the DATA endpoint and the device's ring overflows). The callbacks
// run inside libusb_handle_events_timeout_completed (usb_pump), in the
// caller's thread.
//
// Compiled in with -define:NEBULOSA_USB=true; otherwise the procedures
// below are stubs answering .Usb_Disabled.

import "base:runtime"
import "core:c"
import "core:time"

USB_QUEUE :: 8 // IN transfers kept queued per endpoint
USB_XFER :: 32 * 1024 // bytes per IN transfer

// One device found by list_usb.
Usb_Device_Info :: struct {
	bus:        int,
	address:    int,
	product_id: u16, // USB_PID_HS or USB_PID_FS
	full_speed: bool, // the USB-C (Full-Speed) port
	product:    string, // "" when unreadable (no permission)
	serial:     string,
	can_open:   bool, // false: no permission (udev rule) or busy
}

// Frees what list_usb returned.
usb_device_list_destroy :: proc(devs: []Usb_Device_Info, allocator := context.allocator) {
	for d in devs {
		delete(d.product, allocator)
		delete(d.serial, allocator)
	}
	delete(devs, allocator)
}

// libusb timeouts are milliseconds, 0 meaning "forever": at least 1.
@(private)
usb_ms :: proc(d: time.Duration) -> c.uint {
	ms := i64(d / time.Millisecond)
	if ms < 1 {
		ms = 1
	}
	return c.uint(ms)
}

@(private)
usb_context :: proc "contextless" () -> runtime.Context {
	return runtime.default_context()
}

when NEBULOSA_USB {

	Usb_Link :: struct {
		ctx:         ^Libusb_Context,
		handle:      ^Libusb_Device_Handle,
		iface:       c.int,
		claimed:     bool,
		ep_out:      u8,
		ep_in:       [MAX_IN_ENDPOINTS]u8,
		n_in:        int,
		xfers:       [MAX_IN_ENDPOINTS * USB_QUEUE]^Libusb_Transfer,
		bufs:        [MAX_IN_ENDPOINTS * USB_QUEUE][]u8,
		active:      int, // transfers submitted and not finished
		stopping:    bool, // the callbacks don't resubmit
		err:         Error, // set by a callback
	}

	@(private)
	is_nbp_desc :: proc(d: ^Libusb_Device_Descriptor) -> bool {
		return d.idVendor == USB_VID && (d.idProduct == USB_PID_HS || d.idProduct == USB_PID_FS)
	}

	@(private)
	usb_string :: proc(h: ^Libusb_Device_Handle, index: u8, allocator: runtime.Allocator) -> string {
		if index == 0 {
			return ""
		}
		buf: [256]u8
		n := libusb_get_string_descriptor_ascii(h, index, raw_data(buf[:]), c.int(len(buf)))
		if n <= 0 {
			return ""
		}
		out := make([]u8, int(n), allocator)
		copy(out, buf[:n])
		return string(out)
	}

	@(private)
	usb_rc_error :: proc(rc: c.int) -> Error {
		switch rc {
		case LIBUSB_ERROR_ACCESS:
			return General_Error.Usb_Access
		case LIBUSB_ERROR_BUSY:
			return General_Error.Usb_Busy
		case LIBUSB_ERROR_NO_DEVICE:
			return General_Error.Closed
		case LIBUSB_ERROR_NOT_FOUND:
			return General_Error.Not_Found
		case LIBUSB_ERROR_TIMEOUT:
			return General_Error.Timeout
		case LIBUSB_ERROR_NO_MEM:
			return General_Error.Out_Of_Memory
		}
		return General_Error.Usb
	}

	// The Nebulosa devices on the USB bus. Free with usb_device_list_destroy.
	list_usb :: proc(allocator := context.allocator) -> (devs: []Usb_Device_Info, err: Error) {
		ctx: ^Libusb_Context
		if libusb_init(&ctx) < 0 {
			return nil, General_Error.Usb
		}
		defer libusb_exit(ctx)
		list: [^]^Libusb_Device
		cnt := libusb_get_device_list(ctx, &list)
		if cnt < 0 {
			return nil, General_Error.Usb
		}
		defer libusb_free_device_list(list, 1)
		out := make([dynamic]Usb_Device_Info, 0, 4, allocator)
		for i in 0 ..< cnt {
			d := list[i]
			desc: Libusb_Device_Descriptor
			if libusb_get_device_descriptor(d, &desc) < 0 || !is_nbp_desc(&desc) {
				continue
			}
			info := Usb_Device_Info {
				bus        = int(libusb_get_bus_number(d)),
				address    = int(libusb_get_device_address(d)),
				product_id = desc.idProduct,
				full_speed = desc.idProduct == USB_PID_FS,
			}
			h: ^Libusb_Device_Handle
			if libusb_open(d, &h) == 0 {
				info.can_open = true
				info.product = usb_string(h, desc.iProduct, allocator)
				info.serial = usb_string(h, desc.iSerialNumber, allocator)
				libusb_close(h)
			}
			append(&out, info)
		}
		return out[:], nil
	}

	// IN transfer done (inside libusb_handle_events_*): feed the bytes to
	// the endpoint's parser and resubmit.
	@(private)
	usb_in_done :: proc "c" (t: ^Libusb_Transfer) {
		context = usb_context()
		dev := cast(^Device)t.user_data
		src := 0
		for src < dev.usb.n_in && dev.usb.ep_in[src] != t.endpoint {
			src += 1
		}
		if t.status == LIBUSB_TRANSFER_COMPLETED || t.status == LIBUSB_TRANSFER_TIMED_OUT {
			if t.actual_length > 0 && src < dev.usb.n_in {
				parser_push(&dev.parsers[src], t.buffer[:int(t.actual_length)])
			}
			if !dev.usb.stopping && libusb_submit_transfer(t) == 0 {
				return
			}
		} else if t.status == LIBUSB_TRANSFER_NO_DEVICE ||
		   t.status == LIBUSB_TRANSFER_ERROR ||
		   t.status == LIBUSB_TRANSFER_STALL ||
		   t.status == LIBUSB_TRANSFER_OVERFLOW {
			dev.usb.stopping = true
			if dev.usb.err == nil {
				dev.usb.err = General_Error.Closed
			}
		}
		dev.usb.active -= 1 // cancelled, failed, or not resubmitted
	}

	@(private)
	usb_start_transfers :: proc(dev: ^Device) -> Error {
		k := 0
		for i in 0 ..< dev.usb.n_in {
			for _ in 0 ..< USB_QUEUE {
				t := libusb_alloc_transfer(0)
				if t == nil {
					return General_Error.Out_Of_Memory
				}
				buf := make([]u8, USB_XFER, dev.allocator)
				dev.usb.xfers[k] = t
				dev.usb.bufs[k] = buf
				k += 1
				// libusb_fill_bulk_transfer is a static inline: by hand
				t.dev_handle = dev.usb.handle
				t.endpoint = dev.usb.ep_in[i]
				t.kind = LIBUSB_TRANSFER_TYPE_BULK
				t.timeout = 0
				t.buffer = raw_data(buf)
				t.length = c.int(len(buf))
				t.callback = usb_in_done
				t.user_data = dev
				t.flags = 0 // the buffer is ours (freed in usb_stop_transfers)
				rc := libusb_submit_transfer(t)
				if rc != 0 {
					dev.last_usb_error = i32(rc)
					return usb_rc_error(rc)
				}
				dev.usb.active += 1
			}
		}
		return nil
	}

	@(private)
	usb_stop_transfers :: proc(dev: ^Device) {
		dev.usb.stopping = true
		for t in dev.usb.xfers {
			if t != nil {
				_ = libusb_cancel_transfer(t)
			}
		}
		start := time.tick_now()
		for dev.usb.active > 0 && time.tick_since(start) < time.Second {
			tv := Libusb_Timeval{tv_sec = 0, tv_usec = 50_000}
			_ = libusb_handle_events_timeout_completed(dev.usb.ctx, &tv, nil)
		}
		if dev.usb.active <= 0 {
			// else leaked rather than freed while in flight
			for k in 0 ..< len(dev.usb.xfers) {
				if dev.usb.xfers[k] != nil {
					libusb_free_transfer(dev.usb.xfers[k])
					dev.usb.xfers[k] = nil
				}
				if dev.usb.bufs[k] != nil {
					delete(dev.usb.bufs[k], dev.allocator)
					dev.usb.bufs[k] = nil
				}
			}
		}
	}

	@(private)
	usb_open :: proc(dev: ^Device, bus: int, address: int) -> Error {
		u := &dev.usb
		if libusb_init(&u.ctx) < 0 {
			u.ctx = nil
			return General_Error.Usb
		}
		list: [^]^Libusb_Device
		cnt := libusb_get_device_list(u.ctx, &list)
		if cnt < 0 {
			usb_close(dev)
			return General_Error.Usb
		}
		found: ^Libusb_Device
		for i in 0 ..< cnt {
			d := list[i]
			desc: Libusb_Device_Descriptor
			if libusb_get_device_descriptor(d, &desc) < 0 || !is_nbp_desc(&desc) {
				continue
			}
			if bus >= 0 && int(libusb_get_bus_number(d)) != bus {
				continue
			}
			if address >= 0 && int(libusb_get_device_address(d)) != address {
				continue
			}
			found = d
			break
		}
		if found == nil {
			libusb_free_device_list(list, 1)
			usb_close(dev)
			return General_Error.Not_Found
		}
		rc := libusb_open(found, &u.handle)
		if rc < 0 {
			u.handle = nil
			libusb_free_device_list(list, 1)
			dev.last_usb_error = i32(rc)
			usb_close(dev)
			return usb_rc_error(rc)
		}
		cfg: ^Libusb_Config_Descriptor
		rc = libusb_get_active_config_descriptor(found, &cfg)
		libusb_free_device_list(list, 1) // the open handle keeps its own reference
		if rc < 0 {
			dev.last_usb_error = i32(rc)
			usb_close(dev)
			return usb_rc_error(rc)
		}
		have_iface := false
		have_out := false
		search: for i in 0 ..< int(cfg.bNumInterfaces) {
			itf := &cfg.interfaces[i]
			for j in 0 ..< int(itf.num_altsetting) {
				alt := &itf.altsetting[j]
				if alt.bInterfaceClass != USB_CLASS || alt.bInterfaceSubClass != USB_SUBCLASS || alt.bInterfaceProtocol != USB_PROTOCOL {
					continue
				}
				have_iface = true
				u.iface = c.int(alt.bInterfaceNumber)
				for k in 0 ..< int(alt.bNumEndpoints) {
					ep := &alt.endpoint[k]
					if ep.bmAttributes & 3 != LIBUSB_TRANSFER_TYPE_BULK {
						continue
					}
					if ep.bEndpointAddress & LIBUSB_ENDPOINT_IN != 0 {
						if u.n_in < MAX_IN_ENDPOINTS {
							u.ep_in[u.n_in] = ep.bEndpointAddress
							u.n_in += 1
						}
					} else if !have_out {
						u.ep_out = ep.bEndpointAddress
						have_out = true
					}
				}
				break search
			}
		}
		libusb_free_config_descriptor(cfg)
		if !have_iface || !have_out || u.n_in == 0 {
			usb_close(dev)
			return General_Error.No_Interface
		}
		rc = libusb_claim_interface(u.handle, u.iface)
		if rc < 0 {
			dev.last_usb_error = i32(rc)
			usb_close(dev)
			return usb_rc_error(rc)
		}
		u.claimed = true
		dev.n_parsers = u.n_in
		err := usb_start_transfers(dev)
		if err != nil {
			usb_close(dev)
			return err
		}
		return nil
	}

	@(private)
	usb_close :: proc(dev: ^Device) {
		u := &dev.usb
		if u.handle != nil {
			usb_stop_transfers(dev)
			if u.claimed {
				_ = libusb_release_interface(u.handle, u.iface)
				u.claimed = false
			}
			libusb_close(u.handle)
			u.handle = nil
		}
		if u.ctx != nil {
			libusb_exit(u.ctx)
			u.ctx = nil
		}
	}

	@(private)
	usb_write :: proc(dev: ^Device, data: []u8) -> Error {
		u := &dev.usb
		if u.handle == nil || u.err != nil {
			return General_Error.Closed
		}
		sent := 0
		for sent < len(data) {
			xfer: c.int
			rest := data[sent:]
			rc := libusb_bulk_transfer(u.handle, u.ep_out, raw_data(rest), c.int(len(rest)), &xfer, 1000)
			sent += int(xfer)
			if rc < 0 {
				dev.last_usb_error = i32(rc)
				return usb_rc_error(rc)
			}
		}
		return nil
	}

	// Runs the IN transfers' callbacks, waiting up to `timeout` for one.
	@(private)
	usb_pump :: proc(dev: ^Device, timeout: time.Duration) -> Error {
		u := &dev.usb
		if u.err != nil {
			return u.err
		}
		us := i64(timeout / time.Microsecond)
		if us < 1000 {
			us = 1000
		}
		tv := Libusb_Timeval {
			tv_sec  = c.long(us / 1_000_000),
			tv_usec = c.long(us % 1_000_000),
		}
		rc := libusb_handle_events_timeout_completed(u.ctx, &tv, nil)
		if u.err != nil {
			return u.err
		}
		if rc != 0 && rc != LIBUSB_ERROR_INTERRUPTED && rc != LIBUSB_ERROR_TIMEOUT {
			dev.last_usb_error = i32(rc)
			return General_Error.Usb
		}
		return nil
	}

} else {

	Usb_Link :: struct {}

	// The Nebulosa devices on the USB bus (.Usb_Disabled: built without
	// -define:NEBULOSA_USB=true).
	list_usb :: proc(allocator := context.allocator) -> (devs: []Usb_Device_Info, err: Error) {
		return nil, General_Error.Usb_Disabled
	}

	@(private)
	usb_open :: proc(dev: ^Device, bus: int, address: int) -> Error {
		return General_Error.Usb_Disabled
	}

	@(private)
	usb_close :: proc(dev: ^Device) {}

	@(private)
	usb_write :: proc(dev: ^Device, data: []u8) -> Error {
		return General_Error.Usb_Disabled
	}

	@(private)
	usb_pump :: proc(dev: ^Device, timeout: time.Duration) -> Error {
		return General_Error.Usb_Disabled
	}
}
