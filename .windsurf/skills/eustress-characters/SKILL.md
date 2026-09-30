---
name: eustress-characters
description: >-
  Sets up the player's character in an Eustress game, or turns it off: playing with no
  avatar at all (Players character_auto_loads, LoadCharacter, Character:Destroy), movement switches (jump, sprint,
  climb, vault, ledge leap, wall jump, roll) for the whole Space or per character, walk
  speed, jump height and health, custom character bodies and animations per body option
  (masculine, feminine, robot, or one default for everyone) through
  `StarterPlayer/Characters/*.rig.toml`, and building a blocky custom hero in Blender.
  Use when the user asks to change how the player moves, turn parkour or climbing on or
  off, make the character faster or taller, replace the default character with their
  own model, add custom animations, or make heroes for their game.
---

# Characters in Eustress

Every player gets an animated avatar in Play, driven by W, A, S, D, Space and Shift. A
Space decides three things about it: which movement verbs are on, the numbers (speed,
jump, health), and what the body looks like.

## 0. No character at all

Games played without a body (Pong, a puzzle board, a card table, a strategy map seen from
above) turn characters off in `Players/_service.toml`:

```toml
[properties]
character_auto_loads = false
```

Play then starts with no avatar:

- Scripts read input as usual: `UserInputService:IsKeyDown`, `InputBegan`, the mouse.
  A common pattern is a client script turning keys into an attribute
  (`player:SetAttribute("InputY", 1)`) that the server reads.
- `Players.LocalPlayer` exists and `player.Character` is `nil`.
- The camera opens on the editor's view and belongs to the scripts: set
  `workspace.CurrentCamera.CameraType = Enum.CameraType.Scriptable` and its `CFrame`
  (see the top-down camera in `eustress-scripting`).
- The editor's own keys stay off during Play, so W, S or Delete never touch the scene.

At runtime, either way:

- `player:LoadCharacter()` gives the player a fresh body at a SpawnLocation (a respawn if
  it already has one).
- `player.Character:Destroy()` removes the body; the camera stays where it was and
  scripts can take it over.

## 1. Movement switches for the whole Space

`StarterPlayer/_service.toml`, under `[properties]` (or select StarterPlayer in Studio
and use the Properties panel):

```toml
jump_enabled = true
sprint_enabled = true
climbing_enabled = true
vaulting_enabled = false
ledge_leap_enabled = false
wall_jump_enabled = false
roll_enabled = false
```

All seven default to on. A top-down shooter might keep jump and sprint and turn the
parkour verbs off; an obby wants them all.

### What the character climbs

With climbing on, the character takes hold of the world: anchored parts, terrain, and
anything else that stays where it is. By default it never climbs:

- another player's character, or an NPC: any Model holding a `Humanoid` or an
  `AnimationController`,
- an unanchored part, or anything else that moves,
- a part with a `Transparency` of 0.95 or more. An invisible wall still stops the
  character, and it is never climbed.

A part with `CanCollide` off is a trigger zone, and climbing passes straight through it.

A `Climbable` attribute on a part overrides the defaults for it: `true` makes it climbable
(a statue of a character, a crate that should be climbed), `false` keeps characters off it
(a building's decorative trim). Add it as an attribute in Studio's Properties panel, or in
the part's file:

```toml
# Workspace/Statue/Plinth.instance.toml: part of a statue of a character, made climbable
[attributes]
Climbable = true
```

```toml
# Workspace/Market/Trim.instance.toml: a building's trim, kept off limits
[attributes]
Climbable = false
```

Another player's character is never climbable, whatever the attribute says.

## 2. Per character, from a script

The Humanoid carries the same switches as properties, so a script can change them for
one player at any moment (a power-up, a stun, a cutscene):

```lua
humanoid.SprintEnabled = false
humanoid.ClimbingEnabled = true
humanoid.JumpEnabled = false
humanoid:SetStateEnabled(Enum.HumanoidStateType.Jumping, true)   -- same switch as JumpEnabled
humanoid:SetStateEnabled(Enum.HumanoidStateType.Climbing, false) -- same as ClimbingEnabled
```

The full set: `JumpEnabled`, `SprintEnabled`, `ClimbingEnabled`, `VaultingEnabled`,
`LedgeLeapEnabled`, `WallJumpEnabled`, `RollEnabled`.

Numbers, set when the character arrives (see `eustress-scripting` for the
`CharacterAdded` pattern):

```lua
humanoid.WalkSpeed = 8      -- meters per second
humanoid.JumpHeight = 1.2   -- meters at the top of a jump
humanoid.MaxHealth = 100
humanoid.Health = 100
```

Keep these numbers in the game's Config module so they are tuned in one place.

## 3. Custom bodies and animations

Put the files in `StarterPlayer/Characters/` inside the Space:

| File | Used for |
|---|---|
| `Masculine.rig.toml` (or `Male.rig.toml`) | players whose account has the masculine body |
| `Feminine.rig.toml` (or `Female.rig.toml`) | players whose account has the feminine body |
| `Robot.rig.toml` | agent accounts |
| `Default.rig.toml` | every body option without a file of its own |

The body option belongs to the player's account, set by identity verification. A Space
never picks it; the Space only decides what each option looks like in this game. So a game
offers a hero and a heroine by shipping `Masculine.rig.toml` and `Feminine.rig.toml`, or
one look for everyone with `Default.rig.toml` alone.

```toml
label = "Box Head Hero"
body = "StarterPlayer/Characters/BoxheadMasculine.glb"
height = 2.05                  # meters; optional

[animations]                   # optional; each missing clip uses the built-in one
idle = "StarterPlayer/Characters/HeroIdle.glb"
walk = "StarterPlayer/Characters/HeroWalk.glb"
run  = "StarterPlayer/Characters/HeroRun.glb"
jump = "StarterPlayer/Characters/HeroJump.glb"

[bones]                        # optional; a node name in your file = a standard bone
"Bip01 Pelvis" = "hips"
```

- Paths are relative to the Space folder, using letters, digits and `_ - . /`.
- `height` is clamped to 1.45 to 2.05 m. It does not apply to the Robot body.
- Each animation file's first clip (`Animation0`) is used.
- Bones are matched by name automatically: Mixamo names and the usual variants (hips or
  pelvis, spine, spine1, spine2, neck, head, and the left and right shoulder, arm, forearm,
  hand, upper leg, leg, foot and toe). `[bones]` maps anything else.
- A body built on the Mixamo skeleton plays the built-in clips as they are, so the easy
  path is: rig on Mixamo's skeleton, skip `[animations]`, and it walks, runs, jumps and
  climbs right away.
- A file that fails to parse or names a path outside the Space is skipped with a warning
  naming the file; that body option falls back to the default body, and the rest of the
  Space works.
- The files are read when Play starts. Change them, then Play again.

## 4. Making a blocky hero in Blender

Blocky heroes (box heads, voxel people, toy soldiers) are the quickest custom bodies:

1. Import a Mixamo character (the "Y Bot" or "X Bot" works) into Blender to borrow its
   armature. Delete its mesh and keep the armature.
2. Build the body from boxes: a head, a torso, upper and lower arms, hands, upper and lower
   legs, feet. Place each box by the bone's **head** position. Blender reports end bones
   such as `HeadTop_End` at the origin, so build from bone heads only.
3. Join each box to the armature with one vertex group named after its bone and full
   weight, so every box moves rigidly with its bone.
4. Materials: flat base colours read best from a distance. Very dark colours render lighter
   than you expect under the default lighting, so push "black" a little darker and keep
   whites clean.
5. Export glTF binary (`.glb`) with the armature and the mesh, to
   `<Space>/StarterPlayer/Characters/`.
6. Write the `.rig.toml` beside it, press Play, and `capture_viewport`.

## 5. Check it in Play

Look at a capture, not only the logs:

- Put the character next to something of known size (a door, an enemy, a lamp post). A
  body that is several times too big or floats above the ground is obvious in a picture
  and invisible in a log.
- Walk (`play_input {down: ["W"]}`), sprint (hold `LeftShift` as well), jump
  (`{tap: ["Space"]}`), and capture mid-move: the feet should stay on the ground and the
  arms should swing.
- Compare the hero against the enemies. Players read a hero much smaller than the enemies
  as weak, and one much bigger as clumsy; about the same height reads as fair.
