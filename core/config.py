"""Orion - configuracao central. Zero dependencia externa."""
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data"
DATA.mkdir(exist_ok=True)
ENV = ROOT / ".env"

# Valores de exemplo do .env.example: se aparecerem, valem como vazio.
PLACEHOLDER = ("SEU_USUARIO", "cole_sua_chave")


def _carregar_env():
    if not ENV.exists():
        return
    for linha in ENV.read_text(encoding="utf-8-sig", errors="replace").splitlines():
        linha = linha.strip()
        if not linha or linha.startswith("#") or "=" not in linha:
            continue
        k, v = linha.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))


def _env(nome, padrao=""):
    v = os.environ.get(nome, "").strip()
    if not v or any(p in v for p in PLACEHOLDER):
        return padrao
    return v


def _documentos():
    """Pasta Documentos real, respeitando redirecionamento do OneDrive."""
    if os.name == "nt":
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                                r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders") as k:
                p = Path(os.path.expandvars(winreg.QueryValueEx(k, "Personal")[0]))
                if p.is_dir():
                    return p
        except OSError:
            pass
    return Path.home() / "Documents"


_carregar_env()

API_KEY = _env("GEMINI_API_KEY")
MODELO = _env("ORION_MODELO", "gemini-2.5-flash-lite")
MODELO_RESERVA = _env("ORION_MODELO_RESERVA")  # vazio = descobre sozinho
VAULT = Path(_env("ORION_VAULT", str(_documentos() / "Orion-Vault")))
CHEFE = _env("ORION_CHEFE", "chefe")
ORCAMENTO_DIA = int(_env("ORION_ORCAMENTO_DIA", "400"))
INTERVALO_MIN = float(_env("ORION_INTERVALO_MIN", "1.5"))
TURNOS_CONTEXTO = int(_env("ORION_TURNOS_CONTEXTO", "8"))
SEM_FREIO = _env("ORION_SEM_FREIO", "0") == "1"
AUTO_UPDATE = _env("ORION_AUTO_UPDATE", "1") == "1"

# Voz (Windows SAPI). VELOCIDADE: -10 (lento) a 10 (rapido). NOME: ex "Maria".
VOZ = _env("ORION_VOZ", "1") == "1"
VOZ_NOME = _env("ORION_VOZ_NOME")          # voz do Windows (reserva)
VOZ_MOTOR = _env("ORION_VOZ_MOTOR", "gemini")   # gemini | windows
VOZ_GEMINI = _env("ORION_VOZ_GEMINI", "Charon")  # Charon=grave; Orus, Fenrir, Puck, Kore...
VOZ_MODELO = _env("ORION_VOZ_MODELO", "gemini-2.5-flash-preview-tts")
VOZ_VELOCIDADE = int(_env("ORION_VOZ_VELOCIDADE", "0"))


def salvar_env(chave, valor):
    """Grava/atualiza uma variavel no .env sem mexer no resto."""
    linhas = ENV.read_text(encoding="utf-8-sig").splitlines() if ENV.exists() else []
    novo, achou = [], False
    for l in linhas:
        if l.strip().split("=", 1)[0].strip() == chave:
            novo.append(f"{chave}={valor}")
            achou = True
        else:
            novo.append(l)
    if not achou:
        novo.append(f"{chave}={valor}")
    ENV.write_text("\n".join(novo) + "\n", encoding="utf-8")
    os.environ[chave] = valor
