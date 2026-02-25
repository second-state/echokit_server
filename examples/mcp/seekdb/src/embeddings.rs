//! Client-side embedding generation using fastembed.
//!
//! This module provides embedding functionality similar to pyseekdb's DefaultEmbeddingFunction,
//! using the all-MiniLM-L6-v2 model (384 dimensions) for local embedding generation.

use anyhow::Result;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

/// Default embedding model matching pyseekdb's DefaultEmbeddingFunction
pub const DEFAULT_MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2;

/// Embedding dimension for all-MiniLM-L6-v2
pub const EMBEDDING_DIM: usize = 384;

/// Thread-safe wrapper for TextEmbedding model
pub struct EmbeddingService {
    model: Arc<Mutex<TextEmbedding>>,
    model_name: String,
}

impl EmbeddingService {
    /// Create a new embedding service with the default model (all-MiniLM-L6-v2)
    pub fn new() -> Result<Self> {
        Self::with_model(DEFAULT_MODEL)
    }

    /// Create an embedding service with a specific model
    pub fn with_model(model: EmbeddingModel) -> Result<Self> {
        info!("Initializing embedding model: {:?}", model);
        
        let options = TextInitOptions::new(model.clone())
            .with_show_download_progress(true);
        
        let text_embedding = TextEmbedding::try_new(options)?;
        
        info!("Embedding model initialized successfully");
        
        Ok(Self {
            model: Arc::new(Mutex::new(text_embedding)),
            model_name: format!("{:?}", model),
        })
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Generate embedding for a single text
    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut model = self.model.lock().await;
        let embeddings = model.embed(vec![text], None)?;
        Ok(embeddings.into_iter().next().unwrap_or_default())
    }

    /// Generate embeddings for multiple texts
    pub async fn embed_many(&self, texts: Vec<&str>) -> Result<Vec<Vec<f32>>> {
        let mut model = self.model.lock().await;
        let embeddings = model.embed(texts, None)?;
        Ok(embeddings)
    }

    /// Format embedding as a string for SQL insertion
    /// SeekDB expects vectors in format: [0.1, 0.2, 0.3, ...]
    pub fn format_for_sql(embedding: &[f32]) -> String {
        let values: Vec<String> = embedding.iter().map(|v| format!("{}", v)).collect();
        format!("[{}]", values.join(", "))
    }
}

impl Clone for EmbeddingService {
    fn clone(&self) -> Self {
        Self {
            model: Arc::clone(&self.model),
            model_name: self.model_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_service() -> Result<()> {
        let service = EmbeddingService::new()?;
        
        let embedding = service.embed_one("Hello, world!").await?;
        
        // all-MiniLM-L6-v2 produces 384-dimensional embeddings
        assert_eq!(embedding.len(), EMBEDDING_DIM);
        
        // Embeddings should be normalized (L2 norm ≈ 1)
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.1, "Expected normalized embedding, got norm={}", norm);
        
        Ok(())
    }

    #[tokio::test]
    async fn test_embed_many() -> Result<()> {
        let service = EmbeddingService::new()?;
        
        let texts = vec!["First document", "Second document", "Third document"];
        let embeddings = service.embed_many(texts).await?;
        
        assert_eq!(embeddings.len(), 3);
        for emb in &embeddings {
            assert_eq!(emb.len(), EMBEDDING_DIM);
        }
        
        Ok(())
    }

    #[tokio::test]
    async fn test_similar_texts_have_close_embeddings() -> Result<()> {
        let service = EmbeddingService::new()?;
        
        let emb1 = service.embed_one("The cat sat on the mat").await?;
        let emb2 = service.embed_one("A cat was sitting on a mat").await?;
        let emb3 = service.embed_one("Quantum physics is fascinating").await?;
        
        // Compute cosine similarity
        fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
            let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
            let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (norm_a * norm_b)
        }
        
        let sim_similar = cosine_similarity(&emb1, &emb2);
        let sim_different = cosine_similarity(&emb1, &emb3);
        
        // Similar texts should have higher similarity
        assert!(sim_similar > sim_different, 
            "Similar texts should have higher similarity: {} vs {}", sim_similar, sim_different);
        assert!(sim_similar > 0.8, "Similar texts should have similarity > 0.8, got {}", sim_similar);
        
        Ok(())
    }

    #[test]
    fn test_format_for_sql() {
        let embedding = vec![0.1, 0.2, 0.3];
        let sql = EmbeddingService::format_for_sql(&embedding);
        assert_eq!(sql, "[0.1, 0.2, 0.3]");
    }
}
