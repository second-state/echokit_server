//! SeekDB MCP Server Library
//!
//! A Rust MCP Server providing vector search functionality for SeekDB.
//! Uses client-side embeddings (fastembed) similar to pyseekdb's DefaultEmbeddingFunction.

pub mod config;
pub mod db;
pub mod embeddings;
