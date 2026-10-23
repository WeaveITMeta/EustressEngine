//! Compile-only proof that mlua's Luau backend links for this target.
pub fn roundtrip() -> mlua::Result<i64> {
    let lua = mlua::Lua::new();
    lua.load("return 1 + 2").eval()
}
