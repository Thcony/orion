//! Voz do Orion.
//! Gemini TTS em STREAMING: o audio chega em pedacos e comeca a tocar no
//! primeiro (com ~0,5 s de folga pra nao picotar). A janela so mostra o
//! texto quando o som comeca, entao os dois saem juntos.
//! Reserva: voz do Windows (SAPI) se o Gemini recusar ou a rede cair.
use crate::cerebro::Http;
use crate::config::Config;
use base64::Engine;
use chrono::Local;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_CHARS: usize = 500;
const ESPERA_PRIMEIRO_AUDIO: Duration = Duration::from_secs(12);
/// Modelos de voz em ordem. Cada um tem cota propria: se um estoura, vai pro outro.
const MODELOS_PADRAO: [&str; 2] = ["gemini-3.1-flash-tts-preview", "gemini-2.5-flash-preview-tts"];

type TocarPcm = dyn Fn(Vec<u8>, Receiver<Vec<u8>>, &dyn Fn() -> bool) + Send + Sync;
type TocarSapi = dyn Fn(&str, &dyn Fn() -> bool) + Send + Sync;

/// Por onde o som sai. Separado pra testar sem alto-falante.
pub struct AltoFalante {
    pub pcm: Box<TocarPcm>,
    pub sapi: Box<TocarSapi>,
}

pub struct Voz {
    cfg: Config,
    http: Arc<dyn Http>,
    saida: AltoFalante,
    geracao: AtomicU64,
    tocando: AtomicBool,
    mortos: Mutex<(String, HashSet<String>)>, // (dia, modelos que recusaram hoje)
    pub ligada: AtomicBool,
}

enum Problema {
    Recusou, // cota/modelo/permissao: nao insiste hoje
    Rede,
}

impl Voz {
    pub fn new(cfg: Config, http: Arc<dyn Http>) -> Arc<Self> {
        let sapi_cfg = cfg.clone();
        Self::com_saida(cfg, http, AltoFalante {
            pcm: Box::new(tocar_pcm),
            sapi: Box::new(move |t, cancelado| tocar_sapi(&script_sapi(&sapi_cfg, t), cancelado)),
        })
    }

    pub fn com_saida(cfg: Config, http: Arc<dyn Http>, saida: AltoFalante) -> Arc<Self> {
        let ligada = AtomicBool::new(cfg.voz);
        Arc::new(Voz { cfg, http, saida, geracao: AtomicU64::new(0), tocando: AtomicBool::new(false),
                       mortos: Mutex::new((String::new(), HashSet::new())), ligada })
    }

    fn modelos_vivos(&self) -> Vec<String> {
        let hoje = Local::now().format("%Y-%m-%d").to_string();
        let mut m = self.mortos.lock().unwrap();
        if m.0 != hoje {
            *m = (hoje, HashSet::new()); // virou o dia: tenta todos de novo
        }
        let mut lista: Vec<String> = Vec::new();
        for x in std::iter::once(self.cfg.voz_modelo.as_str()).chain(MODELOS_PADRAO) {
            if !x.is_empty() && !m.1.contains(x) && !lista.iter().any(|y| y == x) {
                lista.push(x.to_string());
            }
        }
        lista
    }

    fn gemini_usavel(&self) -> bool {
        self.cfg.voz_motor == "gemini" && !self.cfg.api_key.is_empty() && !self.modelos_vivos().is_empty()
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
        self.tocando.load(Ordering::Relaxed)
    }

    /// Corta a fala atual e descarta o audio que ainda esta chegando.
    pub fn parar(&self) {
        self.geracao.fetch_add(1, Ordering::SeqCst);
    }

    /// Fala `texto`. `comecou` e chamado no instante em que o som sai
    /// (ou quando desiste do Gemini e vai pra voz do Windows).
    pub fn falar(self: &Arc<Self>, texto: &str, comecou: Box<dyn FnOnce() + Send>) {
        if texto.trim().is_empty() || !self.ligada.load(Ordering::Relaxed) {
            comecou();
            return;
        }
        self.parar();
        let minha = self.geracao.load(Ordering::SeqCst);
        let texto: String = texto.chars().take(MAX_CHARS).collect();
        let eu = self.clone();
        std::thread::spawn(move || {
            let cancelado = || eu.geracao.load(Ordering::SeqCst) != minha;
            let mut comecou = Some(comecou);
            let mut avisar = || if let Some(f) = comecou.take() { f() };

            if eu.gemini_usavel() {
                let (tx, rx) = channel::<Vec<u8>>();
                let problema = Arc::new(Mutex::new(None::<Problema>));
                let (eu2, t2, p2) = (eu.clone(), texto.clone(), problema.clone());
                std::thread::spawn(move || {
                    if let Err(p) = eu2.baixar(&t2, &tx, minha) {
                        *p2.lock().unwrap() = Some(p);
                    }
                }); // ao terminar, tx cai e o rx fica sabendo
                match rx.recv_timeout(ESPERA_PRIMEIRO_AUDIO) {
                    Ok(primeiro) => {
                        if cancelado() { return; }
                        avisar();
                        eu.tocando.store(true, Ordering::Relaxed);
                        (eu.saida.pcm)(primeiro, rx, &cancelado);
                        eu.tocando.store(false, Ordering::Relaxed);
                        return;
                    }
                    Err(RecvTimeoutError::Timeout) => {} // internet lenta: vai de Windows nessa fala
                    Err(RecvTimeoutError::Disconnected) => {} // baixar() ja marcou quem recusou
                }
            }
            if cancelado() { return; }
            avisar();
            eu.tocando.store(true, Ordering::Relaxed);
            (eu.saida.sapi)(&texto, &cancelado);
            eu.tocando.store(false, Ordering::Relaxed);
        });
    }

    /// Baixa o audio em streaming e manda os pedacos de PCM pelo canal.
    fn baixar(&self, texto: &str, tx: &Sender<Vec<u8>>, minha: u64) -> Result<(), Problema> {
        let corpo = json!({
            "contents": [{"parts": [{"text": format!("Fale em portugues do Brasil, tom natural e descontraido: {texto}")}]}],
            "generationConfig": {"responseModalities": ["AUDIO"],
                "speechConfig": {"voiceConfig": {"prebuiltVoiceConfig": {"voiceName": self.cfg.voz_gemini}}}},
        });
        for modelo in self.modelos_vivos() {
            let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{modelo}:streamGenerateContent?alt=sse&key={}",
                              self.cfg.api_key);
            let mut mandou = false;
            let r = self.http.post_linhas(&url, &corpo, &mut |linha| {
                if self.geracao.load(Ordering::SeqCst) != minha { return false; } // chefe cortou
                let Some(dados) = linha.strip_prefix("data:") else { return true };
                let Ok(v) = serde_json::from_str::<Value>(dados.trim()) else { return true };
                for p in v["candidates"][0]["content"]["parts"].as_array().into_iter().flatten() {
                    if let Some(pcm) = p["inlineData"]["data"].as_str()
                        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok()) {
                        mandou = true;
                        if tx.send(pcm).is_err() { return false; }
                    }
                }
                true
            });
            match r {
                Ok((200, _)) if mandou => return Ok(()),
                Ok((200, _)) => return Err(Problema::Rede), // veio vazio
                Ok((400 | 403 | 404 | 429, _)) => { // esse modelo nao serve hoje: tenta o proximo
                    self.mortos.lock().unwrap().1.insert(modelo);
                }
                Ok(_) | Err(_) => return Err(Problema::Rede),
            }
        }
        Err(Problema::Recusou)
    }
}

/// Script da voz do Windows (texto embutido com aspas simples escapadas).
pub fn script_sapi(cfg: &Config, texto: &str) -> String {
    let filtro = if cfg.voz_nome.is_empty() {
        "$_.Culture -like 'pt*'".to_string()
    } else {
        format!("$_.Name -like '*{}*'", cfg.voz_nome.replace('\'', "''"))
    };
    format!("Add-Type -AssemblyName System.Speech; $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.Rate={}; $v=$s.GetInstalledVoices().VoiceInfo | Where-Object {{ {filtro} }} | Select-Object -First 1; \
             if($v){{$s.SelectVoice($v.Name)}}; $s.Speak('{}')",
            cfg.voz_velocidade.clamp(-10, 10), texto.replace('\'', "''"))
}

// ---------------- som de verdade ----------------
fn tocar_sapi(script: &str, cancelado: &dyn Fn() -> bool) {
    let Ok(mut c) = crate::tools::powershell_cmd(script).spawn() else { return };
    while matches!(c.try_wait(), Ok(None)) {
        if cancelado() {
            let _ = c.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }
}

/// Toca PCM 16-bit 24 kHz mono em streaming pelo waveOut do Windows.
#[cfg(windows)]
fn tocar_pcm(primeiro: Vec<u8>, rx: Receiver<Vec<u8>>, cancelado: &dyn Fn() -> bool) {
    use windows_sys::Win32::Media::Audio::*;
    use windows_sys::Win32::Media::MMSYSERR_NOERROR;
    const FOLGA: usize = 24_000; // 0,5 s guardado antes de comecar
    let tam = std::mem::size_of::<WAVEHDR>() as u32;
    unsafe {
        let fmt = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 1, nSamplesPerSec: 24_000,
                                 nAvgBytesPerSec: 48_000, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
        let mut h: HWAVEOUT = std::ptr::null_mut();
        if waveOutOpen(&mut h, WAVE_MAPPER, &fmt, 0, 0, CALLBACK_NULL) != MMSYSERR_NOERROR {
            return;
        }
        // cada buffer fica numa Box: o endereco nao pode mudar enquanto o Windows toca
        let mut bufs: Vec<Box<(WAVEHDR, Vec<u8>)>> = Vec::new();
        let enviar = |dados: Vec<u8>, bufs: &mut Vec<Box<(WAVEHDR, Vec<u8>)>>| {
            if dados.len() < 2 { return; }
            let mut b = Box::new((WAVEHDR::default(), dados));
            b.0.lpData = b.1.as_mut_ptr();
            b.0.dwBufferLength = (b.1.len() & !1) as u32;
            if waveOutPrepareHeader(h, &mut b.0, tam) == MMSYSERR_NOERROR && waveOutWrite(h, &mut b.0, tam) == MMSYSERR_NOERROR {
                bufs.push(b);
            }
        };
        let mut pendente = primeiro;
        let mut tocando = false;
        loop {
            if cancelado() { break; }
            match rx.recv_timeout(Duration::from_millis(40)) {
                Ok(c) => pendente.extend(c),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    enviar(std::mem::take(&mut pendente), &mut bufs);
                    break;
                }
            }
            if tocando || pendente.len() >= FOLGA {
                tocando = true;
                if !pendente.is_empty() {
                    enviar(std::mem::take(&mut pendente), &mut bufs);
                }
            }
        }
        // espera acabar de tocar (ou o chefe cortar)
        // WAVEHDR e packed: sem referencia ao campo, so ponteiro cru (o endereco real e alinhado, vem da Box)
        while !cancelado() && bufs.iter().any(|b| std::ptr::read_volatile(std::ptr::addr_of!(b.0.dwFlags)) & WHDR_DONE == 0) {
            std::thread::sleep(Duration::from_millis(30));
        }
        waveOutReset(h);
        for b in bufs.iter_mut() {
            waveOutUnprepareHeader(h, &mut b.0, tam);
        }
        waveOutClose(h);
    }
}

#[cfg(not(windows))]
fn tocar_pcm(primeiro: Vec<u8>, rx: Receiver<Vec<u8>>, cancelado: &dyn Fn() -> bool) {
    // sem waveOut: so passa o tempo que o audio duraria (48000 bytes = 1 s)
    let total = primeiro.len() + rx.iter().map(|c| c.len()).sum::<usize>();
    let fim = std::time::Instant::now() + Duration::from_secs_f64(total as f64 / 48_000.0);
    while !cancelado() && std::time::Instant::now() < fim {
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::Instant;

    type Resposta = Box<dyn Fn(&str) -> Result<(u16, String), String> + Send + Sync>;
    struct Falsa(Resposta);
    impl Http for Falsa {
        fn post(&self, url: &str, _: &Value) -> Result<(u16, String), String> { (self.0)(url) }
        fn get(&self, _: &str) -> Result<(u16, String), String> { Ok((200, "{}".into())) }
    }

    /// SSE com `n` pedacos de `bytes` cada.
    fn sse(n: usize, bytes: usize) -> Result<(u16, String), String> {
        let b = base64::engine::general_purpose::STANDARD.encode(vec![7u8; bytes]);
        let um = json!({"candidates": [{"content": {"parts": [{"inlineData": {"mimeType": "audio/L16;rate=24000", "data": b}}]}}]});
        Ok((200, (0..n).map(|_| format!("data: {um}\n\n")).collect()))
    }

    struct Teste { voz: Arc<Voz>, ouviu: Arc<Mutex<Vec<String>>> }

    fn montar(nome: &str, resp: Resposta) -> Teste {
        let d = std::env::temp_dir().join(nome);
        let _ = std::fs::create_dir_all(&d);
        std::fs::write(d.join(".env"), "GEMINI_API_KEY=AIzaT\n").unwrap();
        let ouviu = Arc::new(Mutex::new(vec![]));
        let (o1, o2) = (ouviu.clone(), ouviu.clone());
        let saida = AltoFalante {
            pcm: Box::new(move |primeiro, rx, _| {
                let total = primeiro.len() + rx.iter().map(|c| c.len()).sum::<usize>();
                o1.lock().unwrap().push(format!("pcm:{total}"));
            }),
            sapi: Box::new(move |t, _| o2.lock().unwrap().push(format!("sapi:{t}"))),
        };
        Teste { voz: Voz::com_saida(Config::de(d), Arc::new(Falsa(resp)), saida), ouviu }
    }

    fn falar_e_esperar(t: &Teste, texto: &str) -> bool {
        let comecou = Arc::new(AtomicBool::new(false));
        let c = comecou.clone();
        t.voz.falar(texto, Box::new(move || c.store(true, Ordering::SeqCst)));
        std::thread::sleep(Duration::from_millis(300));
        comecou.load(Ordering::SeqCst)
    }

    #[test]
    fn streaming_junta_todos_os_pedacos_e_avisa_que_comecou() {
        let t = montar("orion_vz1", Box::new(|_| sse(3, 1000)));
        assert!(falar_e_esperar(&t, "Fala, chefe!"));
        assert_eq!(*t.ouviu.lock().unwrap(), ["pcm:3000"]);
    }

    #[test]
    fn primeiro_modelo_sem_cota_vai_pro_segundo() {
        let t = montar("orion_vz2", Box::new(|url| if url.contains("3.1") { Ok((429, "{}".into())) } else { sse(1, 500) }));
        falar_e_esperar(&t, "oi");
        assert_eq!(*t.ouviu.lock().unwrap(), ["pcm:500"]);
        assert_eq!(t.voz.modelos_vivos(), ["gemini-2.5-flash-preview-tts"]);
        assert_eq!(t.voz.motor(), "Gemini (Charon)");
    }

    #[test]
    fn todos_recusaram_vai_pro_windows_e_nao_insiste() {
        let t = montar("orion_vz3", Box::new(|_| Ok((429, "{}".into()))));
        assert!(falar_e_esperar(&t, "teste cota"));
        assert_eq!(*t.ouviu.lock().unwrap(), ["sapi:teste cota"]);
        assert_eq!(t.voz.motor(), "Windows");
    }

    #[test]
    fn sem_rede_fala_pelo_windows_mas_tenta_gemini_depois() {
        let t = montar("orion_vz4", Box::new(|_| Err("sem rede".into())));
        falar_e_esperar(&t, "offline");
        assert_eq!(*t.ouviu.lock().unwrap(), ["sapi:offline"]);
        assert_eq!(t.voz.motor(), "Gemini (Charon)");
    }

    #[test]
    fn cortou_no_meio_nao_toca_nada() {
        let t = montar("orion_vz5", Box::new(|_| { std::thread::sleep(Duration::from_millis(250)); sse(1, 100) }));
        let inicio = Instant::now();
        t.voz.falar("frase longa", Box::new(|| {}));
        std::thread::sleep(Duration::from_millis(40));
        t.voz.parar();
        std::thread::sleep(Duration::from_millis(500));
        assert!(t.ouviu.lock().unwrap().is_empty(), "{:?}", t.ouviu.lock().unwrap());
        assert!(inicio.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn voz_desligada_mostra_texto_na_hora() {
        let t = montar("orion_vz6", Box::new(|_| sse(1, 10)));
        t.voz.ligada.store(false, Ordering::Relaxed);
        let ok = Arc::new(AtomicBool::new(false));
        let o = ok.clone();
        t.voz.falar("oi", Box::new(move || o.store(true, Ordering::SeqCst)));
        assert!(ok.load(Ordering::SeqCst)); // sincrono: nem esperou thread
        assert!(t.ouviu.lock().unwrap().is_empty());
    }

    #[test]
    fn script_windows_escapa_aspas() {
        let t = montar("orion_vz7", Box::new(|_| sse(1, 10)));
        let s = script_sapi(&t.voz.cfg, "it's ok");
        assert!(s.contains("Speak('it''s ok')") && s.contains("Culture -like 'pt*'"));
    }
}
