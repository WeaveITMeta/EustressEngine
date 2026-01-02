# Updating Eustress Engine

## Automatic Updates

Eustress Engine checks for updates on launch. When available:
1. A notification appears in the title bar
2. Click **Update Now** to download and install
3. Restart when prompted

## Manual Update

### Windows
1. Download latest installer from [eustress.dev/download](https://eustress.dev/download)
2. Run installer — it will upgrade in place
3. Your projects and settings are preserved

### macOS
1. Download latest `.dmg`
2. Drag new version to Applications (replace existing)
3. Projects in `~/Documents/Eustress` are preserved

### Linux
```bash
# AppImage - just download new version
rm EustressEngine.AppImage
chmod +x EustressEngine-new.AppImage

# Debian/Ubuntu
sudo dpkg -i eustress-engine.deb

# Fedora/RHEL
sudo rpm -U eustress-engine.rpm
```

## Version Check

Check current version: **Help → About Eustress Engine**

Check latest version:
```bash
curl -s https://downloads.eustress.dev/latest.json | jq .version
```

## Rollback

If an update causes issues:

1. **Windows**: Use Control Panel → Uninstall, then install previous version
2. **macOS**: Delete app, download previous version from releases
3. **Linux**: Download previous AppImage or use package manager rollback

Previous versions available at: `https://downloads.eustress.dev/archive/`

## Changelog

See [eustress.dev/changelog](https://eustress.dev/changelog) for release notes.
