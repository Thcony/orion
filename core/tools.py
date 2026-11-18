"""Registry de acoes. Cada tool: funcao + nivel de risco."""
import ctypes
import os
import shutil
import subprocess
from datetime import datetime
from pathlib import Path

from . import config, guard, memory, scheduler

try:
    import psutil
except ImportError:
    psutil = None


# ---------- comando cru ----------
def rodar_cmd(comando, shell="cmd"):
    exe = (["powershell", "-NoProfile", "-Command", comando] if shell == "powershell"
           else ["cmd", "/c", comando])
    try:
        r = subprocess.run(exe, capture_output=True, text=True, timeout=300,
                           errors="replace")
        saida = (r.stdout or "") + (("\n[erro]\n" + r.stderr) if r.stderr else "")
        return (saida.strip() or f"(sem saida, codigo {r.returncode})")[:4000]
    except subprocess.TimeoutExpired:
        return "comando passou de 5 min, matei"
    except FileNotFoundError:
        return f"shell '{shell}' nao achado"


# ---------- apps ----------
def abrir_app(nome):
    try:
        subprocess.Popen(["cmd", "/c", "start", "", nome], shell=False)
        return f"abri {nome}"
    except Exception as e:
        return f"nao abri {nome}: {e}"


def fechar_app(nome):
    if not psutil:
        return rodar_cmd(f"taskkill /im {nome} /f")
    alvo = nome.lower().removesuffix(".exe")
    mortos = []
    for p in psutil.process_iter(["name"]):
        n = (p.info["name"] or "").lower()
        if n.removesuffix(".exe") == alvo:
            try:
                p.terminate()
                mortos.append(str(p.pid))
            except Exception:
                pass
    return f"fechei {nome} (pid {', '.join(mortos)})" if mortos else f"{nome} nao estava aberto"


def listar_processos(filtro=""):
    if not psutil:
        return rodar_cmd("tasklist")
    f = filtro.lower()
    vistos = {}
    for p in psutil.process_iter(["name", "memory_info"]):
        n = p.info["name"] or "?"
        if f and f not in n.lower():
            continue
        mb = (p.info["memory_info"].rss / 1048576) if p.info["memory_info"] else 0
        vistos[n] = vistos.get(n, 0) + mb
    top = sorted(vistos.items(), key=lambda x: -x[1])[:25]
    return "\n".join(f"{n} - {mb:.0f} MB" for n, mb in top) or "nada casou"


def app_aberto(nome):
    if not psutil:
        return "psutil nao instalado"
    alvo = nome.lower().removesuffix(".exe")
    for p in psutil.process_iter(["name"]):
        if (p.info["name"] or "").lower().removesuffix(".exe") == alvo:
            return f"{nome} esta aberto (pid {p.pid})"
    return f"{nome} nao esta aberto"


# ---------- aparencia ----------
def trocar_wallpaper(caminho):
    p = Path(os.path.expandvars(caminho)).expanduser()
    if not p.is_file():
        return f"nao achei a imagem {p}"
    ok = ctypes.windll.user32.SystemParametersInfoW(20, 0, str(p), 3)
    return f"wallpaper trocado pra {p.name}" if ok else "o Windows recusou a troca"


# ---------- arquivos ----------
def listar_pasta(caminho="."):
    p = Path(os.path.expandvars(caminho)).expanduser()
    if not p.is_dir():
        return f"{p} nao e pasta"
    linhas = []
    for x in sorted(p.iterdir(), key=lambda a: (a.is_file(), a.name.lower()))[:200]:
        if x.is_dir():
            linhas.append(f"[pasta] {x.name}")
        else:
            try:
                linhas.append(f"        {x.name}  {x.stat().st_size/1024:.0f} KB")
            except OSError:
                linhas.append(f"        {x.name}")
    return "\n".join(linhas) or "pasta vazia"


def renomear(origem, destino):
    o = Path(os.path.expandvars(origem)).expanduser()
    d = Path(os.path.expandvars(destino)).expanduser()
    if not o.exists():
        return f"nao achei {o}"
    o.rename(d)
    return f"renomeei {o.name} -> {d.name}"


def mover(origem, destino):
    o = Path(os.path.expandvars(origem)).expanduser()
    d = Path(os.path.expandvars(destino)).expanduser()
    if not o.exists():
        return f"nao achei {o}"
    shutil.move(str(o), str(d))
    return f"movi pra {d}"


def criar_pasta(caminho):
    p = Path(os.path.expandvars(caminho)).expanduser()
    p.mkdir(parents=True, exist_ok=True)
    return f"pasta criada: {p}"


def apagar(caminho):
    p = Path(os.path.expandvars(caminho)).expanduser()
    if guard.caminho_intocavel(p):
        return f"NAO. {p} e area do sistema."
    if not p.exists():
        return f"nao achei {p}"
    if p.is_dir():
        shutil.rmtree(p)
        return f"apaguei a pasta {p}"
    p.unlink()
    return f"apaguei {p}"


def escrever_arquivo(caminho, conteudo):
    p = Path(os.path.expandvars(caminho)).expanduser()
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(conteudo, encoding="utf-8")
    return f"escrevi {p} ({len(conteudo)} chars)"


def ler_arquivo(caminho, limite=4000):
    p = Path(os.path.expandvars(caminho)).expanduser()
    if not p.is_file():
        return f"nao achei {p}"
    bruto = p.read_bytes()[:200_000]
    if b"\x00" in bruto[:4096]:
        return f"{p.name} e binario ({p.stat().st_size // 1024} KB); so leio texto aqui"
    return bruto.decode("utf-8", "replace")[:limite]


# ---------- desinstalacao ----------
def achar_app(nome):
    from win import uninstall
    return uninstall.relatorio(uninstall.mapear(nome))


def desinstalar(nome, limpar_sobras=True):
    from win import uninstall
    m = uninstall.mapear(nome)
    if not m["registro"] and not m["pastas"]:
        return f"nao achei rastro de '{nome}'"
    return uninstall.relatorio(m) + "\n\n--- executando ---\n" + uninstall.executar(m, limpar_sobras)


# ---------- sistema ----------
def info_sistema():
    partes = [f"PC: {os.environ.get('COMPUTERNAME','?')}", f"usuario: {os.environ.get('USERNAME','?')}"]
    if psutil:
        partes.append(f"CPU: {psutil.cpu_percent(interval=0.5):.0f}%")
        m = psutil.virtual_memory()
        partes.append(f"RAM: {m.percent:.0f}% ({m.used/1073741824:.1f}/{m.total/1073741824:.1f} GB)")
        for d in psutil.disk_partitions():
            try:
                u = psutil.disk_usage(d.mountpoint)
                partes.append(f"{d.device} {u.percent:.0f}% cheio ({u.free/1073741824:.0f} GB livre)")
            except (PermissionError, OSError):
                pass
    return " | ".join(partes)


def meu_ip():
    pub = rodar_cmd("curl -s --max-time 10 https://api.ipify.org")
    local = rodar_cmd("ipconfig | findstr /i \"IPv4\"")
    return f"IP publico: {pub.strip() or 'nao consegui'}\n{local}"


def rede():
    return rodar_cmd("ipconfig /all")


# ---------- tempo e memoria ----------
def agora():
    d = datetime.now()
    dias = ["segunda", "terca", "quarta", "quinta", "sexta", "sabado", "domingo"]
    return f"{dias[d.weekday()]}, {d:%d/%m/%Y}, {d:%H:%M}"


def criar_lembrete(texto, quando):
    _, msg = scheduler.criar(texto, quando)
    return msg


def listar_lembretes():
    l = scheduler.listar()
    if not l:
        return "nenhum lembrete na fila"
    return "\n".join(f"- {x['texto']} ({datetime.fromisoformat(x['quando']):%d/%m %H:%M})" for x in l)


def anotar(titulo, texto):
    return f"anotei no vault: {memory.anotar(titulo, texto)}"


def lembrar_isso(chave, valor):
    memory.lembrar_fato(chave, valor)
    return f"guardei: {chave} = {valor}"


# ---------- internet ----------
def pesquisar(pergunta):
    from . import brain
    return brain.pesquisar(pergunta)


def abrir_site(url):
    if not url.startswith(("http://", "https://")):
        url = "https://" + url
    return abrir_app(url)


# ---------- auto-update ----------
def buscar_update():
    from . import updater
    tem, info = updater.tem_update()
    if not tem:
        return str(info)
    return f"tem update: {info['msg']} ({info['sha'][:7]}). Manda 'se atualiza' que eu aplico."


def aplicar_update():
    from . import updater
    ok, det = updater.atualizar()
    return det + ("\n(reinicia o Orion pra carregar)" if ok else "")


# ---------- registry ----------
G, C = guard.LIVRE, guard.CONFIRMA

TOOLS = {
    "rodar_cmd": rodar_cmd, "abrir_app": abrir_app, "fechar_app": fechar_app,
    "listar_processos": listar_processos, "app_aberto": app_aberto,
    "trocar_wallpaper": trocar_wallpaper, "listar_pasta": listar_pasta,
    "renomear": renomear, "mover": mover, "criar_pasta": criar_pasta,
    "apagar": apagar, "escrever_arquivo": escrever_arquivo, "ler_arquivo": ler_arquivo,
    "achar_app": achar_app, "desinstalar": desinstalar,
    "info_sistema": info_sistema, "meu_ip": meu_ip, "rede": rede,
    "agora": agora, "criar_lembrete": criar_lembrete, "listar_lembretes": listar_lembretes,
    "anotar": anotar, "lembrar_isso": lembrar_isso,
    "buscar_update": buscar_update, "aplicar_update": aplicar_update,
    "pesquisar": pesquisar, "abrir_site": abrir_site,
}

NIVEL = {
    "rodar_cmd": C, "abrir_app": G, "fechar_app": C, "listar_processos": G,
    "app_aberto": G, "trocar_wallpaper": G, "listar_pasta": G, "renomear": G,
    "mover": C, "criar_pasta": G, "apagar": C, "escrever_arquivo": G,
    "ler_arquivo": G, "achar_app": G, "desinstalar": C, "info_sistema": G,
    "meu_ip": G, "rede": G, "agora": G, "criar_lembrete": G,
    "listar_lembretes": G, "anotar": G, "lembrar_isso": G,
    "buscar_update": G, "aplicar_update": C,
    "pesquisar": G, "abrir_site": G,
}

DESCRICAO = """- rodar_cmd(comando, shell="cmd"|"powershell") comando cru
- abrir_app(nome) / fechar_app(nome) / app_aberto(nome) / listar_processos(filtro)
- trocar_wallpaper(caminho)
- listar_pasta(caminho) / criar_pasta(caminho) / renomear(origem,destino) / mover(origem,destino) / apagar(caminho)
- escrever_arquivo(caminho,conteudo) / ler_arquivo(caminho)
- achar_app(nome) mapeia rastros sem apagar | desinstalar(nome, limpar_sobras=true)
- info_sistema() / meu_ip() / rede()
- agora() hora e data | criar_lembrete(texto, quando) | listar_lembretes()
- anotar(titulo, texto) no Obsidian | lembrar_isso(chave, valor)
- buscar_update() ve se tem versao nova | aplicar_update() baixa e instala
- pesquisar(pergunta) busca na internet e resume com fontes | abrir_site(url)"""


def executar(tool, args):
    fn = TOOLS.get(tool)
    if not fn:
        return f"tool '{tool}' nao existe"
    try:
        return str(fn(**args))
    except TypeError as e:
        return f"argumentos errados pra {tool}: {e}"
    except Exception as e:
        return f"{tool} falhou: {type(e).__name__}: {e}"
