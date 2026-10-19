# Performance on the RG DS+

Measured 2026-10-19 on an RG DS+ running nibOS (kernel 7.3.0-rc7-nibos #37, with uclamp), inkdot
built in the aarch64 build machine with `-C target-cpu=cortex-a55`, release profile.

## The core, headless

`inkdot-test --bench ROM` runs 3600 frames as fast as it can (one core, schedutil at full clock,
1.99 GHz). Milliseconds per frame, median / 99th percentile:

| Game | First build | Now |
|---|---|---|
| Blargg `cpu_instrs` (DMG) | 5.50 / 5.69 | 2.95 / 3.29 |
| Prehistorik Man (DMG, mid-frame scroll effects) | 5.46 / 6.45 | 3.03 / 4.71 |
| cgb-acid2 (CGB) | 6.05 / 6.38 | 3.08 / 3.41 |
| Pokémon Gold (CGB, MBC3 with clock) | 6.28 / 6.77 | 3.32 / 3.83 |
| Link's Awakening DX (CGB, double speed) | 8.25 / 9.09 | 5.10 / 5.61 |

What made the difference (commit 69d5249 and after): lines whose pixel transfer nothing changes
are drawn in one pass, with the FIFO taking over at a mid-line write; idle PPU dots are counted
off in runs; the sound channels report only real changes.

## In play

`INKDOT_DEBUG=stats` on Prehistorik Man, five-second windows (300 vblanks), uclamp 50%:

| | Median | Worst |
|---|---|---|
| Running the frame (game, sound) | 5.4-6.8 ms | 8.5-9.6 ms |
| Drawing the top screen | 0.6-0.7 ms | 1.0-2.2 ms |
| SDL's swap | 0.4-0.6 ms | 5.5-8.4 ms |

3-5 of every 300 vblanks repeat a frame. The panel's period measures 16.6836-16.6849 ms (59.94 Hz);
the sound queue stays between 400 and 1200 frames at 47.9-48.0 kHz.

The frame takes longer in play than headless because the main loop sleeps between frames, so the
cores are slowed: with the clamp applied (`uclamp.min` 819 at 80%) the clock still moves between
816 MHz and 1.99 GHz within each frame, as schedutil ramps up after each wake; 50%, 65% and 80%
measure the same, so inkdot asks for 50%.

## Latency

Button presses injected with nibOS's `lat-press` (timed where they reach the gamepad) and
inkdot's `INKDOT_DEBUG=latency` (when the frame that read the change started, and the vblank that
showed it), both on CLOCK_MONOTONIC. Eight presses on Prehistorik Man:

| | |
|---|---|
| Press on the gamepad → read by inkdot | 4-16 ms (when in the frame the press lands) |
| Read → the vblank that shows it | 11-15 ms |
| Press → first scanout of the frame that used it | 17-30 ms, mean about 23 ms |

The game's own frames of reaction come on top. The read is placed by the frame budget (the 99th
percentile of a frame's cost plus 1.5 ms, about 13 ms now), so a faster frame, or a steadier swap,
lowers it directly.
