# Raspberry Pi Pico 2 port

This board package builds CMSIS-DAP firmware for the Raspberry Pi Pico 2
(RP2350). SWD and JTAG use PIO by default; add `bitbang` to use CPU-controlled
GPIO instead. SWJ combines SWD and JTAG using bit-banging.

Run the commands from this directory:

```sh
# SWD with PIO
cargo build --release --features swd

# JTAG with PIO
cargo build --release --no-default-features --features jtag

# SWD with bit-banging
cargo build --release --no-default-features --features swd,bitbang

# Combined SWD/JTAG
cargo build --release --no-default-features --features swj
```
