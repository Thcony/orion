"""Cerebro: Gemini via REST (stdlib). Sem SDK, sem pip, prompt curto."""
import json
import re
import time
import urllib.error
import urllib.request
from datetime import date

from . import config, memory

BASE = "https://generativelanguage.googleapis.com/v1beta"
_ultima_chamada = 0.0

SISTEMA = """Voce e o Orion, assistente pessoal do {chefe}, rodando no PC dele (Windows).
Fala portugues do Brasil, natural, direto, com gíria leve. Chama ele de "{chefe}".
Nao e robo de manual: puxa assunto, tira duvida, comenta. Resposta curta.

Voce tem maos no PC. Pra agir, devolve SEMPRE um JSON unico assim:
{{"fala": "o que voce diz em voz alta", "acoes": [{{"tool": "nome", "args": {{}}}}]}}

Sem acao nenhuma? Manda "acoes": [].
Nunca escreve nada fora do JSON. Nunca inventa tool que nao esta na lista.

TOOLS:
{tools}

Regras:
- Acao destrutiva (apagar, desinstalar) o sistema ja pergunta pro chefe. Nao finja que perguntou.
- Nao sabe o caminho de um app? Usa achar_app primeiro.
- Hora, data e lembrete sao locais: usa a tool, nao chuta.
- Se o pedido for vago, pergunta na "fala" e deixa "acoes" vazio.
"""


class Orcamento:
    """Contador diario pra nao estourar o free tier do Gemini."""

    def __init__(self):
        self.f = config.DATA / "orcamento.json"
        self._load()

    def _load(self):
        hoje = str(date.today())
        d = {}
        if self.f.exists():
            try:
                d = json.loads(self.f.read_text(encoding="utf-8"))
            except Exception:
                d = {}
        if d.get("dia") != hoje:
            d = {"dia": hoje, "usado": 0}
        self.d = d

    def pode(self):
        self._load()
        return self.d["usado"] < config.ORCAMENTO_DIA

    def gastar(self):
        self._load()
        self.d["usado"] += 1
        self.f.write_text(json.dumps(self.d), encoding="utf-8")

    def restante(self):
        self._load()
        return config.ORCAMENTO_DIA - self.d["usado"]


orcamento = Orcamento()


def _post(modelo, corpo):
    url = f"{BASE}/models/{modelo}:generateContent?key={config.API_KEY}"
    req = urllib.request.Request(
        url,
        data=json.dumps(corpo).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode("utf-8"))


def listar_modelos():
    """Nomes de modelo mudam. Isso mostra o que a sua chave realmente tem."""
    url = f"{BASE}/models?key={config.API_KEY}"
    with urllib.request.urlopen(url, timeout=30) as r:
        d = json.loads(r.read().decode("utf-8"))
    return [
        m["name"].split("/")[-1]
        for m in d.get("models", [])
        if "generateContent" in m.get("supportedGenerationMethods", [])
    ]


def _extrair_json(texto):
    t = re.sub(r"^```(?:json)?|```$", "", texto.strip(), flags=re.M).strip()
    try:
        return json.loads(t)
    except Exception:
        pass
    m = re.search(r"\{.*\}", t, re.S)
    if m:
        try:
            return json.loads(m.group(0))
        except Exception:
            pass
    return {"fala": texto.strip(), "acoes": []}


def pensar(entrada, tools_desc):
    """Manda o turno pro Gemini e devolve {'fala':..., 'acoes':[...]}."""
    global _ultima_chamada

    if not config.API_KEY:
        return {"fala": "Sem GEMINI_API_KEY no .env, chefe. Sem isso eu nao penso.", "acoes": []}

    if not orcamento.pode():
        return {
            "fala": f"Bati o teto de {config.ORCAMENTO_DIA} chamadas hoje, chefe. "
                    "Comandos locais (hora, abrir app, lembrete) ainda funcionam.",
            "acoes": [],
        }

    espera = config.INTERVALO_MIN - (time.time() - _ultima_chamada)
    if espera > 0:
        time.sleep(espera)

    sistema = SISTEMA.format(chefe=config.CHEFE, tools=tools_desc)
    contexto = "\n".join(memory.turnos_recentes())
    resumo = memory.resumo()

    corpo = {
        "systemInstruction": {"parts": [{"text": sistema}]},
        "contents": [{
            "role": "user",
            "parts": [{"text": f"MEMORIA:\n{resumo}\n\nCONVERSA:\n{contexto}\n\nAGORA:\n{entrada}"}],
        }],
        "generationConfig": {
            "temperature": 0.8,
            "maxOutputTokens": 800,
            "responseMimeType": "application/json",
        },
    }

    for modelo in (config.MODELO, config.MODELO_RESERVA):
        try:
            r = _post(modelo, corpo)
            _ultima_chamada = time.time()
            orcamento.gastar()
            txt = r["candidates"][0]["content"]["parts"][0]["text"]
            return _extrair_json(txt)
        except urllib.error.HTTPError as e:
            detalhe = e.read().decode("utf-8", "ignore")[:200]
            if e.code == 429:
                continue  # estourou cota: tenta o reserva
            if e.code == 404:
                continue  # nome de modelo errado: tenta o reserva
            return {"fala": f"Gemini recusou ({e.code}): {detalhe}", "acoes": []}
        except Exception as e:
            return {"fala": f"Deu erro falando com o Gemini: {e}", "acoes": []}

    return {
        "fala": "Os dois modelos recusaram (cota ou nome invalido). "
                "Roda `orion modelos` pra ver quais a sua chave aceita.",
        "acoes": [],
    }
