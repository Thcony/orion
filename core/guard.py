"""Portao de seguranca: separa o que roda livre do que precisa do 'ok, chefe'."""
import os
import re
from pathlib import Path

LIVRE = "livre"        # roda na hora
CONFIRMA = "confirma"  # pergunta antes
BLOQUEIO = "bloqueio"  # nao roda, quebraria o Windows

# Caminhos que nao se mexe nunca: tirar isso nao desinstala nada, so
# deixa o Windows sem dar boot.
INTOCAVEL = [
    r"c:\windows",
    r"c:\program files\windowsapps",
    r"c:\programdata\microsoft\windows\start menu",
    r"c:\$recycle.bin",
    r"c:\boot",
    r"c:\recovery",
    r"c:\system volume information",
    r"c:\program files\microsoft\edge",
]

# Trechos que, aparecendo em qualquer lugar do caminho, ja condenam.
MARCA_SISTEMA = ["\\system32", "\\syswow64", "\\windowsapps", "\\winsxs",
                 "\\drivers\\", "\\boot\\", "\\bootmgr"]

# Processos que nao se mata: o Windows cai ou a tela congela.
PROC_CRITICO = {
    "system", "system idle process", "csrss.exe", "wininit.exe", "winlogon.exe",
    "services.exe", "lsass.exe", "smss.exe", "svchost.exe", "dwm.exe",
    "explorer.exe", "fontdrvhost.exe", "registry", "memory compression",
}

# Comando cru: o que da pra soltar e o que para pra confirmar.
PADRAO_DESTRUTIVO = [
    r"\bdel\b", r"\berase\b", r"\brd\b", r"\brmdir\b", r"\bremove-item\b",
    r"\bformat\b", r"\bdiskpart\b", r"\bcipher\s+/w", r"\bsdelete\b",
    r"\breg\s+delete\b", r"\bremove-itemproperty\b", r"\bbcdedit\b",
    r"\bvssadmin\b", r"\bwmic\b.*\bdelete\b", r"\buninstall\b",
    r"\btaskkill\b", r"\bstop-process\b", r"\bshutdown\b", r"\brestart-computer\b",
    r"\bnet\s+user\b", r"\bicacls\b", r"\btakeown\b", r"\bset-executionpolicy\b",
    r"\bnew-service\b", r"\bsc\s+(delete|config)\b", r"\bschtasks\b.*\/create",
]

# Isso nao passa nem com confirmacao: apaga o disco ou mata o SO.
PADRAO_PROIBIDO = [
    r"format\s+[a-z]:",
    r"diskpart",
    r":\(\)\s*\{.*\};:",            # fork bomb
    r"vssadmin.*delete\s+shadows",  # tipico de ransomware, nunca necessario aqui
    r"bcdedit",
    r"cipher\s+/w",
]

# Alvos que, combinados com qualquer verbo destrutivo, viram bloqueio direto.
ALVO_SISTEMA = [
    r"c:\\windows", r"system32", r"syswow64", r"\\boot\b", r"c:\\recovery",
    r"windowsapps", r"c:\\programdata\\microsoft\\windows",
    r"\bhklm\b", r"hkey_local_machine", r"\bc:\\\s*$", r"\bc:\\\*",
]

VERBO_DESTRUTIVO = [
    r"\bdel\b", r"\berase\b", r"\brd\b", r"\brmdir\b", r"\bremove-item\b",
    r"\breg\s+delete\b", r"\btakeown\b", r"\bicacls\b", r"\bsdelete\b",
]


def _norm(p):
    """Normaliza pra comparacao, sem depender do SO que esta rodando."""
    t = str(p).strip().strip('"').strip("'")
    t = os.path.expandvars(os.path.expanduser(t))
    t = t.replace("/", "\\").lower()
    while "\\\\" in t:
        t = t.replace("\\\\", "\\")
    return t.rstrip("\\")


def caminho_intocavel(caminho):
    c = _norm(caminho)
    if not c:
        return True
    # raiz de disco inteira ("c:", "c:\", "d:\*")
    if re.fullmatch(r"[a-z]:(\\\*?)?", c):
        return True
    if any(c == i or c.startswith(i + "\\") for i in INTOCAVEL):
        return True
    if any(m in c + "\\" for m in MARCA_SISTEMA):
        return True
    return False


def avaliar_comando(cmd):
    """Classifica um comando cru de CMD/PowerShell."""
    c = cmd.lower()
    for p in PADRAO_PROIBIDO:
        if re.search(p, c):
            return BLOQUEIO, "comando quebraria o Windows ou apagaria o disco"
    # verbo destrutivo + alvo de sistema = bloqueio, em qualquer ordem/flags
    if any(re.search(v, c) for v in VERBO_DESTRUTIVO):
        for a in ALVO_SISTEMA:
            if re.search(a, c):
                return BLOQUEIO, "isso apaga area do sistema, o Windows nao da boot depois"
    for p in PADRAO_DESTRUTIVO:
        if re.search(p, c):
            return CONFIRMA, "comando mexe/apaga coisa no sistema"
    return LIVRE, ""


def avaliar(tool, args):
    """Classifica uma acao do registry de tools."""
    from .tools import NIVEL

    nivel = NIVEL.get(tool, CONFIRMA)

    if tool == "rodar_cmd":
        return avaliar_comando(str(args.get("comando", "")))

    if tool in ("apagar", "renomear", "mover"):
        for k in ("caminho", "origem", "destino"):
            if k in args and caminho_intocavel(args[k]):
                return BLOQUEIO, f"{args[k]} e area do sistema"

    if tool == "fechar_app":
        nome = str(args.get("nome", "")).lower()
        if not nome.endswith(".exe"):
            nome += ".exe"
        if nome in PROC_CRITICO:
            return BLOQUEIO, f"{nome} e processo critico do Windows"

    if tool == "desinstalar":
        nome = str(args.get("nome", "")).lower()
        if "edge" in nome or "webview" in nome:
            return CONFIRMA, (
                "o Edge/WebView2 e costurado no Windows: Windows Update, widgets e "
                "apps que usam WebView2 (incluindo muito launcher e Electron) podem "
                "parar. Da pra remover, mas quebra junto"
            )
        if "windows" in nome or "defender" in nome:
            return BLOQUEIO, "componente do proprio Windows"

    return nivel, "acao destrutiva" if nivel == CONFIRMA else ""
