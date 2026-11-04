"""UDP discovery: a GET_INFO header to port 20035, broadcast or unicast."""

from __future__ import annotations

import socket
import time
from typing import List

from .constants import TCP_PORT, UDP_DISCOVERY_PORT, Cmd, rsp
from .frame import FrameParser, encode
from .records import Discovered, Info


def discover(timeout: float = 0.6, address: str = "255.255.255.255",
             port: int = UDP_DISCOVERY_PORT) -> List[Discovered]:
    """Find Nebulosa devices on the network.

    Sends one GET_INFO command (header only) to ``address:port`` and
    collects the answers for ``timeout`` seconds. Each answer's source
    address is the host to connect to (``Nebulosa.tcp(d.host)``).
    """
    found: List[Discovered] = []
    seen = set()
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        s.bind(("", 0))
        s.sendto(encode(Cmd.GET_INFO, 0), (address, port))
        deadline = time.monotonic() + timeout
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                break
            s.settimeout(left)
            try:
                data, (host, _) = s.recvfrom(2048)
            except socket.timeout:
                break
            except OSError:
                break
            p = FrameParser()
            p.push(data)
            f = p.next_frame()
            if f is None or f.type != rsp(Cmd.GET_INFO) or f.status != 0:
                continue
            try:
                info = Info.parse(f.payload)
            except ValueError:
                continue
            if host in seen:
                continue
            seen.add(host)
            found.append(Discovered(host=host, info=info, port=TCP_PORT))
    finally:
        s.close()
    return found
