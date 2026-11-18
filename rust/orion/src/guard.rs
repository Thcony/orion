//! Portao de seguranca: o que roda livre, o que pergunta, o que nunca roda.
//! Porte fiel do core/guard.py (mesmos padroes, mesmos casos de teste).
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Nivel {
    Livre,
    Confirma,
    Bloqueio,
}

const INTOCAVEL: [&str; 8] = [
    r"c:\windows",
    r"c:\program files\windowsapps",
    r"c:\programdata\microsoft\windows\start menu",
    r"c:\$recycle.bin",
    r"c:\boot",
    r"c:\recovery",
    r"c:\system volume information",
    r"c:\program files\microsoft\edge",
];

const MARCA_SISTEMA: [&str; 7] =
    [r"\system32", r"\syswow64", r"\windowsapps", r"\winsxs", r"\drivers\", r"\boot\", r"\bootmgr"];

const PROC_CRITICO: [&str; 14] = [
    "system", "system idle process", "csrss.exe", "wininit.exe", "winlogon.exe",
    "services.exe", "lsass.exe", "smss.exe", "svchost.exe", "dwm.exe",
    "explorer.exe", "fontdrvhost.exe", "registry", "memory compression",
];

const PROIBIDO: [&str; 6] = [
    r"format\s+[a-z]:",
    r"diskpart",
    r":\(\)\s*\{.*\};:",
    r"vssadmin.*delete\s+shadows",
    r"bcdedit",
    r"cipher\s+/w",
];

const DESTRUTIVO: [&str; 26] = [
    r"\bdel\b", r"\berase\b", r"\brd\b", r"\brmdir\b", r"\bremove-item\b",
    r"\bformat\b", r"\bdiskpart\b", r"\bcipher\s+/w", r"\bsdelete\b",
    r"\breg\s+delete\b", r"\bremove-itemproperty\b", r"\bbcdedit\b",
    r"\bvssadmin\b", r"\bwmic\b.*\bdelete\b", r"\buninstall\b",
    r"\btaskkill\b", r"\bstop-process\b", r"\bshutdown\b", r"\brestart-computer\b",
    r"\bnet\s+user\b", r"\bicacls\b", r"\btakeown\b", r"\bset-executionpolicy\b",
    r"\bnew-service\b", r"\bsc\s+(delete|config)\b", r"\bschtasks\b.*/create",
];

const ALVO_SISTEMA: [&str; 11] = [
    r"c:\\windows", r"system32", r"syswow64", r"\\boot\b", r"c:\\recovery",
    r"windowsapps", r"c:\\programdata\\microsoft\\windows",
    r"\bhklm\b", r"hkey_local_machine", r"\bc:\\\s*$", r"\bc:\\\*",
];

const VERBO_DESTRUTIVO: [&str; 9] = [
    r"\bdel\b", r"\berase\b", r"\brd\b", r"\brmdir\b", r"\bremove-item\b",
    r"\breg\s+delete\b", r"\btakeown\b", r"\bicacls\b", r"\bsdelete\b",
];

fn compila(lista: &[&str]) -> Vec<Regex> {
    lista.iter().map(|p| Regex::new(p).expect("regex do guard")).collect()
}

fn regexes() -> &'static [Vec<Regex>; 4] {
    static R: OnceLock<[Vec<Regex>; 4]> = OnceLock::new();
    R.get_or_init(|| [compila(&PROIBIDO), compila(&DESTRUTIVO), compila(&ALVO_SISTEMA), compila(&VERBO_DESTRUTIVO)])
}

/// Normaliza caminho pra comparar, sem depender do SO onde roda.
fn norm(p: &str) -> String {
    let mut t = crate::config::expandir(p).replace('/', "\\").to_lowercase();
    while t.contains("\\\\") {
        t = t.replace("\\\\", "\\");
    }
    t.trim_end_matches('\\').to_string()
}

pub fn caminho_intocavel(caminho: &str) -> bool {
    let c = norm(caminho);
    if c.is_empty() {
        return true;
    }
    // raiz de disco inteira: "c:", "c:\", "d:\*"
    let b = c.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(&c[2..], "" | "\\*" | "*") {
        return true;
    }
    if INTOCAVEL.iter().any(|i| c == *i || c.starts_with(&format!("{i}\\"))) {
        return true;
    }
    let com_barra = format!("{c}\\");
    MARCA_SISTEMA.iter().any(|m| com_barra.contains(m))
}

pub fn avaliar_comando(cmd: &str) -> (Nivel, String) {
    let c = cmd.to_lowercase();
    let [proibido, destrutivo, alvo, verbo] = regexes();
    if proibido.iter().any(|r| r.is_match(&c)) {
        return (Nivel::Bloqueio, "comando quebraria o Windows ou apagaria o disco".into());
    }
    if verbo.iter().any(|r| r.is_match(&c)) && alvo.iter().any(|r| r.is_match(&c)) {
        return (Nivel::Bloqueio, "isso apaga area do sistema, o Windows nao da boot depois".into());
    }
    if destrutivo.iter().any(|r| r.is_match(&c)) {
        return (Nivel::Confirma, "comando mexe/apaga coisa no sistema".into());
    }
    (Nivel::Livre, String::new())
}

fn nivel_padrao(tool: &str) -> Nivel {
    match tool {
        "rodar_cmd" | "fechar_app" | "mover" | "apagar" | "desinstalar" | "aplicar_update" => Nivel::Confirma,
        "abrir_app" | "app_aberto" | "listar_processos" | "trocar_wallpaper" | "listar_pasta"
        | "renomear" | "criar_pasta" | "escrever_arquivo" | "ler_arquivo" | "achar_app"
        | "info_sistema" | "meu_ip" | "rede" | "agora" | "criar_lembrete" | "listar_lembretes"
        | "anotar" | "lembrar_isso" | "buscar_update" | "pesquisar" | "abrir_site" => Nivel::Livre,
        _ => Nivel::Confirma, // tool desconhecida: pergunta
    }
}

pub fn avaliar(tool: &str, args: &Value) -> (Nivel, String) {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let nivel = nivel_padrao(tool);

    if tool == "rodar_cmd" {
        return avaliar_comando(&arg("comando"));
    }
    if matches!(tool, "apagar" | "renomear" | "mover") {
        for k in ["caminho", "origem", "destino"] {
            let v = arg(k);
            if args.get(k).is_some() && caminho_intocavel(&v) {
                return (Nivel::Bloqueio, format!("{v} e area do sistema"));
            }
        }
    }
    if tool == "fechar_app" {
        let mut n = arg("nome").to_lowercase();
        if !n.ends_with(".exe") {
            n.push_str(".exe");
        }
        if PROC_CRITICO.contains(&n.as_str()) {
            return (Nivel::Bloqueio, format!("{n} e processo critico do Windows"));
        }
    }
    if tool == "desinstalar" {
        let n = arg("nome").to_lowercase();
        if n.contains("edge") || n.contains("webview") {
            return (Nivel::Confirma, "o Edge/WebView2 e costurado no Windows: Windows Update, widgets e \
                apps que usam WebView2 podem parar. Da pra remover, mas quebra junto".into());
        }
        if n.contains("windows") || n.contains("defender") {
            return (Nivel::Bloqueio, "componente do proprio Windows".into());
        }
    }
    let motivo = if nivel == Nivel::Confirma { "acao destrutiva".to_string() } else { String::new() };
    (nivel, motivo)
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;
    use Nivel::*;

    #[test]
    fn comandos_iguais_ao_python() {
        for (cmd, esperado) in [
            ("start discord", Livre),
            (r"del /s /q C:\Windows\System32", Bloqueio),
            (r"rd /s /q c:\windows", Bloqueio),
            (r"Remove-Item -Recurse C:\Windows", Bloqueio),
            (r"reg delete HKLM\Software\Microsoft /f", Bloqueio),
            ("winget uninstall Steam", Confirma),
            (r"del %TEMP%\lixo.txt", Confirma),
            ("format c:", Bloqueio),
            ("diskpart", Bloqueio),
            (r"dir C:\Users", Livre),
            ("ipconfig /all", Livre),
            ("taskkill /im chrome.exe", Confirma),
        ] {
            assert_eq!(avaliar_comando(cmd).0, esperado, "comando: {cmd}");
        }
    }

    #[test]
    fn caminhos_iguais_ao_python() {
        for (c, esperado) in [
            (r"C:\Windows\System32", true), ("c:/windows", true), ("C:\\", true), ("c:", true),
            (r"D:\*", true), (r"C:\Program Files\WindowsApps\x", true),
            (r"C:\Program Files\Steam", false), (r"C:\Users\ant\Documents\lixo", false),
            (r"%TEMP%\cache", false), (r"C:\Windows\System32\drivers", true),
            (r"C:\Program Files\Microsoft\Edge", true), ("", true),
        ] {
            assert_eq!(caminho_intocavel(c), esperado, "caminho: {c}");
        }
    }

    #[test]
    fn acoes_iguais_ao_python() {
        assert_eq!(avaliar("apagar", &json!({"caminho": r"C:\Windows\System32"})).0, Bloqueio);
        assert_eq!(avaliar("apagar", &json!({"caminho": r"C:\Program Files\Steam"})).0, Confirma);
        assert_eq!(avaliar("mover", &json!({"origem": "c:", "destino": "d:"})).0, Bloqueio);
        assert_eq!(avaliar("renomear", &json!({"origem": r"C:\Windows", "destino": "x"})).0, Bloqueio);
        assert_eq!(avaliar("fechar_app", &json!({"nome": "explorer"})).0, Bloqueio);
        assert_eq!(avaliar("fechar_app", &json!({"nome": "chrome.exe"})).0, Confirma);
        assert_eq!(avaliar("desinstalar", &json!({"nome": "Microsoft Edge"})).0, Confirma);
        assert_eq!(avaliar("desinstalar", &json!({"nome": "Windows Defender"})).0, Bloqueio);
        assert_eq!(avaliar("abrir_app", &json!({"nome": "discord"})).0, Livre);
        assert_eq!(avaliar("tool_inventada", &json!({})).0, Confirma);
    }
}
