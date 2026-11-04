"""Byte transports: TCP (stdlib) and USB (pyusb, optional).

A transport moves bytes; it owns one ``FrameParser`` per device->host byte
stream (TCP: one; USB: one per bulk IN endpoint) and hands back whole frames.
"""

from __future__ import annotations

import errno
import queue
import select
import socket
import threading
import time
from typing import Any, List, Optional, Tuple

from .constants import (
    TCP_PORT,
    USB_CLASS,
    USB_PID_FS,
    USB_PID_HS,
    USB_PROTOCOL,
    USB_SUBCLASS,
    USB_VID,
)
from .errors import LinkClosed, NebulosaError
from .frame import Frame, FrameParser
from .records import UsbDevice


class Transport:
    """Base class. ``write`` sends bytes; ``poll(timeout)`` waits up to
    ``timeout`` seconds for input and returns the complete frames it got
    (possibly none)."""

    name = "?"

    def write(self, data: bytes) -> None:
        raise NotImplementedError

    def poll(self, timeout: float) -> List[Frame]:
        raise NotImplementedError

    def close(self) -> None:
        raise NotImplementedError

    @property
    def resyncs(self) -> int:
        return 0


class TcpTransport(Transport):
    """NBP over one TCP connection (responses and DATA interleaved)."""

    def __init__(self, host: str, port: int = TCP_PORT, timeout: float = 3.0) -> None:
        try:
            self.sock = socket.create_connection((host, port), timeout=timeout)
        except OSError as e:
            raise LinkClosed("connect %s:%d: %s (only one client at a time: is the Companion connected?)"
                             % (host, port, e)) from e
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        try:
            self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 4 << 20)
        except OSError:  # pragma: no cover
            pass
        self.sock.setblocking(False)
        self.parser = FrameParser()
        self.name = "TCP %s:%d" % (host, port)
        self._closed = False

    def write(self, data: bytes) -> None:
        if self._closed:
            raise LinkClosed()
        view = memoryview(data)
        deadline = time.monotonic() + 5.0
        while view:
            try:
                n = self.sock.send(view)
                view = view[n:]
            except (BlockingIOError, InterruptedError):
                if time.monotonic() > deadline:
                    raise NebulosaError(None, "TCP send stalled")
                select.select([], [self.sock], [], 0.1)
            except OSError as e:
                self._closed = True
                raise LinkClosed("TCP send: %s" % e) from e

    def poll(self, timeout: float) -> List[Frame]:
        if self._closed:
            raise LinkClosed()
        p = self.parser
        frames = list(p.frames())
        if frames:
            return frames
        r, _, _ = select.select([self.sock], [], [], max(0.0, timeout))
        if not r:
            return []
        for _ in range(64):  # drain what is there, without blocking
            try:
                chunk = self.sock.recv(1 << 18)
            except (BlockingIOError, InterruptedError):
                break
            except OSError as e:
                self._closed = True
                raise LinkClosed("TCP: %s" % e) from e
            if not chunk:
                self._closed = True
                frames = list(p.frames())
                if frames:
                    return frames
                raise LinkClosed("the device closed the connection")
            p.push(chunk)
            if len(chunk) < (1 << 18):
                break
        return list(p.frames())

    def close(self) -> None:
        if not self._closed:
            self._closed = True
        try:
            self.sock.close()
        except OSError:  # pragma: no cover
            pass

    @property
    def resyncs(self) -> int:
        return self.parser.resyncs


# ---- USB -------------------------------------------------------------------

def _pyusb() -> Any:
    try:
        import usb.core
        import usb.util
    except ImportError:
        raise ImportError("USB needs pyusb (and libusb): pip install 'nebulosa[usb]'") from None
    return usb


def _is_timeout(e: Exception) -> bool:
    usb = _pyusb()
    if isinstance(e, getattr(usb.core, "USBTimeoutError", ())):
        return True
    return getattr(e, "errno", None) in (errno.ETIMEDOUT, 110, 60) or "timed out" in str(e).lower()


def _find_devices() -> List[Any]:
    usb = _pyusb()
    try:
        devs = usb.core.find(find_all=True, idVendor=USB_VID)
    except usb.core.NoBackendError as e:  # pragma: no cover
        raise NebulosaError(None, "pyusb found no libusb backend: install libusb-1.0") from e
    return [d for d in devs if d.idProduct in (USB_PID_HS, USB_PID_FS)]


def _string(dev: Any, index: int) -> Optional[str]:
    if not index:
        return None
    try:
        return _pyusb().util.get_string(dev, index)
    except Exception:  # no permission, or the device doesn't answer
        return None


def list_usb() -> List[UsbDevice]:
    """Nebulosa devices on USB. ``serial`` / ``product`` are ``None`` when
    the device can't be opened (permissions: see ``tools/99-nebulosa.rules``)."""
    return [
        UsbDevice(
            bus=int(d.bus or 0),
            address=int(d.address or 0),
            vid=d.idVendor,
            pid=d.idProduct,
            serial=_string(d, d.iSerialNumber),
            product=_string(d, d.iProduct),
            full_speed=d.idProduct == USB_PID_FS,
        )
        for d in _find_devices()
    ]


class UsbTransport(Transport):
    """NBP over the vendor interface (class FF / 4E / 01).

    Claims only that interface (the Full-Speed device's CDC log interface is
    left alone). Its bulk OUT endpoint takes commands. Every bulk IN
    endpoint (High-Speed: one for DATA, one for responses; Full-Speed: one
    for both) has its own daemon reader thread that keeps a read
    outstanding all the time (``read_size`` bytes, ``read_timeout_ms``),
    so an idle endpoint never holds up the other one and the device's block
    ring doesn't stall. The bytes go to a queue tagged with their endpoint;
    ``poll`` feeds them to that endpoint's own ``FrameParser`` (a message
    never spans endpoints). pyusb >= 1.2 returns the bytes of a read that
    timed out part-way, so nothing is lost on a timeout.
    """

    def __init__(self, serial: Optional[str] = None, index: int = 0, read_size: int = 64 * 1024,
                 read_timeout_ms: int = 100) -> None:
        usb = _pyusb()
        devs = _find_devices()
        if serial is not None:
            devs = [d for d in devs if _string(d, d.iSerialNumber) == serial]
        if not devs:
            raise NebulosaError(None, "no Nebulosa on USB%s (VID 1a86, PID fe0d / fe0c; permissions: "
                                "tools/99-nebulosa.rules)" % (" with serial %s" % serial if serial else ""))
        if index >= len(devs):
            raise NebulosaError(None, "USB index %d: only %d Nebulosa device(s)" % (index, len(devs)))
        dev = devs[index]
        self.dev = dev
        try:
            cfg = dev.get_active_configuration()
        except usb.core.USBError as e:
            if getattr(e, "errno", None) in (errno.EACCES, errno.EPERM):
                raise NebulosaError(None, "USB: permission denied (install tools/99-nebulosa.rules)") from e
            dev.set_configuration()  # unconfigured
            cfg = dev.get_active_configuration()
        intf = None
        for i in cfg:
            if (i.bInterfaceClass, i.bInterfaceSubClass, i.bInterfaceProtocol) == (
                    USB_CLASS, USB_SUBCLASS, USB_PROTOCOL):
                intf = i
                break
        if intf is None:
            raise NebulosaError(None, "no NBP interface (class FF/4E/01) on this device")
        self.ifnum = intf.bInterfaceNumber
        try:
            if dev.is_kernel_driver_active(self.ifnum):
                dev.detach_kernel_driver(self.ifnum)
        except (NotImplementedError, usb.core.USBError):
            pass
        try:
            usb.util.claim_interface(dev, self.ifnum)
        except usb.core.USBError as e:
            if getattr(e, "errno", None) in (errno.EACCES, errno.EPERM):
                raise NebulosaError(None, "USB: permission denied (install tools/99-nebulosa.rules)") from e
            raise NebulosaError(None, "USB claim: %s (another program using it?)" % e) from e
        bulk = [e for e in intf if usb.util.endpoint_type(e.bmAttributes) == usb.util.ENDPOINT_TYPE_BULK]
        outs = [e for e in bulk if usb.util.endpoint_direction(e.bEndpointAddress) == usb.util.ENDPOINT_OUT]
        ins = [e for e in bulk if usb.util.endpoint_direction(e.bEndpointAddress) == usb.util.ENDPOINT_IN]
        if not outs or not ins:
            usb.util.release_interface(dev, self.ifnum)
            raise NebulosaError(None, "the NBP interface lacks a bulk OUT or IN endpoint")
        self.ep_out = outs[0].bEndpointAddress
        self.eps_in = [e.bEndpointAddress for e in ins]
        self.parsers = [FrameParser() for _ in ins]
        self.name = "USB %s:%s %s" % (dev.bus, dev.address,
                                      "Full-Speed" if dev.idProduct == USB_PID_FS else "High-Speed")
        self._closed = False
        self._released = False
        self._error: Optional[BaseException] = None
        self._q: "queue.Queue[Tuple[int, bytes]]" = queue.Queue()
        self._stop = threading.Event()
        self._threads = []
        for k, ep in enumerate(self.eps_in):
            t = threading.Thread(target=self._reader, args=(k, ep, read_size, read_timeout_ms),
                                 name="nebulosa-usb-in-%02x" % ep, daemon=True)
            t.start()
            self._threads.append(t)

    def _reader(self, k: int, ep: int, size: int, timeout_ms: int) -> None:
        while not self._stop.is_set():
            try:
                data = self.dev.read(ep, size, timeout_ms)
            except Exception as e:
                if _is_timeout(e):
                    continue
                if not self._stop.is_set():
                    self._error = LinkClosed("USB IN 0x%02X: %s" % (ep, e))
                    self._q.put((-1, b""))
                return
            if len(data):
                self._q.put((k, data.tobytes() if hasattr(data, "tobytes") else bytes(data)))

    def write(self, data: bytes) -> None:
        if self._closed:
            raise LinkClosed()
        try:
            self.dev.write(self.ep_out, data, 2000)
        except Exception as e:
            if _is_timeout(e):
                raise NebulosaError(None, "USB write timed out") from e
            self._closed = True
            raise LinkClosed("USB write: %s" % e) from e

    def _frames(self) -> List[Frame]:
        out: List[Frame] = []
        for p in self.parsers:
            out.extend(p.frames())
        return out

    def poll(self, timeout: float) -> List[Frame]:
        if self._closed:
            raise LinkClosed()
        frames = self._frames()
        if frames:
            return frames
        try:
            item = self._q.get(timeout=max(0.0, timeout)) if timeout > 0 else self._q.get_nowait()
        except queue.Empty:
            return []
        while True:
            k, data = item
            if k < 0:
                frames = self._frames()
                if frames:
                    self._q.put(item)  # report the error on the next poll
                    return frames
                self._closed = True
                raise self._error or LinkClosed()
            self.parsers[k].push(data)
            try:
                item = self._q.get_nowait()
            except queue.Empty:
                break
        return self._frames()

    def close(self) -> None:
        if self._released:
            return
        self._closed = True
        self._released = True
        self._stop.set()
        for t in self._threads:
            t.join(2.0)
        usb = _pyusb()
        try:
            usb.util.release_interface(self.dev, self.ifnum)
        except Exception:
            pass
        try:
            usb.util.dispose_resources(self.dev)
        except Exception:
            pass

    @property
    def resyncs(self) -> int:
        return sum(p.resyncs for p in self.parsers)
