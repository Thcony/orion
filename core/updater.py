"""Auto-update do Orion. So stdlib: nao precisa de git nem de pip.

Fluxo: compara o SHA do repo com o local (requisicao de ~1 KB).
Diferente -> baixa o zip, faz backup, sobrescreve so o codigo.
Nunca toca em .env, data/ nem no vault.
"""
import hashlib
import json
import os
import shutil
import tempfile
import urllib.error
import urllib.request
import zipfile
from datetime import datetime
from pathlib import Path

from . import config

REPO = os.environ.get("ORION_REPO", "Thcony/orion")
BRANCH = os.environ.get("ORION_BRANCH", "main")
TOKEN = os.environ.get("ORION_GH_TOKEN", "")  # so pra repo privado

VERSAO = config.DATA / "versao.json"
BACKUPS = config.DATA / "backups"

# Nunca sobrescrever nem apagar:
PRESERVAR = {".env", "data", ".git", ".gitignore", "venv", ".venv",
             "__pycache__", "modelos", "vozes"}


def _req(url, timeout=20, bruto=False):
    h = {"User-Agent": "Orion-Updater", "Accept": "application/vnd.github+json"}
    if TOKEN:
        h["Authorization"] = f"Bearer {TOKEN}"
    r = urllib.request.Request(url, headers=h)
    with urllib.request.urlopen(r, timeout=timeout) as resp:
        return resp.read() if bruto else json.loads(resp.read().decode("utf-8"))


def versao_local():
    if VERSAO.exists():
        try:
            return json.loads(VERSAO.read_text(encoding="utf-8-sig"))
        except Exception:
            pass
    return {"sha": None, "quando": None}


def versao_remota():
    """SHA do ultimo commit. Requisicao minima."""
    d = _req(f"https://api.github.com/repos/{REPO}/commits/{BRANCH}?per_page=1")
    c = d if isinstance(d, dict) else d[0]
    return {
        "sha": c["sha"],
        "msg": c.get("commit", {}).get("message", "").split("\n")[0][:120],
        "quando": c.get("commit", {}).get("author", {}).get("date", ""),
    }


def tem_update():
    """(tem, info). Em caso de rede ruim devolve (False, motivo) sem explodir."""
    try:
        r = versao_remota()
    except urllib.error.HTTPError as e:
        return False, f"GitHub respondeu {e.code} (repo existe? e publico?)"
    except Exception as e:
        return False, f"sem rede pro GitHub: {type(e).__name__}"
    local = versao_local()
    if local["sha"] == r["sha"]:
        return False, f"ja esta na ultima ({r['sha'][:7]})"
    return True, r


def _hash(p):
    try:
        return hashlib.sha256(p.read_bytes()).hexdigest()
    except OSError:
        return ""


def _baixar_extrair(destino, sha=None):
    # por SHA exato: o cache do GitHub nunca entrega versao velha
    ref = sha or f"refs/heads/{BRANCH}"
    url = f"https://codeload.github.com/{REPO}/zip/{ref}"
    if TOKEN:
        url = f"https://api.github.com/repos/{REPO}/zipball/{sha or BRANCH}"
    dados = _req(url, timeout=120, bruto=True)
    z = Path(destino) / "orion.zip"
    z.write_bytes(dados)
    with zipfile.ZipFile(z) as f:
        f.extractall(destino)
    # o GitHub embrulha tudo numa pasta "repo-sha/"
    pastas = [p for p in Path(destino).iterdir() if p.is_dir()]
    if not pastas:
        raise RuntimeError("zip do GitHub veio vazio")
    raiz = pastas[0]
    # se o repo guarda o projeto dentro de uma subpasta orion/, usa ela
    if (raiz / "orion" / "main.py").exists():
        raiz = raiz / "orion"
    return raiz


def atualizar(forcar=False):
    tem, info = tem_update()
    if not tem and not forcar:
        return False, info if isinstance(info, str) else "sem novidade"

    alvo = config.ROOT
    with tempfile.TemporaryDirectory() as tmp:
        try:
            novo = _baixar_extrair(tmp, info["sha"] if isinstance(info, dict) else None)
        except Exception as e:
            return False, f"download falhou: {type(e).__name__}: {e}"

        if not (novo / "main.py").exists():
            return False, "o zip nao parece ser o Orion (sem main.py)"

        sha = info["sha"] if isinstance(info, dict) else "forcado"
        backup = BACKUPS / f"{datetime.now():%Y%m%d-%H%M%S}-{sha[:7]}"
        backup.mkdir(parents=True, exist_ok=True)

        mudados, novos = [], []
        for src in novo.rglob("*"):
            if not src.is_file():
                continue
            rel = src.relative_to(novo)
            if rel.parts[0] in PRESERVAR:
                continue
            dst = alvo / rel
            if dst.exists():
                if _hash(dst) == _hash(src):
                    continue
                bk = backup / rel
                bk.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(dst, bk)
                mudados.append(str(rel))
            else:
                novos.append(str(rel))
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)

    if isinstance(info, dict):
        VERSAO.write_text(json.dumps({
            "sha": info["sha"], "msg": info.get("msg", ""),
            "quando": datetime.now().isoformat(),
        }, ensure_ascii=False, indent=2), encoding="utf-8")

    if not mudados and not novos:
        return False, "nada mudou de verdade"

    L = []
    if isinstance(info, dict) and info.get("msg"):
        L.append(f"commit: {info['msg']}")
    if novos:
        L.append(f"novos ({len(novos)}): {', '.join(novos[:8])}")
    if mudados:
        L.append(f"atualizados ({len(mudados)}): {', '.join(mudados[:8])}")
    L.append(f"backup em data/backups/{backup.name}")
    return True, "\n".join(L)


def reverter():
    """Volta pro backup mais recente."""
    if not BACKUPS.exists():
        return False, "nenhum backup"
    bks = sorted([p for p in BACKUPS.iterdir() if p.is_dir()])
    if not bks:
        return False, "nenhum backup"
    bk = bks[-1]
    n = 0
    for src in bk.rglob("*"):
        if src.is_file():
            dst = config.ROOT / src.relative_to(bk)
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
            n += 1
    if VERSAO.exists():
        VERSAO.unlink()
    return True, f"revertidos {n} arquivos de {bk.name}"


def reiniciar():
    """Re-executa com o codigo novo. No Windows o execv solta o console,
    entao roda como filho e espera."""
    import subprocess
    import sys
    if os.name == "nt":
        raise SystemExit(subprocess.call([sys.executable] + sys.argv))
    os.execv(sys.executable, [sys.executable] + sys.argv)


def garantir_deps():
    """Roda pip quando o requirements.txt muda (o update so copia codigo).
    Sem rede, tenta de novo so no dia seguinte pra nao atrasar o boot."""
    import subprocess
    import sys
    from datetime import date
    req = config.ROOT / "requirements.txt"
    marca = config.DATA / "deps.json"
    if not req.exists():
        return None
    h = hashlib.sha256(req.read_bytes()).hexdigest()
    try:
        m = json.loads(marca.read_text(encoding="utf-8-sig"))
    except Exception:
        m = {}
    if m.get("hash") == h or m.get("falhou") == f"{h}:{date.today()}":
        return None
    cmd = [sys.executable, "-m", "pip", "install", "--quiet", "--disable-pip-version-check"]
    if sys.prefix == sys.base_prefix:  # fora de venv; dentro, --user quebra
        cmd.append("--user")
    try:
        r = subprocess.run(cmd + ["-r", str(req)],
                           capture_output=True, text=True, timeout=900)
        ok = r.returncode == 0
        erro = (r.stderr.strip().splitlines() or ["?"])[-1][:200]
    except Exception as e:
        ok, erro = False, str(e)
    if ok:
        marca.write_text(json.dumps({"hash": h}), encoding="utf-8")
        import importlib
        import site
        site.addsitedir(site.getusersitepackages())  # vale ja nesta execucao
        importlib.invalidate_caches()
        return "dependencias atualizadas"
    marca.write_text(json.dumps({"falhou": f"{h}:{date.today()}"}), encoding="utf-8")
    return f"nao instalei dependencias ({erro}); tento amanha"
