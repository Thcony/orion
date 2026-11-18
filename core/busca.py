"""Plano B de busca: DuckDuckGo HTML (sem chave, stdlib).
Usado quando a busca do Gemini (grounding) nao esta liberada pra chave.
"""
import html
import re
import urllib.parse
import urllib.request

UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36"


def _limpa(t):
    return html.unescape(re.sub(r"<[^>]+>", "", t)).strip()


def _url_real(href):
    href = html.unescape(href)
    q = urllib.parse.urlparse(href if "//" in href[:8] else "https:" + href).query
    u = urllib.parse.parse_qs(q).get("uddg")
    return u[0] if u else href


def parse(pagina, n=6):
    blocos = re.split(r'<div[^>]+class="[^"]*result__body', pagina)[1:]
    res = []
    for b in blocos:
        t = re.search(r'class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>', b, re.S)
        if not t:
            continue
        sn = re.search(r'class="result__snippet"[^>]*>(.*?)</a>', b, re.S)
        url = _url_real(t.group(1))
        if "duckduckgo.com/y.js" in url:  # anuncio
            continue
        res.append({"titulo": _limpa(t.group(2)), "url": url,
                    "trecho": _limpa(sn.group(1)) if sn else ""})
        if len(res) >= n:
            break
    return res


def buscar(termo, n=6):
    url = "https://html.duckduckgo.com/html/?" + urllib.parse.urlencode({"q": termo, "kl": "br-pt"})
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept-Language": "pt-BR,pt;q=0.9"})
    with urllib.request.urlopen(req, timeout=20) as r:
        pagina = r.read().decode("utf-8", "replace")
    if "anomaly" in pagina[:5000] and "result__a" not in pagina:
        raise RuntimeError("DuckDuckGo pediu captcha")
    return parse(pagina, n)


def texto_pagina(url, limite=4000):
    """Texto puro de uma pagina (sem script/estilo), cortado. Vazio se falhar."""
    try:
        req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept-Language": "pt-BR"})
        with urllib.request.urlopen(req, timeout=15) as r:
            if "html" not in (r.headers.get("Content-Type") or ""):
                return ""
            bruto = r.read(600_000).decode("utf-8", "replace")
    except Exception:
        return ""
    bruto = re.sub(r"(?is)<(script|style|noscript|svg|nav|footer|header)[^>]*>.*?</\1>", " ", bruto)
    t = html.unescape(re.sub(r"<[^>]+>", " ", bruto))
    return re.sub(r"\s+", " ", t).strip()[:limite]
