//! Cerebro: Gemini via REST. Porte do brain.py: mesma cota diaria
//! (data/orcamento.json) e mesmo cache de modelos (data/modelo.json).
use crate::config::Config;
use chrono::Local;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Conexao trocavel: a de verdade usa ureq; nos testes, uma falsa.
pub trait Http: Send + Sync {
    fn post(&self, url: &str, corpo: &Value) -> Result<(u16, String), String>;
    fn get(&self, url: &str) -> Result<(u16, String), String>;
    /// Streaming: entrega linha por linha enquanto chega (SSE). `cada` devolve
    /// false pra parar. Com status != 200 devolve o corpo inteiro como erro.
    fn post_linhas(&self, url: &str, corpo: &Value, cada: &mut dyn FnMut(&str) -> bool) -> Result<(u16, String), String> {
        let (s, txt) = self.post(url, corpo)?;
        if s == 200 {
            for l in txt.lines() {
                if !cada(l) { break; }
            }
            return Ok((200, String::new()));
        }
        Ok((s, txt))
    }
}

pub struct Rede {
    agente: ureq::Agent,
}

impl Rede {
    pub fn new() -> Self {
        let cfg = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build();
        Rede { agente: cfg.into() }
    }
}

impl Http for Rede {
    fn post(&self, url: &str, corpo: &Value) -> Result<(u16, String), String> {
        let mut r = self.agente.post(url).header("Content-Type", "application/json")
            .send_json(corpo).map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        Ok((status, r.body_mut().read_to_string().map_err(|e| e.to_string())?))
    }
    fn post_linhas(&self, url: &str, corpo: &Value, cada: &mut dyn FnMut(&str) -> bool) -> Result<(u16, String), String> {
        use std::io::BufRead;
        let mut r = self.agente.post(url).header("Content-Type", "application/json")
            .send_json(corpo).map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        if status != 200 {
            return Ok((status, r.body_mut().read_to_string().unwrap_or_default()));
        }
        let mut leitor = std::io::BufReader::new(r.body_mut().as_reader());
        let mut linha = String::new();
        loop {
            linha.clear();
            match leitor.read_line(&mut linha) {
                Ok(0) => break,
                Ok(_) => if !cada(linha.trim_end()) { break },
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok((200, String::new()))
    }
    fn get(&self, url: &str) -> Result<(u16, String), String> {
        let mut r = self.agente.get(url).call().map_err(|e| e.to_string())?;
        let status = r.status().as_u16();
        Ok((status, r.body_mut().read_to_string().map_err(|e| e.to_string())?))
    }
}

#[derive(Debug, Clone, Default)]
pub struct Acao {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone, Default)]
pub struct Resposta {
    pub fala: String,
    pub acoes: Vec<Acao>,
    pub ver_resultado: bool,
}

impl Resposta {
    pub fn so_fala(t: impl Into<String>) -> Self {
        Resposta { fala: t.into(), ..Default::default() }
    }
}

const SISTEMA: &str = r#"Voce e o Orion, assistente pessoal do {chefe}, rodando no PC dele (Windows).
Fala portugues do Brasil, natural, direto, com giria leve. Chama ele de "{chefe}".
Nao e robo de manual: puxa assunto, tira duvida, comenta. Resposta curta.

Voce tem maos no PC. Pra agir, devolve SEMPRE um JSON unico assim:
{"fala": "o que voce diz em voz alta", "acoes": [{"tool": "nome", "args": {}}]}

Sem acao nenhuma? Manda "acoes": [].
Nunca escreve nada fora do JSON. Nunca inventa tool que nao esta na lista.

TOOLS:
{tools}

Regras:
- Acao destrutiva (apagar, fechar app) o sistema ja pergunta pro chefe. Nao finja que perguntou.
- Hora e data sao locais: usa a tool, nao chuta.
- Se o pedido for vago, pergunta na "fala" e deixa "acoes" vazio.
- Precisa LER a saida das acoes pra responder (arquivo, processos, info do sistema,
  saida de comando)? Poe "ver_resultado": true no JSON; eu te devolvo os resultados
  e voce responde. Abrir/fechar app: nao precisa.
"#;

const RUINS: [&str; 13] = ["image", "tts", "live", "embedding", "audio", "vision", "learnlm",
    "aqa", "exp", "robotics", "computer-use", "native", "thinking"];

/// Modelos de texto da chave, do mais barato/estavel pro menos.
pub fn ranquear(nomes: &[String]) -> Vec<String> {
    let mut ok: Vec<String> = nomes.iter()
        .filter(|m| m.starts_with("gemini") && !RUINS.iter().any(|r| m.contains(r)))
        .cloned().collect();
    ok.sort_by(|a, b| b.cmp(a)); // nome maior = versao mais nova
    ok.sort_by_key(|m| (if m.contains("flash-lite") { 0 } else if m.contains("flash") { 1 } else { 2 }, m.contains("preview")));
    ok
}

/// Junta as partes de texto da resposta, ignorando "pensamento" do modelo.
pub fn texto(r: &Value) -> Result<String, String> {
    let c = &r["candidates"][0];
    let t: String = c["content"]["parts"].as_array().into_iter().flatten()
        .filter(|p| !p["thought"].as_bool().unwrap_or(false))
        .filter_map(|p| p["text"].as_str()).collect();
    if t.trim().is_empty() {
        return Err(format!("resposta vazia ({})", c["finishReason"].as_str().unwrap_or("bloqueada")));
    }
    Ok(t)
}

/// Tolerante: tira ```json, acha o primeiro {...}, e se nada servir vira so fala.
pub fn extrair(txt: &str) -> Resposta {
    let limpo = txt.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    let v: Option<Value> = serde_json::from_str(limpo).ok().or_else(|| {
        let (i, f) = (limpo.find('{')?, limpo.rfind('}')?);
        (i < f).then(|| serde_json::from_str(&limpo[i..=f]).ok()).flatten()
    });
    let Some(v) = v.filter(Value::is_object) else { return Resposta::so_fala(txt.trim()) };
    Resposta {
        fala: v["fala"].as_str().unwrap_or("").to_string(),
        acoes: v["acoes"].as_array().into_iter().flatten()
            .filter_map(|a| Some(Acao { tool: a["tool"].as_str()?.to_string(), args: a.get("args").cloned().unwrap_or(json!({})) }))
            .collect(),
        ver_resultado: v["ver_resultado"].as_bool().unwrap_or(false),
    }
}

pub enum Falha {
    SemChave,
    Teto,
    ChaveInvalida,
    Cota,
    NenhumModelo(Vec<String>),
    Outra(String),
}

impl Falha {
    pub fn fala(&self, teto: u32) -> String {
        match self {
            Falha::SemChave => "Sem chave do Gemini, chefe. Pega uma gratis em aistudio.google.com/apikey \
                e manda aqui: /chave SUA_CHAVE".into(),
            Falha::Teto => format!("Bati o teto de {teto} chamadas hoje, chefe. Coisas locais seguem funcionando."),
            Falha::ChaveInvalida => "Chave do Gemini invalida, chefe. Pega outra em aistudio.google.com/apikey \
                e manda: /chave SUA_CHAVE".into(),
            Falha::Cota => "Cota do Gemini estourou (por minuto ou do dia), chefe. Espera um pouco.".into(),
            Falha::NenhumModelo(e) => format!("Nenhum modelo respondeu: {}", e.join("; ")),
            Falha::Outra(e) => e.clone(),
        }
    }
}

pub struct Cerebro {
    pub cfg: Config,
    http: Box<dyn Http>,
    ultima: Mutex<Option<Instant>>,
}

impl Cerebro {
    pub fn new(cfg: Config, http: Box<dyn Http>) -> Self {
        Cerebro { cfg, http, ultima: Mutex::new(None) }
    }

    // ---------- cota diaria (mesmo arquivo do Python) ----------
    fn orcamento(&self) -> (String, u32) {
        let hoje = Local::now().format("%Y-%m-%d").to_string();
        let v: Value = fs::read_to_string(self.cfg.data.join("orcamento.json")).ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok()).unwrap_or_default();
        let usado = if v["dia"].as_str() == Some(&hoje) { v["usado"].as_u64().unwrap_or(0) as u32 } else { 0 };
        (hoje, usado)
    }
    pub fn restante(&self) -> u32 {
        self.cfg.orcamento_dia.saturating_sub(self.orcamento().1)
    }
    fn gastar(&self) {
        let (dia, usado) = self.orcamento();
        let _ = fs::write(self.cfg.data.join("orcamento.json"), json!({"dia": dia, "usado": usado + 1}).to_string());
    }

    // ---------- cache de modelos (mesmo arquivo do Python) ----------
    fn cache(&self) -> Value {
        fs::read_to_string(self.cfg.data.join("modelo.json")).ok()
            .and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({}))
    }
    fn salvar_cache(&self, chave: &str, valor: Value) {
        let mut c = self.cache();
        if !c.is_object() { c = json!({}); }
        c[chave] = valor;
        let _ = fs::write(self.cfg.data.join("modelo.json"), c.to_string());
    }

    pub fn listar_modelos(&self) -> Result<Vec<String>, String> {
        let (s, corpo) = self.http.get(&format!("{BASE}/models?pageSize=1000&key={}", self.cfg.api_key))?;
        if s != 200 {
            return Err(format!("{s}: {}", msg_erro(&corpo)));
        }
        let v: Value = serde_json::from_str(&corpo).map_err(|e| e.to_string())?;
        Ok(v["models"].as_array().into_iter().flatten()
            .filter(|m| m["supportedGenerationMethods"].as_array().is_some_and(|a| a.iter().any(|x| x == "generateContent")))
            .filter_map(|m| m["name"].as_str().map(|n| n.trim_start_matches("models/").to_string()))
            .collect())
    }

    fn esperar_intervalo(&self) {
        let mut u = self.ultima.lock().unwrap();
        if let Some(t) = *u {
            let min = Duration::from_secs_f32(self.cfg.intervalo_min.max(0.0));
            if let Some(falta) = min.checked_sub(t.elapsed()) {
                std::thread::sleep(falta);
            }
        }
        *u = Some(Instant::now());
    }

    /// Chamada com fila de modelos: 404 marca morto, 429/5xx pula,
    /// e se todos falharem por nome, descobre outro na lista da chave.
    pub fn chamar(&self, corpo: &Value) -> Result<Value, Falha> {
        if self.cfg.api_key.is_empty() {
            return Err(Falha::SemChave);
        }
        if self.restante() == 0 {
            return Err(Falha::Teto);
        }
        let cache = self.cache();
        let mut mortos: HashSet<String> = cache["mortos"].as_array().into_iter().flatten()
            .filter_map(|x| x.as_str().map(String::from)).collect();
        let mut fila: Vec<String> = Vec::new();
        for m in [Some(self.cfg.modelo.clone()), cache["modelo"].as_str().map(String::from), Some(self.cfg.modelo_reserva.clone())]
            .into_iter().flatten()
        {
            if !m.is_empty() && !mortos.contains(&m) && !fila.contains(&m) {
                fila.push(m);
            }
        }
        let (mut tentados, mut descobriu, mut cota, mut erros) = (HashSet::new(), false, false, Vec::new());
        loop {
            if fila.is_empty() {
                if descobriu { break; }
                descobriu = true;
                fila = self.listar_modelos().map(|l| ranquear(&l)).unwrap_or_default().into_iter()
                    .filter(|m| !mortos.contains(m) && !tentados.contains(m)).take(2).collect();
                if fila.is_empty() { break; }
            }
            let modelo = fila.remove(0);
            tentados.insert(modelo.clone());
            self.esperar_intervalo();
            let url = format!("{BASE}/models/{modelo}:generateContent?key={}", self.cfg.api_key);
            match self.http.post(&url, corpo) {
                Err(e) => return Err(Falha::Outra(format!("sem rede pro Gemini: {e}"))),
                Ok((200, txt)) => {
                    self.gastar();
                    if modelo != self.cfg.modelo {
                        self.salvar_cache("modelo", json!(modelo));
                    }
                    return serde_json::from_str(&txt).map_err(|e| Falha::Outra(format!("resposta estranha: {e}")));
                }
                Ok((s, txt)) => {
                    let m = msg_erro(&txt);
                    erros.push(format!("{modelo}: {s}"));
                    if s == 404 || (s == 400 && m.to_lowercase().contains("not found")) {
                        mortos.insert(modelo);
                        let mut v: Vec<&String> = mortos.iter().collect();
                        v.sort();
                        self.salvar_cache("mortos", json!(v));
                    } else if matches!(s, 429 | 500 | 503) {
                        cota |= s == 429;
                    } else if m.contains("API_KEY") || m.contains("API key") {
                        return Err(Falha::ChaveInvalida);
                    } else {
                        return Err(Falha::Outra(format!("Gemini recusou ({s}): {}", m.chars().take(200).collect::<String>())));
                    }
                }
            }
        }
        Err(if cota { Falha::Cota } else { Falha::NenhumModelo(erros) })
    }

    pub fn pensar(&self, entrada: &str, tools_desc: &str, contexto: &[String], resumo: &str) -> Resposta {
        let sistema = SISTEMA.replace("{chefe}", &self.cfg.chefe).replace("{tools}", tools_desc);
        let corpo = json!({
            "systemInstruction": {"parts": [{"text": sistema}]},
            "contents": [{"role": "user", "parts": [{"text":
                format!("MEMORIA:\n{resumo}\n\nCONVERSA:\n{}\n\nAGORA:\n{entrada}", contexto.join("\n"))}]}],
            "generationConfig": {"temperature": 0.8, "maxOutputTokens": 2048, "responseMimeType": "application/json"},
        });
        match self.chamar(&corpo) {
            Ok(r) => match texto(&r) {
                Ok(t) => extrair(&t),
                Err(e) => Resposta::so_fala(format!("O Gemini mandou {e}.")),
            },
            Err(f) => Resposta::so_fala(f.fala(self.cfg.orcamento_dia)),
        }
    }

    /// Funde a conversa nova na memoria de longo prazo.
    pub fn resumir(&self, memoria_atual: &str, conversa: &str) -> Option<String> {
        if conversa.trim().is_empty() { return None; }
        let corpo = json!({
            "contents": [{"role": "user", "parts": [{"text": format!(
                "Voce mantem a memoria de longo prazo do Orion sobre o chefe.\n\
                 Atualize o documento abaixo com o que for DURAVEL da conversa nova: \
                 fatos sobre ele, projetos, preferencias, pedidos em aberto. \
                 Ignore papo passageiro. Mantenha o que ja existe, corrija o que mudou. \
                 Markdown, secoes '## Sobre o chefe', '## Projetos', '## Pendencias', \
                 '## Resumo das conversas' (1 linha por dia, com data). Max 2500 caracteres. \
                 Responda SO o documento.\n\nDOCUMENTO ATUAL:\n{memoria_atual}\n\nCONVERSA NOVA:\n{conversa}")}]}],
            "generationConfig": {"temperature": 0.2, "maxOutputTokens": 2048},
        });
        let t = texto(&self.chamar(&corpo).ok()?).ok()?;
        let t = t.trim().trim_start_matches("```markdown").trim_start_matches("```md")
            .trim_start_matches("```").trim_end_matches("```").trim().to_string();
        Some(if t.starts_with('#') { t } else { format!("# Orion - Memoria\n\n{t}") })
    }
}

fn msg_erro(corpo: &str) -> String {
    serde_json::from_str::<Value>(corpo).ok()
        .and_then(|v| v["error"]["message"].as_str().map(String::from))
        .unwrap_or_else(|| corpo.chars().take(200).collect())
}

#[cfg(test)]
pub mod testes {
    use super::*;
    use std::sync::Arc;

    type Regra = dyn Fn(&str, &Value) -> (u16, String) + Send + Sync;
    pub struct Falsa {
        pub regra: Box<Regra>,
        pub modelos: Vec<&'static str>,
        pub chamadas: Arc<Mutex<Vec<String>>>,
    }
    impl Http for Falsa {
        fn post(&self, url: &str, corpo: &Value) -> Result<(u16, String), String> {
            let m = url.split("/models/").nth(1).unwrap_or("").split(':').next().unwrap_or("").to_string();
            self.chamadas.lock().unwrap().push(m.clone());
            Ok((self.regra)(&m, corpo))
        }
        fn get(&self, _: &str) -> Result<(u16, String), String> {
            let ms: Vec<Value> = self.modelos.iter()
                .map(|n| json!({"name": format!("models/{n}"), "supportedGenerationMethods": ["generateContent"]})).collect();
            Ok((200, json!({"models": ms}).to_string()))
        }
    }

    pub fn ok(fala: &str) -> (u16, String) {
        (200, json!({"candidates": [{"content": {"parts": [
            {"thought": true, "text": "pensando..."},
            {"text": json!({"fala": fala, "acoes": []}).to_string()}]}}]}).to_string())
    }

    pub fn cerebro(nome: &str, regra: Box<Regra>) -> (Cerebro, Arc<Mutex<Vec<String>>>) {
        let d = std::env::temp_dir().join(nome);
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(".env"), "GEMINI_API_KEY=AIzaTESTE\nORION_MODELO_RESERVA=gemini-2.0-flash\nORION_INTERVALO_MIN=0\n").unwrap();
        let chamadas = Arc::new(Mutex::new(vec![]));
        let modelos = vec!["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.0-flash",
            "gemini-2.5-flash-image", "gemini-2.5-flash-preview-tts", "gemini-embedding-001", "gemma-3-27b-it",
            "gemini-3.1-flash-lite-preview", "gemini-flash-lite-latest", "gemini-2.0-flash-thinking-exp"];
        (Cerebro::new(Config::de(d), Box::new(Falsa { regra, modelos, chamadas: chamadas.clone() })), chamadas)
    }

    #[test]
    fn ranking_igual_ao_python() {
        let n: Vec<String> = ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.0-flash",
            "gemini-2.5-flash-image", "gemini-embedding-001", "gemma-3-27b-it", "gemini-3.1-flash-lite-preview",
            "gemini-flash-lite-latest"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ranquear(&n), ["gemini-flash-lite-latest", "gemini-2.5-flash-lite", "gemini-3.1-flash-lite-preview",
            "gemini-2.5-flash", "gemini-2.0-flash", "gemini-2.5-pro"]);
    }

    #[test]
    fn modelo_sumiu_descobre_sozinho_e_lembra() {
        let (c, chamadas) = cerebro("orion_cb_404", Box::new(|m, _| {
            if m == "gemini-2.5-flash-lite" || m == "gemini-2.0-flash" { (404, r#"{"error":{"message":"not found"}}"#.into()) }
            else { ok("Fala, chefe!") }
        }));
        let r = c.pensar("Salve", "-", &[], "");
        assert_eq!(r.fala, "Fala, chefe!"); // e ignorou o "pensando..."
        assert_eq!(*chamadas.lock().unwrap(), ["gemini-2.5-flash-lite", "gemini-2.0-flash", "gemini-flash-lite-latest"]);
        chamadas.lock().unwrap().clear();
        c.pensar("e ai", "-", &[], "");
        assert_eq!(*chamadas.lock().unwrap(), ["gemini-flash-lite-latest"]); // nao repete 404
        assert_eq!(c.restante(), 398);
    }

    #[test]
    fn cota_e_chave_invalida_viram_frase_clara() {
        let (c, _) = cerebro("orion_cb_429", Box::new(|_, _| (429, "{}".into())));
        assert!(c.pensar("oi", "-", &[], "").fala.contains("Cota do Gemini estourou"));
        let (c, _) = cerebro("orion_cb_key", Box::new(|_, _| (400, r#"{"error":{"message":"API key not valid. API_KEY_INVALID"}}"#.into())));
        assert!(c.pensar("oi", "-", &[], "").fala.contains("invalida"));
    }

    #[test]
    fn extrair_tolerante() {
        let r = extrair("```json\n{\"fala\":\"ok\",\"acoes\":[{\"tool\":\"agora\",\"args\":{}}],\"ver_resultado\":true}\n```");
        assert_eq!((r.fala.as_str(), r.acoes.len(), r.acoes[0].tool.as_str(), r.ver_resultado), ("ok", 1, "agora", true));
        assert_eq!(extrair("so texto, sem json").fala, "so texto, sem json");
        assert_eq!(extrair("lixo antes {\"fala\":\"achei\",\"acoes\":[]} lixo depois").fala, "achei");
    }
}
