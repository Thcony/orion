# Orion - agente de PC (Windows)

## Versao Rust (em migracao)

`dist/orion.exe` e o Orion em Rust: um .exe so, sem Python, janela com
orb na placa de video. Usa o MESMO `.env`, vault e `data/` da versao
Python, entao a memoria e a cota sao compartilhadas.

Chega pelo `orion atualizar` em `Orion\dist\orion.exe`. Feche o
orion.exe antes de atualizar (o Windows trava arquivo aberto).

Fase 1 (pronta): conversa, cota, troca automatica de modelo, memoria no
Obsidian, trava de seguranca, comandos, apps, pastas, processos, info do
sistema, IP. Na janela: `/chave SUA_CHAVE` troca a chave, `/sair` fecha.

Fases seguintes: voz Charon, pesquisa, avisos ao abrir app, lembretes,
desinstalador, wallpaper, anexos, auto-update proprio. Ate la, a versao
Python segue completa.

Codigo em `rust/orion` (`cargo test` roda 15 testes).

---


Cerebro: Gemini (nuvem). Maos: Python local. Memoria: vault Obsidian.

## Arquitetura (por que e assim)

Nao existe "agente na nuvem que roda comando no seu CMD". O que roda
na nuvem e so a decisao; quem executa tem que estar na maquina. Logo:

    voce -> main.py (local) -> Gemini decide -> guard aprova -> tools executam
                                   |                               |
                            orcamento diario                 Obsidian vault

## Instalar (uma linha, sem zip)

No PowerShell:

    irm https://raw.githubusercontent.com/Thcony/orion/main/install.ps1 | iex

Baixa, instala o psutil, pede a chave do Gemini, poe no PATH.
Rodar a mesma linha de novo = atualizar, preservando `.env` e memoria.

Depois: `orion` em qualquer terminal. Trocar a chave: `orion chave SUA_CHAVE`.

Se o modelo padrao sumir (os IDs do Gemini mudam), ele descobre
sozinho um que a chave aceita e guarda em `data/modelo.json`.
`orion modelos` lista e reseta essa escolha.

## Janela

    orion janela

Chat com orb animado (azul parado, amarelo pensando, verde falando).
Arraste arquivo pra dentro (ou botao +): imagem, PDF e audio vao direto
pro Gemini ver; texto e codigo ele le. Acao destrutiva abre confirmacao.
Fechar a janela salva a memoria da conversa.

## Atualizar

    orion atualizar     # baixa e aplica
    orion versao        # local vs remoto
    orion reverter      # volta o ultimo update

Com `ORION_AUTO_UPDATE=1` (padrao) ele checa no boot e pergunta antes.
A checagem e ~1 KB no GitHub, nao gasta cota do Gemini. Sem internet,
falha calado e abre normal.

O update sobrescreve so codigo: `.env`, `data/` (memoria, orcamento,
lembretes) e o vault nunca sao tocados, e todo arquivo trocado vai pra
`data/backups/<data>/` antes.

## Economia de free tier (isso importa)

O free tier do Gemini e apertado e muda sem aviso. O codigo defende assim:

- **Flash-Lite** como padrao (maior cota diaria)
- **ORION_ORCAMENTO_DIA**: trava local de chamadas/dia; estourou, os
  comandos locais (hora, abrir app, lembrete) continuam funcionando
- **Watcher sem API**: detectar que voce abriu o Discord e 100% local,
  com frase pronta. Gemini so entra se voce responder
- **Prompt curto**: resumo + ultimos 8 turnos, nao o historico inteiro
- Fallback automatico pro modelo reserva em 429/404

## Portao de seguranca

Tres niveis, em `core/guard.py`:

- **livre**: abrir app, wallpaper, listar, ler, criar pasta, renomear
- **confirma**: apagar, desinstalar, mover, fechar app, comando cru
  destrutivo -> ele pergunta "Rodo isso?" antes
- **bloqueio**: `C:\Windows`, System32, WinSxS, drivers, raiz de disco,
  `format`, `diskpart`, matar explorer/lsass/svchost. Nao roda nem com ok.

`ORION_SEM_FREIO=1` tira a confirmacao. Nao recomendo: o modelo erra
caminho e aí o estrago e definitivo. O bloqueio continua valendo.

## Desinstalacao completa

    achar_app("steam")      # mapeia e NAO apaga
    desinstalar("steam")    # mapeia, mostra, executa

Procura em: 3 chaves de registro (HKLM, WOW6432Node, HKCU), Program Files
(x86 e x64), ProgramData, AppData, LocalAppData, Documents, Saved Games,
e winget. Mostra MB de sobra, roda os desinstaladores silenciosos e limpa
as pastas restantes - pulando qualquer coisa que o guard marcar.

Sobre o Edge: da pra remover, mas ele e costurado no Windows (Update,
widgets, WebView2). Muito app Electron/launcher usa WebView2. O guard
avisa antes em vez de deixar quebrar calado.

## Arquivos

    core/config.py     .env, caminhos, modelo
    core/brain.py      Gemini via urllib + orcamento diario
    core/guard.py      livre / confirma / bloqueio
    core/tools.py      as acoes no PC
    core/memory.py     vault Obsidian + fatos
    core/scheduler.py  lembretes locais
    win/uninstall.py   caca-rastros
    senses/watcher.py  proativo local
    main.py            loop

## Fase 2 (nao esta aqui ainda)

Reconhecimento facial (webcam), Whisper local (fala->texto offline),
TTS/voz, e plugar na dock Electron do Orion.
