---
name: eustress-playtest
description: >-
  Plays an Eustress game the way a player would and judges it: starting and stopping
  Play, walking, jumping, aiming and clicking with play_input, seeing the result with
  capture_viewport, reading script prints and errors with read_output, and checking
  scale, feel and frame rate. Use after any change to a game, when the user says "play
  it", "test it", "does it work", "why doesn't X happen", or when a script seems to do
  nothing. Covers both the MCP path and the command-line bridge.
---

# Playtest an Eustress game

A change is done when you have played it and looked at it. Logs say what ran; a picture
says whether it is right.

## Start and stop Play

| Who | Start Play | Stop Play |
|---|---|---|
| The user in Studio | F5, or the Play button | F8, or Stop |
| An agent with a shell | `eustress bridge --pid <pid> call action.invoke --params '{"action":"PlayWithCharacter"}'` | the same with `"StopPlay"` |
| Studio launched for a test | `eustress-engine --space "<Space path>" --play` starts Play once the Space loads | close Studio, or StopPlay as above |

- `eustress instances` lists the running engines with their pid and bridge port. Address
  one engine by `--pid` when more than one runs.
- Over MCP alone, ask the user to press F5 and tell you when the game is running. The MCP
  `invoke_action` tool can reach every editor action (Delete and Publish included), so MCP
  clients are refused it; `play_input`, `capture_viewport` and `read_output` all work
  over MCP.
- **Stop restores the Space.** Everything scripts created, moved or destroyed returns to
  how it was, so each Play starts clean.
- **Script edits apply at the next Play.** Edit, stop, play again.

## Drive the game: `play_input`

`play_input` presses keys and buttons in the running game without touching the real mouse:

```
play_input {down: ["W"]}                      hold W (walk forward)
play_input {up: ["W"]}                        release it
play_input {down: ["W", "LeftShift"]}         sprint
play_input {tap: ["Space"]}                   jump once
play_input {cursor: [450, 120]}               put the virtual cursor at a viewport pixel
play_input {tap: ["MouseButton1"]}            click there
play_input {down: ["MouseButton1"]}           hold fire
play_input {wheel: 2}                         scroll away from the user
play_input {clear: true}                      release everything, hand the cursor back
```

- Names are Roblox's: `W`, `A`, `S`, `D`, `Space`, `LeftShift`, `One` to `Nine`, `R`, `Q`,
  `MouseButton1`, `MouseButton2`.
- The cursor is in 3D-viewport logical pixels, origin top-left. `mouse.Hit` and aiming in
  scripts follow it.
- Time passes between your calls, so "hold W for about two seconds" is `down`, then a short
  wait, then `up`. Always finish with `clear: true`.

## Look: `capture_viewport`

`capture_viewport` returns the frame exactly as the user sees it, HUD included. Capture:

- right after Play starts (did everything load, is the HUD there),
- mid-action (walking, shooting, jumping),
- after the moment the change was about (the purchase, the wave start, the death screen).

Judge each capture against these, and say what you saw:

- **Scale**: the character next to something of known size (a door, an enemy, a lamp).
  A body several times too big or floating above the ground shows plainly in a picture.
- **Readability**: every HUD number and label readable, nothing overlapping, the middle
  of the screen clear for the action.
- **Feedback**: after an input, something visibly answered (a flash, a number, a sound
  cue on screen).
- **Framing**: the camera shows what the player needs (enemies coming, the next platform).

## Read what the scripts said: `read_output`

`read_output` returns the newest lines of Studio's Output panel: every `print` and `warn`
from Luau and Rune, and runtime errors with the script's name and line.

```
read_output {level: "error"}                  errors only
read_output {contains: "wave", limit: 20}     lines mentioning "wave"
read_output {}                                the last 50 lines
```

- Put short, searchable prints at the moments that matter while building:
  `print("[wave] start", wave, #zombies)`, then `read_output {contains: "[wave]"}`.
- When a script seems to do nothing, check in this order:
  1. `read_output {level: "error"}`: a runtime error stops that script.
  2. Is it a script folder with `[script] source` and a matching `run_context`, in a place
     that runs (`eustress-space-layout`)? `list_scripts` shows what exists.
  3. Did you Play again after editing?
  4. Is it waiting forever on `WaitForChild` for something that never appears? Print
     before and after the wait.

## A playtest pass

1. Start Play. Capture.
2. `read_output {level: "warn"}`: fix any error before judging the game.
3. Do the verb (walk, jump, shoot, buy) with `play_input`. Capture mid-action.
4. Push on the change you just made: trigger it, then capture the result.
5. Try one thing a player would try that you did not plan for (walk off the edge, spam the
   button, stand in the doorway).
6. `play_input {clear: true}`, stop Play.
7. Tell the user in two or three lines: what works, what looked wrong, what you will change
   next. Show the most telling capture.

## Frame rate

Watch for stutter in captures taken during busy moments (many enemies, explosions). The
usual causes in game scripts:

- A HUD written every frame (`eustress-game-ui`: write only on change).
- Creating and destroying parts per shot or per particle: pool them.
- Scanning `workspace:GetDescendants()` every frame: keep your own tables.

Studio's microprofiler is the authority on frame time; a feeling of lag is a reason to
measure, not a measurement.
