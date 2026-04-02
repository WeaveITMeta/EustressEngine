# Eustress Experiences Worker

Cloudflare Worker for managing published experiences on R2.

## Setup

### 1. Create R2 Bucket

```bash
wrangler r2 bucket create eustress-experiences
```

### 2. Set JWT Secret

```bash
wrangler secret put JWT_SECRET
# Enter your JWT secret (same as backend)
```

### 3. Deploy

```bash
# Development
wrangler deploy --env dev

# Production
wrangler deploy
```

## API Endpoints

### Public (No Auth)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/experience/:id` | Get experience manifest |
| GET | `/api/experience/:id/download` | Download scene package |
| GET | `/api/experience/:id/thumbnail` | Get thumbnail image |
| GET | `/api/experience/:id/versions` | List version history |

### Protected (JWT Required)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/experience/publish` | Initiate new experience publish |
| PUT | `/api/experience/:id` | Update existing experience |
| POST | `/api/experience/upload/:key` | Upload asset to R2 |
| POST | `/api/experience/:id/commit` | Finalize publish |
| DELETE | `/api/experience/:id` | Delete experience |

## Publish Flow

```
┌─────────────────┐
│  Eustress       │
│  Studio         │
└────────┬────────┘
         │
         │ 1. POST /api/experience/publish
         │    (name, description, genre, etc.)
         ▼
┌─────────────────┐
│  Worker         │──────▶ Returns: experience_id, upload_urls
└────────┬────────┘
         │
         │ 2. POST /api/experience/upload/:key
         │    (scene.eustress, thumbnail.webp, assets/*)
         ▼
┌─────────────────┐
│  R2 Bucket      │
└────────┬────────┘
         │
         │ 3. POST /api/experience/:id/commit
         │    (finalize, archive previous version)
         ▼
┌─────────────────┐
│  Published!     │
└─────────────────┘
```

## R2 Structure

```
eustress-experiences/
├── {experience_id}/
│   ├── manifest.json          # Current metadata
│   ├── scene.eustress         # Current scene package
│   ├── thumbnail.webp         # 512x512 thumbnail
│   ├── assets/                # Bundled assets (optional)
│   │   ├── textures/
│   │   ├── models/
│   │   └── audio/
│   └── versions/              # Version history
│       ├── v1/
│       │   ├── manifest.json
│       │   └── scene.eustress
│       ├── v2/
│       │   ├── manifest.json
│       │   └── scene.eustress
│       └── ...
```

## Manifest Schema

```json
{
  "id": "uuid",
  "name": "My Experience",
  "description": "An awesome experience",
  "genre": "adventure",
  "max_players": 10,
  "is_public": true,
  "allow_copying": false,
  "author_id": "user-uuid",
  "author_name": "PlayerOne",
  "version": 1,
  "created_at": "2024-01-01T00:00:00Z",
  "published_at": "2024-01-01T00:00:00Z",
  "updated_at": "2024-01-01T00:00:00Z"
}
```

## JWT Claims Required

```json
{
  "sub": "user-uuid",
  "user_id": "user-uuid",
  "username": "PlayerOne",
  "exp": 1234567890
}
```

## Local Development

```bash
# Run locally with miniflare
wrangler dev --env dev

# Test endpoints
curl http://localhost:8787/api/experience/test-id
```
