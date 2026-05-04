# Chat

**Category:** Core  
**Class Name:** `Chat`  
**Learn URL:** `/learn/services/chat`

## Overview

The Chat service provides the default text chat system with support for bubble
chat (speech bubbles above characters), message filtering for safety, and
customizable chat windows. It can be extended with ChatModules for custom
commands and behaviors.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `BubbleChatEnabled` | bool | `true` | Show speech bubble above character when they chat. |
| `LoadDefaultChat` | bool | `true` | Load the built-in chat GUI. Disable for custom chat UI. |
| `FilteringEnabled` | bool | `true` | Enable server-side text filtering for player safety. |

## Key Responsibilities

- **Bubble Chat** — When `BubbleChatEnabled` is true, a speech bubble appears
  above each character when they send a message. The bubble fades after a few
  seconds.

- **Default Chat GUI** — The built-in chat window with message history, team
  chat, and whisper support. Set `LoadDefaultChat` to `false` to replace it
  with your own custom chat UI.

- **Text Filtering** — `FilteringEnabled` controls server-side text filtering
  that censors inappropriate content. This should remain `true` for public
  Spaces to comply with safety guidelines.

- **ChatModules** — Scripts placed inside the Chat service that extend chat
  functionality. Use them to add slash commands (e.g., `/mute`, `/team`),
  custom formatting, or chat bots.

## Usage Example (Rune)

```rune
let chat = game.get_service("Chat");

// Disable bubble chat for a cleaner UI
chat.BubbleChatEnabled = false;

// Use a custom chat system
chat.LoadDefaultChat = false;
```

## Related Services

- [Players](players.md) — Chat messages are associated with Player objects
- [Teams](teams.md) — Team chat routes messages to teammates only
