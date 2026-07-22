use fastembed::{
    InitOptionsUserDefined, Pooling, QuantizationMode, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub(crate) const MODEL_ID: &str = "BAAI/bge-small-zh-v1.5";
pub(crate) const MODEL_DIMENSIONS: usize = 512;

const MODEL_FILES: [(&str, &str); 5] = [
    (
        "config.json",
        "d4193ead3a810fd694fa8a31d7fc72fbaebc0668b603e398734bf2f6538ff42f",
    ),
    (
        "model_quantized.onnx",
        "b9837c19ce154ff0726d398ee77abbc03a7faf0476c6f93016c84e531be7ebb5",
    ),
    (
        "special_tokens_map.json",
        "b6d346be366a7d1d48332dbc9fdf3bf8960b5d879522b7799ddba59e76237ee3",
    ),
    (
        "tokenizer.json",
        "48cea5d44424912a6fd1ea647bf4fe50b55ab8b1e5879c3275f80e339e8fae26",
    ),
    (
        "tokenizer_config.json",
        "e6f3b96db926a37d4039995fbf5ad17de158dfb8f6343d607e4dbaad18d75f5a",
    ),
];

struct EmbeddingRuntime {
    model_dir: PathBuf,
    model: Mutex<TextEmbedding>,
}

static EMBEDDING_RUNTIME: OnceLock<Result<EmbeddingRuntime, String>> = OnceLock::new();

pub(crate) fn model_status(model_dir: &Path) -> Value {
    match verify_model_files(model_dir) {
        Ok(total_bytes) => json!({
            "supported": true,
            "ready": true,
            "backend": "fastembed",
            "model_id": MODEL_ID,
            "dimensions": MODEL_DIMENSIONS,
            "model_dir": model_dir.display().to_string(),
            "model_bytes": total_bytes,
            "verified": true
        }),
        Err(error) => json!({
            "supported": true,
            "ready": false,
            "backend": "fastembed",
            "model_id": MODEL_ID,
            "dimensions": MODEL_DIMENSIONS,
            "model_dir": model_dir.display().to_string(),
            "verified": false,
            "error": error
        }),
    }
}

pub(crate) fn embed(model_dir: &Path, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let runtime = EMBEDDING_RUNTIME.get_or_init(|| initialize_runtime(model_dir));
    let runtime = runtime.as_ref().map_err(Clone::clone)?;
    if runtime.model_dir != model_dir {
        return Err("embedding model directory changed after initialization".to_string());
    }
    let mut model = runtime
        .model
        .lock()
        .map_err(|_| "embedding model lock is poisoned".to_string())?;
    let vectors = model
        .embed(texts, Some(1))
        .map_err(|error| format!("failed to generate local embeddings: {error}"))?;
    vectors
        .into_iter()
        .map(normalize_vector)
        .collect::<Result<Vec<_>, _>>()
}

fn initialize_runtime(model_dir: &Path) -> Result<EmbeddingRuntime, String> {
    verify_model_files(model_dir)?;
    let tokenizer_files = TokenizerFiles {
        tokenizer_file: read_model_file(model_dir, "tokenizer.json")?,
        config_file: read_model_file(model_dir, "config.json")?,
        special_tokens_map_file: read_model_file(model_dir, "special_tokens_map.json")?,
        tokenizer_config_file: read_model_file(model_dir, "tokenizer_config.json")?,
    };
    let user_model = UserDefinedEmbeddingModel::new(
        read_model_file(model_dir, "model_quantized.onnx")?,
        tokenizer_files,
    )
    .with_pooling(Pooling::Mean)
    .with_quantization(QuantizationMode::Dynamic);
    let model = TextEmbedding::try_new_from_user_defined(
        user_model,
        InitOptionsUserDefined::new()
            .with_max_length(512)
            .with_intra_threads(2),
    )
    .map_err(|error| format!("failed to initialize local embedding model: {error}"))?;
    Ok(EmbeddingRuntime {
        model_dir: model_dir.to_path_buf(),
        model: Mutex::new(model),
    })
}

fn verify_model_files(model_dir: &Path) -> Result<u64, String> {
    let mut total_bytes = 0u64;
    for (name, expected_hash) in MODEL_FILES {
        let path = model_dir.join(name);
        let bytes = fs::read(&path)
            .map_err(|error| format!("missing embedding model file {}: {error}", path.display()))?;
        let actual_hash = sha256_hex(&bytes);
        if actual_hash != expected_hash {
            return Err(format!(
                "embedding model checksum mismatch for {name}: expected {expected_hash}, got {actual_hash}"
            ));
        }
        total_bytes = total_bytes.saturating_add(bytes.len() as u64);
    }
    Ok(total_bytes)
}

fn read_model_file(model_dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    let path = model_dir.join(name);
    fs::read(&path).map_err(|error| format!("failed to read {}: {error}", path.display()))
}

fn normalize_vector(mut vector: Vec<f32>) -> Result<Vec<f32>, String> {
    if vector.len() != MODEL_DIMENSIONS {
        return Err(format!(
            "embedding dimension mismatch: expected {MODEL_DIMENSIONS}, got {}",
            vector.len()
        ));
    }
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::EPSILON {
        return Err("embedding model returned an invalid zero vector".to_string());
    }
    for value in &mut vector {
        *value /= norm;
    }
    Ok(vector)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_incomplete_model_directory() {
        let path = std::env::temp_dir().join("gp-assistant-missing-embedding-model");
        let status = model_status(&path);
        assert_eq!(status["ready"], false);
        assert_eq!(status["verified"], false);
    }

    #[test]
    fn local_release_model_produces_normalized_512d_vectors_when_available() {
        let model_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/bge-small-zh-v1.5-int8");
        if !model_dir.join("model_quantized.onnx").exists() {
            return;
        }
        let vectors = embed(&model_dir, &["储能订单增长".to_string()])
            .expect("verified local model should embed Chinese text");
        assert_eq!(vectors.len(), 1);
        assert_eq!(vectors[0].len(), MODEL_DIMENSIONS);
        let norm = vectors[0]
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }
}
