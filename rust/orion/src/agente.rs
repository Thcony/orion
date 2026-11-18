//! Um turno: Gemini decide, guard aprova, tools executam.
//! Se o Gemini pedir (ver_resultado), ele le o resultado e responde em cima.
use crate::cerebro::{Cerebro, Resposta};
use crate::guard::{self, Nivel};
use crate::tools::{self, Ctx};
use serde_json::Value;

const MAX_RODADAS: usize = 2;

pub enum Saida {
    Orion(String),
    Info(String),
    Erro(String),
}

pub fn responder(ctx: &Ctx, cerebro: &Cerebro, entrada: &str, dizer: &dyn Fn(Saida),
                 confirmar: &dyn Fn(&str, &Value, &str) -> bool) {
    let (contexto, resumo) = {
        let mut m = ctx.memoria.lock().unwrap();
        m.registrar(&ctx.cfg.chefe, entrada);
        (m.turnos_recentes(), m.resumo())
    };
    let mut r = cerebro.pensar(entrada, tools::DESCRICAO, &contexto, &resumo);
    for rodada in 0..=MAX_RODADAS {
        let lidos = executar(ctx, &r, dizer, confirmar);
        if !r.ver_resultado || lidos.is_empty() || rodada == MAX_RODADAS {
            return;
        }
        let bloco: Vec<String> = lidos.iter().map(|(t, res)| format!("{t} -> {}", res.chars().take(3000).collect::<String>())).collect();
        let contexto = ctx.memoria.lock().unwrap().turnos_recentes();
        r = cerebro.pensar(
            &format!("{entrada}\n\nRESULTADO DAS SUAS ACOES:\n{}\n\nAgora responde o chefe com base nisso.", bloco.join("\n\n")),
            tools::DESCRICAO, &contexto, &resumo);
    }
}

fn executar(ctx: &Ctx, r: &Resposta, dizer: &dyn Fn(Saida), confirmar: &dyn Fn(&str, &Value, &str) -> bool) -> Vec<(String, String)> {
    if !r.fala.trim().is_empty() {
        dizer(Saida::Orion(r.fala.trim().to_string()));
        ctx.memoria.lock().unwrap().registrar("Orion", r.fala.trim());
    }
    let mut lidos = Vec::new();
    for a in &r.acoes {
        let (nivel, motivo) = guard::avaliar(&a.tool, &a.args);
        let registrar = |res: &str| ctx.memoria.lock().unwrap().registrar_acao(&a.tool, &a.args, res);
        if nivel == Nivel::Bloqueio {
            dizer(Saida::Erro(format!("[bloqueado] {}: {motivo}", a.tool)));
            registrar(&format!("BLOQUEADO: {motivo}"));
            continue;
        }
        if nivel == Nivel::Confirma && !ctx.cfg.sem_freio && !confirmar(&a.tool, &a.args, &motivo) {
            dizer(Saida::Info("deixei pra la".into()));
            registrar("recusado pelo chefe");
            continue;
        }
        let res = tools::executar(ctx, &a.tool, &a.args);
        registrar(&res);
        if r.ver_resultado {
            let curto: String = res.chars().take(400).collect();
            dizer(Saida::Info(if res.chars().count() > 400 { format!("{curto} ...") } else { curto }));
            lidos.push((a.tool.clone(), res));
        } else {
            dizer(Saida::Info(res));
        }
    }
    lidos
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::cerebro::testes::cerebro;
    use crate::memoria::Memoria;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn le_resultado_confirma_e_bloqueia() {
        let (c, _) = cerebro("orion_ag_t", Box::new(|_, corpo| {
            let txt = corpo["contents"][0]["parts"][0]["text"].as_str().unwrap_or("");
            let agora = txt.split("AGORA:").last().unwrap_or("");
            let r = if agora.contains("RESULTADO DAS SUAS ACOES") {
                json!({"fala": "Achei: ta escrito oi.", "acoes": []})
            } else if agora.contains("le o arquivo") {
                json!({"fala": "Deixa eu ver.", "ver_resultado": true,
                       "acoes": [{"tool": "ler_arquivo", "args": {"caminho": "/tmp/orion_ag_arquivo.txt"}}]})
            } else {
                json!({"fala": "Beleza.", "acoes": [
                    {"tool": "apagar", "args": {"caminho": "/tmp/orion_ag_naoapaga.txt"}},
                    {"tool": "fechar_app", "args": {"nome": "explorer"}}]})
            };
            (200, json!({"candidates": [{"content": {"parts": [{"text": r.to_string()}]}}]}).to_string())
        }));
        std::fs::write("/tmp/orion_ag_arquivo.txt", "oi").unwrap();
        std::fs::write("/tmp/orion_ag_naoapaga.txt", "x").unwrap();
        let ctx = Ctx { memoria: Arc::new(Mutex::new(Memoria::new(&c.cfg))), cfg: c.cfg.clone(),
                        http: Arc::new(crate::cerebro::Rede::new()) };
        let saidas = Mutex::new(Vec::<String>::new());
        let dizer = |s: Saida| saidas.lock().unwrap().push(match s {
            Saida::Orion(t) => format!("orion:{t}"), Saida::Info(t) => format!("info:{t}"), Saida::Erro(t) => format!("erro:{t}") });
        let perguntou = Mutex::new(vec![]);
        let confirmar = |t: &str, _: &Value, _: &str| { perguntou.lock().unwrap().push(t.to_string()); false };

        responder(&ctx, &c, "le o arquivo", &dizer, &confirmar);
        assert!(saidas.lock().unwrap().contains(&"orion:Achei: ta escrito oi.".to_string()));

        saidas.lock().unwrap().clear();
        responder(&ctx, &c, "faz ai", &dizer, &confirmar);
        let s = saidas.lock().unwrap().join(" | ");
        assert!(s.contains("info:deixei pra la") && s.contains("erro:[bloqueado] fechar_app"), "{s}");
        assert_eq!(*perguntou.lock().unwrap(), ["apagar"]);
        assert!(std::path::Path::new("/tmp/orion_ag_naoapaga.txt").exists()); // recusou = nao apagou
    }
}
