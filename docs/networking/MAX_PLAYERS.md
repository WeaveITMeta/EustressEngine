# Max players: one value, from the Space

Status: **decided by McKale 2026-10-01; build 19.** One authored number says
how many players a session holds. Studio's Properties, the Publish dialog,
the MCP publish tool, the host, the Worker and the gallery all read it, or
write it, through the same place.

## The value

- **Where:** each Space's `Players.MaxPlayers`, in
  `<Space>/Players/_service.toml` as `max_players` (the Roblox name
  `MaxPlayers` reads too). Per Space, as Roblox keeps MaxPlayers per place:
  each Space hosts as its own server. A Universe's gallery listing shows its
  **start Space's** value, the Space a join opens.
- **What it counts:** everyone in the session, the host's own character
  included while the host plays. A host with no character (headless, or
  hosting from Edit) counts nobody. Box Head at 4 is the host plus 3
  joiners, and Live shows 2/4 with the host as one of the two.
- **Range:** a whole number from 1 to 10,000. 10,000 is the gallery's own
  bound (its live heartbeat refuses more), and the only upper bound; fields
  say so. There is **no engine cap**: a Space's number is the limit as
  authored. Unset (or not a whole number of at least 1) means 10.
- **Load testing:** sessions above 4 players have not been load-tested yet.
  The Publish dialog and the host's Output say so, and never refuse or lower
  the number for it.
- New Spaces carry `max_players = 12` from the template.

## Readers and the writer

| What | Where | Signature |
|---|---|---|
| Read a Space's value from its files | `common/src/play_session.rs` | `space_max_players(space_root: &Path) -> SpaceMaxPlayers` |
| Read the open Space's value in Studio (its loaded Players service, so a database-only Space works; the file otherwise) | `engine/src/multiplayer.rs` | `space_max_players(world: &World) -> SpaceMaxPlayers` |
| Set it, as a Properties edit does (Players service, saved to its file, signed when signed in) | `engine/src/multiplayer.rs` | `set_space_max_players(world: &mut World, players: u16) -> Result<(), String>` |

`SpaceMaxPlayers { authored: Option<u32>, players: u16 }`: `players` is what
a session holds; `untested()` says it is above the load-tested size.
Constants in `play_session`: `DEFAULT_MAX_PLAYERS` (10), `MAX_PLAYERS_LIMIT`
(10,000), `LOAD_TESTED_PLAYERS` (4).

**Setting it from a publish:** the Publish dialog and the MCP publish tool
**write** the value (`set_space_max_players` on the start Space) and then
publish. There is no listing-only number: the publish reads it back from
the Space, so hosting, Properties and the listing always agree.

## The host

- `HostConfig.max_players` is the hosted Space's value, read as hosting
  starts. `HostSession::set_host_plays` (Studio sets it every frame from its
  local character) makes the host's own character count.
- A full session turns the next player away with "This session is full (4
  of 4 players)." Players still joining hold their places, so two arriving
  together never overfill it.
- The Output says what the session holds and why: "This session holds 4
  players, the host's own character included while it plays (this Space's
  Players.MaxPlayers)."
- `EUSTRESS_HOST_MAX_PLAYERS` is a developer override only; when set, the
  Output says it overrides the Space's value and is for development.
- The gallery heartbeat's `players` and `max_players` count the same way:
  everyone, the host while it plays.

## The publish and the Worker

- `echk_publish` sends `max_players` (the start Space's value) when it
  creates a listing and in the listing fields of **every whole-Universe
  publish**, so a change to it alone is a change to publish. A Space-only
  publish leaves it as the last whole publish set it.
- **Worker (Website):** create (`POST /api/simulations/publish`) and update
  (`world.mjs` `applyListing`, on commit) accept `max_players` as a whole
  number from 1 to 10,000 and store it; create without it stores 10. A value
  outside that is refused with a message saying the range.
- **Gallery (Website):** "Up to N players" from the listing's `max_players`;
  Live shows `players/max_players` from the heartbeat.

## The Publish dialog (UI, Jev)

- A **Max players** field showing the open Space's value
  (`multiplayer::space_max_players`) when the dialog opens.
- Whole numbers from 1 to 10,000; the field says "1 to 10,000 (the
  gallery's limit)".
- Above 4, a short neutral note: "Sessions above 4 players haven't been
  load-tested yet." Never a refusal.
- On Publish, the handler calls `set_space_max_players` before
  `echk_publish::plan_export`, so the plan reads the new value.

## MCP

The publish tool takes an optional `max_players` (whole number, 1 to
10,000). Given, it calls `set_space_max_players` first; absent, the Space's
value stands.

## Other numbers that are not this one

- `common/src/services/player.rs` `max_players` (100): the runtime Players
  service struct's default, not read for a session's size.
- `engine/src/ui/slint_ui.rs` Forge allocation (`max_players_per_server`
  100): Forge's cloud servers, a separate path (Forge's to point at
  `space_max_players` when it hosts a Space).
- `common/src/project_manifest.rs` `max_players`: what an older API returned
  about a published experience; data, not a source.

## Tests

- `play_session::tests::a_space_says_how_many_players_a_session_holds`:
  file key and Roblox name, the template's 12 taken as authored (with the
  untested note), the gallery bound, unset and odd values.
- `session::tests::a_full_session_turns_the_next_player_away`: the host
  plus 3, the refusal's text, no host character means 4 joiners, and the
  counts the heartbeat sends.
- `multiplayer::tests::a_session_holds_its_spaces_max_players`: the Output
  line, the untested note, the default, the developer override.
- `multiplayer::tests::studio_reads_and_writes_a_spaces_max_players`: read
  from the loaded Players service, write through the Properties path, the
  rest of the file kept, 0 and above 10,000 refused.
