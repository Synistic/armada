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
- Skirmish: stations, credits, population cap, build queue, capture points, AI opponent,
  victory and defeat, start menu, three AI levels
- Ship models for every class with turrets on the hardpoints, destroyer and carrier classes

## 1. Skirmish loop

Turns the tech demo into a game: you can win or lose against an AI.

- [x] Space station per side: builds ships, is the win condition
- [x] Credits from income and capture points, population cap
- [x] Build queue in the HUD
- [x] Enemy AI that only uses commands, like a player would
- [x] Victory and defeat screen
- [x] Start menu with scenario choice and AI difficulty
- [ ] Balance pass: match length, station strength, costs

## 2. Combat depth

What made the original's space battles fun.

- Target single hardpoints (click a weapon or engine on an enemy ship)
- Point defense that shoots down missiles and torpedoes
- Fighter and bomber squadrons, launched from carriers and the station
- Ship abilities with cooldowns: power to shields, power to engines, focus fire
- Map objects: asteroid fields (block fire), nebulae (no shields), debris
- Clear counters per class, balanced with headless batch runs

## 3. Look and feel

- Damage states on the models: destroyed turrets, hull scars
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
