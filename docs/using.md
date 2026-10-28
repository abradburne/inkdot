# Using inkdot

inkdot plays games in `.gb`, `.gbc`, or a `.zip` with one in it. It can be started three ways:

- **From DSlot** (nibOS), for games in `Roms/GB` and `Roms/GBC` on either card, or in `Games/GB`
  and `Games/GBC` on a card set up for slot CFW.
- **From Apps on the stock OS**: copy the stock zip's `Roms` folder onto the games card. It
  opens with a list of the games in `Roms/GB` and `Roms/GBC` on both cards, and in `Games/GB` and
  `Games/GBC`. A game in both `Games/` and `Roms/` is listed once.
- **By hand**: `inkdot game.gbc`, or `inkdot` alone for the list. `inkdot --help` gives the
  options, which are the same as on the menu's Game page (`--look pocket`, `--no-lcd`, `--fast 6`).

Leaving the game (Menu, or Quit on the bottom screen) saves where you are; the next start
resumes there.

## Controls

| Control | |
|---|---|
| A, B, Select, Start, D-pad or stick | the Game Boy's buttons |
| X, Y | turbo A, turbo B |
| R2 (held) | fast-forward (silent) |
| Back | the menu on the bottom screen; the game pauses; D-pad, A and L / R to choose, B to go back to the game |
| Menu | save and leave (back to DSlot's shelf) |
| Menu + R / Menu + L | quick save / quick load |
| Menu + Select | fast-forward on or off |
| Menu + volume | both screens' brightness (nibOS's own shortcut) |

On a computer the window shows both screens, the bottom one under the top; the mouse is a finger
on it. The keys: the arrows for the D-pad, **X** and **Z** for A and B, **S** and **A** for X and Y,
**Q** and **W** for L and R, **Return** for Start, **Right Shift** or **Backspace** for Select,
**Tab** held to fast-forward, **Escape** for the menu (Back on the DS+) and **Home** to save and
quit (Menu on the DS+), as closing the window does.

## The bottom screen

While the game plays the bottom screen is blank. **Back**, or a tap on it, pauses the game and
opens the menu there: DSlot's own menu, with its tabs, rows and button hints, so it works the
same way.

| Page | |
|---|---|
| **Play** | Resume, Save state, Load state, Fast-forward, Save and quit; and, when the look you chose runs on the other console, starting the game again on it (A, then A again: where you are is saved first) |
| **States** | The six slots as cards, and to the left of slot 1 the resume point (where you last left the game, written on leaving): the highlighted one's picture, date and play time on the top screen. A loads; X saves into a slot (X again to save over a state) |
| **Game**, **GB** / **GBC** | The options for this game, and for every game of its console |
| **System** | Both screens' brightness, the volume, and the clock (12- or 24-hour, for every app) |

L and R (or a tap on a tab) change page; up and down choose a row, left and right change it, A
acts on it; B goes back to the game. While the menu is open the top screen shows the game
dimmed, or as it is while you change how it looks.

## Options

Each option can be set for one game (the **Game** page) or for all of its console's games (the
**GB** or **GBC** page). A row's first choice is **Use …**, where its value comes from when it
isn't set there: "Use GB (Light)" when all GB games have it, "Use default (On)" when nothing sets
it. While a row uses that, it shows it muted; set there, it shows the value. Stepping back round
to "Use …", or Y on a row set there, clears it at that level. DSlot's menu shows the same pages. An option
given on the command line wins for that run, until it's changed in the menu.

The choices are kept in `settings.toml`. In the nibOS package (built with DSlot's part) that is
DSlot's own file, however inkdot is started, so DSlot's menu and inkdot's agree; otherwise it is
inkdot's (`~/.config/inkdot/`, or beside the program on the stock OS).

| Option | | Default |
|---|---|---|
| Look (GB games) | Game Boy Color (the CGB's own colouring), Game Boy, Game Boy Pocket, Game Boy Light, plain grey. The Color look runs the game on a CGB, the others on a DMG. A new look shows at once, even paused; a DMG look on a game running as a CGB uses that look's colours straight away, and Play offers to restart on a real DMG; the Color look on a game running as a DMG needs a restart, which Play offers (where you are is saved first, and each console keeps its own resume point). | Game Boy Color |
| Colours (Color look) | By title (as a Game Boy Color picks them), or one of the twelve palettes a Game Boy Color offers for a button held at start-up | By title |
| Colours (GBC games) | as on its screen (darker and softer, like the original), or as stored | As on its screen |
| LCD texture | a faint grid between pixels and the original screens' slow pixels | On |
| Timing | Smooth: one game frame per refresh of the panel, 0.35% fast, never a repeated frame; Exact: the game's own speed, one frame shown twice every 4.8 s | Smooth |
| Bottom screen in play | dim when idle, off when idle, always lit | Dim |
| Resume where I left off | start each game where it was left | On |
| Fast-forward speed | 2× to 8× | 4× |

## RetroAchievements

Sign in once, on DSlot's **Achievements** tab. From then on every game DSlot starts gets the
sign-in (the token DSlot kept, never the password) in its environment, and inkdot signs in with
it, looks the game up by its ROM's hash and, when the service has a set for it, checks the
achievements as you play.

On the top screen, at the bottom right:

- as play begins, the game's placard: its title and `Casual: 12 of 40 unlocked` (and how many
  of them can't be earned in inkdot, when some read memory it doesn't offer);
- each achievement you earn, for four seconds, with its badge and points (several in a row show
  one after another);
- `Completed` when you have them all, or a problem (the sign-in refused: sign in again on
  DSlot's Achievements tab; the service doesn't know this dump, a different revision or a
  patched one; offline, the unlocks wait and are sent when the connection returns).

At the bottom left, while they last, the same panels as DSperate's: how far you are with an
achievement that counts something (*Progress*, its title, `3/10`), and under it a *Challenge*
panel for each achievement you can earn right now (finish without being hit, say), its badge in
a gold ring that pulses slowly; when the challenge is the achievement in progress, the two are
one panel. Badges are
kept with DSlot's in `~/.cache/retroachievements/badges` and fetched when one isn't there.

The log (`nib bug-report`, or `journalctl -u dslot`) says the same, one `cheevos:` line each:
`Signed in -- <you>`, `<game> -- Casual: 12 of 40 unlocked`, `Achievement unlocked: <title>
(<points>) -- <what for>`, `No achievements for this ROM -- ... (hash <md5>)`.

inkdot plays **Casual** (softcore) only: save states, fast-forward and the resume point all stay
usable. A save state keeps how far each achievement had got (hit counts and the like), and
loading it brings that back, so a state loaded mid-way through something doesn't lose it; a
state saved before this, or restarting the game, starts the achievements' progress over. DSlot's
**Mode: Hardcore** is noted in the log and played as Casual. Started by hand, without DSlot,
there is no sign-in and no session.

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

A game in a slot CFW `Games/GB` or `Games/GBC` folder saves in `Saves/` like any other. The first
time it starts with no `<game>.sav` there, the `.sav` slot left beside the ROM (`<game>.sav` or
`<game>.gbc.sav`) is copied in, so the game goes on from where it was in slot. The original is never
changed or deleted, so slot still has it; from then on the two saves are apart.

## On the stock OS

As on nibOS, except: the brightness is the stock OS's own (its keys, not inkdot's sliders), the
bottom screen stays lit (the stock OS keeps its backlight to itself), the frame waits on the display's own sync rather than
the panel's vblank, and the sound is at 44.1 kHz with bigger buffers, so it lags a little more.
