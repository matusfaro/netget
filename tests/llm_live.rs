//! Live-LLM protocol integration tests.
//!
//! Every test here drives the real netget binary against a real Ollama model
//! (default: `qwen3.8:27b-mlx`) and asserts on actual wire behavior — how the
//! model sets up each protocol server from a natural-language prompt, and how
//! it answers each request type of that protocol. Tests skip unless
//! `NETGET_USE_OLLAMA=1` is set. See `tests/llm_live/CLAUDE.md`.

pub mod helpers;

#[path = "llm_live/mod.rs"]
mod llm_live;
