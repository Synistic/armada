# Roadmap

Space battles only. Each milestone ends in something you can play, not just code.

## Done

- Deterministic fixed-point sim, 2000 ships at about 1 ms per tick
- Replays with checkpoint hashes, headless runner
- Hardpoints with damage, shield generator and engine effects
- Headings, turn rates, firing arcs, preferred bearing
- Orders: move, attack-move, attack, hold, engage. Line formations, control groups
- Beams, homing missiles and torpedoes with shield bypass
- Bevy viewer with selection, HUD, explosions, photo mode

## 1. Skirmish loop

Turns the tech demo into a game: you can win or lose against an AI.

- Space station per side: builds ships, is the win condition
- Credits from income and capture points, population cap
- Build queue in the HUD
- Enemy AI that only uses commands, like a player would
- Victory and defeat screen, start menu with scenario choice

## 2. Combat depth

What made the original's space battles fun.

- Target single hardpoints (click a weapon or engine on an enemy ship)
- Point defense that shoots down missiles and torpedoes
- Fighter and bomber squadrons, launched from carriers and the station
- Ship abilities with cooldowns: power to shields, power to engines, focus fire
- Map objects: asteroid fields (block fire), nebulae (no shields), debris
- Clear counters per class, balanced with headless batch runs

## 3. Look and feel

- Real ship models with visible hardpoints and damage states
- Better weapon effects, shield hits, wrecks
- Sound and music
- Minimap, unit cards, health bars over ships

## 4. Multiplayer

- Lockstep for 2 players over the network, commands only
- Desync detection with the existing state hashes
- Lobby, reconnect, replay of every match

## 5. Modding

- Layered content folders: mods override and extend base RON files
- Hot reload of ship stats in the viewer
- Docs for modders: ships, weapons, scenarios

## Not planned

- Ground battles
- Galactic map, maybe later and only if the battles are good

## Things the original got wrong that we avoid

- Tiny population cap
- Ships clumping and drifting into each other
- No formations, no facing control
- AI that ignores what it can actually see
