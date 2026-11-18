"""Olho no que voce abre. Regras locais: NAO gasta chamada do Gemini.
So fala o Gemini se voce responder a oferta.
"""
import random
import threading
import time

try:
    import psutil
except ImportError:
    psutil = None

# processo -> falas possiveis. Edite a gosto.
REGRAS = {
    "discord": [
        "Vi que abriu o Discord, chefe. Quer que eu escreva alguma mensagem?",
        "No Discord? Se precisar redigir algo, fala.",
    ],
    "robloxstudio": [
        "Roblox Studio aberto. Quer que eu entre e ajude no script?",
        "Mexendo no Studio, chefe? Posso dar uma mao no Luau.",
    ],
    "robloxplayerbeta": ["Bora jogar? Se travar algo, me chama."],
    "blender": [
        "Blender na area. Modelando o que hoje, chefe?",
        "Se quiser script em Python pro Blender, e so pedir.",
    ],
    "code": ["VS Code aberto. Em que projeto a gente ta hoje?"],
    "brave": ["Pesquisando algo? Me fala que eu busco e te resumo."],
    "steam": ["Steam aberta. Quer que eu veja o que ta instalado ou libere espaco?"],
    "obs64": ["Vai gravar? Quer que eu feche o que ta comendo CPU?"],
}

INTERVALO = 6          # segundos entre varreduras
SILENCIO = 900         # nao repete a mesma oferta antes disso (15 min)


def _chave(nome):
    return (nome or "").lower().removesuffix(".exe")


def vigiar(falar, intervalo=INTERVALO):
    """falar(texto) e chamado quando um app monitorado aparece."""
    if not psutil:
        return None

    visto = {}
    ultimo_aviso = {}

    def loop():
        nonlocal visto
        # primeira varredura: so registra, nao comenta o que ja estava aberto
        visto = {_chave(p.info["name"]) for p in psutil.process_iter(["name"])}
        while True:
            try:
                agora = {_chave(p.info["name"]) for p in psutil.process_iter(["name"])}
                novos = agora - visto
                visto = agora
                for n in novos:
                    if n not in REGRAS:
                        continue
                    t = time.time()
                    if t - ultimo_aviso.get(n, 0) < SILENCIO:
                        continue
                    ultimo_aviso[n] = t
                    falar(random.choice(REGRAS[n]))
            except Exception:
                pass
            time.sleep(intervalo)

    th = threading.Thread(target=loop, daemon=True)
    th.start()
    return th
