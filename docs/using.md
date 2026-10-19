# Using inkdot

DSlot starts inkdot for games in `Roms/GB` and `Roms/GBC` on either card (`.gb`, `.gbc`, or a
`.zip` with one in it). Leaving the game (Back, or Quit on the bottom screen) saves where you are;
the next start resumes there.

## Controls

| Control | |
|---|---|
| A, B, Select, Start, D-pad or stick | the Game Boy's buttons |
| X, Y | turbo A, turbo B |
| R2 (held) | fast-forward (silent) |
| Menu | the menu on the bottom screen; the game pauses; D-pad and A to choose, B to go back |
| Menu + R / Menu + L | quick save / quick load |
| Menu + Select | fast-forward on or off |
| Back | save and leave |
| Menu + volume | both screens' brightness (nibOS's own shortcut) |

## The bottom screen

While the game plays, the bottom screen has six tiles: **Save** (into the quick slot), **Load**
(pick one of six slots, each with its picture, date and play time), **Look**, **Fast**,
**Options** and **Quit**. Untouched for ten seconds it dims, or turns off (see Options); a touch
wakes it without doing anything else.

## Options

Each option can be set for one game or for all of its console's games (the two buttons at the top
of the Look and Options pages); DSlot's menu has the same options per console and per game.

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

In the game's saves folder (`/mnt/roms/Saves/GB/<game>/` or `GBC`): `<game>.sav` (battery RAM, the
usual format), `<game>.rtc` (the cartridge clock), `<game>.state1`–`.state6` (save states with
their pictures) and `<game>.auto.state` (where the game was left). DSlot's **Clear resume state**
deletes the last.
