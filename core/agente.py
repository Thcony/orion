"""Logica de um turno, compartilhada pelo terminal e pela janela.
A UI injeta como falar, informar e confirmar — o miolo e o mesmo.
"""
from . import brain, config, guard, memory, tools


MAX_RODADAS = 2  # leituras de resultado por mensagem (cada uma = 1 chamada)


def responder(entrada, dizer, confirmar, falar=None, anexos=None):
    """
    dizer(texto, tipo)  tipo: 'orion' | 'info' | 'aviso' | 'erro'
    confirmar(tool, args, motivo) -> bool
    falar(texto)        opcional, pra voz
    """
    falar = falar or (lambda t: None)
    from pathlib import Path
    nomes = ", ".join(Path(a).name for a in anexos or [])
    memory.registrar(config.CHEFE, entrada + (f" (anexou: {nomes})" if nomes else ""))

    r = brain.pensar(entrada, tools.DESCRICAO, anexos)
    for rodada in range(MAX_RODADAS + 1):
        resultados = _executar(r, dizer, confirmar, falar)
        if not (r.get("ver_resultado") and resultados) or rodada == MAX_RODADAS:
            return
        bloco = "\n\n".join(f"{t} -> {res[:3000]}" for t, res in resultados)
        r = brain.pensar(f"{entrada}\n\nRESULTADO DAS SUAS ACOES:\n{bloco}\n\n"
                         "Agora responde o chefe com base nisso.", tools.DESCRICAO)


def _executar(r, dizer, confirmar, falar):
    """Fala e roda as acoes de uma resposta. Devolve [(tool, resultado)] lidos."""
    f = (r.get("fala") or "").strip()
    if f:
        dizer(f, "orion")
        falar(f)
        memory.registrar("Orion", f)
    lidos = []
    for a in r.get("acoes") or []:
        tool = a.get("tool", "")
        args = a.get("args", {}) or {}
        nivel, motivo = guard.avaliar(tool, args)

        if nivel == guard.BLOQUEIO:
            dizer(f"[bloqueado] {tool}: {motivo}", "erro")
            memory.registrar_acao(tool, args, f"BLOQUEADO: {motivo}")
            continue

        if nivel == guard.CONFIRMA and not config.SEM_FREIO:
            if not confirmar(tool, args, motivo):
                dizer("deixei pra la", "info")
                memory.registrar_acao(tool, args, "recusado pelo chefe")
                continue

        res = tools.executar(tool, args)
        memory.registrar_acao(tool, args, res)

        if tool == "pesquisar" and not res.startswith("nao consegui pesquisar"):
            dizer(res, "info")
            falar(res.split("\n\nFontes:")[0])
        elif r.get("ver_resultado"):
            dizer(res if len(res) < 400 else res[:400] + " ...", "info")
            lidos.append((tool, res))
        else:
            dizer(res, "info")
    return lidos
