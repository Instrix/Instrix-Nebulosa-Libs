"""Exceptions."""

from __future__ import annotations

import builtins
from typing import Any, Optional

from .constants import status_name


class NebulosaError(Exception):
    """A device error.

    ``status`` is the NBP status code (``Status``) the device answered, or
    ``None`` for errors raised by the library itself (timeouts, a closed link,
    a malformed response). ``name`` is the status name (``"BAD_ARG"``, ...).
    ``cmd`` is the command that failed, when known; ``payload`` the response
    payload; ``info`` a parsed record attached to it (``update()`` attaches
    the ``UpdateInfo`` the device sends with a failed operation).
    """

    def __init__(
        self,
        status: Optional[int],
        message: Optional[str] = None,
        *,
        cmd: Optional[int] = None,
        payload: bytes = b"",
        info: Any = None,
    ) -> None:
        self.status = status
        self.cmd = cmd
        self.payload = payload
        self.info = info
        if message is None:
            message = "device answered %s (%s)" % (self.name, status)
            if cmd is not None:
                message = "command 0x%02X: %s" % (cmd, message)
        super().__init__(message)

    @property
    def name(self) -> str:
        """Status name, ``"TIMEOUT"`` / ``"CLOSED"`` / ``"ERROR"`` for library errors."""
        if self.status is None:
            return "ERROR"
        return status_name(self.status)


class TimeoutError(NebulosaError, builtins.TimeoutError):  # noqa: A001 - deliberate
    """No response in time. Also an instance of the built-in ``TimeoutError``."""

    def __init__(self, message: str = "timed out", *, cmd: Optional[int] = None) -> None:
        NebulosaError.__init__(self, None, message, cmd=cmd)

    @property
    def name(self) -> str:
        return "TIMEOUT"


class LinkClosed(NebulosaError, builtins.ConnectionError):
    """The connection went away (the device reset, the cable, ...)."""

    def __init__(self, message: str = "link closed") -> None:
        NebulosaError.__init__(self, None, message)

    @property
    def name(self) -> str:
        return "CLOSED"


class VerifyError(NebulosaError):
    """Flash read back differs from what was written."""

    def __init__(self, message: str, addr: int) -> None:
        NebulosaError.__init__(self, None, message)
        self.addr = addr


class ImageError(ValueError):
    """A gateware / firmware file that isn't usable (no sync word, bad .nbfw header, ...)."""
