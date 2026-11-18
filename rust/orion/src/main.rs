//! Orion - assistente do chefe. Janela em Rust (egui + OpenGL),
//! cerebro no Gemini, maos no PC. Compartilha .env, vault e dados com a versao Python.
#![windows_subsystem = "windows"] // sem console preto atras no Windows

mod agente;
mod cerebro;
mod config;
mod guard;
mod memoria;
mod tools;

use agente::Saida;
use cerebro::{Cerebro, Rede};
use config::Config;
use eframe::egui::{self, Color32, CornerRadius, Frame, Id, Margin, Pos2, RichText, Shape, Stroke, Vec2};
use memoria::Memoria;
use serde_json::Value;
use std::f32::consts::TAU;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tools::Ctx;

const BG: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const PAINEL: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const MUTE: Color32 = Color32::from_rgb(0x7d, 0x85, 0x90);
const C_VERDE: Color32 = Color32::from_rgb(63, 185, 80);
const C_AZUL: Color32 = Color32::from_rgb(88, 166, 255);
const C_AMAR: Color32 = Color32::from_rgb(210, 153, 34);
const C_VERM: Color32 = Color32::from_rgb(248, 81, 73);
const AZUL: [f32; 3] = [88.0, 166.0, 255.0];
const AMAR: [f32; 3] = [235.0, 170.0, 40.0];
const VERDE: [f32; 3] = [63.0, 205.0, 95.0];

#[derive(Clone, Copy, PartialEq)]
enum Estado {
    Parado,
    Pensando,
    Falando,
}

enum Tipo {
    Orion,
    Chefe,
    Info,
    Aviso,
    Erro,
}

enum Evento {
    Saida(Saida),
    Confirmar(String, Value, String, Sender<bool>),
    Fim,
}

struct Pedido {
    tool: String,
    args: Value,
    motivo: String,
    resp: Sender<bool>,
}

struct App {
    ctx: Arc<Ctx>,
    cerebro: Arc<Cerebro>,
    tx: Sender<Evento>,
    rx: Receiver<Evento>,
    linhas: Vec<(Tipo, String)>,
    entrada: String,
    ocupado: bool,
    pedido: Option<Pedido>,
    avisou_memoria: bool,
    estado: Estado,
    desde: Instant,
    cor: [f32; 3],
    fase: f32,
    fps: f32,
    ram_mb: f64,
    medido: Instant,
    ultimo: Instant,
}

fn rgba(c: [f32; 3], a: f32) -> Color32 {
    let a = (a.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(c[0] as u8, c[1] as u8, c[2] as u8, a)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Ponto de uma orbita inclinada: posicao na tela + profundidade (-1 atras, 1 na frente).
fn orbita(c: Pos2, raio: f32, inclinacao: f32, giro: f32, ang: f32) -> (Pos2, f32) {
    let (x, y) = (raio * ang.cos(), raio * ang.sin());
    let (y2, z) = (y * inclinacao.cos(), y * inclinacao.sin());
    let (s, k) = giro.sin_cos();
    (Pos2::new(c.x + x * k - y2 * s, c.y + x * s + y2 * k), z / raio)
}

// (raio, inclinacao, giro, velocidade)
const ORBITAS: [(f32, f32, f32, f32); 3] =
    [(60.0, 1.18, 0.35, 0.9), (70.0, 1.28, -0.65, -0.7), (52.0, 1.05, 1.6, 1.25)];

fn montar(cfg: Config) -> (Arc<Ctx>, Arc<Cerebro>) {
    let memoria = Arc::new(Mutex::new(Memoria::new(&cfg)));
    let ctx = Arc::new(Ctx { cfg: cfg.clone(), memoria, http: Arc::new(Rede::new()) });
    #[cfg(debug_assertions)] // so em build de teste; o .exe de verdade nunca tem isso
    if std::env::var("ORION_FALSO").is_ok() {
        return (ctx, Arc::new(Cerebro::new(cfg, Box::new(teste::Falsa))));
    }
    (ctx, Arc::new(Cerebro::new(cfg, Box::new(Rede::new()))))
}

#[cfg(debug_assertions)]
mod teste {
    use serde_json::{json, Value};
    /// Gemini de mentira pra testar a janela sem chave e sem rede.
    pub struct Falsa;
    impl crate::cerebro::Http for Falsa {
        fn post(&self, _: &str, corpo: &Value) -> Result<(u16, String), String> {
            let txt = corpo["contents"][0]["parts"][0]["text"].as_str().unwrap_or("");
            let agora = txt.rsplit("AGORA:").next().unwrap_or("").trim().to_string();
            std::thread::sleep(std::time::Duration::from_millis(900)); // simula a rede
            if txt.contains("memoria de longo prazo") { // pedido de resumo vem antes de tudo
                return Ok((200, json!({"candidates": [{"content": {"parts": [{"text": "# Orion - Memoria\n\n## Resumo das conversas\n- teste da janela Rust"}]}}]}).to_string()));
            }
            let r = if agora.contains("apaga") {
                json!({"fala": "Beleza, vou apagar.", "acoes": [{"tool": "apagar", "args": {"caminho": "/tmp/orion_ui_teste.txt"}}]})
            } else {
                json!({"fala": format!("Fala, chefe! Recebi: {agora}"), "acoes": [{"tool": "agora", "args": {}}]})
            };
            Ok((200, json!({"candidates": [{"content": {"parts": [{"text": r.to_string()}]}}]}).to_string()))
        }
        fn get(&self, _: &str) -> Result<(u16, String), String> {
            Ok((200, r#"{"models":[]}"#.into()))
        }
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.extreme_bg_color = PAINEL;
        cc.egui_ctx.set_visuals(v);
        let (ctx, cerebro) = montar(Config::carregar());
        ctx.memoria.lock().unwrap().marcar_inicio();
        let (tx, rx) = channel();
        let mut app = App {
            linhas: vec![(Tipo::Info, format!("online · {} chamadas hoje · pasta {}", cerebro.restante(), ctx.cfg.root.display()))],
            ctx, cerebro, tx, rx,
            entrada: String::new(),
            ocupado: false,
            pedido: None,
            avisou_memoria: false,
            estado: Estado::Parado,
            desde: Instant::now(),
            cor: AZUL,
            fase: 0.0,
            fps: 60.0,
            ram_mb: 0.0,
            medido: Instant::now() - Duration::from_secs(1),
            ultimo: Instant::now(),
        };
        if app.ctx.cfg.api_key.is_empty() {
            app.linhas.push((Tipo::Aviso, "Sem chave do Gemini. Pega uma gratis em aistudio.google.com/apikey \
                e manda aqui:  /chave SUA_CHAVE".into()));
        }
        app
    }

    fn enviar(&mut self, ui_ctx: &egui::Context) {
        let t = self.entrada.trim().to_string();
        if t.is_empty() || self.ocupado {
            return;
        }
        self.entrada.clear();
        if t == "/sair" {
            ui_ctx.send_viewport_cmd(egui::ViewportCommand::Close); // on_exit salva a memoria
            return;
        }
        if let Some(chave) = t.strip_prefix("/chave ") {
            self.trocar_chave(chave.trim());
            return;
        }
        self.linhas.push((Tipo::Chefe, t.clone()));
        self.ocupado = true;
        self.estado = Estado::Pensando;
        let (ctx, cerebro, tx, repintar) = (self.ctx.clone(), self.cerebro.clone(), self.tx.clone(), ui_ctx.clone());
        std::thread::spawn(move || {
            let mandar = |e: Evento| { let _ = tx.send(e); repintar.request_repaint(); };
            let dizer = |s: Saida| mandar(Evento::Saida(s));
            let confirmar = |tool: &str, args: &Value, motivo: &str| {
                let (rtx, rrx) = channel();
                mandar(Evento::Confirmar(tool.into(), args.clone(), motivo.into(), rtx));
                rrx.recv().unwrap_or(false) // janela fechada = nao
            };
            agente::responder(&ctx, &cerebro, &t, &dizer, &confirmar);
            mandar(Evento::Fim);
        });
    }

    fn trocar_chave(&mut self, chave: &str) {
        if let Err(e) = config::salvar_env(&self.ctx.cfg.env_path, "GEMINI_API_KEY", chave) {
            self.linhas.push((Tipo::Erro, format!("nao salvei a chave: {e}")));
            return;
        }
        let (ctx, cerebro) = montar(Config::carregar());
        let ok = cerebro.listar_modelos();
        ctx.memoria.lock().unwrap().marcar_inicio();
        (self.ctx, self.cerebro) = (ctx, cerebro);
        self.linhas.push(match ok {
            Ok(l) => (Tipo::Info, format!("chave salva e valida ({} modelos liberados)", l.len())),
            Err(e) => (Tipo::Erro, format!("chave salva, mas o Gemini recusou: {e}")),
        });
    }

    fn drenar(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Evento::Saida(Saida::Orion(t)) => {
                    self.linhas.push((Tipo::Orion, t));
                    self.estado = Estado::Falando;
                    self.desde = Instant::now();
                }
                Evento::Saida(Saida::Info(t)) => self.linhas.push((Tipo::Info, t)),
                Evento::Saida(Saida::Erro(t)) => self.linhas.push((Tipo::Erro, t)),
                Evento::Confirmar(tool, args, motivo, resp) => self.pedido = Some(Pedido { tool, args, motivo, resp }),
                Evento::Fim => self.ocupado = false,
            }
        }
        if !self.avisou_memoria {
            if let Some(a) = self.ctx.memoria.lock().unwrap().aviso.clone() {
                self.linhas.push((Tipo::Aviso, a));
                self.avisou_memoria = true;
            }
        }
    }

    fn tick(&mut self, dt: f32) {
        // falando dura um pouco depois da fala chegar; depois volta a pensar (se ainda ocupado) ou parar
        if self.estado == Estado::Falando && self.desde.elapsed() > Duration::from_millis(2500) {
            self.estado = if self.ocupado { Estado::Pensando } else { Estado::Parado };
        }
        if self.estado == Estado::Pensando && !self.ocupado {
            self.estado = Estado::Parado;
        }
        let alvo = match self.estado {
            Estado::Parado => AZUL,
            Estado::Pensando => AMAR,
            Estado::Falando => VERDE,
        };
        self.cor = mix(self.cor, alvo, 1.0 - (-dt * 6.0).exp());
        let vel = match self.estado {
            Estado::Parado => 1.0,
            Estado::Pensando => 2.8,
            Estado::Falando => 1.6,
        };
        self.fase += dt * vel;
        if dt > 0.0 {
            self.fps = self.fps * 0.92 + (1.0 / dt) * 0.08;
        }
        if self.medido.elapsed() > Duration::from_millis(500) {
            if let Some(m) = memory_stats::memory_stats() {
                self.ram_mb = m.physical_mem as f64 / 1_048_576.0;
            }
            self.medido = Instant::now();
        }
    }

    fn desenhar_orb(&self, ui: &mut egui::Ui) {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 168.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let c = rect.center();
        let f = self.fase;
        let cor = self.cor;

        let rc = match self.estado {
            Estado::Parado => 25.0 + 2.5 * (f * 1.3).sin(),
            Estado::Pensando => 25.0 + 3.5 * (f * 3.1).sin(),
            Estado::Falando => 25.0 + 6.0 * (0.6 * (f * 9.0).sin() + 0.4 * (f * 13.7).sin()).abs(),
        };

        // brilho em volta (gradiente radial)
        for i in (0..22).rev() {
            let k = i as f32 / 22.0;
            p.circle_filled(c, rc + 6.0 + k * 62.0, rgba(cor, 0.035 * (1.0 - k)));
        }

        // ondas saindo quando fala
        if self.estado == Estado::Falando {
            for n in 0..3 {
                let t = (f * 0.9 + n as f32 / 3.0).fract();
                p.circle_stroke(c, rc + 8.0 + t * 72.0, Stroke::new(2.0, rgba(cor, (1.0 - t) * 0.45)));
            }
        }

        // orbitas (linha inteira bem fraca, metade da frente reforcada depois do nucleo)
        let mut frente: Vec<Shape> = Vec::new();
        for &(r, inc, giro, _) in &ORBITAS {
            let pts: Vec<(Pos2, f32)> = (0..=72).map(|i| orbita(c, r, inc, giro, i as f32 / 72.0 * TAU)).collect();
            p.add(Shape::line(pts.iter().map(|x| x.0).collect(), Stroke::new(1.0, rgba(cor, 0.12))));
            for w in pts.windows(2) {
                if w[0].1 > 0.0 && w[1].1 > 0.0 {
                    frente.push(Shape::line_segment([w[0].0, w[1].0], Stroke::new(1.0, rgba(cor, 0.30))));
                }
            }
        }

        // particulas com rastro; separa as de tras das da frente
        let mut tras_p: Vec<Shape> = Vec::new();
        let mut frente_p: Vec<Shape> = Vec::new();
        for &(r, inc, giro, vel) in &ORBITAS {
            for j in 0..5 {
                let base = f * vel + j as f32 * TAU / 5.0;
                for t in (0..6).rev() {
                    let ang = base - vel.signum() * t as f32 * 0.07;
                    let (pos, z) = orbita(c, r, inc, giro, ang);
                    let prof = (z + 1.0) / 2.0;
                    let fade = 1.0 - t as f32 / 6.0;
                    let s = Shape::circle_filled(
                        pos,
                        (1.2 + 2.0 * prof) * (0.45 + 0.55 * fade),
                        rgba(mix(cor, [255.0; 3], 0.35 * prof), (0.25 + 0.75 * prof) * fade),
                    );
                    if z < 0.0 { tras_p.push(s) } else { frente_p.push(s) }
                }
            }
        }
        p.extend(tras_p);

        // nucleo com sombreado de esfera (luz vindo de cima-esquerda)
        for i in 0..14 {
            let k = i as f32 / 14.0;
            let off = Vec2::new(-rc * 0.28, -rc * 0.32) * k;
            let tom = mix(mix(cor, [0.0; 3], 0.45), mix(cor, [255.0; 3], 0.55), k);
            p.circle_filled(c + off, rc * (1.0 - k * 0.78), rgba(tom, 1.0));
        }

        p.extend(frente);
        p.extend(frente_p);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let dt = self.ultimo.elapsed().as_secs_f32().min(0.1); // tempo real entre frames
        self.ultimo = Instant::now();
        self.drenar();
        self.tick(dt);

        // economia: 30 fps parado, teto de 60 animando, quase nada minimizado
        let ctx = ui.ctx().clone();
        let minimizado = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimizado {
            ctx.request_repaint_after(Duration::from_millis(500));
        } else if self.estado == Estado::Parado {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else {
            ctx.request_repaint_after(Duration::from_millis(16));
        }

        egui::Panel::top("orb")
            .frame(Frame::NONE.fill(BG))
            .exact_size(192.0)
            .show_separator_line(false)
            .show(ui, |ui| {
                self.desenhar_orb(ui);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(format!("RAM {:.0} MB  ·  {:.0} fps  ·  {} chamadas restantes",
                        self.ram_mb, self.fps, self.cerebro.restante())).size(11.0).color(MUTE));
                });
            });

        egui::Panel::bottom("entrada")
            .frame(Frame::NONE.fill(BG).inner_margin(Margin::same(10)))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let largura = ui.available_width() - 84.0;
                    let dica = if self.ocupado { "pensando..." } else { "fala com o Orion..." };
                    let resp = ui.add(egui::TextEdit::singleline(&mut self.entrada)
                        .hint_text(dica).desired_width(largura).margin(Margin::symmetric(8, 8)));
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let botao = ui.add_enabled(!self.ocupado, egui::Button::new(RichText::new("Enviar").color(BG).strong())
                        .fill(C_VERDE).corner_radius(CornerRadius::same(6)).min_size(Vec2::new(76.0, 32.0)));
                    if enter || botao.clicked() {
                        self.enviar(&ctx);
                        resp.request_focus();
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(BG).inner_margin(Margin::symmetric(10, 0)))
            .show(ui, |ui| {
                Frame::NONE.fill(PAINEL).corner_radius(CornerRadius::same(8)).inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                        for (tipo, t) in &self.linhas {
                            let texto = match tipo {
                                Tipo::Orion => RichText::new(format!("Orion: {t}")).color(C_VERDE).size(14.5),
                                Tipo::Chefe => RichText::new(format!("{}: {t}", self.ctx.cfg.chefe)).color(C_AZUL).size(14.5),
                                Tipo::Info => RichText::new(t.chars().take(2000).collect::<String>()).color(MUTE).monospace().size(11.5),
                                Tipo::Aviso => RichText::new(t).color(C_AMAR).size(13.0),
                                Tipo::Erro => RichText::new(t).color(C_VERM).size(13.0),
                            };
                            ui.label(texto);
                            ui.add_space(8.0);
                        }
                    });
                });
            });

        // acao perigosa: caixa na tela, a conversa espera a resposta
        let mut decidiu = None;
        if let Some(p) = &self.pedido {
            egui::Modal::new(Id::unique("orion-confirmar")).show(&ctx, |ui| {
                ui.set_width(330.0);
                ui.label(RichText::new(format!("Rodar {}?", p.tool)).size(17.0).strong());
                ui.add_space(6.0);
                ui.label(RichText::new(p.args.to_string()).monospace().size(11.5).color(MUTE));
                if !p.motivo.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&p.motivo).color(C_AMAR));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Pode rodar").color(BG).strong()).fill(C_VERDE)).clicked() {
                        decidiu = Some(true);
                    }
                    if ui.button("Não").clicked() {
                        decidiu = Some(false);
                    }
                });
            });
        }
        if let Some(v) = decidiu {
            if let Some(p) = self.pedido.take() {
                let _ = p.resp.send(v);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // resume a sessao na memoria de longo prazo (max 25 s, pra nao travar o fechamento)
        let (ctx, cerebro) = (self.ctx.clone(), self.cerebro.clone());
        let t = std::thread::spawn(move || {
            let (turnos, resumo) = {
                let mut m = ctx.memoria.lock().unwrap();
                (m.turnos_da_sessao(), m.resumo())
            };
            if turnos.len() >= 2 {
                if let Some(novo) = cerebro.resumir(&resumo, &turnos.join("\n")) {
                    let _ = ctx.memoria.lock().unwrap().atualizar_resumo(&novo);
                }
            }
        });
        let inicio = Instant::now();
        while !t.is_finished() && inicio.elapsed() < Duration::from_secs(25) {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn main() -> eframe::Result {
    let opcoes = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 640.0])
            .with_min_inner_size([360.0, 480.0])
            .with_title("Orion"),
        ..Default::default()
    };
    eframe::run_native("orion", opcoes, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
