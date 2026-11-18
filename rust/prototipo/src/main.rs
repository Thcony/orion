//! Orion - prototipo da interface em Rust (egui + OpenGL).
//! So a casca visual: nao pensa, nao executa nada. Serve pra comparar
//! visual, RAM e CPU com a janela atual em Python.
#![windows_subsystem = "windows"] // sem console preto atras no Windows

use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, Pos2, RichText, Shape, Stroke, Vec2};
use std::f32::consts::TAU;
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
const PAINEL: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
const MUTE: Color32 = Color32::from_rgb(0x7d, 0x85, 0x90);
const AZUL: [f32; 3] = [88.0, 166.0, 255.0];
const AMAR: [f32; 3] = [235.0, 170.0, 40.0];
const VERDE: [f32; 3] = [63.0, 205.0, 95.0];

#[derive(Clone, Copy, PartialEq)]
enum Estado {
    Parado,
    Pensando,
    Falando,
}

struct Msg {
    orion: bool,
    texto: String,
}

struct App {
    msgs: Vec<Msg>,
    entrada: String,
    pendente: Option<String>,
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

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.extreme_bg_color = PAINEL;
        cc.egui_ctx.set_visuals(v);
        App {
            msgs: vec![Msg {
                orion: true,
                texto: "Protótipo em Rust. Aqui eu ainda não penso: sou só a casca visual \
                        pra você comparar com a janela atual. Manda qualquer coisa pra ver \
                        as cores mudando."
                    .into(),
            }],
            entrada: String::new(),
            pendente: None,
            estado: Estado::Parado,
            desde: Instant::now(),
            cor: AZUL,
            fase: 0.0,
            fps: 60.0,
            ram_mb: 0.0,
            medido: Instant::now() - Duration::from_secs(1),
            ultimo: Instant::now(),
        }
    }

    fn enviar(&mut self) {
        let t = self.entrada.trim().to_string();
        if t.is_empty() || self.estado != Estado::Parado {
            return;
        }
        self.msgs.push(Msg { orion: false, texto: t.clone() });
        self.entrada.clear();
        self.pendente = Some(t);
        self.estado = Estado::Pensando;
        self.desde = Instant::now();
    }

    fn tick(&mut self, dt: f32) {
        let passou = self.desde.elapsed().as_secs_f32();
        match self.estado {
            Estado::Pensando if passou > 1.4 => {
                let t = self.pendente.take().unwrap_or_default();
                self.msgs.push(Msg {
                    orion: true,
                    texto: format!("Recebi \"{t}\". No Orion de verdade eu responderia aqui."),
                });
                self.estado = Estado::Falando;
                self.desde = Instant::now();
            }
            Estado::Falando if passou > 2.6 => self.estado = Estado::Parado,
            _ => {}
        }
        let alvo = match self.estado {
            Estado::Parado => AZUL,
            Estado::Pensando => AMAR,
            Estado::Falando => VERDE,
        };
        self.cor = mix(self.cor, alvo, 1.0 - (-dt * 6.0).exp()); // transicao suave de cor
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
        self.tick(dt);

        // economia: 30 fps parado, teto de 60 animando, nada minimizado
        let ctx = ui.ctx().clone();
        let minimizado = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimizado {
            if self.estado != Estado::Parado {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
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
                    ui.label(
                        RichText::new(format!("RAM {:.0} MB  ·  {:.0} fps  ·  protótipo Rust", self.ram_mb, self.fps))
                            .size(11.0)
                            .color(MUTE),
                    );
                });
            });

        egui::Panel::bottom("entrada")
            .frame(Frame::NONE.fill(BG).inner_margin(Margin::same(10)))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let largura = ui.available_width() - 84.0;
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.entrada)
                            .hint_text("fala com o Orion...")
                            .desired_width(largura)
                            .margin(Margin::symmetric(8, 8)),
                    );
                    let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let botao = ui.add_sized(
                        [76.0, 32.0],
                        egui::Button::new(RichText::new("Enviar").color(BG).strong())
                            .fill(Color32::from_rgb(63, 185, 80))
                            .corner_radius(CornerRadius::same(6)),
                    );
                    if enter || botao.clicked() {
                        self.enviar();
                        resp.request_focus();
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(BG).inner_margin(Margin::symmetric(10, 0)))
            .show(ui, |ui| {
                Frame::NONE
                    .fill(PAINEL)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::same(12))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for m in &self.msgs {
                                    let (quem, cor) = if m.orion {
                                        ("Orion", Color32::from_rgb(63, 185, 80))
                                    } else {
                                        ("chefe", Color32::from_rgb(88, 166, 255))
                                    };
                                    ui.label(RichText::new(format!("{quem}: {}", m.texto)).color(cor).size(14.5));
                                    ui.add_space(8.0);
                                }
                            });
                    });
            });
    }
}

fn main() -> eframe::Result {
    let opcoes = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 640.0])
            .with_min_inner_size([360.0, 480.0])
            .with_title("Orion - protótipo Rust"),
        ..Default::default()
    };
    eframe::run_native("orion-prototipo", opcoes, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
