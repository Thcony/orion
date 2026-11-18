//! Configuracao. Le o MESMO .env do Orion em Python: chave, vault e dados
//! sao compartilhados entre as duas versoes.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

/// Valores de exemplo do .env.example: valem como vazio.
const PLACEHOLDER: [&str; 2] = ["SEU_USUARIO", "cole_sua_chave"];

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub data: PathBuf,
    pub env_path: PathBuf,
    pub api_key: String,
    pub modelo: String,
    pub modelo_reserva: String,
    pub vault: PathBuf,
    pub chefe: String,
    pub orcamento_dia: u32,
    pub intervalo_min: f32,
    pub turnos_contexto: usize,
    pub sem_freio: bool,
}

/// Pasta do Orion: ORION_DIR, ou a do exe, ou a de cima (exe em dist/).
fn achar_root() -> PathBuf {
    if let Ok(d) = env::var("ORION_DIR") {
        return d.into();
    }
    if let Some(dir) = env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for c in [dir.clone(), dir.parent().map(Path::to_path_buf).unwrap_or(dir.clone())] {
            if c.join(".env").exists() || c.join("main.py").exists() {
                return c;
            }
        }
        return dir;
    }
    env::current_dir().unwrap_or_default()
}

pub fn ler_env(p: &Path) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let Ok(txt) = fs::read_to_string(p) else { return m };
    for linha in txt.trim_start_matches('\u{feff}').lines() {
        let l = linha.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            m.insert(k.trim().to_string(), v.to_string());
        }
    }
    m
}

impl Config {
    pub fn carregar() -> Self {
        Self::de(achar_root())
    }

    pub fn de(root: PathBuf) -> Self {
        let env_path = root.join(".env");
        let arq = ler_env(&env_path);
        // variavel do sistema vence o arquivo, igual ao Python
        let get = |k: &str, padrao: &str| -> String {
            let v = env::var(k).ok().or_else(|| arq.get(k).cloned()).unwrap_or_default();
            let v = v.trim();
            if v.is_empty() || PLACEHOLDER.iter().any(|p| v.contains(p)) {
                padrao.to_string()
            } else {
                v.to_string()
            }
        };
        let docs = dirs::document_dir() // respeita redirecionamento do OneDrive
            .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
            .unwrap_or_default();
        let data = root.join("data");
        let _ = fs::create_dir_all(&data);
        Config {
            api_key: get("GEMINI_API_KEY", ""),
            modelo: get("ORION_MODELO", "gemini-2.5-flash-lite"),
            modelo_reserva: get("ORION_MODELO_RESERVA", ""),
            vault: PathBuf::from(get("ORION_VAULT", &docs.join("Orion-Vault").to_string_lossy())),
            chefe: get("ORION_CHEFE", "chefe"),
            orcamento_dia: get("ORION_ORCAMENTO_DIA", "400").parse().unwrap_or(400),
            intervalo_min: get("ORION_INTERVALO_MIN", "1.5").parse().unwrap_or(1.5),
            turnos_contexto: get("ORION_TURNOS_CONTEXTO", "8").parse().unwrap_or(8),
            sem_freio: get("ORION_SEM_FREIO", "0") == "1",
            root,
            data,
            env_path,
        }
    }
}

/// Grava/atualiza uma variavel no .env sem mexer no resto.
pub fn salvar_env(path: &Path, chave: &str, valor: &str) -> io::Result<()> {
    let txt = fs::read_to_string(path).unwrap_or_default();
    let mut achou = false;
    let mut linhas: Vec<String> = txt
        .trim_start_matches('\u{feff}')
        .lines()
        .map(|l| {
            if l.trim().split('=').next().map(str::trim) == Some(chave) {
                achou = true;
                format!("{chave}={valor}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if !achou {
        linhas.push(format!("{chave}={valor}"));
    }
    fs::write(path, linhas.join("\n") + "\n")
}

/// Expande %VAR% e ~ (o modelo manda caminhos assim). %X% sem variavel fica como esta.
pub fn expandir(caminho: &str) -> String {
    let mut s = caminho.trim().trim_matches('"').trim_matches('\'').to_string();
    if let (Some(resto), Some(h)) = (s.strip_prefix('~').map(str::to_string), dirs::home_dir()) {
        s = format!("{}{}", h.to_string_lossy(), resto);
    }
    let mut out = String::new();
    let mut resto = s.as_str();
    while let Some(ini) = resto.find('%') {
        out.push_str(&resto[..ini]);
        let depois = &resto[ini + 1..];
        let valor = depois
            .find('%')
            .filter(|&f| f > 0)
            .and_then(|f| env::var(&depois[..f]).ok().map(|v| (v, f)));
        match valor {
            Some((v, f)) => {
                out.push_str(&v);
                resto = &depois[f + 1..];
            }
            None => {
                out.push('%');
                resto = depois;
            }
        }
    }
    out.push_str(resto);
    out
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn placeholder_vira_vazio_e_vault_padrao() {
        let d = env::temp_dir().join("orion_cfg_teste");
        let _ = fs::create_dir_all(&d);
        fs::write(d.join(".env"), "\u{feff}GEMINI_API_KEY=cole_sua_chave_aqui\nORION_VAULT=C:\\Users\\SEU_USUARIO\\Documents\\Orion-Vault\nORION_CHEFE=Anthony\n").unwrap();
        let c = Config::de(d.clone());
        assert_eq!(c.api_key, "");
        assert!(!c.vault.to_string_lossy().contains("SEU_USUARIO"));
        assert!(c.vault.ends_with("Orion-Vault"));
        assert_eq!(c.chefe, "Anthony");
    }

    #[test]
    fn salvar_env_preserva_resto() {
        let p = env::temp_dir().join("orion_env_teste");
        fs::write(&p, "GEMINI_API_KEY=velha\nORION_CHEFE=chefe\n").unwrap();
        salvar_env(&p, "GEMINI_API_KEY", "nova").unwrap();
        let t = fs::read_to_string(&p).unwrap();
        assert_eq!(t.matches("GEMINI_API_KEY").count(), 1);
        assert!(t.contains("GEMINI_API_KEY=nova") && t.contains("ORION_CHEFE=chefe"));
    }

    #[test]
    fn expande_variaveis() {
        // SAFETY: teste single-thread, ninguem mais le essa variavel
        unsafe { env::set_var("ORION_T", "C:\\Users\\antho") };
        assert_eq!(expandir("%ORION_T%\\Desktop"), "C:\\Users\\antho\\Desktop");
        assert_eq!(expandir("100%"), "100%");
        assert_eq!(expandir("%NAO_EXISTE_X%\\a"), "%NAO_EXISTE_X%\\a");
    }
}
