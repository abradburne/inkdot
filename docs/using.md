# Using inkdot

inkdot plays games in `.gb`, `.gbc`, or a `.zip` with one in it. It can be started three ways:

- **From DSlot** (nibOS), for games in `Roms/GB` and `Roms/GBC` on either card.
- **From Apps on the stock OS**: copy the stock zip's `Roms` folder onto the games card. It
  opens with a list of the games in `Roms/GB` and `Roms/GBC` on both cards.
- **By hand**: `inkdot game.gbc`, or `inkdot` alone for the list. `inkdot --help` gives the
  options, which are the same as on the Options page (`--look pocket`, `--no-lcd`, `--fast 6`).

Leaving the game (Menu, or Quit on the bottom screen) saves where you are; the next start
resumes there.

## Controls

| Control | |
|---|---|
| A, B, Select, Start, D-pad or stick | the Game Boy's buttons |
| X, Y | turbo A, turbo B |
| R2 (held) | fast-forward (silent) |
| Back | the menu on the bottom screen; the game pauses; D-pad and A to choose, B to go back, Back again to play |
| Menu | save and leave (back to DSlot's shelf) |
| Menu + R / Menu + L | quick save / quick load |
| Menu + Select | fast-forward on or off |
| Menu + volume | both screens' brightness (nibOS's own shortcut) |

## The bottom screen

While the game plays, the bottom screen has six tiles: **Save** and **Load** (each opens the six
slots, with each state's picture, date and play time, and empty ones marked; saving over a state
asks for a second tap), **Look**, **Fast**, **Options** and **Quit**. Menu + R saves straight into
the quick slot, the last one saved or loaded, and Menu + L loads it. Untouched for ten seconds it dims, or turns off (see Options); a touch
wakes it without doing anything else.

## Options

Each option can be set for one game or for all of its console's games (the two buttons at the top
of the Look and Options pages); DSlot's menu has the same options per console and per game. An
option given on the command line wins for that run. **Reset** clears what is set at that level,
so the console's setting, or else the default, applies again.

The choices are kept in `settings.toml`. In the nibOS package (built with DSlot's part) that is
DSlot's own file, however inkdot is started, so DSlot's menu and inkdot's agree; otherwise it is
inkdot's (`~/.config/inkdot/`, or beside the program on the stock OS).

| Option | | Default |
|---|---|---|
| Look (GB games) | Game Boy Color (the CGB's own colouring), Game Boy, Game Boy Pocket, Game Boy Light, plain grey. The Color look runs the game on a CGB, the others on a DMG; changing between them takes effect the next time the game starts. | Game Boy Color |
| Colours (Color look) | By title (as a Game Boy Color picks them), or one of the twelve palettes a Game Boy Color offers for a button held at start-up | By title |
| Colours (GBC games) | as on its screen (darker and softer, like the original), or as stored | As on its screen |
| LCD texture | a faint grid between pixels and the original screens' slow pixels | On |
| Timing | Smooth: one game frame per refresh of the panel, 0.35% fast, never a repeated frame; Exact: the game's own speed, one frame shown twice every 4.8 s | Smooth |
| Bottom screen in play | dim when idle, off when idle, always lit | Dim |
| Resume where I left off | start each game where it was left | On |
| Fast-forward speed | 2× to 8× | 4× |

## Files

In the game's saves folder: the one DSlot gives, else `Saves/GB/<game>/` (or `GBC`) on the card
the system starts from (`/mnt/roms/Saves` on nibOS, `/mnt/mmc/Saves` on the stock OS), else, on a
computer, `~/.local/share/inkdot/saves/`. `--saves DIR` names another.

`<game>.sav` (battery RAM, the usual format; a `<game>.gbc.sav` or `.gb.sav` from elsewhere is
read too), `<game>.rtc` (the cartridge clock), `<game>.state1`–`.state6` (save states with their
pictures) and `<game>.auto.state` (where the game was left). A Game Boy game runs on a Game Boy
Color or a Game Boy by its look, and a resume state made on one can't continue on the other: when
a new look changes the console the game starts again on it, and the old console's resume state is
kept as `<game>.auto-cgb.state` (or `-dmg`), to resume if the look is changed back. DSlot's **Clear resume state**
deletes the last.

## On the stock OS

As on nibOS, except: the brightness is the stock OS's own (its keys, not inkdot's sliders), the
bottom screen stays lit (the stock OS keeps its backlight to itself), the frame waits on the display's own sync rather than
the panel's vblank, and the sound is at 44.1 kHz with bigger buffers, so it lags a little more.
