"""Shared fixtures: the package from src/, and a simulator subprocess."""

import os
import socket
import subprocess
import sys
import time

import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "src"))

SIM = os.environ.get("NEBULOSA_SIM") or os.path.join(HERE, "..", "..", "tools", "nebulosa_sim.py")


def free_port(kind=socket.SOCK_STREAM):
    s = socket.socket(socket.AF_INET, kind)
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


@pytest.fixture(scope="session")
def sim():
    """Start tools/nebulosa_sim.py on free ports; yields (host, tcp_port, udp_port)."""
    if not os.path.exists(SIM):
        pytest.skip("simulator not found: %s" % SIM)
    port = free_port()
    udp = free_port(socket.SOCK_DGRAM)
    p = subprocess.Popen([sys.executable, SIM, "--host", "127.0.0.1", "--port", str(port), "--udp-port", str(udp)],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
            break
        except OSError:
            if p.poll() is not None:
                raise RuntimeError("simulator exited: %s" % p.stdout.read().decode())
            time.sleep(0.05)
    # the probe connection above was served and closed; give the sim a moment
    time.sleep(0.1)
    yield "127.0.0.1", port, udp
    p.terminate()
    try:
        p.wait(5)
    except subprocess.TimeoutExpired:  # pragma: no cover
        p.kill()


@pytest.fixture
def dev(sim):
    import nebulosa

    host, port, _ = sim
    d = nebulosa.Nebulosa.tcp(host, port)
    yield d
    d.close()
    time.sleep(0.05)  # the sim serves one client at a time
