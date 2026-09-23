"""Blender helper: repack a GLB with 1K preview maps without changing its rig.

Geometry, skins, and animation accessors are copied byte-for-byte. The native
asset stays in common/assets; only the bounded preview belongs in Pages.
"""
import json
import struct
import tempfile
from pathlib import Path

import bpy

PAGES_MAX_BYTES = 25 * 1024 * 1024
PREVIEW_MAX_BYTES = 24 * 1024 * 1024


def build_preview(source, destination):
    raw = Path(source).read_bytes()
    json_length = struct.unpack_from('<I', raw, 12)[0]
    doc = json.loads(raw[20:20 + json_length])
    binary = raw[28 + json_length:]
    replacements = {}
    with tempfile.TemporaryDirectory(prefix='voltec-preview-') as folder:
        for index, entry in enumerate(doc.get('images', [])):
            view_index = entry['bufferView']
            view = doc['bufferViews'][view_index]
            start = view.get('byteOffset', 0)
            path = Path(folder) / f'{index}.png'
            path.write_bytes(binary[start:start + view['byteLength']])
            image = bpy.data.images.load(str(path), check_existing=False)
            width, height = image.size
            if max(width, height) > 1024:
                ratio = 1024 / max(width, height)
                image.scale(round(width * ratio), round(height * ratio))
                image.file_format = 'PNG'
                image.filepath_raw = str(path)
                image.save()
                replacements[view_index] = path.read_bytes()
                entry['mimeType'] = 'image/png'
            bpy.data.images.remove(image)
    rebuilt = bytearray()
    for index, view in enumerate(doc['bufferViews']):
        start = view.get('byteOffset', 0)
        payload = replacements.get(index, binary[start:start + view['byteLength']])
        rebuilt.extend(b'\0' * (-len(rebuilt) % 4))
        view['byteOffset'], view['byteLength'] = len(rebuilt), len(payload)
        rebuilt.extend(payload)
    doc['buffers'][0]['byteLength'] = len(rebuilt)
    rebuilt.extend(b'\0' * (-len(rebuilt) % 4))
    encoded = json.dumps(doc, separators=(',', ':')).encode()
    encoded += b' ' * (-len(encoded) % 4)
    result = (struct.pack('<III', 0x46546C67, 2, 28 + len(encoded) + len(rebuilt))
              + struct.pack('<I4s', len(encoded), b'JSON') + encoded
              + struct.pack('<I4s', len(rebuilt), b'BIN\0') + rebuilt)
    if len(result) >= PREVIEW_MAX_BYTES:
        raise ValueError(f'Avatar preview is {len(result):,} bytes; exceeds the 24 MiB preview budget. '
                         'Reduce mesh size or host it in object storage. The Pages copy was not changed.')
    destination = Path(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    staged = destination.with_suffix('.glb.tmp')
    staged.write_bytes(result)
    staged.replace(destination)
    print(f'WEB PREVIEW {destination.name}: {len(result):,} bytes', flush=True)


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[1]
    web = root.parents[2] / 'web/assets/characters'
    build_preview(root / 'voltec_supreme.glb', web / 'voltec_supreme_preview.glb')
    # Remove only the obsolete generated copy, never the production source.
    (web / 'voltec_supreme.glb').unlink(missing_ok=True)
