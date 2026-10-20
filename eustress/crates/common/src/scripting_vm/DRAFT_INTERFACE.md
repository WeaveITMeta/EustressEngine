# Scripting VM interface (DRAFT, not yet wired)

Goal: one script API (host functions, userdata, instance proxies, budget,
sandbox) written once against `ScriptVm`. Backends: `mlua` (native),
`pure` (wasm32, pure Rust). Feature gate picks the backend. Nothing outside
this module names `mlua::` directly after migration.

## Surface, from the current mlua call sites

Counts are from a grep of common/engine/play-runtime, not exhaustive.

VM lifecycle
- construct VM, `globals()`, `load(source, chunk_name) -> Function`
- `sandbox(true)`, `set_interrupt(fn)` (budget hook), `set_script_timeout`
- `used_memory()`, `set_memory_limit` (if used)

Registry and app data
- `named_registry_value`, `set_named_registry_value`, `remove_registry_value`
- `create_registry_value` / RegistryKey
- `set_app_data<T>`, `app_data_ref<T>` (typed side-channel for host state)

Value construction
- `create_table`, `create_table_with_capacity`, `create_sequence_from`
- `create_string`, `create_function(Fn(&Vm, MultiValue) -> Result<MultiValue>)`
- `create_userdata<T: UserData>` with `add_fields`, `add_methods`,
  `__index`, `__newindex`, `__eq`, `__add`/`__sub`/`__mul`/`__div`/`__unm`,
  `__tostring`, `__call`

Value model (replaces `mlua::Value` and friends)
- Nil, Boolean, Integer(i64), Number(f64), String, Table, Function,
  UserData, plus whatever `Thread` use exists (not yet confirmed)
- `Variadic<T>`, `MultiValue`, `FromLua`/`IntoLua` conversions
- `serde` bridge (`LuaSerdeExt`): to_value / from_value

Errors
- `RuntimeError`, `SyntaxError{message}`, `FromLuaConversionError`,
  `external(e)`; mapped to one `VmError` enum

Compiler check
- `mlua::Compiler::new().compile(source)`: used only by lang/mod.rs for
  editor diagnostics. Backend may return SyntaxError from `load` instead,
  which needs a decision (see open questions).

## Proposed trait shape (sketch)

```rust
pub trait ScriptVm: Sized {
    type Value: Clone;
    type Table: Clone;
    type Function: Clone;
    type UserData: Clone;

    fn new() -> Result<Self, VmError>;
    fn globals(&self) -> Self::Table;
    fn load(&self, source: &str, name: &str) -> Result<Self::Function, VmError>;
    fn create_table(&self) -> Result<Self::Table, VmError>;
    fn create_function<F>(&self, f: F) -> Result<Self::Function, VmError>
        where F: Fn(&Self, Vec<Self::Value>) -> Result<Vec<Self::Value>, VmError> + 'static;
    fn create_userdata<T: ScriptUserData>(&self, t: T) -> Result<Self::UserData, VmError>;
    fn set_app_data<T: 'static>(&self, t: T);
    fn app_data_ref<T: 'static>(&self) -> Option<std::cell::Ref<'_, T>>;
    fn set_interrupt(&self, f: impl Fn() -> bool + 'static);
    fn sandbox(&self, on: bool) -> Result<(), VmError>;
    fn used_memory(&self) -> usize;
}
```

The trait shape is a sketch. Associated-type ergonomics and the `Value`
enum are the parts most likely to change after review.

## Open questions for review

1. Value model: one enum for both backends (recommended), or each backend
   exposes its own and the API is generic?
2. Userdata: do we define our own `ScriptUserData` trait that both backends
   implement, or keep the `mlua::UserData` derive pattern and reimplement it
   for pure?
3. Compile check: keep a separate `check_syntax` on the trait, or have
   `load` return SyntaxError on both backends?
4. Coroutines / `Thread`: grep found no direct use yet. Confirm they are not
   needed in browser v1.
5. Budget hook: mlua's interrupt is per-instruction. Is a pure backend
   allowed a coarser budget (per host call) for v1?

## Not in this draft

- No code moved. No mlua call sites changed.
- No pure-Rust backend chosen. Candidate crates have not been evaluated.
