# Local patches to rpi-led-panel 0.9.1

Upstream: https://github.com/EmbersArc/rpi_led_panel (GPL-3.0, see LICENSE). This copy replaces the crates.io
release through `[patch.crates-io]` in the top-level Cargo.toml. The examples and assets were left out.

1. `src/registers.rs`, system timer: upstream read the low and high words of the timer with one 8-byte
   volatile load starting at offset 4. On aarch64 that is a single `ldr x0, [x0]` from an address that is only
   4-byte aligned, and on 64-bit Raspberry Pi OS the access to device memory raises SIGBUS (BUS_ADRALN): the
   program died with "Bus error" as soon as the first frame was pushed. The two words are now read separately
   (high, low, high again).

Drop this directory, and the `[patch]` entry, once a release of the crate contains an equivalent fix.
