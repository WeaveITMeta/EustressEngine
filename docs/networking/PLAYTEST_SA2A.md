# Play test: characters, GUIs, animation and seats across machines

What to check in a two-window session (Studio hosting, one Player joined on
the same machine) once the build carries server authority phase 2a
([SERVER_AUTHORITY.md](SERVER_AUTHORITY.md), [SEATS.md](SEATS.md)). Each
check says what to do, what should happen, and the log line or place that
proves it.

## Setup

1. Studio: open a Space with a character (Box Head works; Vehicle Simulator
   for the seat checks), press F9. Log: `Preparing to host ...`, then
   `multiplayer: replicating the Play session`.
2. Player: join with the link Studio shows. Studio's log:
   `net: <name> (<addr>) joined as peer 1, running eustress-client 0.1.0`.
3. Studio's log should not stall on F9 when nothing is unsaved: no multi-second
   `start_host_export` in the phase watchdog.

## Players and characters

| Do | Expect | Proof |
|---|---|---|
| Join | Both machines list both players | A LocalScript printing `#Players:GetPlayers()` prints 2 on the Player |
| Join | The host's scripts see the joined character | A server Script's `Players.PlayerAdded` → `player.CharacterAdded` prints the joined player's name |
| Walk the joined avatar onto a kill brick or a coin | The host's `Touched` handler fires for it | The script's effect (respawn, coin count) happens for the joined player |
| On the Player, a LocalScript reads another character's `HumanoidRootPart.Position` every second | The value moves as that avatar walks | Printed positions change |
| Respawn the joined player (reset or a kill brick) | `CharacterRemoving`, then `CharacterAdded`, on both machines, once each | Script prints |

## GUIs

| Do | Expect | Proof |
|---|---|---|
| Join a Space with a HUD in StarterGui | The Player shows its HUD; the host shows only its own, never a second copy | Look at both windows |
| A server Script writes into `player.PlayerGui` (a label's text) | The Player's HUD shows the change | Look |
| Respawn the joined player | The HUD resets (`ResetOnSpawn`), and its LocalScript does not run twice | The HUD script's startup print appears once per respawn |

## Animation

| Do | Expect | Proof |
|---|---|---|
| Walk the joined avatar | The host shows it walking with the Animate gait, not a rest pose | Look at Studio |
| Walk the host avatar | The Player shows the host walking | Look at the Player |
| An emote from the host's server Script on the joined character | Both machines play it, at the same phase | Look |
| Join a second Player late while the host's NPC idles | The late Player shows the idle already running | Look |

## Seats (Vehicle Simulator)

| Do | Expect | Proof |
|---|---|---|
| Walk the joined avatar into a car's VehicleSeat | It sits on both machines in the same frame; Studio's scripts see `Occupant`, `Sit`, `SeatPart`, `SeatWeld` | A server Script printing on `seat:GetPropertyChangedSignal("Occupant")` |
| Drive with W/A/S/D as the joined player | The car moves on the host; the Player sees it and its own avatar in the seat | Look at both |
| Press Space while driving (Vehicle Simulator's handbrake, jumping off) | The car brakes; the driver stays seated | Look |
| Press F (Vehicle Simulator's leave key) | The driver gets up once, with no stale jump | Look |
| In a Space whose seats allow jumping, press Space in a seat | The occupant leaves with a jump, and cannot re-sit for one second | Look |
| Drive fast | The driver stays in the seat with no lag behind the car | Look at the Player at speed |
| Wheels at speed | They spin forward, never backward | Look |

## If something fails

- `net: peer N's avatar went non-finite on this machine`: a replica's pose
  broke; the line names the peer.
- `limb IK skipped a non-finite ...` or `avatar: non-finite move on ...`:
  Client's guards; the line names the avatar and the stage.
- `multiplayer: refused N animation change(s) from peer N`: the host refused
  a player's track changes; the joined player's script played something not
  its own or outside the world.
