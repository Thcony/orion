"""Memoria do Orion em cima de um vault Obsidian (markdown local).
Erro de disco aqui nunca derruba a conversa: avisa uma vez e segue,
com o contexto recente mantido em RAM.
"""
import json
import re
import sys
from collections import deque
from datetime import datetime

from . import config

VAULT = config.VAULT
CONVERSAS = VAULT / "Conversas"
RESUMO = VAULT / "Orion - Memoria.md"
FATOS = config.DATA / "fatos.json"

_buffer = deque(maxlen=60)
_semeado = False
_avisou = False


def _falhou(e):
    global _avisou
    if not _avisou:
        _avisou = True
        print(f"\033[93m[memoria] nao consegui gravar em {VAULT} ({e}). "
              f"Sigo sem salvar; ajuste ORION_VAULT no .env.\033[0m", file=sys.stderr)


def _init():
    CONVERSAS.mkdir(parents=True, exist_ok=True)
    if not RESUMO.exists():
        RESUMO.write_text("# Orion - Memoria\n\n## Sobre o chefe\n\n## Resumo das conversas\n",
                          encoding="utf-8")


def _arquivo_hoje():
    _init()
    f = CONVERSAS / f"{datetime.now():%Y-%m-%d}.md"
    if not f.exists():
        f.write_text(f"# Conversa {datetime.now():%d/%m/%Y}\n\n", encoding="utf-8")
    return f


def _semear():
    """Na primeira chamada, recupera os turnos de hoje do vault."""
    global _semeado
    if _semeado:
        return
    _semeado = True
    try:
        blocos = _arquivo_hoje().read_text(encoding="utf-8").split("\n\n")
        _buffer.extend(b.strip() for b in blocos if b.strip().startswith("**"))
    except OSError as e:
        _falhou(e)


def _anexar(texto):
    try:
        with _arquivo_hoje().open("a", encoding="utf-8") as fh:
            fh.write(texto)
    except OSError as e:
        _falhou(e)


def registrar(quem, texto):
    _semear()
    linha = f"**{quem}** ({datetime.now():%H:%M}): {texto}"
    _buffer.append(linha)
    _anexar(linha + "\n\n")


def registrar_acao(tool, args, resultado):
    _anexar(f"> `{tool}` {json.dumps(args, ensure_ascii=False)} -> {str(resultado)[:300]}\n\n")


def turnos_recentes(n=None):
    _semear()
    return list(_buffer)[-(n or config.TURNOS_CONTEXTO):]


def resumo():
    try:
        _init()
        return RESUMO.read_text(encoding="utf-8")[:3000]
    except OSError as e:
        _falhou(e)
        return ""


_inicio_sessao = None


def marcar_inicio():
    """Guarda quantos turnos ja existiam, pra resumir so o que e novo."""
    global _inicio_sessao
    _semear()
    _inicio_sessao = len(_buffer)


def turnos_da_sessao():
    _semear()
    return list(_buffer)[_inicio_sessao or 0:]


def consolidar():
    """Funde a sessao na memoria de longo prazo. True se gravou."""
    from . import brain
    novos = turnos_da_sessao()
    if len(novos) < 2:
        return False
    texto = brain.resumir(resumo(), "\n".join(novos))
    if not texto:
        return False
    try:
        atualizar_resumo(texto)
        return True
    except OSError as e:
        _falhou(e)
        return False


def atualizar_resumo(texto):
    _init()
    RESUMO.write_text(texto, encoding="utf-8")


def anotar(titulo, texto):
    _init()
    nome = re.sub(r"[^\w\s-]", "", titulo).strip()[:60] or "nota"
    f = VAULT / f"{nome}.md"
    f.write_text(f"# {titulo}\n\n{texto}\n", encoding="utf-8")
    return str(f)


def lembrar_fato(chave, valor):
    d = fatos()
    d[chave] = valor
    FATOS.write_text(json.dumps(d, ensure_ascii=False, indent=2), encoding="utf-8")


def fatos():
    try:
        return json.loads(FATOS.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}
