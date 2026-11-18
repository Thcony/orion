//! Voz do Orion. Principal: Gemini TTS (Charon). Reserva automatica: vozes
//! do Windows (SAPI) quando a cota do TTS estoura, a rede cai ou o modelo some.
//! Mandar mensagem nova corta a fala (barge-in). Erro aqui nunca derruba nada.
use crate::cerebro::Http;
use crate::config::Config;
use base64::Engine;
use chrono::Local;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_CHARS: usize = 500;

/// O que sai no alto-falante. Separado pra poder testar sem som.
#[cfg_attr(not(windows), allow(dead_code))] // no Linux (testes) o WAV nao toca
pub enum Som {
    Wav(Vec<u8>),
    Sapi(String),
}

pub struct Voz {
    cfg: Config,
    http: Arc<dyn Http>,
    tocar: Box<dyn Fn(Som) + Send + Sync>,
    geracao: AtomicU64,
    tocando: AtomicBool,
    tts_morto: Mutex<Option<String>>, // dia em que o TTS do Gemini recusou
    pub ligada: AtomicBool,
}

pub fn wav(pcm: &[u8]) -> Vec<u8> {
    let mut w = Vec::with_capacity(44 + pcm.len());
    let n = pcm.len() as u32;
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + n).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&24_000u32.to_le_bytes());
    w.extend_from_slice(&48_000u32.to_le_bytes()); // bytes/s
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&n.to_le_bytes());
    w.extend_from_slice(pcm);
    w
}

impl Voz {
    pub fn new(cfg: Config, http: Arc<dyn Http>) -> Arc<Self> {
        Self::com_saida(cfg, http, Box::new(tocar_de_verdade))
    }

    pub fn com_saida(cfg: Config, http: Arc<dyn Http>, tocar: Box<dyn Fn(Som) + Send + Sync>) -> Arc<Self> {
        let ligada = AtomicBool::new(cfg.voz);
        Arc::new(Voz { cfg, http, tocar, geracao: AtomicU64::new(0), tocando: AtomicBool::new(false),
                       tts_morto: Mutex::new(None), ligada })
    }

    fn gemini_usavel(&self) -> bool {
        let hoje = Local::now().format("%Y-%m-%d").to_string();
        self.cfg.voz_motor == "gemini" && !self.cfg.api_key.is_empty()
            && self.tts_morto.lock().unwrap().as_deref() != Some(hoje.as_str())
    }

    pub fn motor(&self) -> String {
        if !self.ligada.load(Ordering::Relaxed) {
            "mudo".into()
        } else if self.gemini_usavel() {
            format!("Gemini ({})", self.cfg.voz_gemini)
        } else {
            "Windows".into()
        }
    }

    pub fn falando(&self) -> bool {
        self.tocando.load(Ordering::Relaxed) || sapi_ativo()
    }

    /// Corta a fala atual e invalida a que ainda esta baixando.
    pub fn parar(&self) {
        self.geracao.fetch_add(1, Ordering::SeqCst);
        parar_de_verdade();
    }

    pub fn falar(self: &Arc<Self>, texto: &str) {
        if texto.trim().is_empty() || !self.ligada.load(Ordering::Relaxed) {
            return;
        }
        self.parar();
        let minha = self.geracao.load(Ordering::SeqCst);
        let texto: String = texto.chars().take(MAX_CHARS).collect();
        let eu = self.clone();
        std::thread::spawn(move || {
            let som = if eu.gemini_usavel() {
                match eu.gemini(&texto) {
                    Ok(pcm) => Som::Wav(wav(&pcm)),
                    Err(recusou) => {
                        if recusou { // cota/modelo/permissao: para de tentar hoje
                            *eu.tts_morto.lock().unwrap() = Some(Local::now().format("%Y-%m-%d").to_string());
                        }
                        Som::Sapi(texto)
                    }
                }
            } else {
                Som::Sapi(texto)
            };
            if minha != eu.geracao.load(Ordering::SeqCst) {
                return; // o chefe ja mandou outra coisa enquanto baixava
            }
            eu.tocando.store(true, Ordering::Relaxed);
            (eu.tocar)(som);
            eu.tocando.store(false, Ordering::Relaxed);
        });
    }

    /// Ok(pcm 16-bit 24 kHz) ou Err(true se o Gemini RECUSOU, false se foi rede).
    fn gemini(&self, texto: &str) -> Result<Vec<u8>, bool> {
        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                          self.cfg.voz_modelo, self.cfg.api_key);
        let corpo = json!({
            "contents": [{"parts": [{"text": format!("Fale em portugues do Brasil, tom natural e descontraido: {texto}")}]}],
            "generationConfig": {"responseModalities": ["AUDIO"],
                "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": self.cfg.voz_gemini}}}},
        });
        let (status, txt) = self.http.post(&url, &corpo).map_err(|_| false)?;
        if status != 200 {
            return Err(matches!(status, 400 | 403 | 404 | 429));
        }
        let v: Value = serde_json::from_str(&txt).map_err(|_| false)?;
        v["candidates"][0]["content"]["parts"].as_array().into_iter().flatten()
            .find_map(|p| p["inlineData"]["data"].as_str())
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            .ok_or(false)
    }

    /// Script da voz do Windows (texto embutido com aspas simples escapadas).
    pub fn script_sapi(&self, texto: &str) -> String {
        let filtro = if self.cfg.voz_nome.is_empty() {
            "$_.Culture -like 'pt*'".to_string()
        } else {
            format!("$_.Name -like '*{}*'", self.cfg.voz_nome.replace('\'', "''"))
        };
        format!("Add-Type -AssemblyName System.Speech; $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; \
                 $s.Rate={}; $v=$s.GetInstalledVoices().VoiceInfo | Where-Object {{ {filtro} }} | Select-Object -First 1; \
                 if($v){{$s.SelectVoice($v.Name)}}; $s.Speak('{}')",
                self.cfg.voz_velocidade.clamp(-10, 10), texto.replace('\'', "''"))
    }
}

// ---------------- saida de som de verdade ----------------
static SAPI: Mutex<Option<std::process::Child>> = Mutex::new(None);
static CFG_SAPI: Mutex<Option<String>> = Mutex::new(None); // ultimo script montado

fn sapi_ativo() -> bool {
    SAPI.lock().unwrap().as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)))
}

#[cfg(windows)]
mod win {
    #[link(name = "winmm")]
    unsafe extern "system" {
        pub fn PlaySoundW(som: *const u16, modulo: *mut core::ffi::c_void, flags: u32) -> i32;
    }
    pub const SND_MEMORY: u32 = 0x0004;
    pub const SND_NODEFAULT: u32 = 0x0002;
}

fn tocar_de_verdade(som: Som) {
    match som {
        #[cfg(windows)]
        Som::Wav(w) => unsafe {
            // bloqueia so esta thread; PlaySoundW(null) de outra thread corta
            win::PlaySoundW(w.as_ptr() as *const u16, std::ptr::null_mut(), win::SND_MEMORY | win::SND_NODEFAULT);
        },
        #[cfg(not(windows))]
        Som::Wav(_) => {}
        Som::Sapi(texto) => {
            let script = CFG_SAPI.lock().unwrap().clone().unwrap_or_default().replace("{TEXTO}", &texto.replace('\'', "''"));
            if let Ok(mut c) = crate::tools::powershell_cmd(&script).spawn() {
                let _ = c.try_wait();
                *SAPI.lock().unwrap() = Some(c);
                while sapi_ativo() {
                    std::thread::sleep(std::time::Duration::from_millis(80));
                }
            }
        }
    }
}

fn parar_de_verdade() {
    #[cfg(windows)]
    unsafe {
        win::PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0);
    }
    if let Some(mut c) = SAPI.lock().unwrap().take() {
        let _ = c.kill();
    }
}

/// Guarda o modelo do script da voz do Windows (voz e velocidade do .env).
pub fn preparar_sapi(v: &Voz) {
    *CFG_SAPI.lock().unwrap() = Some(v.script_sapi("{TEXTO}"));
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::Duration;

    struct Falsa(Box<dyn Fn() -> Result<(u16, String), String> + Send + Sync>);
    impl Http for Falsa {
        fn post(&self, _: &str, _: &Value) -> Result<(u16, String), String> { (self.0)() }
        fn get(&self, _: &str) -> Result<(u16, String), String> { Ok((200, "{}".into())) }
    }

    fn voz(nome: &str, resp: Box<dyn Fn() -> Result<(u16, String), String> + Send + Sync>) -> (Arc<Voz>, Arc<Mutex<Vec<String>>>) {
        let d = std::env::temp_dir().join(nome);
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join(".env"), "GEMINI_API_KEY=AIzaT\n").unwrap();
        let ouvido = Arc::new(Mutex::new(vec![]));
        let o = ouvido.clone();
        let v = Voz::com_saida(Config::de(d), Arc::new(Falsa(resp)), Box::new(move |s| o.lock().unwrap().push(match s {
            Som::Wav(w) => format!("wav:{}:{}", &String::from_utf8_lossy(&w[..4]), w.len()),
            Som::Sapi(t) => format!("sapi:{t}"),
        })));
        (v, ouvido)
    }

    fn audio(pcm_bytes: usize) -> Result<(u16, String), String> {
        let b = base64::engine::general_purpose::STANDARD.encode(vec![0u8; pcm_bytes]);
        Ok((200, json!({"candidates": [{"content": {"parts": [{"inlineData": {"mimeType": "audio/L16;rate=24000", "data": b}}]}}]}).to_string()))
    }

    fn espera() { std::thread::sleep(Duration::from_millis(250)); }

    #[test]
    fn gemini_toca_wav_valido() {
        let (v, o) = voz("orion_voz_t1", Box::new(|| audio(48_000)));
        assert_eq!(v.motor(), "Gemini (Charon)");
        v.falar("Fala, chefe!"); espera();
        assert_eq!(*o.lock().unwrap(), ["wav:RIFF:48044"]); // 1 s de audio + cabecalho de 44 bytes
    }

    #[test]
    fn cota_estourada_vai_pro_windows_e_nao_insiste() {
        let (v, o) = voz("orion_voz_t2", Box::new(|| Ok((429, "{}".into()))));
        v.falar("teste cota"); espera();
        assert_eq!(*o.lock().unwrap(), ["sapi:teste cota"]);
        assert_eq!(v.motor(), "Windows");
    }

    #[test]
    fn sem_rede_fala_pelo_windows_mas_tenta_gemini_de_novo() {
        let (v, o) = voz("orion_voz_t3", Box::new(|| Err("sem rede".into())));
        v.falar("offline"); espera();
        assert_eq!(*o.lock().unwrap(), ["sapi:offline"]);
        assert_eq!(v.motor(), "Gemini (Charon)"); // rede cai e volta; nao marca morto
    }

    #[test]
    fn mandou_outra_coisa_no_meio_nao_toca() {
        let (v, o) = voz("orion_voz_t4", Box::new(|| { std::thread::sleep(Duration::from_millis(200)); audio(100) }));
        v.falar("frase longa..."); std::thread::sleep(Duration::from_millis(50)); v.parar();
        std::thread::sleep(Duration::from_millis(400));
        assert!(o.lock().unwrap().is_empty());
    }

    #[test]
    fn script_windows_escapa_aspas() {
        let (v, _) = voz("orion_voz_t5", Box::new(|| audio(10)));
        let s = v.script_sapi("it's ok");
        assert!(s.contains("Speak('it''s ok')") && s.contains("Culture -like 'pt*'"));
    }
}
