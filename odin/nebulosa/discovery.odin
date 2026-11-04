package nebulosa

import "core:net"
import "core:time"

// A device that answered UDP discovery.
Discovered :: struct {
	address: net.IP4_Address, // connect here (the answer's source address)
	info:    Info,
}

// Finds devices on the network: sends an 8-byte GET_INFO header to
// `target` (an IPv4 address; "" = broadcast 255.255.255.255) on
// UDP_DISCOVERY_PORT and collects the answers for `listen`. The result is
// allocated with `allocator` (delete it with the same allocator).
discover :: proc(listen: time.Duration = DISCOVERY_TIME, target: string = "", allocator := context.allocator) -> (found: []Discovered, err: Error) {
	dest: net.Address = net.IP4_Address{255, 255, 255, 255}
	if target != "" {
		dest = net.parse_address(target)
		if dest == nil {
			return nil, General_Error.Resolve
		}
	}
	sock, serr := net.make_unbound_udp_socket(.IP4)
	if net_failed(serr) {
		return nil, General_Error.Network
	}
	defer net.close(sock)
	_ = net.set_option(sock, .Broadcast, true)

	// GET_INFO header: 4E 42 02 00 00 00 00 00
	msg: [HDR_LEN]u8
	_, _ = frame_encode(msg[:], u8(Cmd.Get_Info), 0, nil)
	_, werr := net.send_udp(sock, msg[:], net.Endpoint{address = dest, port = UDP_DISCOVERY_PORT})
	if net_failed(werr) {
		return nil, General_Error.Network
	}

	out := make([dynamic]Discovered, 0, 4, allocator)
	buf: [1500]u8
	start := time.tick_now()
	for {
		remaining := listen - time.tick_since(start)
		if remaining <= 0 {
			break
		}
		_ = net.set_option(sock, .Receive_Timeout, max(remaining, time.Millisecond))
		n, from, rerr := net.recv_udp(sock, buf[:])
		if net_failed(rerr) {
			if net_is_timeout(rerr) {
				break
			}
			delete(out)
			return nil, General_Error.Network
		}
		b := buf[:n]
		if n < HDR_LEN || b[0] != 'N' || b[1] != 'B' || b[2] != u8(Cmd.Get_Info) | RSP_FLAG || b[3] != 0 {
			continue
		}
		plen := int(le16(b, 6))
		if HDR_LEN + plen > n {
			continue
		}
		info, ok := parse_info(b[HDR_LEN:HDR_LEN + plen])
		if !ok {
			continue
		}
		ip4, is4 := from.address.(net.IP4_Address)
		if !is4 {
			continue
		}
		dup := false
		for d in out {
			if d.address == ip4 {
				dup = true
				break
			}
		}
		if !dup {
			append(&out, Discovered{address = ip4, info = info})
		}
	}
	return out[:], nil
}
