# Eustress Backend - Quick Start

## Setup

1. Copy `.env.example` to `.env`:
   ```powershell
   cp .env.example .env
   ```

2. Edit `.env` and set your values:
   - `JWT_SECRET` - Generate with: `openssl rand -base64 32`
   - `STEAM_API_KEY` - Get from https://steamcommunity.com/dev/apikey

3. Run the server:
   ```powershell
   cargo run --package eustress-backend
   ```

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| POST | `/api/auth/register` | Register new user |
| POST | `/api/auth/login` | Login with email/password |
| GET | `/api/auth/me` | Get current user (requires token) |
| POST | `/api/auth/refresh` | Refresh JWT token |
| GET | `/api/auth/steam` | Redirect to Steam login |
| GET | `/api/auth/steam/callback` | Steam OAuth callback |

## Steam Login Flow

1. User clicks "Login with Steam" button
2. Frontend redirects to `http://localhost:7000/api/auth/steam`
3. Backend redirects to Steam OpenID login page
4. User authenticates on Steam
5. Steam redirects to `http://localhost:7000/api/auth/steam/callback`
6. Backend verifies, creates/updates user, generates JWT
7. Backend redirects to `http://localhost:3000?token=xxx&user_id=xxx`
8. Frontend extracts token from URL and stores it

## Development

Run both servers:
```powershell
# Terminal 1 - Backend API (port 7000)
cd crates/backend
cargo run

# Terminal 2 - Frontend (port 3000)
cd crates/web
trunk serve --port 3000
```
