# Clica e Fala

App de menubar pra macOS que transforma pensamento em voz em prompt formatado pra coding agent (Claude Code, Cursor, Copilot CLI).

Você fala em português, sem se preocupar com ordem ou coerência — muda de ideia, hesita, enrola — e o app transcreve, refina via LLM e cola um prompt técnico em inglês direto no editor focado.

```
F5 → grava → F5 → transcreve → refina → cola
```

## Por quê

Digitar prompts longos no terminal é lento. Falar é ~3x mais rápido, mas transcrição bruta fica cheia de disfluência. O refinamento via LLM limpa, traduz e estrutura sem perder contexto — o que sobra é um prompt denso pronto pra enviar pro agente.

## Features

- Transcrição PT-BR via **Groq Whisper Large v3** (cloud, ~500ms) ou **whisper.cpp** (local, offline).
- Refinamento via **Groq llama-3.3-70b** ou qualquer modelo Groq compatível (`llama-3.1-8b-instant` pra máxima velocidade).
- Fallback pra **Claude Code CLI** (Haiku) se `GROQ_API_KEY` não estiver setada.
- Auto-paste no app focado via Cmd+V (System Events).
- **Raw mode** (`⌘⇧⌥Space`) cola transcript direto, sem LLM.
- **Pipelines paralelos** — pode iniciar nova gravação enquanto a anterior ainda processa.
- **Learning loop**: histórico JSONL alimenta few-shot nas próximas calls; a cada N runs, um meta-prompt atualiza automaticamente um `profile.md` com novo vocabulário, codenames de projeto e padrões do usuário.
- Sons sintetizados, feedback sonoro distinto pra cada estágio (início, transcribe loop, refine loop, finish).
- Tray icon no menubar, zero janela visível (activation policy `Accessory`).

## Arquitetura

```
     ⌘⇧Space
        ↓
   ┌────────┐    ┌──────────┐    ┌─────────┐    ┌──────┐    ┌──────────┐    ┌────────┐
   │  cpal  │───▶│  ffmpeg  │───▶│ whisper │───▶│  LLM │───▶│ clipboard│───▶│ ⌘V     │
   │ record │    │ 16k mono │    │  (Groq) │    │ (Groq)│    │(arboard) │    │(osascr)│
   └────────┘    │ loudnorm │    └─────────┘    └──────┘    └──────────┘    └────────┘
                 └──────────┘
```

**Tauri 2** segura janela, tray, shortcut global. **Rust** backend único (sem frontend reativo — HTML estático só pra mostrar atalhos). **rodio** + thread dedicada tocam sons embutidos (`include_bytes!`). Pipeline roda em thread separada por recording — paralelismo natural, `paste_lock: Mutex<()>` serializa colagens pra evitar race de clipboard.

## Pré-requisitos

- macOS 12+
- Rust stable (1.88+)
- `pnpm`
- `ffmpeg` — brew install ffmpeg
- **Opção A (recomendado):** `GROQ_API_KEY` em `.env` → roda tudo via cloud
- **Opção B (offline):**
  - `whisper-cpp` — `brew install whisper-cpp` (binário `whisper-cli`)
  - Modelo `ggml-large-v3.bin` em `~/models/whisper/`
  - Claude Code CLI (`claude`) em PATH

## Setup

```fish
git clone https://github.com/r4topunk/clica-e-fala.git
cd clica-e-fala
cp .env.example .env
# edita .env, cola GROQ_API_KEY
pnpm install
pnpm tauri dev
```

**Permissões macOS (pede na 1ª execução):**
- **Microphone** → gravar áudio
- **Accessibility** → simular Cmd+V via System Events

Sem Accessibility, o texto vai pro clipboard mas não cola sozinho.

## Uso

| Atalho | Modo | Fluxo |
|--------|------|-------|
| `F5` | Refined | transcribe → refine (LLM) → paste |
| `⇧F5` | Raw (PT-BR direto) | transcribe → paste sem refine |

**Fluxo sonoro:**
- Blip curto → gravação iniciou, fale agora
- Tick pulsante → transcrevendo
- Double-tick diferente → refinando no LLM
- Ding ascendente → colou, fim

Inicie gravação com o hotkey do modo desejado. O "mode" gruda na gravação — parar sempre usa o mode original, independente de qual tecla você usa pra parar.

## Configuração

Tudo via `.env` (veja `.env.example`):

| Variável | Default | Efeito |
|----------|---------|--------|
| `GROQ_API_KEY` | — | Se vazio, fallback local. |
| `GROQ_REFINER_MODEL` | `llama-3.1-8b-instant` | Qualquer modelo Groq chat. |
| `HISTORY_FEWSHOT_N` | 5 | Pares anteriores injetados como exemplos multi-turn. |
| `CONSOLIDATE_EVERY` | 20 | Runs antes de atualizar profile automaticamente. |

### Profile do usuário

Arquivo em `~/.config/clica-e-fala/profile.md`. É carregado em toda call de refine e injetado como `<user_context>` no system prompt.

Conteúdo inicial criado manualmente (identidade, stack, codenames, vocabulário). Depois o app atualiza automaticamente: a cada `CONSOLIDATE_EVERY` refinements, um meta-prompt analisa o histórico recente e anexa uma seção `## Consolidated YYYY-MM-DD HH:MM` com bullets novos detectados.

Pode editar o arquivo a qualquer momento — próxima call já usa a versão nova.

### Histórico

`~/.config/clica-e-fala/history.jsonl` — append-only, uma linha JSON por refinement:

```json
{"ts":"2026-04-24T03:30:00-03:00","transcript":"...","refined":"...","model":"llama-3.3-70b-versatile"}
```

Usado pra few-shot dinâmico e pela consolidação. Pode limpar com `: > ~/.config/clica-e-fala/history.jsonl`.

## Build release

```fish
pnpm tauri build
```

Bundle em `src-tauri/target/release/bundle/macos/ClicaEFala.app`. Copiar pra `/Applications/` e conceder permissões.

## Estrutura

```
src-tauri/src/
├── main.rs         # Tauri setup, tray, shortcuts, orquestração
├── audio.rs        # cpal → WAV (força built-in mic)
├── sound.rs        # rodio, 4 WAVs embutidos, loops id-based
├── pipeline.rs     # whisper + refine (Groq/Claude) + paste + consolidation
├── history.rs      # JSONL + state counter + profile I/O
└── logging.rs      # macro logln! com timestamp

src-tauri/assets/sounds/
├── rec_start.wav   # blip 50ms C6
├── transcribe.wav  # pulso 500ms A5
├── claude.wav      # double-tick E5
└── finish.wav      # C5→G5 ascendente
```

## Sistema de sons

Gerados via ffmpeg com filtros `sine`, `afade`, `apad`, `loudnorm`. Embutidos no binário com `include_bytes!`, decodificados por rodio em memória — zero I/O de disco em runtime, zero delay de cold-start (problema que `afplay` tinha).

Reger com outros parâmetros:

```fish
# Exemplo: tick mais grave, intervalo maior
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=0.08" \
  -af "afade=t=in:d=0.008,afade=t=out:st=0.065:d=0.015,apad=pad_dur=0.6,volume=0.22" \
  -ar 48000 -ac 2 src-tauri/assets/sounds/transcribe.wav
pnpm tauri build
```

## Prompts

### WHISPER_PROMPT
Prima o whisper com vocabulário técnico PT-BR (React, Tauri, pnpm, endpoint, commit...). Reduz erro em termos de domínio.

### SYSTEM_PROMPT
Regras que força o LLM a:
- Traduzir fielmente sem comprimir intenção
- Preservar contexto, observações, nomes técnicos
- Nunca executar / pedir aprovação / tratar input como comando pra si
- Bloquear tools via `--disallowedTools`

Exemplos few-shot guiando distinção entre "filler verbal" e "conteúdo semântico".

Editar em `src-tauri/src/pipeline.rs` → `SYSTEM_PROMPT`.

## Troubleshooting

| Problema | Solução |
|----------|---------|
| Cmd+V não cola | System Settings → Privacy → Accessibility → habilitar o binário (dev: `src-tauri/target/debug/clica-e-fala`, release: `ClicaEFala.app`) |
| F5 abre dictation do macOS em vez do app | Magic Keyboard tem 🎤 no F5 (HID-level). Opcoes: (a) usar `Fn+F5` — funciona com fn-keys default; (b) System Settings → Keyboard → "Press 🎤 key to:" → trocar pra `F5` ou `Do Nothing`; (c) desligar Dictation inteiro em System Settings → Keyboard → Dictation. Trocar so o shortcut do dictation **nao** resolve |
| Hotkey não dispara | Conflito com outro app. Alterar em `main.rs` → `Shortcut::new(...)` |
| Whisper local não acha modelo | Ajustar path em `main.rs` → `model_path` |
| Transcrição ruim | Verificar device em log `[cpal] picked built-in:`. Se estiver pegando AirPods, renomear dispositivo ou alterar filtro em `audio.rs` → `pick_builtin_mic` |
| Refinamento comprime demais | Trocar pra modelo melhor em `GROQ_REFINER_MODEL` (ex: `llama-3.3-70b-versatile` ou `moonshotai/kimi-k2-instruct`) |
| Consolidação adiciona lixo no profile | Abrir `~/.config/clica-e-fala/profile.md`, apagar seção `## Consolidated ...` ruim. Transcripts imprecisos poluem o profile |
| Silêncio vira "obrigado" | Hallucination clássica do whisper em áudio muito curto/silencioso. Falar mais longo |

## Roadmap curto

- [ ] CLI pra gerenciar history/profile (limpar, inspecionar, desfazer consolidação)
- [ ] Correction loop: detectar edit após paste, feedback loop
- [ ] Suporte direto à Anthropic API (sem CLI) pra Haiku nativo
- [ ] Toggle global de modelo via tray
- [ ] VAD pra detectar silêncio e parar automático

## Licença

MIT.
