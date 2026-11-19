#!/system/bin/sh
# Emulator-only. gfxstream Vulkan blocked in vkCreateImageView on this host.
# arm64-v8a has no wrap.sh, so a device still picks Vulkan or GL.
export WGPU_BACKEND=gl
exec "$@"
