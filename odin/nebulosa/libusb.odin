package nebulosa

// A minimal hand-written libusb-1.0 binding: only what usb.odin uses.
// The types are always declared (they link nothing); the foreign import
// and the procedures exist only with -define:NEBULOSA_USB=true. Layouts
// follow /usr/include/libusb-1.0/libusb.h (Odin structs use C layout).

import "core:c"

NEBULOSA_USB :: #config(NEBULOSA_USB, false)

Libusb_Context :: struct {}
Libusb_Device :: struct {}
Libusb_Device_Handle :: struct {}

LIBUSB_SUCCESS :: 0
LIBUSB_ERROR_IO :: -1
LIBUSB_ERROR_INVALID_PARAM :: -2
LIBUSB_ERROR_ACCESS :: -3
LIBUSB_ERROR_NO_DEVICE :: -4
LIBUSB_ERROR_NOT_FOUND :: -5
LIBUSB_ERROR_BUSY :: -6
LIBUSB_ERROR_TIMEOUT :: -7
LIBUSB_ERROR_OVERFLOW :: -8
LIBUSB_ERROR_PIPE :: -9
LIBUSB_ERROR_INTERRUPTED :: -10
LIBUSB_ERROR_NO_MEM :: -11

LIBUSB_TRANSFER_TYPE_BULK :: 2
LIBUSB_ENDPOINT_IN :: 0x80

// enum libusb_transfer_status
LIBUSB_TRANSFER_COMPLETED :: 0
LIBUSB_TRANSFER_ERROR :: 1
LIBUSB_TRANSFER_TIMED_OUT :: 2
LIBUSB_TRANSFER_CANCELLED :: 3
LIBUSB_TRANSFER_STALL :: 4
LIBUSB_TRANSFER_NO_DEVICE :: 5
LIBUSB_TRANSFER_OVERFLOW :: 6

Libusb_Device_Descriptor :: struct {
	bLength:            u8,
	bDescriptorType:    u8,
	bcdUSB:             u16,
	bDeviceClass:       u8,
	bDeviceSubClass:    u8,
	bDeviceProtocol:    u8,
	bMaxPacketSize0:    u8,
	idVendor:           u16,
	idProduct:          u16,
	bcdDevice:          u16,
	iManufacturer:      u8,
	iProduct:           u8,
	iSerialNumber:      u8,
	bNumConfigurations: u8,
}

Libusb_Endpoint_Descriptor :: struct {
	bLength:          u8,
	bDescriptorType:  u8,
	bEndpointAddress: u8,
	bmAttributes:     u8,
	wMaxPacketSize:   u16,
	bInterval:        u8,
	bRefresh:         u8,
	bSynchAddress:    u8,
	extra:            [^]u8,
	extra_length:     c.int,
}

Libusb_Interface_Descriptor :: struct {
	bLength:            u8,
	bDescriptorType:    u8,
	bInterfaceNumber:   u8,
	bAlternateSetting:  u8,
	bNumEndpoints:      u8,
	bInterfaceClass:    u8,
	bInterfaceSubClass: u8,
	bInterfaceProtocol: u8,
	iInterface:         u8,
	endpoint:           [^]Libusb_Endpoint_Descriptor,
	extra:              [^]u8,
	extra_length:       c.int,
}

Libusb_Interface :: struct {
	altsetting:     [^]Libusb_Interface_Descriptor,
	num_altsetting: c.int,
}

Libusb_Config_Descriptor :: struct {
	bLength:             u8,
	bDescriptorType:     u8,
	wTotalLength:        u16,
	bNumInterfaces:      u8,
	bConfigurationValue: u8,
	iConfiguration:      u8,
	bmAttributes:        u8,
	MaxPower:            u8,
	interfaces:          [^]Libusb_Interface, // C: `interface`
	extra:               [^]u8,
	extra_length:        c.int,
}

Libusb_Iso_Packet_Descriptor :: struct {
	length:        c.uint,
	actual_length: c.uint,
	status:        c.int, // enum libusb_transfer_status
}

Libusb_Transfer_Cb :: #type proc "c" (transfer: ^Libusb_Transfer)

// struct libusb_transfer. Allocated by libusb_alloc_transfer (never by
// us), so the trailing flexible array need not be declared.
Libusb_Transfer :: struct {
	dev_handle:      ^Libusb_Device_Handle,
	flags:           u8,
	endpoint:        u8,
	kind:            u8, // C: `type`
	timeout:         c.uint,
	status:          c.int, // enum libusb_transfer_status
	length:          c.int,
	actual_length:   c.int,
	callback:        Libusb_Transfer_Cb,
	user_data:       rawptr,
	buffer:          [^]u8,
	num_iso_packets: c.int,
	// iso_packet_desc[]: the C flexible array member (offset 60 on 64-bit),
	// left out: bulk transfers are allocated with 0 iso packets and the
	// field is never touched from Odin.
}

// struct timeval. Linux / macOS (64-bit): tv_sec is a long; tv_usec a long
// on Linux, an int (+ padding) on macOS, which a little-endian long write
// of a value below 1e6 fills the same way.
Libusb_Timeval :: struct {
	tv_sec:  c.long,
	tv_usec: c.long,
}

when NEBULOSA_USB {
	foreign import libusb "system:usb-1.0"

	@(default_calling_convention = "c")
	foreign libusb {
		libusb_init :: proc(ctx: ^^Libusb_Context) -> c.int ---
		libusb_exit :: proc(ctx: ^Libusb_Context) ---
		libusb_get_device_list :: proc(ctx: ^Libusb_Context, list: ^[^]^Libusb_Device) -> int --- // ssize_t
		libusb_free_device_list :: proc(list: [^]^Libusb_Device, unref_devices: c.int) ---
		libusb_get_device_descriptor :: proc(dev: ^Libusb_Device, desc: ^Libusb_Device_Descriptor) -> c.int ---
		libusb_get_active_config_descriptor :: proc(dev: ^Libusb_Device, config: ^^Libusb_Config_Descriptor) -> c.int ---
		libusb_free_config_descriptor :: proc(config: ^Libusb_Config_Descriptor) ---
		libusb_get_bus_number :: proc(dev: ^Libusb_Device) -> u8 ---
		libusb_get_device_address :: proc(dev: ^Libusb_Device) -> u8 ---
		libusb_open :: proc(dev: ^Libusb_Device, handle: ^^Libusb_Device_Handle) -> c.int ---
		libusb_close :: proc(handle: ^Libusb_Device_Handle) ---
		libusb_claim_interface :: proc(handle: ^Libusb_Device_Handle, interface_number: c.int) -> c.int ---
		libusb_release_interface :: proc(handle: ^Libusb_Device_Handle, interface_number: c.int) -> c.int ---
		libusb_get_string_descriptor_ascii :: proc(handle: ^Libusb_Device_Handle, desc_index: u8, data: [^]u8, length: c.int) -> c.int ---
		libusb_bulk_transfer :: proc(handle: ^Libusb_Device_Handle, endpoint: u8, data: [^]u8, length: c.int, transferred: ^c.int, timeout: c.uint) -> c.int ---
		libusb_alloc_transfer :: proc(iso_packets: c.int) -> ^Libusb_Transfer ---
		libusb_free_transfer :: proc(transfer: ^Libusb_Transfer) ---
		libusb_submit_transfer :: proc(transfer: ^Libusb_Transfer) -> c.int ---
		libusb_cancel_transfer :: proc(transfer: ^Libusb_Transfer) -> c.int ---
		libusb_handle_events_timeout_completed :: proc(ctx: ^Libusb_Context, tv: ^Libusb_Timeval, completed: ^c.int) -> c.int ---
	}
}
