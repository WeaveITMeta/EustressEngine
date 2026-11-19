//! Agent-surface evaluation for Eustress.
//!
//! The first instrument here is the tool-surface conformance census (gauntlet
//! item G7.01): every tool an AI agent is offered is invoked twice against a
//! live, windowless engine, once with a minimal valid input and once with a
//! required parameter removed, and each outcome lands in exactly one of six
//! classes with a measured latency.
//!
//! Two surfaces are covered, and the union of their tool names is the census:
//!
//! * the MCP server (`eustress-mcp`), driven over stdio JSON-RPC exactly as an
//!   IDE drives it (`tools/list`, `tools/call`);
//! * the engine's Workshop registry, driven over the Engine Bridge
//!   (`tools.list`, `tools.call`), for the names the MCP server does not list.
//!
//! This crate never links the engine. It reaches it through
//! `eustress-bridge-client`, so a census iteration costs seconds.

pub mod census;
pub mod classify;
pub mod digest;
pub mod inputs;
pub mod mcp;
pub mod schema;
pub mod source_index;
pub mod util;
