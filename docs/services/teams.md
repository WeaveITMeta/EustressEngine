# Teams

**Category:** Player  
**Class Name:** `Teams`  
**Learn URL:** `/learn/services/teams`

## Overview

The Teams service holds Team objects that players can be assigned to. Each Team
has a TeamColor and AutoAssignable flag. When teams exist, player names appear
in their team color and the leaderboard shows team groupings.

## Properties

Teams has no configurable properties on the service itself. Configuration is
done by adding Team child objects with these properties:

| Property (per Team) | Type | Default | Description |
|---------------------|------|---------|-------------|
| `TeamColor` | BrickColor | varies | The color identifier for this team. |
| `AutoAssignable` | bool | `true` | Whether new players are auto-assigned to this team. |
| `Name` | string | `"Team"` | Display name shown in leaderboard and nametags. |

## Key Responsibilities

- **Team Assignment** — Players are assigned to teams via their `Team`
  property. When `AutoAssignable` is true, the engine balances new players
  across available teams.

- **Colored Nametags** — When teams exist, each player's overhead name is
  colored to match their team color. This provides instant visual team
  identification.

- **Leaderboard Grouping** — The default leaderboard groups players by team,
  showing team totals alongside individual scores.

- **Team Chat** — When teams exist, players can use `/team` to send messages
  only to their teammates.

## Common Patterns

### Two-Team Game
Create two Team children (e.g., "Red" and "Blue") with distinct TeamColors
and `AutoAssignable = true`. The engine auto-balances new players.

### Free-for-All with Spectators
Create a "Playing" team (`AutoAssignable = true`) and a "Spectators" team
(`AutoAssignable = false`). Scripts move eliminated players to Spectators.

### Round-Based Assignment
Set all teams to `AutoAssignable = false`. A server script manually assigns
players at the start of each round.

## Usage Example (Rune)

```rune
let teams = game.get_service("Teams");

// Create teams programmatically
let red = Instance.new("Team");
red.Name = "Red Team";
red.TeamColor = BrickColor.new("Bright red");
red.AutoAssignable = true;
red.Parent = teams;

let blue = Instance.new("Team");
blue.Name = "Blue Team";
blue.TeamColor = BrickColor.new("Bright blue");
blue.AutoAssignable = true;
blue.Parent = teams;
```

## Related Services

- [Players](players.md) — Each Player has a Team property
- [Chat](chat.md) — Team chat integration
