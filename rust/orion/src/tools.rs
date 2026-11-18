//! Acoes no PC. Comandos rodam sem piscar janela preta (CREATE_NO_WINDOW)
//! e com saida em UTF-8 (acento nao vira lixo).
use crate::cerebro::Http;
use crate::config::{expandir, Config};
use crate::guard;
use crate::memoria::Memoria;
use base64::Engine;
use chrono::{Datelike, Local};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Ctx {
    pub cfg: Config,
    pub memoria: Arc<Mutex<Memoria>>,
    pub http: Arc<dyn Http>,
}

pub const DESCRICAO: &str = "- rodar_cmd(comando, shell=\"cmd\"|\"powershell\") comando cru
- abrir_app(nome) abre app, jogo, atalho, arquivo ou site | fechar_app(nome) | app_aberto(nome)
- listar_processos(filtro) o que ta aberto e quanto de RAM usa
- listar_pasta(caminho) / criar_pasta(caminho) / renomear(origem,destino) / mover(origem,destino) / apagar(caminho)
- escrever_arquivo(caminho,conteudo) / ler_arquivo(caminho)
- info_sistema() CPU, RAM, discos | meu_ip() | rede()
- agora() hora e data
- anotar(titulo, texto) no Obsidian | lembrar_isso(chave, valor)";

// ---------------- processos ----------------
pub(crate) fn comando(programa: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(programa);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// Roda com limite de tempo, lendo stdout/stderr em paralelo (sem travar em pipe cheio).
fn rodar(mut c: Command, limite: Duration) -> (Option<i32>, String, String) {
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut filho = match c.spawn() {
        Ok(f) => f,
        Err(e) => return (None, String::new(), format!("nao consegui rodar: {e}")),
    };
    let ler = |p: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            if let Some(mut p) = p { let _ = p.read_to_end(&mut b); }
            String::from_utf8_lossy(&b).into_owned()
        })
    };
    let (o, e) = (ler(filho.stdout.take().map(|x| Box::new(x) as _)), ler(filho.stderr.take().map(|x| Box::new(x) as _)));
    let inicio = Instant::now();
    let codigo = loop {
        match filho.try_wait() {
            Ok(Some(s)) => break s.code(),
            Ok(None) if inicio.elapsed() > limite => {
                let _ = filho.kill();
                let _ = filho.wait();
                return (None, o.join().unwrap_or_default(), format!("passou de {}s, matei", limite.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => break { let _ = e; None },
        }
    };
    (codigo, o.join().unwrap_or_default(), e.join().unwrap_or_default())
}

fn cmd(linha: &str, limite: u64) -> (Option<i32>, String, String) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = comando("cmd");
        c.raw_arg(format!("/d /s /c \"chcp 65001>nul & {linha}\"")); // raw: aspas do comando chegam intactas
        rodar(c, Duration::from_secs(limite))
    }
    #[cfg(not(windows))]
    {
        let mut c = comando("sh");
        c.args(["-c", linha]);
        rodar(c, Duration::from_secs(limite))
    }
}

/// PowerShell via -EncodedCommand: zero problema com aspas.
pub(crate) fn powershell_cmd(script: &str) -> Command {
    let s = format!("[Console]::OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'; {script}");
    let utf16: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut c = comando("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &base64::engine::general_purpose::STANDARD.encode(utf16)]);
    c
}

fn powershell(script: &str, limite: u64) -> (Option<i32>, String, String) {
    rodar(powershell_cmd(script), Duration::from_secs(limite))
}

fn juntar((codigo, out, err): (Option<i32>, String, String)) -> String {
    let mut s = out.trim_end().to_string();
    if !err.trim().is_empty() {
        s.push_str(&format!("\n[erro]\n{}", err.trim_end()));
    }
    if s.trim().is_empty() {
        s = format!("(sem saida, codigo {})", codigo.map_or("?".into(), |c| c.to_string()));
    }
    s.chars().take(4000).collect()
}

pub fn rodar_cmd(comando: &str, shell: &str) -> String {
    juntar(if shell == "powershell" { powershell(comando, 300) } else { cmd(comando, 300) })
}

// ---------------- apps ----------------
/// Procura atalho (.lnk/.url) no Menu Iniciar e na Area de Trabalho.
/// E assim que "abre o discord" ou "abre o jogo X da Steam" funciona.
fn achar_atalho(nome: &str) -> Option<PathBuf> {
    let alvo = nome.to_lowercase();
    let mut bases: Vec<PathBuf> = Vec::new();
    for var in ["APPDATA", "ProgramData"] {
        if let Ok(b) = std::env::var(var) {
            bases.push(Path::new(&b).join("Microsoft\\Windows\\Start Menu\\Programs"));
        }
    }
    if let Some(d) = dirs::desktop_dir() { bases.push(d); }
    if let Ok(p) = std::env::var("PUBLIC") { bases.push(Path::new(&p).join("Desktop")); }

    let mut achados: Vec<PathBuf> = Vec::new();
    let mut pilha: Vec<(PathBuf, u8)> = bases.into_iter().map(|b| (b, 0)).collect();
    while let Some((dir, nivel)) = pilha.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && nivel < 4 {
                pilha.push((p, nivel + 1));
            } else if matches!(p.extension().and_then(|x| x.to_str()).map(str::to_lowercase).as_deref(), Some("lnk" | "url")) {
                let n = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                if n.contains(&alvo) && !n.contains("uninstall") && !n.contains("desinstalar") {
                    achados.push(p);
                }
            }
        }
    }
    // o nome mais curto costuma ser o app principal ("Discord" e nao "Discord PTB Update")
    achados.sort_by_key(|p| p.file_stem().map(|s| s.len()).unwrap_or(usize::MAX));
    achados.into_iter().next()
}

fn iniciar(alvo: &str) -> bool {
    matches!(cmd(&format!("start \"\" \"{alvo}\""), 20).0, Some(0))
}

pub fn abrir_app(nome: &str) -> String {
    let n = expandir(nome);
    let eh_caminho = Path::new(&n).exists() || n.contains("://") || n.starts_with("www.");
    if eh_caminho {
        let u = if n.starts_with("www.") { format!("https://{n}") } else { n.clone() };
        return if iniciar(&u) { format!("abri {u}") } else { format!("o Windows nao abriu {u}") };
    }
    if let Some(a) = achar_atalho(&n) {
        let nome_at = a.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        return if iniciar(&a.to_string_lossy()) { format!("abri {nome_at}") } else { format!("achei o atalho {nome_at} mas nao abriu") };
    }
    let exe = if n.to_lowercase().ends_with(".exe") { n.clone() } else { format!("{n}.exe") };
    if matches!(cmd(&format!("where \"{exe}\""), 10).0, Some(0)) && iniciar(&exe) {
        return format!("abri {n}");
    }
    format!("nao achei '{nome}' no Menu Iniciar, na Area de Trabalho nem no PATH")
}

fn nome_exe(nome: &str) -> String {
    let n = nome.trim().to_lowercase();
    if n.ends_with(".exe") { n } else { format!("{n}.exe") }
}

/// (nome, MB) de cada processo, somando as instancias.
fn processos() -> Vec<(String, u64)> {
    let (_, out, _) = cmd("tasklist /fo csv /nh", 30);
    let mut soma: HashMap<String, u64> = HashMap::new();
    for l in out.lines() {
        let campos: Vec<&str> = l.split("\",\"").map(|c| c.trim_matches('"')).collect();
        if campos.len() >= 5 {
            let kb: u64 = campos[4].chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0);
            *soma.entry(campos[0].to_string()).or_default() += kb / 1024;
        }
    }
    let mut v: Vec<(String, u64)> = soma.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

pub fn fechar_app(nome: &str) -> String {
    let exe = nome_exe(nome);
    let (codigo, out, err) = cmd(&format!("taskkill /im \"{exe}\" /f /t"), 30);
    if codigo == Some(0) { format!("fechei {exe}") } else { format!("{exe} nao fechou: {}", (out + &err).trim()) }
}

pub fn app_aberto(nome: &str) -> String {
    let exe = nome_exe(nome);
    match processos().into_iter().find(|(n, _)| n.to_lowercase() == exe) {
        Some((n, mb)) => format!("{n} esta aberto ({mb} MB)"),
        None => format!("{exe} nao esta aberto"),
    }
}

pub fn listar_processos(filtro: &str) -> String {
    let f = filtro.to_lowercase();
    let l: Vec<String> = processos().into_iter().filter(|(n, _)| f.is_empty() || n.to_lowercase().contains(&f))
        .take(25).map(|(n, mb)| format!("{n} - {mb} MB")).collect();
    if l.is_empty() { "nada casou".into() } else { l.join("\n") }
}

// ---------------- arquivos ----------------
fn caminho(c: &str) -> PathBuf {
    PathBuf::from(expandir(c))
}

pub fn listar_pasta(c: &str) -> String {
    let p = caminho(if c.is_empty() { "." } else { c });
    let Ok(rd) = fs::read_dir(&p) else { return format!("{} nao e pasta", p.display()) };
    let mut itens: Vec<(bool, String, u64)> = rd.flatten()
        .map(|e| { let m = e.metadata().ok(); (m.as_ref().is_some_and(|m| m.is_dir()), e.file_name().to_string_lossy().into_owned(), m.map_or(0, |m| m.len())) })
        .collect();
    itens.sort_by(|a, b| (!a.0, a.1.to_lowercase()).cmp(&(!b.0, b.1.to_lowercase())));
    let l: Vec<String> = itens.iter().take(200).map(|(dir, n, t)| if *dir { format!("[pasta] {n}") } else { format!("        {n}  {} KB", t / 1024) }).collect();
    if l.is_empty() { "pasta vazia".into() } else { l.join("\n") }
}

pub fn criar_pasta(c: &str) -> String {
    let p = caminho(c);
    match fs::create_dir_all(&p) { Ok(_) => format!("pasta criada: {}", p.display()), Err(e) => format!("nao criei: {e}") }
}

pub fn renomear(o: &str, d: &str) -> String {
    let (o, d) = (caminho(o), caminho(d));
    // "destino" so com nome novo: fica na mesma pasta
    let d = if d.parent().is_none_or(|p| p.as_os_str().is_empty()) { o.with_file_name(&d) } else { d };
    match fs::rename(&o, &d) { Ok(_) => format!("renomeei pra {}", d.display()), Err(e) => format!("nao renomeei: {e}") }
}

pub fn mover(o: &str, d: &str) -> String {
    let (o, mut d) = (caminho(o), caminho(d));
    if d.is_dir() { if let Some(n) = o.file_name() { d = d.join(n); } }
    match fs::rename(&o, &d) {
        Ok(_) => format!("movi pra {}", d.display()),
        // outro disco: rename nao atravessa, entao copia e apaga
        Err(_) if o.is_file() => match fs::copy(&o, &d).and_then(|_| fs::remove_file(&o)) {
            Ok(_) => format!("movi pra {}", d.display()),
            Err(e) => format!("nao movi: {e}"),
        },
        Err(e) => format!("nao movi: {e}"),
    }
}

pub fn apagar(c: &str) -> String {
    if guard::caminho_intocavel(c) {
        return format!("NAO. {c} e area do sistema.");
    }
    let p = caminho(c);
    let r = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
    match r { Ok(_) => format!("apaguei {}", p.display()), Err(e) => format!("nao apaguei {}: {e}", p.display()) }
}

pub fn escrever_arquivo(c: &str, conteudo: &str) -> String {
    let p = caminho(c);
    if let Some(pai) = p.parent() { let _ = fs::create_dir_all(pai); }
    match fs::write(&p, conteudo) { Ok(_) => format!("escrevi {} ({} chars)", p.display(), conteudo.chars().count()), Err(e) => format!("nao escrevi: {e}") }
}

pub fn ler_arquivo(c: &str) -> String {
    let p = caminho(c);
    let Ok(b) = fs::read(&p) else { return format!("nao achei {}", p.display()) };
    if b.iter().take(4096).any(|&x| x == 0) {
        return format!("{} e binario ({} KB); so leio texto aqui", p.display(), b.len() / 1024);
    }
    String::from_utf8_lossy(&b[..b.len().min(200_000)]).chars().take(4000).collect()
}

// ---------------- sistema ----------------
pub fn info_sistema() -> String {
    juntar(powershell(r#"
$c=(Get-CimInstance Win32_Processor | Measure-Object LoadPercentage -Average).Average
$o=Get-CimInstance Win32_OperatingSystem
$t=$o.TotalVisibleMemorySize/1MB; $u=$t-$o.FreePhysicalMemory/1MB
"PC: $env:COMPUTERNAME | CPU: $c% | RAM: {0:N1}/{1:N1} GB" -f $u,$t
Get-CimInstance Win32_LogicalDisk -Filter "DriveType=3" | ForEach-Object { "{0} {1:N0} GB livres de {2:N0} GB" -f $_.DeviceID,($_.FreeSpace/1GB),($_.Size/1GB) }
"#, 60))
}

pub fn meu_ip(http: &dyn Http) -> String {
    let publico = match http.get("https://api.ipify.org") {
        Ok((200, ip)) => ip.trim().to_string(),
        _ => "nao consegui (sem internet?)".into(),
    };
    let local = juntar(cmd("ipconfig | findstr /i \"IPv4\"", 20));
    format!("IP publico: {publico}\n{local}")
}

pub fn agora() -> String {
    let d = Local::now();
    let dias = ["segunda", "terca", "quarta", "quinta", "sexta", "sabado", "domingo"];
    format!("{}, {}", dias[d.weekday().num_days_from_monday() as usize], d.format("%d/%m/%Y, %H:%M"))
}

// ---------------- despacho ----------------
pub fn executar(ctx: &Ctx, tool: &str, args: &Value) -> String {
    let a = |k: &str| args.get(k).and_then(|v| v.as_str().map(String::from).or_else(|| (!v.is_null()).then(|| v.to_string())));
    let obrig = |k: &str| a(k).ok_or_else(|| format!("argumentos errados pra {tool}: falta '{k}'"));
    let r: Result<String, String> = (|| Ok(match tool {
        "rodar_cmd" => rodar_cmd(&obrig("comando")?, &a("shell").unwrap_or_default()),
        "abrir_app" => abrir_app(&obrig("nome")?),
        "fechar_app" => fechar_app(&obrig("nome")?),
        "app_aberto" => app_aberto(&obrig("nome")?),
        "listar_processos" => listar_processos(&a("filtro").unwrap_or_default()),
        "listar_pasta" => listar_pasta(&a("caminho").unwrap_or_default()),
        "criar_pasta" => criar_pasta(&obrig("caminho")?),
        "renomear" => renomear(&obrig("origem")?, &obrig("destino")?),
        "mover" => mover(&obrig("origem")?, &obrig("destino")?),
        "apagar" => apagar(&obrig("caminho")?),
        "escrever_arquivo" => escrever_arquivo(&obrig("caminho")?, &obrig("conteudo")?),
        "ler_arquivo" => ler_arquivo(&obrig("caminho")?),
        "info_sistema" => info_sistema(),
        "meu_ip" => meu_ip(ctx.http.as_ref()),
        "rede" => juntar(cmd("ipconfig /all", 30)),
        "agora" => agora(),
        "anotar" => match ctx.memoria.lock().unwrap().anotar(&obrig("titulo")?, &obrig("texto")?) {
            Ok(f) => format!("anotei no vault: {f}"),
            Err(e) => format!("nao anotei: {e}"),
        },
        "lembrar_isso" => {
            let (k, v) = (obrig("chave")?, obrig("valor")?);
            match ctx.memoria.lock().unwrap().lembrar_fato(&k, &v) { Ok(_) => format!("guardei: {k} = {v}"), Err(e) => format!("nao guardei: {e}") }
        }
        outra => format!("tool '{outra}' ainda nao existe nesta versao"),
    }))();
    r.unwrap_or_else(|e| e)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn arquivos_e_binario() {
        let d = std::env::temp_dir().join("orion_tools_t");
        let _ = fs::remove_dir_all(&d);
        let f = d.join("sub").join("a.txt");
        assert!(escrever_arquivo(&f.to_string_lossy(), "olá, chefe").starts_with("escrevi"));
        assert_eq!(ler_arquivo(&f.to_string_lossy()), "olá, chefe");
        assert!(renomear(&f.to_string_lossy(), "b.txt").contains("b.txt"));
        assert!(listar_pasta(&d.join("sub").to_string_lossy()).contains("b.txt"));
        fs::write(d.join("x.bin"), [0u8, 1, 2, 3]).unwrap();
        assert!(ler_arquivo(&d.join("x.bin").to_string_lossy()).contains("binario"));
        assert!(apagar(&d.join("sub").to_string_lossy()).starts_with("apaguei"));
        assert!(apagar(r"C:\Windows\System32").starts_with("NAO"));
    }

    #[test]
    fn comando_com_aspas_e_limite_de_tempo() {
        // cmd.exe imprime as aspas, sh tira; o que importa e chegar intacto (sem \" escapado)
        let esperado = if cfg!(windows) { "\"oi chefe\"" } else { "oi chefe" };
        assert_eq!(rodar_cmd("echo \"oi chefe\"", "cmd"), esperado);
        let inicio = Instant::now();
        let r = juntar(rodar({ let mut c = Command::new("sleep"); c.arg("5"); c }, Duration::from_millis(300)));
        assert!(r.contains("matei") && inicio.elapsed() < Duration::from_secs(2));
    }
}
