package nebulosa

// Library-side failures. Values start at 1: an `Error` holding one is
// never nil.
General_Error :: enum u8 {
	Timeout = 1,        // no matching response in time
	Closed,             // the connection / device went away
	Network,            // a socket call failed: see Device.last_net_error
	Connect_Failed,     // TCP connect refused or failed (one client at a time: close the Companion)
	Resolve,            // host name or address not understood
	Usb,                // a libusb call failed: see Device.last_usb_error
	Usb_Disabled,       // built without -define:NEBULOSA_USB=true
	Usb_Access,         // no permission to open the device (udev rule, tools/99-nebulosa.rules)
	Usb_Busy,           // the interface is claimed by another program (the Companion?)
	Not_Found,          // no matching device
	No_Interface,       // the device has no NBP vendor interface / endpoints
	Short_Payload,      // a response shorter than its record
	Payload_Too_Long,   // a command payload above MAX_PAYLOAD
	Invalid_Argument,   // an argument out of range (flash alignment, lengths, ...)
	Verify_Failed,      // upload_flash: the read-back differs
	Unsupported_Format, // read_samples on a format-4 FPGA: use read_block
	Out_Of_Memory,
}

// Every procedure that can fail returns an `Error` as its last value; nil
// is success. A device answering with a non-zero status gives that
// `Status` (its code; `status_name` for its name).
Error :: union {
	General_Error,
	Status,
}

general_error_string :: proc(e: General_Error) -> string {
	switch e {
	case .Timeout:            return "timeout"
	case .Closed:             return "connection closed"
	case .Network:            return "network error"
	case .Connect_Failed:     return "connect failed (is another client, e.g. the Companion, connected?)"
	case .Resolve:            return "cannot resolve host"
	case .Usb:                return "USB error"
	case .Usb_Disabled:       return "USB support not compiled in (-define:NEBULOSA_USB=true)"
	case .Usb_Access:         return "USB access denied (install tools/99-nebulosa.rules)"
	case .Usb_Busy:           return "USB interface busy (claimed by another program)"
	case .Not_Found:          return "device not found"
	case .No_Interface:       return "no NBP interface on the device"
	case .Short_Payload:      return "response payload too short"
	case .Payload_Too_Long:   return "command payload too long"
	case .Invalid_Argument:   return "invalid argument"
	case .Verify_Failed:      return "flash verify failed"
	case .Unsupported_Format: return "stream format not decoded (FPGA older than format 5): use read_block"
	case .Out_Of_Memory:      return "out of memory"
	}
	return "?"
}

// A short description of `err` ("ok" for nil). The string is static.
error_string :: proc(err: Error) -> string {
	switch e in err {
	case General_Error:
		return general_error_string(e)
	case Status:
		return status_name(e)
	}
	return "ok"
}

// true when `err` is the timeout error.
is_timeout :: proc(err: Error) -> bool {
	if e, ok := err.(General_Error); ok {
		return e == .Timeout
	}
	return false
}
