# 01 — Unsloth Studio: come serve un LLM locale (endpoint/protocollo/auth)

## Parent Spec

[local-llm-provider-wayfinder.md](../../specs/local-llm-provider-wayfinder.md)

## Type

research

## Outcome

Una comprensione documentata e verificata di **cos'è Unsloth Studio** e **come serve un modello per
inferenza**: espone un endpoint HTTP OpenAI-compatible? Su quale URL/porta di default? Richiede auth?
Quale formato modello (GGUF / vLLM / transformers)? Come si avvia il server? Con la conclusione su come
translate-lector può collegarcisi.

## Acceptance Criteria

- [ ] Chiarito cos'è "Unsloth Studio" (prodotto/tooling) e se offre un server di inferenza o solo
      fine-tuning/export. Fonti citate (docs ufficiali, repo).
- [ ] Documentato **se e come** espone un endpoint HTTP **OpenAI-compatible** (`/v1/chat/completions`):
      URL/porta di default, header/auth richiesti (key opzionale?), supporto a `response_format`/`json_schema`.
- [ ] Se Unsloth **non** serve direttamente: identificato il percorso pratico (es. export GGUF → llama.cpp
      server / Ollama / LM Studio / vLLM) che espone OpenAI-compat, con pro/contro.
- [ ] Verificato con una **chiamata reale** (curl/spike) a un endpoint locale OpenAI-compatible che
      restituisce una chat-completion — anche con un modello segnaposto se Unsloth non è ancora pronto.
- [ ] Nota su formato modello, quantizzazione tipica e requisiti hardware indicativi (input al Ticket 04).

## Blocked By

- None — can start immediately.

## Frontier

È l'edge che sblocca tutto: l'astrazione di provider (02) e la validazione del contratto (03) dipendono dal
sapere endpoint, protocollo e auth reali del serving locale.

## Work Plan

1. `find-docs` / ricerca su Unsloth: capacità di inferenza/serving vs fine-tuning; eventuale "Studio".
2. Determinare il percorso di serving OpenAI-compatible più semplice sul setup dell'utente (Windows).
3. Fare una chiamata `curl` reale a `/v1/chat/completions` locale e catturare la risposta.
4. Documentare endpoint/porta/auth/formato/`json_schema` support e la conclusione di integrazione nel
   parent spec ("Not Yet Specified" → "Decisions So Far" del blocco serving).

## Evidence to Capture

- Link a docs/repo ufficiali di Unsloth.
- Comando `curl` e risposta JSON reale dall'endpoint locale.
- Tabella: endpoint, porta, auth, formato modello, supporto `response_format`.
- Requisiti hardware indicativi.

## Out of Scope

- Modifiche all'app (spetta al Ticket 02).
- Validazione del contratto percettore (Ticket 03).
- Fine-tuning del modello.
