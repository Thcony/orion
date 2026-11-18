//! Memoria no vault Obsidian. MESMO formato do Python: as duas versoes
//! leem e escrevem os mesmos arquivos. Erro de disco nunca derruba a conversa.
use crate::config::Config;
use chrono::Local;
use serde_json::{Map, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

pub struct Memoria {
    pub vault: PathBuf,
    conversas: PathBuf,
    pub resumo_path: PathBuf,
    fatos_path: PathBuf,
    buffer: VecDeque<String>,
    semeado: bool,
    inicio_sessao: usize,
    turnos_contexto: usize,
    pub aviso: Option<String>,
}

impl Memoria {
    pub fn new(cfg: &Config) -> Self {
        Memoria {
            conversas: cfg.vault.join("Conversas"),
            resumo_path: cfg.vault.join("Orion - Memoria.md"),
            fatos_path: cfg.data.join("fatos.json"),
            vault: cfg.vault.clone(),
            buffer: VecDeque::with_capacity(60),
            semeado: false,
            inicio_sessao: 0,
            turnos_contexto: cfg.turnos_contexto,
            aviso: None,
        }
    }

    fn falhou(&mut self, e: io::Error) {
        if self.aviso.is_none() {
            self.aviso = Some(format!(
                "nao consegui gravar em {} ({e}). Sigo sem salvar; ajuste ORION_VAULT no .env.",
                self.vault.display()
            ));
        }
    }

    fn init(&self) -> io::Result<()> {
        fs::create_dir_all(&self.conversas)?;
        if !self.resumo_path.exists() {
            fs::write(&self.resumo_path, "# Orion - Memoria\n\n## Sobre o chefe\n\n## Resumo das conversas\n")?;
        }
        Ok(())
    }

    fn arquivo_hoje(&self) -> io::Result<PathBuf> {
        self.init()?;
        let agora = Local::now();
        let f = self.conversas.join(format!("{}.md", agora.format("%Y-%m-%d")));
        if !f.exists() {
            fs::write(&f, format!("# Conversa {}\n\n", agora.format("%d/%m/%Y")))?;
        }
        Ok(f)
    }

    /// Na primeira chamada, recupera os turnos de hoje do vault.
    fn semear(&mut self) {
        if self.semeado {
            return;
        }
        self.semeado = true;
        match self.arquivo_hoje().and_then(fs::read_to_string) {
            Ok(t) => {
                for b in t.split("\n\n").map(str::trim).filter(|b| b.starts_with("**")) {
                    self.empilhar(b.to_string());
                }
            }
            Err(e) => self.falhou(e),
        }
    }

    fn empilhar(&mut self, linha: String) {
        if self.buffer.len() == 60 {
            self.buffer.pop_front();
            self.inicio_sessao = self.inicio_sessao.saturating_sub(1);
        }
        self.buffer.push_back(linha);
    }

    fn anexar(&mut self, texto: &str) {
        let r = self.arquivo_hoje().and_then(|f| {
            OpenOptions::new().append(true).open(f)?.write_all(texto.as_bytes())
        });
        if let Err(e) = r {
            self.falhou(e);
        }
    }

    pub fn registrar(&mut self, quem: &str, texto: &str) {
        self.semear();
        let linha = format!("**{quem}** ({}): {texto}", Local::now().format("%H:%M"));
        self.empilhar(linha.clone());
        self.anexar(&format!("{linha}\n\n"));
    }

    pub fn registrar_acao(&mut self, tool: &str, args: &Value, resultado: &str) {
        let r: String = resultado.chars().take(300).collect();
        self.anexar(&format!("> `{tool}` {args} -> {r}\n\n"));
    }

    pub fn turnos_recentes(&mut self) -> Vec<String> {
        self.semear();
        let n = self.turnos_contexto;
        self.buffer.iter().skip(self.buffer.len().saturating_sub(n)).cloned().collect()
    }

    pub fn resumo(&mut self) -> String {
        match self.init().and_then(|_| fs::read_to_string(&self.resumo_path)) {
            Ok(t) => t.chars().take(3000).collect(),
            Err(e) => {
                self.falhou(e);
                String::new()
            }
        }
    }

    pub fn atualizar_resumo(&self, texto: &str) -> io::Result<()> {
        self.init()?;
        fs::write(&self.resumo_path, texto)
    }

    pub fn marcar_inicio(&mut self) {
        self.semear();
        self.inicio_sessao = self.buffer.len();
    }

    pub fn turnos_da_sessao(&mut self) -> Vec<String> {
        self.semear();
        self.buffer.iter().skip(self.inicio_sessao).cloned().collect()
    }

    pub fn anotar(&self, titulo: &str, texto: &str) -> io::Result<String> {
        self.init()?;
        let nome: String = titulo
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
            .take(60)
            .collect();
        let nome = if nome.trim().is_empty() { "nota".to_string() } else { nome.trim().to_string() };
        let f = self.vault.join(format!("{nome}.md"));
        fs::write(&f, format!("# {titulo}\n\n{texto}\n"))?;
        Ok(f.display().to_string())
    }

    pub fn fatos(&self) -> Map<String, Value> {
        fs::read_to_string(&self.fatos_path)
            .ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    pub fn lembrar_fato(&self, chave: &str, valor: &str) -> io::Result<()> {
        let mut d = self.fatos();
        d.insert(chave.into(), Value::String(valor.into()));
        fs::write(&self.fatos_path, serde_json::to_string_pretty(&d).unwrap_or_default())
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn cfg(nome: &str, vault: &str) -> Config {
        let d = std::env::temp_dir().join(nome);
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join(".env"), format!("ORION_VAULT={vault}\n")).unwrap();
        Config::de(d)
    }

    #[test]
    fn grava_no_formato_do_python_e_resgata_ao_reabrir() {
        let v = std::env::temp_dir().join("orion_vault_t1");
        let _ = fs::remove_dir_all(&v);
        let c = cfg("orion_mem_t1", &v.to_string_lossy());
        let mut m = Memoria::new(&c);
        m.registrar("chefe", "Salve");
        m.registrar("Orion", "Fala, chefe!");
        let hoje = fs::read_to_string(v.join("Conversas").join(format!("{}.md", Local::now().format("%Y-%m-%d")))).unwrap();
        assert!(hoje.contains("**chefe** (") && hoje.contains("): Salve\n\n"));
        // reabriu o Orion: recupera o dia, e a sessao nova comeca vazia
        let mut m2 = Memoria::new(&c);
        m2.marcar_inicio();
        assert_eq!(m2.turnos_recentes().len(), 2);
        m2.registrar("chefe", "e ai");
        assert_eq!(m2.turnos_da_sessao().len(), 1);
    }

    #[test]
    fn vault_impossivel_nao_derruba() {
        let c = cfg("orion_mem_t2", "/proc/orion-nao-da");
        let mut m = Memoria::new(&c);
        m.registrar("chefe", "oi");
        assert_eq!(m.turnos_recentes().len(), 1); // contexto segue em RAM
        assert_eq!(m.resumo(), "");
        assert!(m.aviso.is_some());
    }
}
