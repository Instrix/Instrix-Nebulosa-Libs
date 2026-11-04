#!/usr/bin/env python3
"""Run a test binary against tools/nebulosa_sim.py on a free port:
run_with_sim.py <sim.py> <program> [args...]  (the port goes in NEBULOSA_SIM_PORT)."""
import os, socket, subprocess, sys, time

sim, prog = sys.argv[1], sys.argv[2:]
s = socket.socket()
s.bind(("127.0.0.1", 0))
port = s.getsockname()[1]
s.close()
p = subprocess.Popen([sys.executable, sim, "--host", "127.0.0.1", "--port", str(port), "--no-udp"],
                     stdout=subprocess.DEVNULL)
try:
    for _ in range(100):
        try:
            socket.create_connection(("127.0.0.1", port), 0.1).close()
            break
        except OSError:
            time.sleep(0.05)
    time.sleep(0.2)  # the sim serves one client at a time: let it see the probe go
    env = dict(os.environ, NEBULOSA_SIM_PORT=str(port))
    sys.exit(subprocess.call(prog, env=env))
finally:
    p.terminate()
    p.wait()
