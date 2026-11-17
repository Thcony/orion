"""Caca-rastros de app no Windows: registro + disco + winget.
Primeiro mapeia TUDO, mostra o relatorio, e so apaga depois do ok.
"""
import os
import subprocess
from pathlib import Path

try:
    import winreg
except ImportError:  # fora do Windows (dev/teste)
    winreg = None

CHAVES = [
    ("HKLM", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ("HKLM", r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
    ("HKCU", r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
]

RAIZ = {"HKLM": getattr(winreg, "HKEY_LOCAL_MACHINE", None) if winreg else None,
        "HKCU": getattr(winreg, "HKEY_CURRENT_USER", None) if winreg else None}


def _ler(k, nome):
    try:
        return winreg.QueryValueEx(k, nome)[0]
    except OSError:
        return None


def entradas_registro(termo):
    """Entradas de desinstalacao cujo nome casa com o termo."""
    if not winreg:
        return []
    termo = termo.lower()
    achados = []
    for raiz_nome, caminho in CHAVES:
        raiz = RAIZ[raiz_nome]
        try:
            base = winreg.OpenKey(raiz, caminho)
        except OSError:
            continue
        with base:
            i = 0
            while True:
                try:
                    sub = winreg.EnumKey(base, i)
                except OSError:
                    break
                i += 1
                try:
                    with winreg.OpenKey(base, sub) as k:
                        nome = _ler(k, "DisplayName")
                        if not nome or termo not in nome.lower():
                            continue
                        achados.append({
                            "nome": nome,
                            "versao": _ler(k, "DisplayVersion"),
                            "local": _ler(k, "InstallLocation"),
                            "comando": _ler(k, "QuietUninstallString") or _ler(k, "UninstallString"),
                            "chave": f"{raiz_nome}\\{caminho}\\{sub}",
                        })
                except OSError:
                    continue
    return achados


def pastas_sobra(termo):
    """Pastas que o desinstalador costuma deixar pra tras."""
    termo = termo.lower()
    bases = [
        os.environ.get("ProgramFiles"),
        os.environ.get("ProgramFiles(x86)"),
        os.environ.get("ProgramData"),
        os.environ.get("APPDATA"),
        os.environ.get("LOCALAPPDATA"),
        str(Path.home() / "Documents"),
        str(Path.home() / "Saved Games"),
    ]
    achados = []
    for b in bases:
        if not b or not Path(b).is_dir():
            continue
        try:
            for p in Path(b).iterdir():
                if p.is_dir() and termo in p.name.lower():
                    achados.append(str(p))
        except (PermissionError, OSError):
            continue
    return achados


def entradas_winget(termo):
    try:
        r = subprocess.run(
            ["winget", "list", "--name", termo],
            capture_output=True, text=True, timeout=60,
        )
        return r.stdout.strip() if r.returncode == 0 else ""
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return ""


def mapear(termo):
    """Relatorio completo. NAO apaga nada."""
    reg = entradas_registro(termo)
    sobras = pastas_sobra(termo)
    wg = entradas_winget(termo)
    tamanho = 0
    for s in sobras:
        try:
            tamanho += sum(f.stat().st_size for f in Path(s).rglob("*") if f.is_file())
        except (PermissionError, OSError):
            pass
    return {
        "termo": termo,
        "registro": reg,
        "pastas": sobras,
        "winget": wg,
        "mb": round(tamanho / 1048576, 1),
    }


def relatorio(m):
    L = [f"Rastros de '{m['termo']}':"]
    if m["registro"]:
        L.append(f"  Instalacoes registradas ({len(m['registro'])}):")
        for e in m["registro"]:
            L.append(f"    - {e['nome']} {e.get('versao') or ''}")
            if e.get("local"):
                L.append(f"      pasta: {e['local']}")
            L.append(f"      desinstalador: {'sim' if e.get('comando') else 'NAO TEM'}")
    else:
        L.append("  Nada no registro de desinstalacao.")
    if m["pastas"]:
        L.append(f"  Pastas sobrando ({len(m['pastas'])}, ~{m['mb']} MB):")
        L.extend(f"    - {p}" for p in m["pastas"])
    if m["winget"]:
        L.append("  winget conhece esse pacote.")
    return "\n".join(L)


def executar(m, apagar_sobras=True):
    """Roda os desinstaladores e, se pedido, limpa as pastas. So depois do ok."""
    import shutil
    log = []

    for e in m["registro"]:
        cmd = e.get("comando")
        if not cmd:
            log.append(f"{e['nome']}: sem desinstalador, vai na pasta")
            continue
        if "msiexec" in cmd.lower() and "/quiet" not in cmd.lower():
            cmd += " /quiet /norestart"
        try:
            r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=900)
            log.append(f"{e['nome']}: desinstalador saiu com codigo {r.returncode}")
        except subprocess.TimeoutExpired:
            log.append(f"{e['nome']}: desinstalador travou (15 min)")

    if m["winget"]:
        try:
            r = subprocess.run(
                ["winget", "uninstall", "--name", m["termo"],
                 "--silent", "--accept-source-agreements"],
                capture_output=True, text=True, timeout=900,
            )
            log.append(f"winget: codigo {r.returncode}")
        except (FileNotFoundError, subprocess.TimeoutExpired) as e:
            log.append(f"winget falhou: {e}")

    if apagar_sobras:
        from core import guard
        for p in m["pastas"]:
            if guard.caminho_intocavel(p):
                log.append(f"PULEI {p} (area do sistema)")
                continue
            try:
                shutil.rmtree(p, ignore_errors=False)
                log.append(f"apagei {p}")
            except Exception as e:
                log.append(f"nao deu pra apagar {p}: {e}")

    return "\n".join(log)
