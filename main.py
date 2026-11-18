"""Orion - assistente do chefe. Cerebro no Gemini, maos no PC.

    python main.py              conversa
    python main.py modelos      lista modelos que a sua chave aceita
    python main.py "comando"    one-shot
"""
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from core import brain, config, guard, memory, scheduler, tools, updater
from senses import watcher

if os.name == "nt":
    os.system("")  # liga as cores ANSI no console do Windows

VERDE, AMARELO, VERMELHO, CINZA, RESET = (
    "\033[92m", "\033[93m", "\033[91m", "\033[90m", "\033[0m"
)


def fala(texto):
    print(f"{VERDE}Orion:{RESET} {texto}")


def confirmar(pergunta):
    try:
        return input(f"{AMARELO}>> {pergunta} [s/N] {RESET}").strip().lower() in ("s", "sim", "y")
    except (EOFError, KeyboardInterrupt):
        return False


def rodar_acoes(acoes):
    for a in acoes:
        tool = a.get("tool", "")
        args = a.get("args", {}) or {}
        nivel, motivo = guard.avaliar(tool, args)

        if nivel == guard.BLOQUEIO:
            print(f"{VERMELHO}[bloqueado]{RESET} {tool}: {motivo}")
            memory.registrar_acao(tool, args, f"BLOQUEADO: {motivo}")
            continue

        if nivel == guard.CONFIRMA and not config.SEM_FREIO:
            print(f"{CINZA}  {tool}({args}){RESET}")
            if motivo:
                print(f"{AMARELO}  aviso: {motivo}{RESET}")
            if not confirmar(f"Rodo {tool}?"):
                print(f"{CINZA}  pulei{RESET}")
                memory.registrar_acao(tool, args, "recusado pelo chefe")
                continue

        res = tools.executar(tool, args)
        print(f"{CINZA}{res}{RESET}")
        memory.registrar_acao(tool, args, res)


def turno(entrada):
    memory.registrar(config.CHEFE, entrada)
    r = brain.pensar(entrada, tools.DESCRICAO)
    f = r.get("fala") or ""
    if f:
        fala(f)
        memory.registrar("Orion", f)
    rodar_acoes(r.get("acoes") or [])


def checar_update(silencioso=True):
    tem, info = updater.tem_update()
    if tem:
        msg = info["msg"] if isinstance(info, dict) else ""
        print(f"{AMARELO}[update disponivel]{RESET} {msg}")
        if confirmar("Atualizo agora?"):
            ok, det = updater.atualizar()
            print(f"{CINZA}{det}{RESET}")
            if ok:
                fala("Codigo novo no lugar. Reiniciando.")
                updater.reiniciar()
    elif not silencioso:
        print(f"{CINZA}{info if isinstance(info, str) else 'sem novidade'}{RESET}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] in ("atualizar", "update"):
        ok, det = updater.atualizar(forcar="--forcar" in sys.argv)
        print(det)
        return

    if len(sys.argv) > 1 and sys.argv[1] == "reverter":
        print(updater.reverter()[1])
        return

    if len(sys.argv) > 1 and sys.argv[1] == "versao":
        v = updater.versao_local()
        print(f"local: {(v['sha'] or 'nunca atualizado')[:7]}  {v.get('msg','')}")
        tem, info = updater.tem_update()
        print(f"remoto: {info['sha'][:7] + '  ' + info.get('msg','') if tem else info}")
        return

    if len(sys.argv) > 1 and sys.argv[1] == "modelos":
        try:
            r = brain.ranquear(brain.listar_modelos())
            brain.CACHE.unlink(missing_ok=True)
            print("\n".join(r) or "nenhum modelo de texto liberado pra essa chave")
            if r:
                print(f"\n-> uso: {config.MODELO if config.MODELO in r else r[0]}")
        except Exception as e:
            print(f"falhou: {e}")
        return

    if len(sys.argv) > 1 and sys.argv[1] == "chave":
        if len(sys.argv) < 3:
            print("uso: orion chave SUA_CHAVE   (gratis em https://aistudio.google.com/apikey)")
            return
        config.salvar_env("GEMINI_API_KEY", sys.argv[2].strip())
        config.API_KEY = sys.argv[2].strip()
        try:
            print(f"chave salva e valida ({len(brain.listar_modelos())} modelos liberados)")
        except Exception as e:
            print(f"chave salva, mas o Gemini recusou: {e}")
        return

    if len(sys.argv) > 1:
        turno(" ".join(sys.argv[1:]))
        return

    if config.AUTO_UPDATE:
        checar_update()
    if not config.API_KEY:
        print(f"{AMARELO}Sem chave do Gemini: roda  orion chave SUA_CHAVE{RESET}")

    scheduler.vigiar(lambda t: fala(f"Lembrete: {t}"))
    watcher.vigiar(fala)

    print(f"{VERDE}Orion online.{RESET} vault: {config.VAULT}")
    print(f"{CINZA}modelo {config.MODELO} | {brain.orcamento.restante()} chamadas hoje | 'sair' encerra{RESET}\n")

    while True:
        try:
            e = input(f"{AMARELO}{config.CHEFE}:{RESET} ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if not e:
            continue
        if e.lower() in ("sair", "exit", "quit"):
            break
        turno(e)

    fala("Fechando. Ta tudo no vault.")


if __name__ == "__main__":
    main()
