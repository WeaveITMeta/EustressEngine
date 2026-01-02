# Store Assets

This directory contains assets required for app store submissions.

## Google Play Store

Location: `google-play/`

### Required Assets
| Asset | Size | Format | Notes |
|-------|------|--------|-------|
| `icon.png` | 512x512 | PNG | App icon |
| `feature-graphic.png` | 1024x500 | PNG | Store banner |
| `screenshot-1.png` | 1080x1920 or 1920x1080 | PNG | Phone screenshot |
| `screenshot-2.png` | 1080x1920 or 1920x1080 | PNG | Phone screenshot |
| `screenshot-tablet-1.png` | 1200x1920 or 1920x1200 | PNG | Tablet screenshot |

### Text Content
- **Short Description**: 80 characters max
- **Full Description**: 4000 characters max
- **Privacy Policy URL**: Required

## Apple App Store

Location: `app-store/`

### Required Assets
| Asset | Size | Format | Notes |
|-------|------|--------|-------|
| `icon.png` | 1024x1024 | PNG | No alpha/transparency |
| `screenshot-6.7.png` | 1290x2796 | PNG | iPhone 14 Pro Max |
| `screenshot-6.5.png` | 1284x2778 | PNG | iPhone 14 Plus |
| `screenshot-5.5.png` | 1242x2208 | PNG | iPhone 8 Plus |
| `screenshot-ipad-12.9.png` | 2048x2732 | PNG | iPad Pro 12.9" |

### Text Content
- **Name**: 30 characters max
- **Subtitle**: 30 characters max
- **Description**: 4000 characters max
- **Keywords**: 100 characters max (comma-separated)
- **Privacy Policy URL**: Required
- **Support URL**: Required

## Asset Generation

Use the master icon to generate all sizes:

```bash
# Install ImageMagick
# Windows: choco install imagemagick
# macOS: brew install imagemagick

# Generate Google Play icon
magick master-icon.png -resize 512x512 google-play/icon.png

# Generate App Store icon (remove alpha)
magick master-icon.png -resize 1024x1024 -background white -alpha remove app-store/icon.png
```

## Screenshots

Capture screenshots from the running app:
1. Run player on device/emulator at target resolution
2. Use platform screenshot tools
3. Add device frames (optional) using tools like:
   - [Previewed](https://previewed.app/)
   - [AppMockUp](https://app-mockup.com/)
   - [Shotbot](https://shotbot.io/)
