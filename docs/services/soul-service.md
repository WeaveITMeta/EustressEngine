# SoulService

**Category:** Scripting  
**Class Name:** `SoulService`  
**Learn URL:** `/learn/services/soul-service`

## Overview

SoulService is the Eustress-native scripting service that manages the Rune
virtual machine. It controls script execution permissions, AI-assisted code
generation, and security sandboxing for scripts.

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `EnableAI` | bool | `true` | Enable AI-assisted code generation and suggestions. |
| `AllowFileSystemAccess` | bool | `false` | Allow scripts to read/write the local file system. |
| `AllowNetworkAccess` | bool | `false` | Allow scripts to make HTTP requests and open sockets. |
| `SandboxEnabled` | bool | `true` | Run scripts in an isolated sandbox for security. |

## Key Responsibilities

- **Rune VM Management** — SoulService hosts the Rune virtual machine that
  executes all `.soul` and `.rune` scripts. It manages the compilation
  pipeline, module resolution, and runtime lifecycle.

- **AI Integration** — When `EnableAI` is true, the Workshop AI assistant
  can generate, modify, and explain code. It integrates with the script
  editor for inline suggestions and auto-completion.

- **Security Sandboxing** — `SandboxEnabled` isolates script execution in a
  secure sandbox. Scripts cannot access the host file system or network
  unless explicitly permitted via `AllowFileSystemAccess` and
  `AllowNetworkAccess`.

- **Permission Model** — Fine-grained control over what scripts can do:
  - File system access (read/write local files)
  - Network access (HTTP requests, WebSocket connections)
  - Sandbox isolation (prevents escape to host environment)

## Security Model

SoulService enforces a capability-based security model:

1. **Default: Sandboxed** — Scripts run in isolation with no host access
2. **Opt-in Permissions** — Enable file system or network access per-Space
3. **AI Guardrails** — AI-generated code is reviewed before execution
4. **Audit Trail** — All permission escalations are logged

## Usage Example (Rune)

```rune
let soul = game.get_service("SoulService");

// Check if AI features are available
if soul.EnableAI {
    print("AI-assisted scripting is active");
}

// Note: AllowFileSystemAccess and AllowNetworkAccess
// are configured in Studio, not at runtime
```

## Related Services

- [ServerScriptService](server-script-service.md) — Container for server scripts
- [Workspace](workspace.md) — Where script-controlled objects live
