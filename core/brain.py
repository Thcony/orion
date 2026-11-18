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
    url = f"{BASE}/models?pageSize=1000&key={config.API_KEY}"
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


CACHE = config.DATA / "modelo.json"
RUINS = ("image", "tts", "live", "embedding", "audio", "vision", "learnlm",
         "aqa", "exp", "robotics", "computer-use", "native", "thinking")


def ranquear(nomes):
    """Modelos de texto da chave, do mais barato/estavel pro menos."""
    ok = [m for m in nomes if m.startswith("gemini") and not any(r in m for r in RUINS)]
    ok.sort(reverse=True)  # nome maior = versao mais nova
    ok.sort(key=lambda m: (0 if "flash-lite" in m else 1 if "flash" in m else 2, "preview" in m))
    return ok


def _cache():
    try:
        return json.loads(CACHE.read_text(encoding="utf-8"))
    except Exception:
        return {}


def _salvar_cache(**kv):
    d = _cache()
    d.update({k: sorted(v) if isinstance(v, set) else v for k, v in kv.items()})
    try:
        CACHE.write_text(json.dumps(d), encoding="utf-8")
    except OSError:
        pass


def _texto(r):
    c = (r.get("candidates") or [{}])[0]
    partes = (c.get("content") or {}).get("parts") or []
    t = "".join(x.get("text", "") for x in partes if not x.get("thought"))
    if not t.strip():
        raise RuntimeError(f"resposta vazia ({c.get('finishReason', 'bloqueada')})")
    return t


def pensar(entrada, tools_desc):
    """Manda o turno pro Gemini e devolve {'fala':..., 'acoes':[...]}."""
    global _ultima_chamada

    if not config.API_KEY:
        return {"fala": "Sem chave do Gemini, chefe. Pega uma gratis em aistudio.google.com/apikey "
                        "e roda no terminal: orion chave SUA_CHAVE", "acoes": []}

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
            "maxOutputTokens": 2048,
            "responseMimeType": "application/json",
        },
    }

    cache = _cache()
    mortos = set(cache.get("mortos", []))
    fila = [m for m in dict.fromkeys([config.MODELO, cache.get("modelo"), config.MODELO_RESERVA])
            if m and m not in mortos]
    tentados, descobriu, cota = set(), False, False

    while True:
        if not fila:
            if descobriu:
                break
            descobriu = True
            try:
                fila = [m for m in ranquear(listar_modelos())
                        if m not in mortos and m not in tentados][:2]
            except Exception:
                fila = []
            if not fila:
                break

        modelo = fila.pop(0)
        tentados.add(modelo)
        try:
            r = _post(modelo, corpo)
            _ultima_chamada = time.time()
            orcamento.gastar()
            if modelo != config.MODELO:
                _salvar_cache(modelo=modelo)
            return _extrair_json(_texto(r))
        except urllib.error.HTTPError as e:
            detalhe = e.read().decode("utf-8", "ignore")
            if e.code == 404 or (e.code == 400 and "not found" in detalhe.lower()):
                mortos.add(modelo)
                _salvar_cache(mortos=mortos)
                continue
            if e.code in (429, 500, 503):
                cota = cota or e.code == 429
                continue
            if "API_KEY" in detalhe or "API key" in detalhe:
                return {"fala": "Chave do Gemini invalida, chefe. Pega outra em "
                                "aistudio.google.com/apikey e roda: orion chave SUA_CHAVE",
                        "acoes": []}
            return {"fala": f"Gemini recusou ({e.code}): {detalhe[:200]}", "acoes": []}
        except Exception as e:
            return {"fala": f"Deu erro falando com o Gemini: {e}", "acoes": []}

    if cota:
        return {"fala": "Cota do Gemini estourou (por minuto ou do dia), chefe. Espera um pouco; "
                        "hora, lembrete e abrir app seguem funcionando.", "acoes": []}
    return {"fala": "Nenhum modelo respondeu. Roda `orion modelos` pra ver o que tua chave aceita.",
            "acoes": []}


def pesquisar(pergunta):
    """Busca no Google via grounding do Gemini. Devolve resumo + fontes."""
    global _ultima_chamada
    if not config.API_KEY:
        return "sem chave do Gemini (orion chave SUA_CHAVE)"
    if not orcamento.pode():
        return "bati o teto de chamadas de hoje"
    corpo = {
        "contents": [{"role": "user", "parts": [{"text":
            f"Pesquise e responda em portugues do Brasil, curto e direto: {pergunta}"}]}],
        "tools": [{"google_search": {}}],
        "generationConfig": {"temperature": 0.3, "maxOutputTokens": 1024},
    }
    cache = _cache()
    mortos = set(cache.get("mortos", []))
    fila = [m for m in dict.fromkeys([config.MODELO, cache.get("modelo"), config.MODELO_RESERVA])
            if m and m not in mortos]
    for modelo in fila:
        try:
            r = _post(modelo, corpo)
        except urllib.error.HTTPError as e:
            if e.code in (400, 404, 429, 500, 503):
                continue  # modelo sem busca, cota ou fora do ar: tenta o proximo
            return f"busca falhou ({e.code})"
        except Exception as e:
            return f"sem rede pra pesquisar: {type(e).__name__}"
        _ultima_chamada = time.time()
        orcamento.gastar()
        try:
            texto = _texto(r)
        except RuntimeError as e:
            return str(e)
        meta = (r.get("candidates") or [{}])[0].get("groundingMetadata") or {}
        fontes = []
        for c in meta.get("groundingChunks", [])[:5]:
            w = c.get("web") or {}
            if w.get("title"):
                fontes.append(f"- {w['title']}")
        return texto.strip() + ("\n\nFontes:\n" + "\n".join(fontes) if fontes else "")
    return "nenhum modelo liberou a busca agora (cota ou modelo sem suporte)"


def resumir(memoria_atual, conversa):
    """Funde a conversa nova na memoria de longo prazo. Devolve texto ou None."""
    if not config.API_KEY or not orcamento.pode() or not conversa.strip():
        return None
    corpo = {
        "contents": [{"role": "user", "parts": [{"text":
            "Voce mantem a memoria de longo prazo do Orion sobre o chefe.\n"
            "Atualize o documento abaixo com o que for DURAVEL da conversa nova: "
            "fatos sobre ele, projetos, preferencias, pedidos em aberto. "
            "Ignore papo passageiro. Mantenha o que ja existe, corrija o que mudou. "
            "Markdown, secoes '## Sobre o chefe', '## Projetos', '## Pendencias', "
            "'## Resumo das conversas' (1 linha por dia, com data). Max 2500 caracteres. "
            "Responda SO o documento.\n\n"
            f"DOCUMENTO ATUAL:\n{memoria_atual}\n\nCONVERSA NOVA:\n{conversa}"}]}],
        "generationConfig": {"temperature": 0.2, "maxOutputTokens": 2048},
    }
    cache = _cache()
    mortos = set(cache.get("mortos", []))
    for modelo in [m for m in dict.fromkeys([config.MODELO, cache.get("modelo"), config.MODELO_RESERVA])
                   if m and m not in mortos]:
        try:
            r = _post(modelo, corpo)
            orcamento.gastar()
            t = _texto(r).strip()
            t = re.sub(r"^```(?:markdown|md)?\s*|\s*```$", "", t).strip()
            return t if t.startswith("#") else "# Orion - Memoria\n\n" + t
        except Exception:
            continue
    return None
