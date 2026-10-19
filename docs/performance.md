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
measure the same; inkdot now asks for 75% (768), and 0 while the menu is open.

## Repeated frames, and the deadline

Measured with `INKDOT_DEBUG=frames` (a line per frame: the vblank, the start, the game done,
the swap) on Boulder Dash, scrolling from a save state, a minute at a time.

Frames that showed a refresh late weren't slow ones: the game and drawing were done ~6 ms after
the vblank as usual, but the swap then waited until just after the next vblank, because the
frame before had been handed over too late. A swap ending up to ~7 ms after the vblank's
timestamp always made the next refresh; from 9.5 ms on it nearly always missed it. So the
deadline is about 7 ms after the vblank, not the next vblank (likely because the panel takes a
new picture as its scan starts). The pacer aimed at the next vblank, and a frame that blocked in
its swap raised the frame budget to its cap, so frames started ~2 ms after the vblank and their
swaps fell past the deadline: 10-25 repeated refreshes in every 300 (the stutter seen in play).

Now the pacer aims each swap at 6.5 ms after the vblank, learns the frame's cost without the
time blocked in the swap, and starts a frame that doesn't fit after the vblank before it, from
the vblanks' times recorded by a thread of their own. The bottom screen's swap doesn't wait for
its refresh, and its clock isn't redrawn while it is dimmed in play (a redraw had stalled the
loop for ~48 ms once a minute).

Repeated refreshes per 300, after the first five seconds, for a minute (uclamp 768):

| Game | Before | Now |
|---|---|---|
| Boulder Dash (scrolling) | 10-25 | 0 |
| Link's Awakening DX (walking) | not measured | 0 |
| Pokémon Gold (walking) | not measured | 0 |

The first five seconds still repeat 1-3 refreshes (the first draw of the bottom screen). Battery
draw is unchanged: 2.75-2.83 W before and 2.75-2.77 W now (rk817 `current_avg` × `voltage_avg`,
Boulder Dash, two runs each); the maximum clamp (1024) didn't remove the misses before the
change, so the clamp stays at 768.

## Latency

Button presses injected with nibOS's `lat-press` (timed where they reach the gamepad) and
inkdot's `INKDOT_DEBUG=latency` (when the frame that read the change started, and the vblank that
showed it), both on CLOCK_MONOTONIC. Eight presses on Prehistorik Man:

| | |
|---|---|
| Press on the gamepad → read by inkdot | 4-16 ms (when in the frame the press lands) |
| Read → the vblank that shows it | 11-15 ms |
| Press → first scanout of the frame that used it | 17-30 ms, mean about 23 ms |

The game's own frames of reaction come on top. These figures are from before the deadline-based
pacing above; the read is now placed by the deadline (the 99th percentile of a frame's cost plus
1.5 ms before it), so a faster frame lowers it directly.
