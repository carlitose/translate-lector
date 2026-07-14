# Research — Unsloth Studio & serving a local LLM over an OpenAI-compatible endpoint

**Ticket:** `docs/tickets/local-llm-provider/01-research-unsloth-serving.md`
**Parent spec:** `docs/specs/local-llm-provider-wayfinder.md`
**Date:** 2026-07-14
**Status:** research complete; one acceptance criterion (live curl call) deferred to the user (see §7).

## TL;DR

- **"Unsloth Studio" is a real, official Unsloth product** — an open-source, no-code desktop/web UI (beta) for training, running, and exporting open models locally on Windows/Linux/WSL/macOS. It is **not** just the fine-tuning library, and it is not a community fork.
- **Yes, it exposes an OpenAI-compatible HTTP endpoint** (`/v1/chat/completions`, plus an Anthropic-compatible `/v1/messages`). Under the hood it serves models — including GGUFs — through **`llama-server`** (llama.cpp).
- **Auth is mandatory** on the Studio-managed endpoint: a generated `sk-unsloth-…` bearer key. The port is **not a fixed default** in the docs (Studio boots on "whichever port it booted on", cited as *typically* `localhost:8000` or `localhost:8888`) — treat the base URL/port as **user-configurable**, do not hardcode.
- **Structured outputs (`response_format`/`json_schema`) are not documented for the Studio endpoint**, but Studio runs on `llama-server`, which **does** support `response_format` with `json_schema`/grammar-constrained decoding (with some known rough edges — see §3). Our app already degrades gracefully if `response_format` is rejected, so this is low-risk.
- **Simplest reliable path for translate-lector:** point the existing OpenAI-chat-completions client at **any** local OpenAI-compatible `/v1` base URL. The lowest-friction options on Windows are **LM Studio** (best json_schema support, dummy key) or **Ollama** (`localhost:11434/v1`, dummy key); `llama-server`/Unsloth Studio also work. The app already speaks this protocol — the only real change is making **base URL + API key configurable** instead of the hardcoded OpenRouter URL.

---

## Q1 — What is "Unsloth Studio"? Official product? Inference server or only fine-tuning?

**Unsloth Studio is an official Unsloth product** (currently Beta), announced by the Unsloth team ("we're launching Unsloth Studio"). It is an **open-source, no-code web/desktop UI** for **training, running (inference), and exporting** open models, and it runs **100% locally / offline** ("Unsloth Studio can be used 100% offline and locally on your computer"). It works on **Mac, Windows, Linux (and WSL)**, with GPU support for NVIDIA, Intel, and Apple Silicon (MLX). It advertises 500+ models across text/vision/TTS/embeddings and can "Run GGUF and safetensor models locally."

The Unsloth GitHub README frames Studio as "a web UI for training and running open models … locally" and, under inference, lists an "API inference endpoint: Deploy and run local LLMs in Claude Code, Codex tools."

So the earlier team assumption is only half-right: Unsloth is *historically* known as a fast fine-tuning/LoRA + GGUF-export library ("Train and RL 500+ models up to 2x faster with up to 70% less VRAM"), **but Unsloth Studio does now provide a first-class local inference/serving path** (via `llama-server`), not just training/export.

Sources:
- https://unsloth.ai/docs/new/studio
- https://github.com/unslothai/unsloth
- https://unsloth.ai/docs/basics/inference-and-deployment

## Q2 — Does Unsloth expose an OpenAI-compatible `/v1/chat/completions`? URL/port/auth/json_schema?

**Yes.** Unsloth Studio exposes OpenAI-compatible `/v1/chat/completions` (and `/v1/responses`) endpoints, plus an Anthropic-compatible `/v1/messages` on the same port, usable from the OpenAI SDK and OpenAI-compatible tools. Models loaded in Studio (including GGUFs) are "exposed as an authenticated API via `llama-server`."

| Property | Finding |
|---|---|
| OpenAI-compat path | `POST /v1/chat/completions` (also `/v1/responses`; Anthropic `/v1/messages`) |
| Base URL / port | **No fixed default in docs.** Studio "exposes these endpoints on whichever port it booted on (typically `http://localhost:8000` or `http://localhost:8888`)." Treat as user-configurable. |
| Auth | **Mandatory.** `Authorization: Bearer sk-unsloth-…` on every request; keys created in Studio → Settings → API; missing/invalid key → `401`. |
| `response_format` / `json_schema` | **Not documented** for the Studio endpoint. But it runs on `llama-server`, which supports `response_format`+`json_schema`/grammar (see §3), and extra flags are "forwarded directly to the underlying inference server." Not guaranteed; must be verified against the running instance. |
| Model format | GGUF (and safetensors) loaded in Studio; served by `llama-server`. |

> Caveat on the port: the "8000/8888" figure comes from doc prose hedged with "typically" and via a summarizing fetch — I could not confirm a single hardcoded default. **Design for a configurable base URL**, not a fixed port.

Sources:
- https://unsloth.ai/docs/basics/api
- https://unsloth.ai/docs/new/studio
- https://unsloth.ai/docs/basics/inference-and-deployment/llama-server-and-openai-endpoint

## Q3 — If not serving directly: the practical path to serve an Unsloth model over OpenAI-compat on Windows

Unsloth **does** serve (via Studio → `llama-server`), but the same model artifacts (GGUF export, or merged HF weights) can be served by any of these. Comparison for Windows:

| Server | OpenAI-compat `/v1/chat/completions` | Default base URL | Auth | `json_schema` structured output | Windows friendliness | How an Unsloth model gets in |
|---|---|---|---|---|---|---|
| **Unsloth Studio** (bundles `llama-server`) | Yes (+ `/v1/responses`, `/v1/messages`) | No fixed default; *typically* `http://localhost:8000` or `:8888` | **Required** `sk-unsloth-…` bearer | Inherits `llama-server` (see below); not explicitly documented | Native (it *is* the Unsloth app); no-code UI | GGUF/safetensors loaded directly in Studio |
| **llama.cpp `llama-server`** | Yes | `http://127.0.0.1:8080` (docs example uses `:8001`) | None by default (use dummy `sk-no-key-required`); optional `--api-key` | Yes: `response_format` w/ `json_schema` + GBNF grammar; **known rough edges** (e.g. json_schema sometimes ignored on `/v1/chat/completions`, or "json_schema or grammar but not both") | Prebuilt Windows binaries; CLI | GGUF (Unsloth's primary export) |
| **Ollama** | Yes (`/v1` compat layer) | `http://localhost:11434/v1` | None (send any dummy key) | Partial: native `format` param works; **OpenAI-style `response_format: json_schema` is currently ignored** on the `/v1` path | Official Windows app/installer; easiest UX | GGUF via `Modelfile` (`FROM ./model.gguf`) or pull |
| **LM Studio** | Yes | `http://localhost:1234/v1` | None (dummy key e.g. `lm-studio`) | **Best**: follows OpenAI Structured Output spec (`response_format` + `json_schema`) | First-class Windows GUI; toggle server in UI | GGUF loaded via GUI (also MLX on Mac) |
| **vLLM** | Yes (`vllm serve`, OpenAI server) | `http://localhost:8000/v1` | Optional `--api-key` | Yes (guided decoding / `response_format`) | Weak on native Windows (Linux/WSL/Docker in practice); needs CUDA GPU | Merged HF weights (safetensors); GGUF support limited |

Practical takeaway: since Unsloth's headline export is **GGUF**, the realistic serving stack on Windows is a `llama.cpp`-family server — i.e. **Unsloth Studio itself**, standalone **`llama-server`**, **Ollama**, or **LM Studio**. vLLM is the odd one out on Windows (wants Linux/WSL + a merged HF model). For guaranteed OpenAI-style `json_schema`, **LM Studio** is the strongest; for zero-config, **Ollama**.

Sources:
- https://unsloth.ai/docs/basics/inference-and-deployment/llama-server-and-openai-endpoint
- https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
- https://github.com/ggml-org/llama.cpp/issues/10732 · https://github.com/ggml-org/llama.cpp/issues/11847
- https://docs.ollama.com/api/openai-compatibility · https://github.com/ollama/ollama/issues/10001
- https://lmstudio.ai/docs/developer/openai-compat/structured-output
- https://unsloth.ai/docs/integrations/connections/vllm

## Q4 — Indicative hardware / quantization for a translation-capable 7B–14B local model

Rule of thumb: divide FP16 VRAM by ~4 for Q4, by ~2 for Q8, then add ~10–20% for the KV cache/context. **Q4_K_M GGUF** is the sweet spot (4-bit, keeps most quality, ~halves memory vs FP16).

| Model size | Quant | Approx. weights | Recommended VRAM (comfortable) | Notes |
|---|---|---|---|---|
| ~7B | Q4_K_M | ~4.7 GB | **8 GB** | Room for weights + a working context window |
| ~13–14B | Q4_K_M | ~8–9 GB | **12 GB** | 8 GB forces CPU offload → big slowdown |

Key perf cliff: the moment a model spills from GPU to system RAM, throughput collapses (e.g. a 7B at ~45 tok/s on GPU can drop to ~8 tok/s once ~10% of layers hit CPU). CPU-only / RAM inference is possible but slow; for interactive translation, keep the model fully on GPU. This is input to Ticket 04 (human decision on model + hardware target).

Sources:
- https://www.promptquorum.com/local-llms/local-llm-hardware-guide-2026
- https://localllm.in/blog/ollama-vram-requirements-for-local-llms
- https://www.spheron.network/blog/gpu-memory-requirements-llm/

## Q5 — Recommendation for translate-lector

The app **already speaks OpenAI chat-completions** and already has a graceful-degradation ladder (`ChatRequest::degrade` strips `provider` → `response_format` → `temperature`) plus robust JSON-block extraction (`parse_content` / `extract_first_json_block`) in `src-tauri/src/llm.rs`. That means it can talk to **any** OpenAI-compatible local server with **minimal change**. The only hardcoded blockers are:

- `OPENROUTER_URL` constant (`src-tauri/src/llm.rs:13`) — the endpoint is fixed to `https://openrouter.ai/api/v1/chat/completions`.
- OpenRouter-specific attribution headers (`HTTP-Referer`, `X-Title`) — harmless to a local server but should be optional.
- Error copy hardcodes "OpenRouter" (e.g. `EC03`, `LlmError::user_message`) — cosmetic.

**Recommended approach (for Ticket 02 to implement):** introduce a provider abstraction where the **base URL and API key are configurable** (the request/response shape stays identical). Keep sending `response_format`; rely on the existing degrade ladder to drop it if a local server rejects it. Then translate-lector is provider-agnostic: OpenRouter (cloud) or any local `/v1` server (Unsloth Studio, LM Studio, Ollama, `llama-server`).

**Assumed defaults the app should offer** (user-overridable — do **not** hardcode a single one):

| Provider | Assumed base URL | Assumed API key default |
|---|---|---|
| **LM Studio** (recommended for reliable `json_schema`) | `http://localhost:1234/v1/chat/completions` | any non-empty dummy, e.g. `lm-studio` |
| **Ollama** (recommended for zero-config) | `http://localhost:11434/v1/chat/completions` | any non-empty dummy |
| **llama.cpp `llama-server`** | `http://127.0.0.1:8080/v1/chat/completions` | `sk-no-key-required` (or `--api-key`) |
| **Unsloth Studio** | `http://localhost:8000/v1/chat/completions` (**verify actual port in Studio**) | **required** `sk-unsloth-…` |

Because the app's EC03 guard rejects an empty key, local providers that need no auth should default to a **dummy non-empty key** so the guard passes.

---

## §6 — Exact verification curl command (run by the user)

No local OpenAI-compatible server is running in this research environment, so the live call is **deferred to the user**. Start a local server, then run (Git Bash / PowerShell):

```bash
# LM Studio (default port 1234); swap model to whatever is loaded.
curl http://localhost:1234/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer lm-studio" \
  -d '{
    "model": "local-model",
    "messages": [
      {"role": "system", "content": "Rispondi con un solo oggetto JSON."},
      {"role": "user",   "content": "Traduci in italiano: hello world. Rispondi come {\"translated_text\": string}."}
    ],
    "temperature": 0.2,
    "max_tokens": 256,
    "stream": false
  }'
```

Variants (same body, change URL + key):
- **Ollama:** `http://localhost:11434/v1/chat/completions`, `-H "Authorization: Bearer ollama"`, `"model": "llama3.1"` (a pulled tag).
- **llama-server:** `http://127.0.0.1:8080/v1/chat/completions`, `-H "Authorization: Bearer sk-no-key-required"`.
- **Unsloth Studio:** confirm the port in Studio, `-H "Authorization: Bearer sk-unsloth-…"`.

Optional structured-output probe (add to the body; expect success on LM Studio / `llama-server`, likely ignored on Ollama's `/v1`):

```json
"response_format": { "type": "json_schema", "json_schema": { "name": "t", "strict": true,
  "schema": { "type": "object", "additionalProperties": false,
    "required": ["translated_text"], "properties": { "translated_text": { "type": "string" } } } } }
```

**Expected success shape** (must have `choices[0].message.content`, matching `ChatResponse` in `llm.rs`):

```json
{ "choices": [ { "message": { "role": "assistant", "content": "{\"translated_text\": \"ciao mondo\"}" } } ],
  "usage": { "prompt_tokens": 12, "completion_tokens": 6, "total_tokens": 18 } }
```

## §7 — What could NOT be verified AFK

1. **Live curl round-trip** — no local server exists in this environment; not fabricated. **Deferred to the user** (§6). This is the one unchecked ticket acceptance criterion.
2. **Unsloth Studio's exact default port** — docs hedge ("whichever port it booted on", *typically* 8000/8888). No single hardcoded default confirmed; must be read from the running Studio instance.
3. **Whether the Unsloth Studio endpoint honors `response_format`/`json_schema`** — not documented; it runs on `llama-server` (which does support it, with caveats), but this must be confirmed against a running instance.
4. **Some search-summary details** (e.g. exact model names like "Gemma 4 / Qwen3.6" in one search snippet) look like model-paraphrase artifacts and were **not** relied on for any conclusion; primary doc/README text was preferred.

## Sources

- https://unsloth.ai/docs/new/studio
- https://unsloth.ai/docs/basics/api
- https://unsloth.ai/docs/basics/inference-and-deployment
- https://unsloth.ai/docs/basics/inference-and-deployment/llama-server-and-openai-endpoint
- https://unsloth.ai/docs/integrations/connections · https://unsloth.ai/docs/integrations/connections/vllm
- https://github.com/unslothai/unsloth
- https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
- https://github.com/ggml-org/llama.cpp/issues/10732 · https://github.com/ggml-org/llama.cpp/issues/11847
- https://docs.ollama.com/api/openai-compatibility · https://ollama.com/blog/openai-compatibility · https://ollama.com/blog/structured-outputs · https://github.com/ollama/ollama/issues/10001
- https://lmstudio.ai/docs/developer/openai-compat · https://lmstudio.ai/docs/developer/openai-compat/structured-output
- https://www.promptquorum.com/local-llms/local-llm-hardware-guide-2026 · https://localllm.in/blog/ollama-vram-requirements-for-local-llms · https://www.spheron.network/blog/gpu-memory-requirements-llm/
