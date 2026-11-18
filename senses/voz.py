"""Voz do Orion: fala em voz alta com as vozes nativas do Windows (SAPI),
via PowerShell. Nao baixa nada, funciona offline. Fora do Windows, fica mudo
sem reclamar. Erro aqui nunca derruba a conversa.
"""
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from core import config

_proc = None
_ok = None


def _powershell():
    return shutil.which("powershell") or shutil.which("pwsh")


def disponivel():
    global _ok
    if _ok is None:
        _ok = os.name == "nt" and _powershell() is not None
    return _ok


def listar_vozes():
    ps = _powershell()
    if not ps:
        return []
    script = ("Add-Type -AssemblyName System.Speech;"
              "(New-Object System.Speech.Synthesis.SpeechSynthesizer)."
              "GetInstalledVoices().VoiceInfo | ForEach-Object "
              "{ \"$($_.Name) [$($_.Culture)]\" }")
    try:
        r = subprocess.run([ps, "-NoProfile", "-Command", script],
                           capture_output=True, text=True, timeout=30)
        return [l.strip() for l in r.stdout.splitlines() if l.strip()]
    except Exception:
        return []


def parar():
    """Corta a fala atual (barge-in quando o chefe manda outra coisa)."""
    global _proc
    if _proc and _proc.poll() is None:
        try:
            _proc.terminate()
        except Exception:
            pass
    _proc = None


def falar(texto):
    global _proc
    if not texto or not config.VOZ or not disponivel():
        return
    parar()
    ps = _powershell()
    tmp = Path(tempfile.gettempdir()) / "orion_fala.txt"
    try:
        tmp.write_text(texto, encoding="utf-8")
    except OSError:
        return

    nome = config.VOZ_NOME
    if nome:
        escolha = (f"$v=$s.GetInstalledVoices().VoiceInfo|"
                   f"Where-Object {{$_.Name -like '*{nome}*'}}|Select-Object -First 1;")
    else:  # sem nome: tenta uma voz em portugues
        escolha = ("$v=$s.GetInstalledVoices().VoiceInfo|"
                   "Where-Object {$_.Culture -like 'pt*'}|Select-Object -First 1;")

    script = ("Add-Type -AssemblyName System.Speech;"
              "$s=New-Object System.Speech.Synthesis.SpeechSynthesizer;"
              f"$s.Rate={int(config.VOZ_VELOCIDADE)};"
              + escolha +
              "if($v){$s.SelectVoice($v.Name)};"
              f"$s.Speak([IO.File]::ReadAllText('{tmp}',[Text.Encoding]::UTF8))")
    try:
        _proc = subprocess.Popen([ps, "-NoProfile", "-Command", script],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except Exception:
        _proc = None
