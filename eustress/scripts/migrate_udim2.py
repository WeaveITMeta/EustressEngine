#!/usr/bin/env python3
"""
Migrate Eustress GUI TOML files to the strict UDim2 schema.

This is the v2 migration — uses Python's tomllib to handle multi-line
arrays correctly, and also cleans up legacy split fields that were
introduced before the UDim2 refactor.

Per-field rules (only inside `[gui]` sections):

  position = [x, y]            → [0, x, 0, y]                 (2D pixel position → UDim2)
  position = [x, y, z]         → moves to units_offset = [x, y, z],
                                 sets position = [0, 0, 0, 0]   (BillboardGui legacy 3D)
  position = [a, b, c, d]      → unchanged                    (already UDim2)

  size = [w, h]                → [0, w, 0, h]
  size = [a, b, c, d]          → unchanged

  Legacy split fields (deleted, merged into the unified UDim2):
    size_scale + size_offset (each [f32; 2]) → folded into size as UDim2 when
                                                main size is absent or 2-float
    position_scale + position_offset → folded into position similarly

  studs_offset = [x, y, z]     → renamed to units_offset (Roblox naming)

  size_offset (when size is already 4-float UDim2) → kept as Option<UDim2>
                                                     if it's a 4-tuple,
                                                     dropped otherwise

Idempotent. Re-running on a migrated file is a no-op.

Usage:  python migrate_udim2.py <root_dir> [--dry-run] [--verbose]
"""
import sys
import os
from pathlib import Path

# Python 3.11+ has tomllib in stdlib for reading. tomli_w is optional.
try:
    import tomllib  # type: ignore
except ImportError:
    try:
        import tomli as tomllib  # type: ignore
    except ImportError:
        print("error: this script needs Python 3.11+ (tomllib) or `pip install tomli`",
              file=sys.stderr)
        sys.exit(2)


def is_udim2_4tuple(v) -> bool:
    return isinstance(v, list) and len(v) == 4 and all(isinstance(x, (int, float)) for x in v)


def is_pixel_2tuple(v) -> bool:
    return isinstance(v, list) and len(v) == 2 and all(isinstance(x, (int, float)) for x in v)


def is_xyz_3tuple(v) -> bool:
    return isinstance(v, list) and len(v) == 3 and all(isinstance(x, (int, float)) for x in v)


def migrate_gui_section(gui: dict) -> tuple[dict, int]:
    """Return (migrated_gui_dict, number_of_changes)."""
    changes = 0
    out = dict(gui)

    # ── Rename studs_offset → units_offset (Roblox → engine internal) ──
    if "studs_offset" in out and "units_offset" not in out:
        out["units_offset"] = out.pop("studs_offset")
        changes += 1
    elif "studs_offset" in out:
        # Both present — drop the duplicate.
        del out["studs_offset"]
        changes += 1

    # ── Position ──
    pos = out.get("position")
    if isinstance(pos, list):
        if is_pixel_2tuple(pos):
            x, y = pos
            out["position"] = [0.0, float(x), 0.0, float(y)]
            changes += 1
        elif is_xyz_3tuple(pos):
            # BillboardGui legacy: 3D world-space offset → units_offset
            ux, uy, uz = pos
            if "units_offset" not in out:
                out["units_offset"] = [float(ux), float(uy), float(uz)]
            out["position"] = [0.0, 0.0, 0.0, 0.0]
            changes += 1
        elif is_udim2_4tuple(pos):
            pass  # already UDim2
        else:
            # Unknown shape — coerce to default.
            out["position"] = [0.0, 0.0, 0.0, 0.0]
            changes += 1
    # ── If only split position_scale + position_offset are present, fold ──
    has_legacy_pos = "position_scale" in out or "position_offset" in out
    if has_legacy_pos:
        ps = out.pop("position_scale", [0.0, 0.0])
        po = out.pop("position_offset", [0.0, 0.0])
        if is_pixel_2tuple(ps) and is_pixel_2tuple(po):
            cur = out.get("position")
            if not is_udim2_4tuple(cur):
                out["position"] = [float(ps[0]), float(po[0]), float(ps[1]), float(po[1])]
                changes += 1
        else:
            changes += 1  # at least the field rename is a change

    # ── Size ──
    size = out.get("size")
    if isinstance(size, list):
        if is_pixel_2tuple(size):
            w, h = size
            out["size"] = [0.0, float(w), 0.0, float(h)]
            changes += 1
        elif is_udim2_4tuple(size):
            pass
        else:
            # 3-float or other — coerce.
            w = float(size[0]) if len(size) >= 1 else 100.0
            h = float(size[1]) if len(size) >= 2 else 30.0
            out["size"] = [0.0, w, 0.0, h]
            changes += 1
    # ── If only split size_scale + size_offset are present, fold ──
    has_legacy_size = "size_scale" in out or ("size_offset" in out and not is_udim2_4tuple(out.get("size_offset")))
    if has_legacy_size:
        ss = out.pop("size_scale", None)
        so = out.pop("size_offset", None)
        # Only fold the SPLIT 2-tuple form. A 4-tuple `size_offset` is the
        # post-refactor UDim2 SizeOffset and should be kept under that name.
        if is_pixel_2tuple(ss) and is_pixel_2tuple(so):
            cur = out.get("size")
            if not is_udim2_4tuple(cur):
                out["size"] = [float(ss[0]), float(so[0]), float(ss[1]), float(so[1])]
            # We dropped both legacy fields; that itself is the change.
            changes += 1
        else:
            # ss alone or so alone or wrong shapes — drop and let defaults
            # fill in. Re-insert size_offset only if it was already a UDim2.
            if is_udim2_4tuple(so):
                out["size_offset"] = so
            changes += 1

    return out, changes


def write_toml_value(v, indent: str = "") -> str:
    """Minimal TOML value writer covering everything we emit:
    bool, int, float, str, list-of-numbers, list-of-strings.
    Lists are emitted on a single line (compact)."""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        # Always include a decimal point so floats round-trip predictably.
        if v == int(v):
            return f"{v:.1f}"
        return repr(v)
    if isinstance(v, str):
        # Escape quotes + backslashes.
        esc = v.replace("\\", "\\\\").replace('"', '\\"')
        return f'"{esc}"'
    if isinstance(v, list):
        items = ", ".join(write_toml_value(x) for x in v)
        return f"[{items}]"
    raise TypeError(f"Unsupported TOML value type: {type(v).__name__}")


def serialise_toml(doc: dict) -> str:
    """Write a flat-section TOML doc back as text. Section order:
    instance, metadata, asset, gui, text — anything else preserved
    in iteration order at the end."""
    SECTION_ORDER = ["instance", "metadata", "asset", "transform", "gui", "text",
                     "image", "video", "properties"]
    seen = set()
    out_lines: list[str] = []

    def emit_section(name: str, body: dict):
        out_lines.append(f"[{name}]")
        for k, v in body.items():
            out_lines.append(f"{k} = {write_toml_value(v)}")
        out_lines.append("")  # blank line between sections

    for name in SECTION_ORDER:
        if name in doc and isinstance(doc[name], dict):
            emit_section(name, doc[name])
            seen.add(name)
    for name, body in doc.items():
        if name in seen:
            continue
        if isinstance(body, dict):
            emit_section(name, body)
        else:
            # Top-level scalar — emit before any section.
            # (Not expected in our schema; preserved defensively.)
            out_lines.insert(0, f"{name} = {write_toml_value(body)}")
    return "\n".join(out_lines).rstrip() + "\n"


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        return 2

    root = Path(sys.argv[1])
    dry = "--dry-run" in sys.argv[1:]
    verbose = "--verbose" in sys.argv[1:]

    if not root.exists():
        print(f"error: {root} does not exist", file=sys.stderr)
        return 1

    total_files = 0
    total_changes = 0
    files_changed: list[str] = []
    parse_errors: list[tuple[str, str]] = []

    for path in root.rglob("*.toml"):
        total_files += 1
        try:
            with open(path, "rb") as fh:
                doc = tomllib.load(fh)
        except (PermissionError, OSError):
            continue
        except tomllib.TOMLDecodeError as e:
            parse_errors.append((str(path), str(e)))
            continue

        gui = doc.get("gui")
        if not isinstance(gui, dict):
            continue  # not a GUI TOML

        new_gui, changes = migrate_gui_section(gui)
        if changes == 0:
            continue

        total_changes += changes
        files_changed.append(str(path))
        if verbose:
            print(f"  {path}  ({changes} changes)")

        if not dry:
            doc["gui"] = new_gui
            text = serialise_toml(doc)
            try:
                path.write_text(text, encoding="utf-8")
            except (PermissionError, OSError) as e:
                print(f"warning: failed to write {path}: {e}", file=sys.stderr)

    print(f"Scanned {total_files} TOML files")
    print(f"  Changes: {total_changes} field rewrites across {len(files_changed)} files")
    if parse_errors:
        print(f"  Parse errors: {len(parse_errors)} files (skipped)")
        for p, err in parse_errors[:5]:
            print(f"    {p}: {err}")
        if len(parse_errors) > 5:
            print(f"    ... and {len(parse_errors) - 5} more")
    if dry:
        print(f"  DRY RUN — no files written. Re-run without --dry-run to apply.")
    else:
        print(f"  Rewrote {len(files_changed)} files.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
