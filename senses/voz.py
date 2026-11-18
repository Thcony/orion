"""Voz do Orion.
Principal: voz do Gemini (TTS). Reserva automatica: vozes do Windows (SAPI),
quando a cota do TTS estoura, a rede cai ou o modelo some.
Erro aqui nunca derruba a conversa.
"""
import base64
import json
import os
import shutil
import subprocess
import tempfile
import threading
import urllib.error
import urllib.request
import wave
from datetime import date
from pathlib import Path

from core import config

_proc = None
_ok = None
_geracao = 0           # cada fala nova invalida a anterior (barge-in)
_tts_morto = None      # dia em que a cota do TTS estourou
MAX_CHARS = 500
TMP = Path(tempfile.gettempdir())


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
    global _proc, _geracao
    _geracao += 1
    try:
        import winsound
        winsound.PlaySound(None, 0)
    except Exception:
        pass
    if _proc and _proc.poll() is None:
        try:
            _proc.terminate()
        except Exception:
            pass
    _proc = None


# ---------------- Gemini TTS ----------------
def _gemini_pcm(texto):
    """Devolve PCM 16-bit 24 kHz mono, ou levanta erro."""
    url = (f"https://generativelanguage.googleapis.com/v1beta/models/"
           f"{config.VOZ_MODELO}:generateContent?key={config.API_KEY}")
    corpo = {
        "contents": [{"parts": [{"text":
            f"Fale em portugues do Brasil, tom natural e descontraido: {texto}"}]}],
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig":
                                             {"voiceName": config.VOZ_GEMINI}}},
        },
    }
    req = urllib.request.Request(url, data=json.dumps(corpo).encode("utf-8"),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=45) as r:
        d = json.loads(r.read().decode("utf-8"))
    for parte in d["candidates"][0]["content"]["parts"]:
        if "inlineData" in parte:
            return base64.b64decode(parte["inlineData"]["data"])
    raise RuntimeError("TTS sem audio")


def _wav(pcm, caminho):
    with wave.open(str(caminho), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(24000)
        w.writeframes(pcm)
    return caminho


def _tocar(caminho):
    import winsound
    winsound.PlaySound(str(caminho), winsound.SND_FILENAME)  # bloqueia a thread, nao o terminal


def _gemini_usavel():
    return (config.VOZ_MOTOR == "gemini" and config.API_KEY
            and _tts_morto != str(date.today()))


def _thread_gemini(texto, minha):
    global _tts_morto
    try:
        pcm = _gemini_pcm(texto)
        if minha != _geracao:
            return  # chefe ja mandou outra coisa
        _tocar(_wav(pcm, TMP / "orion_fala.wav"))
    except urllib.error.HTTPError as e:
        if e.code in (429, 404, 403):
            _tts_morto = str(date.today())  # para de tentar hoje, vai de SAPI
        if minha == _geracao:
            _sapi(texto)
    except Exception:
        if minha == _geracao:
            _sapi(texto)


# ---------------- entrada ----------------
def falar(texto):
    if not texto or not config.VOZ or not disponivel():
        return
    parar()
    texto = texto[:MAX_CHARS]
    if _gemini_usavel():
        threading.Thread(target=_thread_gemini, args=(texto, _geracao), daemon=True).start()
    else:
        _sapi(texto)


def motor_atual():
    if not disponivel():
        return "mudo"
    return f"Gemini ({config.VOZ_GEMINI})" if _gemini_usavel() else "Windows (SAPI)"


def _sapi(texto):
    global _proc
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
