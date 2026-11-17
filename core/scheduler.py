"""Lembretes e agendamentos locais. Sem Google Calendar, sem nuvem."""
import json
import threading
import time
from datetime import datetime, timedelta

from . import config

ARQ = config.DATA / "lembretes.json"
_lock = threading.Lock()


def _ler():
    if not ARQ.exists():
        return []
    try:
        return json.loads(ARQ.read_text(encoding="utf-8"))
    except Exception:
        return []


def _gravar(l):
    ARQ.write_text(json.dumps(l, ensure_ascii=False, indent=2), encoding="utf-8")


def quando(texto):
    """Aceita 'em 10 min', 'em 2h', '18:30', '2026-11-20 09:00'."""
    t = texto.strip().lower()
    agora = datetime.now()
    import re
    m = re.match(r"em\s+(\d+)\s*(min|minuto|minutos|h|hora|horas|seg|segundos?)", t)
    if m:
        n = int(m.group(1))
        u = m.group(2)
        if u.startswith("min"):
            return agora + timedelta(minutes=n)
        if u.startswith("h"):
            return agora + timedelta(hours=n)
        return agora + timedelta(seconds=n)
    m = re.match(r"^(\d{1,2}):(\d{2})$", t)
    if m:
        alvo = agora.replace(hour=int(m.group(1)), minute=int(m.group(2)),
                             second=0, microsecond=0)
        return alvo if alvo > agora else alvo + timedelta(days=1)
    for fmt in ("%Y-%m-%d %H:%M", "%d/%m/%Y %H:%M", "%d/%m %H:%M"):
        try:
            d = datetime.strptime(t, fmt)
            return d.replace(year=agora.year) if "%Y" not in fmt else d
        except ValueError:
            continue
    return None


def criar(texto, quando_txt):
    alvo = quando(quando_txt)
    if not alvo:
        return None, f"nao entendi o horario '{quando_txt}'"
    with _lock:
        l = _ler()
        l.append({"texto": texto, "quando": alvo.isoformat(), "feito": False})
        _gravar(l)
    return alvo, f"marcado pra {alvo:%d/%m %H:%M}"


def listar():
    return [x for x in _ler() if not x["feito"]]


def vencidos():
    agora = datetime.now()
    saiu = []
    with _lock:
        l = _ler()
        for x in l:
            if not x["feito"] and datetime.fromisoformat(x["quando"]) <= agora:
                x["feito"] = True
                saiu.append(x)
        if saiu:
            _gravar(l)
    return saiu


def vigiar(avisar, intervalo=20):
    """Thread que dispara o callback quando um lembrete vence."""
    def loop():
        while True:
            for x in vencidos():
                try:
                    avisar(x["texto"])
                except Exception:
                    pass
            time.sleep(intervalo)
    t = threading.Thread(target=loop, daemon=True)
    t.start()
    return t
