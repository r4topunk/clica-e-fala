# Clica e Fala

Macos menubar app. Grava voz PT-BR, transcreve com `whisper.cpp`, passa transcript pro `claude` CLI refinar + traduzir pra prompt EN, cola no app focado.

## Fluxo

```
hotkey → record mic → hotkey → whisper-cli → claude -p → clipboard → auto-paste
```

## Pré-requisitos

- macOS 12+
- `whisper-cli` em PATH (`brew install whisper-cpp`)
- modelo em `~/models/whisper/ggml-large-v3.bin`
- `claude` CLI em PATH (Claude Code)
- Rust toolchain
- `pnpm`
- ffmpeg (só pra gerar ícone uma vez)

## Setup

```fish
pnpm install
pnpm tauri dev
```

Primeira execução pede permissão de:
- **Microfone** (gravar áudio)
- **Accessibility** (System Events → Cmd+V)

Conceder ambos. Sem Accessibility o clipboard é preenchido mas não cola sozinho.

## Uso

- `⌘⇧Space` → inicia gravação (som `Tink`)
- Fale pensamento em voz alta, sem se preocupar com ordem/coerência
- `⌘⇧Space` de novo → para (som `Pop`) e processa
- Pipeline roda: whisper → claude → clipboard → paste (som `Glass` no fim)
- Erro toca `Basso`

Foco em qualquer app antes de gravar. O resultado cola onde você estava (Claude Code CLI, Cursor, etc).

## Build prod

```fish
pnpm tauri build
```

Binário em `src-tauri/target/release/bundle/macos/ClicaEFala.app`.

## System prompt

Em `src-tauri/src/pipeline.rs` constante `SYSTEM_PROMPT`. Editar ali pra mudar tom/formato do output.

## Troubleshooting

- **Whisper não acha modelo:** ajustar path em `main.rs` → `model_path`.
- **Cmd+V não cola:** System Settings → Privacy → Accessibility → adicionar `ClicaEFala` (ou binário de dev em `src-tauri/target/debug`).
- **Hotkey não dispara:** conflito com outro app (Spotlight usa ⌘Space). Mudar em `main.rs` → `Shortcut::new(...)`.
- **Silêncio vira transcript vazio:** pipeline aborta, toca `Basso`. Falar mais alto / mais perto do mic.
