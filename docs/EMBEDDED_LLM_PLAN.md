# Embedded LLM Fallback - Implementation Plan

## Executive Summary

**Goal**: Add optional embedded LLM inference to NetGet as a fallback when external Ollama is unavailable.

**Approach**: Use `llama-cpp` Rust bindings to run GGUF models locally. Keep existing Ollama integration as primary, use embedded as fallback.

**Benefits**:
- ✅ Works offline (no network required)
- ✅ No external dependencies (Ollama installation optional)
- ✅ Deployment flexibility (embedded devices, containers, air-gapped systems)
- ✅ Performance parity with Ollama (both use llama.cpp under the hood)
- ✅ Minimal code changes (clean abstraction)

**Status**: Planning phase - awaiting approval to implement

---

## Research Findings

### Can Ollama Be Embedded?

**❌ NO** - Ollama is a Go-based HTTP service, not an embeddable library. It cannot be directly integrated into Rust code.

### Best Alternative: llama.cpp Rust Bindings

After evaluating 5+ options (see `docs/EMBEDDED_LLM_RESEARCH.md` for details), the recommended approach is:

**Primary Choice**: `llama-cpp` crate (FFI bindings to llama.cpp)
- Fastest inference (20-50 tokens/sec on CPU, 100+ on GPU)
- GGUF model support (same format Ollama uses)
- Production-proven (200K+ downloads)
- Minimal binary overhead (~10-15 MB)
- GPU support (CUDA, Metal, Intel MKL)

**Alternatives Considered**:
- ❌ Embed Ollama service - Not possible (Go service, not library)
- 🟡 mistral.rs - Good but GitHub-only, heavier dependencies
- 🟡 Candle - Pure Rust but slower on CPU
- 🟡 kalosm - Simpler API but less control

---

## Architecture Design

### High-Level Architecture

```
┌─────────────────────────────────────────────────────┐
│                    NetGet CLI                        │
└───────────────────┬─────────────────────────────────┘
                    │
                    ▼
         ┌──────────────────────┐
         │  HybridLLMManager    │
         │  (Orchestration)     │
         └──────────┬───────────┘
                    │
         ┌──────────┴──────────┐
         │                     │
         ▼                     ▼
┌──────────────────┐  ┌──────────────────────┐
│  OllamaClient    │  │ EmbeddedLLMBackend   │
│  (HTTP API)      │  │ (llama-cpp FFI)      │
│                  │  │                      │
│  - Primary       │  │  - Fallback          │
│  - Fast          │  │  - Offline           │
│  - External      │  │  - Self-contained    │
└──────────────────┘  └──────────────────────┘
         │                     │
         ▼                     ▼
┌──────────────────┐  ┌──────────────────────┐
│  Ollama Service  │  │  GGUF Model File     │
│  (localhost)     │  │  (~4 GB on disk)     │
└──────────────────┘  └──────────────────────┘
```

### Decision Flow

```
User starts NetGet
        │
        ▼
┌───────────────────────┐
│ Check Ollama health   │ (http://localhost:11434)
└───────┬───────────────┘
        │
        ├─── Available ──────► Use OllamaClient (primary)
        │
        └─── Unavailable ────► Check for embedded model
                                    │
                                    ├─── Model exists ──────► Use EmbeddedLLMBackend
                                    │
                                    └─── No model ──────────► Log error + suggest download
```

### Component Design

#### 1. HybridLLMManager (New)

```rust
// src/llm/hybrid_manager.rs

pub struct HybridLLMManager {
    primary: Option<OllamaClient>,
    fallback: Option<EmbeddedLLMBackend>,
    config: HybridConfig,
}

pub struct HybridConfig {
    pub prefer_ollama: bool,        // Default: true
    pub ollama_health_check: bool,  // Default: true
    pub fallback_on_error: bool,    // Default: true
    pub model_path: Option<PathBuf>,
}

impl HybridLLMManager {
    /// Initialize with Ollama check + optional embedded fallback
    pub async fn new(config: HybridConfig) -> Result<Self> {
        let ollama = if config.prefer_ollama {
            Self::try_init_ollama().await.ok()
        } else {
            None
        };

        let fallback = if let Some(path) = &config.model_path {
            Some(EmbeddedLLMBackend::new(path).await?)
        } else {
            None
        };

        if ollama.is_none() && fallback.is_none() {
            return Err(anyhow!("No LLM backend available"));
        }

        Ok(Self { primary: ollama, fallback, config })
    }

    /// Call LLM with fallback strategy
    pub async fn call(&self, prompt: &str) -> Result<String> {
        // Try primary (Ollama)
        if let Some(ollama) = &self.primary {
            match ollama.prompt(prompt).await {
                Ok(response) => return Ok(response),
                Err(e) if !self.config.fallback_on_error => return Err(e),
                Err(e) => {
                    warn!("Ollama failed, using embedded fallback: {}", e);
                }
            }
        }

        // Try fallback (embedded)
        if let Some(embedded) = &self.fallback {
            return embedded.prompt(prompt).await;
        }

        Err(anyhow!("All LLM backends failed"))
    }

    async fn try_init_ollama() -> Result<OllamaClient> {
        // Existing Ollama initialization code
        // Add health check: GET http://localhost:11434/api/tags
        todo!()
    }
}
```

#### 2. EmbeddedLLMBackend (New)

```rust
// src/llm/embedded_inference.rs

use llama_cpp::{LlamaModel, LlamaParams, SessionParams, StandardSampler};
use std::sync::Arc;
use std::path::Path;

pub struct EmbeddedLLMBackend {
    model: Arc<LlamaModel>,
    config: InferenceConfig,
}

pub struct InferenceConfig {
    pub context_size: usize,          // Default: 4096
    pub max_tokens: usize,            // Default: 2048
    pub temperature: f32,             // Default: 0.7
    pub top_p: f32,                   // Default: 0.9
    pub n_gpu_layers: u16,            // Default: u16::MAX (auto)
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            context_size: 4096,
            max_tokens: 2048,
            temperature: 0.7,
            top_p: 0.9,
            n_gpu_layers: u16::MAX,  // Use GPU if available
        }
    }
}

impl EmbeddedLLMBackend {
    /// Load GGUF model from disk
    pub async fn new(model_path: impl AsRef<Path>) -> Result<Self> {
        let path = model_path.as_ref();
        if !path.exists() {
            return Err(anyhow!("Model file not found: {}", path.display()));
        }

        info!("Loading embedded LLM from: {}", path.display());

        let config = InferenceConfig::default();

        // Load model (blocking operation, run in spawn_blocking)
        let model = tokio::task::spawn_blocking({
            let path = path.to_path_buf();
            let context_size = config.context_size;
            let n_gpu_layers = config.n_gpu_layers;

            move || {
                LlamaModel::load_from_file(
                    path,
                    LlamaParams::default()
                        .with_context_size(context_size)
                        .with_n_gpu_layers(n_gpu_layers)
                )
            }
        })
        .await??;

        info!("Embedded LLM loaded successfully");

        Ok(Self {
            model: Arc::new(model),
            config,
        })
    }

    /// Generate completion (streaming)
    pub async fn prompt(&self, prompt: &str) -> Result<String> {
        let model = self.model.clone();
        let prompt = prompt.to_string();
        let max_tokens = self.config.max_tokens;

        // Run inference in blocking task (CPU-intensive)
        let response = tokio::task::spawn_blocking(move || {
            let mut session = model.create_session(SessionParams::default())?;
            session.advance_context(&prompt)?;

            let completions = session.start_completing_with(
                StandardSampler::default(),
                max_tokens,
            )
            .into_strings();

            let mut result = String::new();
            for token in completions {
                result.push_str(&token);
            }

            Ok::<_, anyhow::Error>(result)
        })
        .await??;

        Ok(response)
    }

    /// Get model info (useful for debugging)
    pub fn get_model_info(&self) -> ModelInfo {
        ModelInfo {
            context_size: self.config.context_size,
            max_tokens: self.config.max_tokens,
            backend: "llama.cpp".to_string(),
        }
    }
}

#[derive(Debug)]
pub struct ModelInfo {
    pub context_size: usize,
    pub max_tokens: usize,
    pub backend: String,
}
```

#### 3. Integration with Existing LLM Code

**Minimal Changes Required**:

```rust
// src/llm/mod.rs (existing)

#[cfg(feature = "embedded-llm")]
pub mod embedded_inference;
#[cfg(feature = "embedded-llm")]
pub mod hybrid_manager;

// Replace direct OllamaClient usage with HybridLLMManager
pub async fn initialize_llm(config: LlmConfig) -> Result<Arc<HybridLLMManager>> {
    #[cfg(feature = "embedded-llm")]
    {
        let hybrid_config = HybridConfig {
            prefer_ollama: true,
            ollama_health_check: true,
            fallback_on_error: true,
            model_path: config.embedded_model_path,
        };
        HybridLLMManager::new(hybrid_config).await.map(Arc::new)
    }

    #[cfg(not(feature = "embedded-llm"))]
    {
        // Existing Ollama-only initialization
        OllamaClient::new().await.map(Arc::new)
    }
}
```

---

## Implementation Phases

### Phase 1: Foundation (Week 1)

**Goal**: Add llama-cpp dependency and basic embedded backend

**Tasks**:
1. Add `llama-cpp` to `Cargo.toml` with optional feature
2. Create `src/llm/embedded_inference.rs` with `EmbeddedLLMBackend`
3. Implement model loading (GGUF files)
4. Implement basic prompt/completion
5. Add unit tests (mock model)

**Deliverables**:
- ✅ Compiles with `--features embedded-llm`
- ✅ Can load GGUF model and generate text
- ✅ Tests pass

### Phase 2: Hybrid Manager (Week 2)

**Goal**: Create orchestration layer for Ollama + embedded fallback

**Tasks**:
1. Create `src/llm/hybrid_manager.rs` with `HybridLLMManager`
2. Implement Ollama health check on startup
3. Implement fallback logic (try Ollama → fallback to embedded)
4. Add configuration options (CLI flags)
5. Update `src/llm/mod.rs` to use hybrid manager

**Deliverables**:
- ✅ Automatic Ollama detection
- ✅ Fallback to embedded when Ollama unavailable
- ✅ Tests for both paths

### Phase 3: Model Management (Week 3)

**Goal**: User-friendly model downloading and management

**Tasks**:
1. Add slash command `/download-model <name>`
2. Implement Hugging Face Hub integration (optional `hf-hub` crate)
3. Add model listing `/list-models`
4. Add model switching `/use-model <path>`
5. Store models in `~/.netget/models/`

**Deliverables**:
- ✅ Can download popular models (Mistral, Llama, Qwen)
- ✅ User can switch models at runtime
- ✅ Models cached locally

### Phase 4: UX Polish (Week 4)

**Goal**: Improve user experience and documentation

**Tasks**:
1. Add startup banner showing LLM backend (Ollama vs Embedded)
2. Add model info to TUI footer
3. Show download progress for model downloads
4. Add comprehensive error messages
5. Write user documentation
6. Create example workflow videos

**Deliverables**:
- ✅ Clear indication of which backend is active
- ✅ User documentation in `docs/EMBEDDED_LLM_GUIDE.md`
- ✅ Example prompts work with both backends

---

## Integration Points

### Cargo.toml Changes

```toml
[features]
default = ["ollama"]
ollama = ["dep:ollama-rs"]
embedded-llm = ["dep:llama-cpp", "dep:hf-hub"]  # New feature
all-protocols = [
    # ... existing protocols ...
    "embedded-llm",  # Include in full builds
]

[dependencies]
# Existing
ollama-rs = { version = "0.3", optional = true }

# New (embedded LLM support)
llama-cpp = { version = "0.2", optional = true, features = ["cuda", "metal"] }
hf-hub = { version = "0.3", optional = true, features = ["tokio"] }
```

### CLI Flag Changes

```rust
// cli/args.rs (new flags)

#[derive(Parser)]
pub struct CliArgs {
    // ... existing flags ...

    #[cfg(feature = "embedded-llm")]
    #[arg(long, help = "Use embedded LLM instead of Ollama")]
    pub use_embedded: bool,

    #[cfg(feature = "embedded-llm")]
    #[arg(long, help = "Path to GGUF model file")]
    pub embedded_model: Option<PathBuf>,

    #[cfg(feature = "embedded-llm")]
    #[arg(long, help = "Disable Ollama fallback (embedded only)")]
    pub no_ollama: bool,
}
```

### Startup Flow Changes

```rust
// main.rs (updated)

#[tokio::main]
async fn main() -> Result<()> {
    let args = CliArgs::parse();

    // Initialize LLM backend
    #[cfg(feature = "embedded-llm")]
    let llm_manager = {
        let config = HybridConfig {
            prefer_ollama: !args.use_embedded && !args.no_ollama,
            ollama_health_check: true,
            fallback_on_error: true,
            model_path: args.embedded_model,
        };
        HybridLLMManager::new(config).await?
    };

    #[cfg(not(feature = "embedded-llm"))]
    let llm_manager = OllamaClient::new().await?;

    // Log which backend is active
    match llm_manager.active_backend() {
        LlmBackend::Ollama => info!("Using Ollama backend"),
        LlmBackend::Embedded => info!("Using embedded LLM backend"),
    }

    // Continue with existing startup...
}
```

---

## User Experience

### Scenario 1: User Has Ollama (Current Behavior)

```bash
$ cargo run -- --server tcp
[INFO] Checking Ollama connection...
[INFO] ✓ Ollama available at http://localhost:11434
[INFO] Using Ollama backend with model: qwen3-coder:30b
[INFO] Starting TCP server on 0.0.0.0:8000...
```

**No change** - works exactly as before.

### Scenario 2: User Has No Ollama, No Embedded Model

```bash
$ cargo run -- --server tcp
[INFO] Checking Ollama connection...
[WARN] Ollama not available (connection refused)
[ERROR] No embedded LLM model configured
[ERROR]
[ERROR] To use NetGet without Ollama, download a model:
[ERROR]   1. Download GGUF model manually:
[ERROR]      wget https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.1-GGUF/resolve/main/Mistral-7B-Instruct-v0.1.Q4_K_M.gguf
[ERROR]
[ERROR]   2. Run NetGet with embedded model:
[ERROR]      cargo run --features embedded-llm -- --server tcp --embedded-model ./Mistral-7B-Instruct-v0.1.Q4_K_M.gguf
[ERROR]
[ERROR] Or install Ollama: https://ollama.com/download
```

**Action**: User downloads model or installs Ollama.

### Scenario 3: User Has No Ollama, But Has Embedded Model

```bash
$ cargo run --features embedded-llm -- --server tcp --embedded-model ~/.netget/models/mistral-7b.Q4_K_M.gguf
[INFO] Checking Ollama connection...
[WARN] Ollama not available (connection refused)
[INFO] Loading embedded LLM from: ~/.netget/models/mistral-7b.Q4_K_M.gguf
[INFO] ✓ Embedded LLM loaded successfully (4.3 GB, 4096 context)
[INFO] Using embedded LLM backend
[INFO] Starting TCP server on 0.0.0.0:8000...
```

**Works offline** - no network needed.

### Scenario 4: User Wants to Download Model via Slash Command

```bash
$ cargo run --features embedded-llm -- --server tcp
[WARN] Ollama not available
[ERROR] No embedded model configured
[INFO] Starting TUI...

> /download-model mistral-7b

[INFO] Downloading Mistral-7B-Instruct-v0.1-GGUF from Hugging Face...
[INFO] Progress: 1.2 GB / 4.3 GB (28%) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[INFO] Progress: 2.5 GB / 4.3 GB (58%) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[INFO] Progress: 4.3 GB / 4.3 GB (100%) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[INFO] ✓ Model saved to: ~/.netget/models/mistral-7b-instruct-v0.1.Q4_K_M.gguf
[INFO] Loading model...
[INFO] ✓ Embedded LLM ready

> /list-models

Available models:
  * mistral-7b-instruct-v0.1.Q4_K_M.gguf (active) - 4.3 GB
  - llama-2-7b-chat.Q4_K_M.gguf - 3.8 GB

> /use-model llama-2-7b-chat.Q4_K_M.gguf

[INFO] Switching to model: llama-2-7b-chat.Q4_K_M.gguf
[INFO] ✓ Model loaded successfully
```

**Convenient** - download and manage models without leaving NetGet.

---

## Technical Considerations

### Memory Usage

| Configuration | RAM Required | Disk Space |
|---------------|--------------|------------|
| Ollama only | ~100 MB | ~500 MB (Ollama binary) |
| Embedded 7B Q4 | 8-12 GB | 4.3 GB (model file) |
| Embedded 7B Q5 | 10-14 GB | 5.2 GB (model file) |
| Embedded 13B Q4 | 16-20 GB | 8.1 GB (model file) |

**Recommendation**: Start with 7B Q4 models (Mistral, Llama-2, Qwen3).

### Performance Comparison

| Backend | Tokens/Sec (CPU) | Tokens/Sec (GPU) | Latency (First Token) |
|---------|------------------|------------------|-----------------------|
| Ollama HTTP | 15-25 | 80-120 | 500-1000ms |
| Embedded (llama.cpp) | 15-25 | 80-120 | 200-400ms |

**Note**: Both use llama.cpp under the hood, so performance is nearly identical. Embedded has lower latency (no HTTP overhead).

### Binary Size Impact

```
Release binary size:
- Current (Ollama only): ~30 MB
- With embedded-llm feature: ~45 MB (+15 MB)

Dependencies added:
- llama-cpp: ~10 MB (C++ bindings)
- hf-hub: ~5 MB (model downloading)
```

**Impact**: Acceptable for the flexibility gained.

### GPU Support

```rust
// Auto-detect GPU and use if available
let params = LlamaParams::default()
    .with_n_gpu_layers(u16::MAX);  // Use all GPU layers if available

// Force CPU only
let params = LlamaParams::default()
    .with_n_gpu_layers(0);

// Manual GPU layer count
let params = LlamaParams::default()
    .with_n_gpu_layers(33);  // Offload 33 layers to GPU
```

**Platforms**:
- ✅ NVIDIA (CUDA) - Best performance
- ✅ Apple (Metal) - M1/M2/M3 Macs
- ✅ Intel (MKL) - CPU optimization
- ✅ CPU fallback - Always available

---

## Testing Strategy

### Unit Tests

```rust
// tests/llm/embedded_inference_test.rs

#[cfg(all(test, feature = "embedded-llm"))]
mod embedded_llm_tests {
    use super::*;

    #[tokio::test]
    async fn test_load_model() {
        let backend = EmbeddedLLMBackend::new("./tests/fixtures/tiny-model.gguf")
            .await
            .expect("Failed to load model");

        assert!(backend.get_model_info().context_size > 0);
    }

    #[tokio::test]
    async fn test_prompt_completion() {
        let backend = EmbeddedLLMBackend::new("./tests/fixtures/tiny-model.gguf")
            .await
            .expect("Failed to load model");

        let response = backend.prompt("Say hello").await.expect("Prompt failed");
        assert!(!response.is_empty());
    }

    #[tokio::test]
    async fn test_hybrid_fallback() {
        let config = HybridConfig {
            prefer_ollama: true,
            ollama_health_check: false,  // Skip health check (Ollama won't be available in test)
            fallback_on_error: true,
            model_path: Some("./tests/fixtures/tiny-model.gguf".into()),
        };

        let manager = HybridLLMManager::new(config).await.expect("Failed to create manager");

        // Should use embedded fallback since Ollama unavailable
        let response = manager.call("Test prompt").await.expect("Call failed");
        assert!(!response.is_empty());
    }
}
```

### E2E Tests

```bash
# Test with embedded LLM
./test-e2e.sh tcp --use-embedded --model ./models/mistral-7b.Q4_K_M.gguf

# Test with Ollama (existing)
./test-e2e.sh tcp --use-ollama
```

### Test Models

**Tiny Models for CI** (~100 MB):
- TinyLlama-1.1B-Chat-v1.0.Q4_K_M.gguf (600 MB)
- Phi-2-Q4_K_M.gguf (1.6 GB)

**Production Models** (4-5 GB):
- Mistral-7B-Instruct-v0.1.Q4_K_M.gguf (4.3 GB)
- Llama-2-7B-Chat.Q4_K_M.gguf (3.8 GB)

---

## Slash Commands

### New Commands

```
/download-model <name>        Download popular GGUF model
/list-models                  List available models
/use-model <path>             Switch to different model
/model-info                   Show current model information
/backend-status               Show active LLM backend (Ollama vs Embedded)
```

### Implementation

```rust
// cli/slash_commands.rs (new)

pub async fn handle_download_model(name: &str, status_tx: Sender<String>) -> Result<()> {
    let model_url = match name {
        "mistral-7b" => "https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.1-GGUF/resolve/main/Mistral-7B-Instruct-v0.1.Q4_K_M.gguf",
        "llama-2-7b" => "https://huggingface.co/TheBloke/Llama-2-7B-Chat-GGUF/resolve/main/Llama-2-7B-Chat.Q4_K_M.gguf",
        "qwen-7b" => "https://huggingface.co/Qwen/Qwen-7B-Chat-GGUF/resolve/main/qwen-7b-chat-q4_k_m.gguf",
        _ => return Err(anyhow!("Unknown model: {}", name)),
    };

    let models_dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot find home directory"))?
        .join(".netget/models");

    std::fs::create_dir_all(&models_dir)?;

    let filename = model_url.split('/').last().unwrap();
    let output_path = models_dir.join(filename);

    status_tx.send(format!("Downloading {} from Hugging Face...", name)).await?;

    // Use reqwest for streaming download with progress
    let client = reqwest::Client::new();
    let response = client.get(model_url).send().await?;
    let total_size = response.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(&output_path).await?;
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        let progress = (downloaded as f64 / total_size as f64 * 100.0) as u32;
        status_tx.send(format!(
            "Progress: {:.1} GB / {:.1} GB ({}%)",
            downloaded as f64 / 1_000_000_000.0,
            total_size as f64 / 1_000_000_000.0,
            progress
        )).await?;
    }

    status_tx.send(format!("✓ Model saved to: {}", output_path.display())).await?;

    Ok(())
}
```

---

## Alternatives Considered

### 1. Embed Ollama Service

**Why not**: Ollama is written in Go, requires separate process, cannot be linked as library.

### 2. Use mistral.rs Instead of llama.cpp

**Pros**: Higher-level API, more features (prefix caching, speculative decoding)
**Cons**: GitHub-only (not on crates.io), heavier dependencies, slightly slower

**Decision**: llama.cpp is simpler, faster, and more widely adopted.

### 3. Pure Rust (Candle) Instead of C++ FFI

**Pros**: No C++ build dependency, pure Rust safety
**Cons**: 15-20% slower on CPU, larger binary size

**Decision**: Performance matters for real-time protocol control. llama.cpp is worth the FFI overhead.

### 4. Require External Model Download

**Pros**: Smaller binary, user controls model choice
**Cons**: Worse UX, extra setup step

**Decision**: Offer built-in download via slash commands for convenience.

---

## Recommended Models

### Tier 1: Best Quality/Speed Balance

1. **Mistral-7B-Instruct-v0.1 (Q4_K_M)** - 4.3 GB
   - Best overall performance
   - Fast inference (20-30 tok/s CPU)
   - Good instruction following

2. **Qwen3-Coder-7B (Q4_K_M)** - 4.5 GB
   - Best for technical tasks
   - Strong code understanding
   - Good with protocol specs

### Tier 2: Smallest Models

3. **TinyLlama-1.1B (Q4_K_M)** - 600 MB
   - Fastest inference (50+ tok/s CPU)
   - Lowest memory (4 GB RAM)
   - Acceptable quality for simple tasks

4. **Phi-3-Mini (Q4_K_M)** - 2.2 GB
   - Surprisingly capable for size
   - Microsoft-trained
   - Good for resource-constrained systems

### Tier 3: Largest Quality

5. **Llama-2-13B-Chat (Q4_K_M)** - 8.1 GB
   - Best quality
   - Requires 16+ GB RAM
   - Slower inference (8-12 tok/s CPU)

---

## Deployment Scenarios

### Scenario 1: Development (Ollama Preferred)

```bash
# Standard development setup
cargo run -- --server tcp

# Uses Ollama by default (faster iteration, easy model switching)
```

### Scenario 2: Production Container (Embedded)

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release --features embedded-llm

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/netget /usr/local/bin/
COPY ./models/mistral-7b.Q4_K_M.gguf /models/
ENTRYPOINT ["netget", "--embedded-model", "/models/mistral-7b.Q4_K_M.gguf"]
```

### Scenario 3: Air-Gapped System (Embedded Only)

```bash
# On internet-connected system
wget https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.1-GGUF/resolve/main/Mistral-7B-Instruct-v0.1.Q4_K_M.gguf

# Transfer binary + model to air-gapped system
scp netget mistral-7b.Q4_K_M.gguf airgap-host:/opt/netget/

# Run on air-gapped system
./netget --embedded-model ./mistral-7b.Q4_K_M.gguf --server tcp
```

### Scenario 4: Raspberry Pi / Edge Device (Embedded)

```bash
# Use small model for low-memory device
cargo build --release --features embedded-llm --target armv7-unknown-linux-gnueabihf

./netget --embedded-model ./tinyllama-1.1b.Q4_K_M.gguf --server tcp
```

---

## Documentation Requirements

### User-Facing Docs

1. **docs/EMBEDDED_LLM_GUIDE.md**
   - What is embedded LLM?
   - When to use Ollama vs embedded?
   - Model downloading guide
   - Performance tuning (GPU, quantization)
   - Troubleshooting

2. **README.md updates**
   - Add embedded LLM section
   - Update installation instructions
   - Add CLI flag examples

3. **CLAUDE.md updates**
   - Document new feature flag
   - Update build instructions
   - Add testing guidance

### Developer Docs

1. **src/llm/embedded_inference.rs CLAUDE.md**
   - Architecture decisions
   - llama-cpp integration details
   - Performance characteristics
   - Future improvements

2. **tests/llm/CLAUDE.md**
   - Testing strategy for embedded LLM
   - Mock model requirements
   - E2E test approach
   - CI/CD considerations

---

## Timeline & Effort Estimate

### Phase 1: Foundation (1 week)
- 3 days: llama-cpp integration, basic inference
- 2 days: Unit tests, error handling
- 1 day: Documentation

### Phase 2: Hybrid Manager (1 week)
- 3 days: HybridLLMManager implementation
- 2 days: Ollama health check, fallback logic
- 1 day: Integration with existing code

### Phase 3: Model Management (1 week)
- 3 days: Slash commands (/download-model, /list-models)
- 2 days: Hugging Face Hub integration
- 1 day: UX polish (progress bars, etc.)

### Phase 4: Testing & Docs (1 week)
- 3 days: E2E testing with real models
- 2 days: User documentation
- 1 day: Example workflows

**Total Estimated Effort**: 4 weeks (1 month)

---

## Success Criteria

### Must Have
- ✅ Works without Ollama installed
- ✅ Automatic fallback when Ollama unavailable
- ✅ Model downloading via slash command
- ✅ Compiles with `--features embedded-llm`
- ✅ E2E tests pass with both backends
- ✅ Performance parity with Ollama (both use llama.cpp)

### Nice to Have
- 🟡 Model caching (avoid re-downloading)
- 🟡 Model pruning (delete unused models)
- 🟡 Auto-update models (check for newer versions)
- 🟡 Multi-model support (switch at runtime)
- 🟡 Web UI for model management

### Won't Have (Out of Scope)
- ❌ Training/fine-tuning (use external tools)
- ❌ Model quantization (use pre-quantized GGUF files)
- ❌ Custom model formats (GGUF only)
- ❌ Distributed inference (single-machine only)

---

## Open Questions

1. **Should we bundle a tiny model with the binary?**
   - Pros: Zero setup, works out of the box
   - Cons: Larger binary (600 MB+), licensing concerns
   - **Recommendation**: No, require manual download

2. **Should we auto-download models on first run?**
   - Pros: Better UX, no manual steps
   - Cons: Unexpected network usage (4+ GB), slow first startup
   - **Recommendation**: Prompt user with instructions, don't auto-download

3. **Should we support multiple models loaded simultaneously?**
   - Pros: Can compare responses, use specialized models
   - Cons: 2x memory usage (16+ GB for two 7B models)
   - **Recommendation**: Phase 5 feature, not initial scope

4. **Should we use mmap for model loading?**
   - Pros: Lower memory usage, faster startup
   - Cons: llama-cpp handles this automatically
   - **Recommendation**: Use llama-cpp defaults (already uses mmap)

---

## Next Steps

1. **Approval**: Review plan with stakeholders
2. **Prototype**: Build minimal POC (Phase 1)
3. **Validate**: Test with real protocols (TCP, HTTP, DNS)
4. **Iterate**: Refine based on feedback
5. **Document**: Write user and developer guides
6. **Release**: Ship as optional feature in v0.x

---

## References

- llama.cpp: https://github.com/ggml-org/llama.cpp
- llama-cpp Rust bindings: https://crates.io/crates/llama-cpp
- GGUF format spec: https://github.com/ggml-org/ggml/blob/master/docs/gguf.md
- Hugging Face Hub: https://huggingface.co/docs/hub/index
- TheBloke GGUF models: https://huggingface.co/TheBloke?search=GGUF

---

**Status**: 📋 PLANNING PHASE - Ready for review
**Last Updated**: 2025-01-18
**Author**: Claude Code (Anthropic)
