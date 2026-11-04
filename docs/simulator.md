# Simulator

`tools/nebulosa_sim.py` is a fake Nebulosa: Python 3 standard library only.
It answers every command and streams real format-5 data, so you can write
and test scripts without hardware. Every library's test suite uses it.

```sh
python3 tools/nebulosa_sim.py                  # TCP 20034, UDP discovery 20035
python3 tools/nebulosa_sim.py --port 21000 --no-udp -v
```

Then connect to `127.0.0.1` (or let discovery find it).

What it does:

- **stream**: a log-swept current from ~2 µA to ~1 A (4 s period, with a
  1 kHz ripple) so auto-ranging walks through every range, and a voltage
  around 3.3 V; decimation honoured; paced in real time;
- **one client at a time**, like the device: a second connection is closed at once;
- registers, a 1 MB flash (NOR semantics), board status (all self-tests
  pass, a 12 V PD contract), update info, update CRC checks.

Limits: Python caps it at a few hundred thousand samples/s (use log2n ≥ 2),
and it does not model `PROTECTED` flash ranges or link-test blocks.

## In your own tests

Start it on a free port, wait until it accepts connections, run your code,
stop it. `c/tests/run_with_sim.py` does exactly that and passes the port in
`NEBULOSA_SIM_PORT`:

```sh
python3 c/tests/run_with_sim.py tools/nebulosa_sim.py ./my_test_program
```
