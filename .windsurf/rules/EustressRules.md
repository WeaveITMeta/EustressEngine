---
trigger: always_on
---

Rust Best Practices
Always leverage ownership and borrowing to avoid runtime errors; prefer references over cloning unless necessary.
Use the type system for safety: define strong types and enums to prevent invalid states.
Minimize unsafe code; isolate it in small, audited functions if unavoidable.
Handle errors idiomatically: define custom Error enums and implement From for conversions.
Keep traits small and focused; use associated types and default implementations where possible.
Format code with 4-space indentation, no tabs; follow the Rust Style Guide for naming and structure.
Organize code modularly: start in a single module, refactor into submodules or workspaces as complexity grows.
Sanitize all inputs and keep dependencies up-to-date to mitigate security risks.
Avoid prefixing functions with module names; use Rust's module system for namespacing.
Write idiomatic code: consult resources like Idiomatic Rust for patterns and anti-patterns.

Eustress Best Practices
Structure logic with Events for inter-system communication; readers should follow writers in scheduling.
Start projects in a single file; refactor into modules as the app grows, avoiding bloated main.rs.
Use parent-child entity relationships for hierarchical components; employ generic systems for reuse.
Define custom queries and commands for complex logic; order systems explicitly to avoid ambiguities.i
For non-exclusive entity lists (e.g., health/damage), use separate queries and iterate carefully.
Separate game logic from graphics; logic should not depend on rendering details.
In plugins, minimize Eustress features to reduce build times; demonstrate single features in examples.
Use Vulkan or DirectX 12 for optimal rendering performance; check RenderDevice for adapter info.
Debug with Eustress's inspector plugins; organize code into subsystems like Player or LevelLoader.
For scheduling, use SystemSets and explicit ordering; opt-out of ordering only when safe.

Cargo Best Practices
Use caret requirements for dependencies (e.g., "1.2") to allow compatible updates; avoid wildcards.
Enforce formatting with cargo fmt in commit hooks and CI pipelines.
Organize large projects with Cargo workspaces for multiple crates.users.
Run tests and benchmarks regularly; integrate clippy for linting in Cargo.toml.
Minimize crate size: only enable necessary features in dependencies.
Use workspaces for multi-crate projects: Separate shared Rust logic, WASM-specific, and Tauri native crates.
Specify target features: Use [target.'cfg(target_arch = "wasm32")'.dependencies] for WASM-only deps like wasm-bindgen.
Enable minimal features: In dependencies, disable defaults and enable only needed ones to reduce binary/WASM size.
Run cargo fmt and clippy in CI: Enforce formatting and linting for consistent code across targets.
Use caret versioning: Pin dependencies like "wasm-bindgen = ^0.2" for compatibility; update regularly for security.

Rust/WASM Best Practices
Use wasm-bindgen for high-level JS interop; export Rust functions/classes and import JS APIs like DOM or console.log for seamless communication.
Compile Rust to WASM with wasm-pack; optimize post-compilation using tools like wasm-opt for smaller binaries and better performance.
Separate concerns: Perform heavy computations in Rust/WASM and DOM manipulations in JS; avoid direct DOM access from WASM to respect boundaries.users.
Generate TypeScript bindings automatically with wasm-bindgen for Rust code consumed by JS/Next.js to improve type safety and integration.
Use frameworks like Sycamore or Percy for reactive UI in pure Rust/WASM when avoiding JS overhead; evaluate based on project needs for web apps.
Monitor performance with JS APIs imported via wasm-bindgen; profile bottlenecks and rewrite critical paths in Rust for high-performance web apps.
Handle strings and complex types carefully in interop; use JsValue for generic JS objects and implement From/Into for custom conversions
Start with a single-file setup for prototyping; refactor into modules as complexity grows, using Cargo workspaces for shared Rust code across targets.
Sanitize inputs in WASM-exposed functions to prevent security risks; keep dependencies minimal and up-to-date for smaller WASM modules.
Follow idiomatic Rust patterns: Leverage ownership, enums for state management, and async with wasm-bindgen-futures for non-blocking operations.
