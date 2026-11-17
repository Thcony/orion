"""Orion - configuracao central. Zero dependencia externa."""
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "data"
DATA.mkdir(exist_ok=True)


def _carregar_env():
    f = ROOT / ".env"
    if not f.exists():
        return
    for linha in f.read_text(encoding="utf-8").splitlines():
        linha = linha.strip()
        if not linha or linha.startswith("#") or "=" not in linha:
            continue
        k, v = linha.split("=", 1)
        os.environ.setdefault(k.strip(), v.strip().strip('"').strip("'"))


_carregar_env()

API_KEY = os.environ.get("GEMINI_API_KEY", "")

# Flash-Lite = maior folga diaria no free tier. Troque se quiser.
MODELO = os.environ.get("ORION_MODELO", "gemini-2.5-flash-lite")
MODELO_RESERVA = os.environ.get("ORION_MODELO_RESERVA", "gemini-2.0-flash")

# Obsidian: onde ficam as conversas e a memoria. Nada sai da maquina.
VAULT = Path(os.environ.get("ORION_VAULT", str(Path.home() / "Documents" / "Orion-Vault")))

CHEFE = os.environ.get("ORION_CHEFE", "chefe")

# Economia de free tier
ORCAMENTO_DIA = int(os.environ.get("ORION_ORCAMENTO_DIA", "400"))
INTERVALO_MIN = float(os.environ.get("ORION_INTERVALO_MIN", "1.5"))
TURNOS_CONTEXTO = int(os.environ.get("ORION_TURNOS_CONTEXTO", "8"))

# Modo: se True, acoes destrutivas rodam sem perguntar (NAO recomendado)
SEM_FREIO = os.environ.get("ORION_SEM_FREIO", "0") == "1"
