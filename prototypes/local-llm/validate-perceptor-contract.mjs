#!/usr/bin/env node
// Ticket 03 — validazione AFK-preparata del contratto del percettore su un LLM locale.
//
// Perché esiste: il verdetto del Ticket 03 (un modello locale produce il JSON del
// percettore in modo affidabile, con qualità/latenza usabili?) richiede un endpoint
// locale OpenAI-compatible IN ESECUZIONE. Non è eseguibile AFK senza server. Questo
// script è l'harness pronto all'uso: avvia il tuo server locale e lancialo.
//
// Uso:
//   1. Avvia un server locale OpenAI-compatible (LM Studio :1234 / Ollama :11434 /
//      llama-server :8080 / Unsloth Studio :PORT). Vedi docs/specs/research-unsloth-serving.md §6.
//   2. node prototypes/local-llm/validate-perceptor-contract.mjs \
//        --base http://localhost:1234/v1/chat/completions \
//        --model local-model \
//        --key lm-studio            # ometti --key per server senza auth
//        [--schema]                 # aggiunge response_format json_schema (probe strutturato)
//
// Cosa misura (mappa agli acceptance criteria del Ticket 03):
//   - il modello restituisce un oggetto JSON con le chiavi del contratto §4.4?
//   - regge SENZA response_format (fallback: estrazione blocco JSON) e CON json_schema?
//   - coerenza su piu' pagine consecutive (summary che si accumula, glossario)?
//   - latenza per pagina.
//
// NB: lo script replica il CONTRATTO del percettore (§4.4) in forma minima, non importa
// il prompt Rust reale. Quando esegui, confronta l'output con src-tauri/src/llm.rs.

import { argv, exit } from 'node:process';

function arg(name, def = undefined) {
  const i = argv.indexOf(`--${name}`);
  if (i === -1) return def;
  const v = argv[i + 1];
  return v && !v.startsWith('--') ? v : true;
}

const BASE = arg('base', 'http://localhost:1234/v1/chat/completions');
const MODEL = arg('model', 'local-model');
const KEY = arg('key', '');
const USE_SCHEMA = !!arg('schema', false);

// Tre "pagine" campione (testo breve, lingue diverse) per testare la coerenza del percettore.
const PAGES = [
  `The board convened at dawn. Captain Rhys addressed the crew about the coming storm.`,
  `Rhys ordered the mainsail reefed. The board had voted, reluctantly, to change course.`,
  `By nightfall the storm passed. The crew thanked Rhys; the board's decision had saved them.`
];

const TARGET_LANG = 'Italiano';

// Contratto del percettore (§4.4), forma minima.
const SYSTEM = `Sei un traduttore. Traduci il testo della pagina in ${TARGET_LANG} in modo coerente con
il riassunto e il glossario forniti. Rispondi con UN SOLO oggetto JSON, senza testo attorno, con le chiavi:
{"translated_text": string, "updated_summary": string, "new_glossary_terms": [{"source_term": string, "translation": string, "type": string, "note": string}]}`;

function jsonSchema() {
  return {
    type: 'json_schema',
    json_schema: {
      name: 'perceptor',
      strict: true,
      schema: {
        type: 'object',
        additionalProperties: false,
        required: ['translated_text', 'updated_summary', 'new_glossary_terms'],
        properties: {
          translated_text: { type: 'string' },
          updated_summary: { type: 'string' },
          new_glossary_terms: {
            type: 'array',
            items: {
              type: 'object',
              additionalProperties: false,
              required: ['source_term', 'translation', 'type', 'note'],
              properties: {
                source_term: { type: 'string' },
                translation: { type: 'string' },
                type: { type: 'string' },
                note: { type: 'string' }
              }
            }
          }
        }
      }
    }
  };
}

// Estrazione robusta del primo blocco JSON — specchia extract_first_json_block in llm.rs (fallback).
function extractFirstJsonBlock(text) {
  const start = text.indexOf('{');
  if (start === -1) return null;
  let depth = 0, inStr = false, esc = false;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (inStr) {
      if (esc) esc = false;
      else if (c === '\\') esc = true;
      else if (c === '"') inStr = false;
    } else if (c === '"') inStr = true;
    else if (c === '{') depth++;
    else if (c === '}') { depth--; if (depth === 0) return text.slice(start, i + 1); }
  }
  return null;
}

async function callPage(pageText, summary, glossary) {
  const messages = [
    { role: 'system', content: SYSTEM },
    {
      role: 'user',
      content: `RIASSUNTO_FINORA:\n${summary || '(vuoto)'}\n\nGLOSSARIO:\n${
        glossary.length ? glossary.map((g) => `- ${g.source_term} => ${g.translation}`).join('\n') : '(vuoto)'
      }\n\nPAGINA:\n${pageText}`
    }
  ];
  const body = { model: MODEL, messages, temperature: 0.2, stream: false };
  if (USE_SCHEMA) body.response_format = jsonSchema();

  const headers = { 'Content-Type': 'application/json' };
  if (KEY) headers['Authorization'] = `Bearer ${KEY}`;

  const t0 = Date.now();
  const res = await fetch(BASE, { method: 'POST', headers, body: JSON.stringify(body) });
  const ms = Date.now() - t0;
  const raw = await res.text();
  if (!res.ok) return { ok: false, ms, status: res.status, error: raw.slice(0, 500) };

  let content;
  try {
    content = JSON.parse(raw).choices?.[0]?.message?.content ?? '';
  } catch {
    return { ok: false, ms, status: res.status, error: `risposta non-JSON: ${raw.slice(0, 300)}` };
  }

  const block = extractFirstJsonBlock(content) ?? content;
  let parsed, parseErr = null;
  try {
    parsed = JSON.parse(block);
  } catch (e) {
    parseErr = String(e);
  }
  return { ok: true, ms, status: res.status, content, parsed, parseErr };
}

function checkContract(p) {
  if (!p) return ['parsing fallito'];
  const errs = [];
  if (typeof p.translated_text !== 'string' || !p.translated_text.trim()) errs.push('translated_text mancante/vuoto');
  if (typeof p.updated_summary !== 'string') errs.push('updated_summary mancante');
  if (!Array.isArray(p.new_glossary_terms)) errs.push('new_glossary_terms non è array');
  return errs;
}

async function main() {
  console.log(`# Validazione contratto percettore (Ticket 03)`);
  console.log(`base=${BASE} model=${MODEL} key=${KEY ? 'sì' : 'no'} json_schema=${USE_SCHEMA}\n`);

  let summary = '';
  let glossary = [];
  const times = [];
  let contractOk = 0;

  for (let i = 0; i < PAGES.length; i++) {
    console.log(`--- Pagina ${i + 1}/${PAGES.length} ---`);
    let r;
    try {
      r = await callPage(PAGES[i], summary, glossary);
    } catch (e) {
      console.log(`ERRORE rete: ${e.message}\n(Il server locale è avviato e raggiungibile su ${BASE}?)`);
      exit(1);
    }
    if (!r.ok) {
      console.log(`HTTP ${r.status} — ${r.error}`);
      if (USE_SCHEMA && /response_format|json_schema|schema/i.test(r.error || '')) {
        console.log(`↳ Il server sembra rifiutare json_schema. Riprova SENZA --schema (percorso fallback).`);
      }
      continue;
    }
    times.push(r.ms);
    const errs = checkContract(r.parsed);
    if (r.parseErr) console.log(`parse JSON: FALLITO (${r.parseErr})`);
    if (errs.length === 0 && !r.parseErr) {
      contractOk++;
      console.log(`contratto: OK  (${r.ms} ms)`);
      console.log(`  IT: ${r.parsed.translated_text.slice(0, 120)}...`);
      console.log(`  summary(${r.parsed.updated_summary.length} char): ${r.parsed.updated_summary.slice(0, 120)}...`);
      console.log(`  nuovi termini: ${r.parsed.new_glossary_terms.map((g) => g.source_term).join(', ') || '(nessuno)'}`);
      summary = r.parsed.updated_summary;
      for (const g of r.parsed.new_glossary_terms) {
        if (!glossary.find((x) => x.source_term === g.source_term)) glossary.push(g);
      }
    } else {
      console.log(`contratto: FALLITO -> ${errs.join('; ')}`);
      console.log(`  contenuto grezzo: ${String(r.content).slice(0, 300)}`);
    }
    console.log('');
  }

  const avg = times.length ? Math.round(times.reduce((a, b) => a + b, 0) / times.length) : 0;
  console.log(`=== VERDETTO ===`);
  console.log(`contratto rispettato: ${contractOk}/${PAGES.length} pagine`);
  console.log(`latenza media: ${avg} ms/pagina`);
  console.log(`coerenza: il summary si accumula? il glossario cresce senza duplicati? (ispeziona sopra)`);
  console.log(`\nAnnota questi numeri nel Ticket 03 e nel parent spec per chiudere il verdetto.`);
}

main();
