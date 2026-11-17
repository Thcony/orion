"""Memoria do Orion em cima de um vault Obsidian (markdown local)."""
import json
import re
from datetime import datetime
from . import config

VAULT = config.VAULT
CONVERSAS = VAULT / "Conversas"
RESUMO = VAULT / "Orion - Memoria.md"
FATOS = config.DATA / "fatos.json"


def _init():
    CONVERSAS.mkdir(parents=True, exist_ok=True)
    if not RESUMO.exists():
        RESUMO.write_text(
            "# Orion - Memoria\n\n## Sobre o chefe\n\n## Resumo das conversas\n",
            encoding="utf-8",
        )


def _arquivo_hoje():
    _init()
    f = CONVERSAS / f"{datetime.now():%Y-%m-%d}.md"
    if not f.exists():
        f.write_text(f"# Conversa {datetime.now():%d/%m/%Y}\n\n", encoding="utf-8")
    return f


def registrar(quem, texto):
    """Grava um turno no Obsidian."""
    f = _arquivo_hoje()
    with f.open("a", encoding="utf-8") as fh:
        fh.write(f"**{quem}** ({datetime.now():%H:%M}): {texto}\n\n")


def registrar_acao(tool, args, resultado):
    f = _arquivo_hoje()
    with f.open("a", encoding="utf-8") as fh:
        fh.write(f"> `{tool}` {json.dumps(args, ensure_ascii=False)} -> {resultado[:300]}\n\n")


def turnos_recentes(n=None):
    """Ultimos turnos do dia, pra montar prompt curto."""
    n = n or config.TURNOS_CONTEXTO
    f = _arquivo_hoje()
    blocos = [b.strip() for b in f.read_text(encoding="utf-8").split("\n\n") if b.strip()]
    falas = [b for b in blocos if b.startswith("**")]
    return falas[-n:]


def resumo():
    _init()
    return RESUMO.read_text(encoding="utf-8")[:3000]


def atualizar_resumo(texto):
    _init()
    RESUMO.write_text(texto, encoding="utf-8")


def anotar(titulo, texto):
    """Cria uma nota solta no vault."""
    _init()
    nome = re.sub(r"[^\w\s-]", "", titulo).strip()[:60] or "nota"
    f = VAULT / f"{nome}.md"
    f.write_text(f"# {titulo}\n\n{texto}\n", encoding="utf-8")
    return str(f)


def lembrar_fato(chave, valor):
    d = json.loads(FATOS.read_text(encoding="utf-8")) if FATOS.exists() else {}
    d[chave] = valor
    FATOS.write_text(json.dumps(d, ensure_ascii=False, indent=2), encoding="utf-8")


def fatos():
    return json.loads(FATOS.read_text(encoding="utf-8")) if FATOS.exists() else {}
