// Multi-meeting synthesis family: each query spans two named projects whose
// decisions live in different meetings. Snapshot/today hydration keeps the
// declared meeting order deterministic.

use super::super::EvaluationCase;
use super::{case, dm, ev, mtg, scope, Evidence, Language, MeetingState, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    vec![
        multi_case(
            "pt-multi-aurora-boreal",
            Language::Portuguese,
            "decisões dos projetos Aurora e Boreal?",
            ("mtg-projeto-aurora", "Projeto Aurora — kickoff", "2026-06-14", Some("produto")),
            vec![
                ev("au-escopo", "summary", "Decisões do kick-off do Projeto Aurora: escopo fechado de faturamento e time dedicado de quatro pessoas."),
                ev("au-revisao", "note", "Revisão do Aurora acontece em 3 novembro 2026 com demonstração interna das automações."),
            ],
            ("mtg-projeto-boreal", "Projeto Boreal — piloto", "2026-05-18", Some("produto")),
            vec![
                ev("bo-piloto", "note", "Piloto do Projeto Boreal fica limitado a duas filiais até a avaliação de custo operacional."),
                ev("bo-metrica", "summary", "Métrica de sucesso do Boreal compara notas de satisfação antes e depois da trilha nova."),
            ],
            &["escopo fechado de faturamento e time dedicado de quatro pessoas", "limitado a duas filiais"],
            &["expansão imediata para todas as filiais"],
            ("mtg-outros-projetos", "Portfólio — projetos paralelos", "2026-04-20", Some("produto"),
                "Projetos paralelos seguem no portfólio trimestral com donos nomeados."),
            false,
        ),
        multi_case(
            "en-multi-northwind-ironwood",
            Language::English,
            "Northwind and Ironwood project calls?",
            ("mtg-project-northwind", "Project Northwind — kickoff", "2026-06-09", Some("product")),
            vec![
                ev("nw-call", "summary", "Project Northwind moves forward with weekly sync calls and a single decision log."),
                ev("nw-date", "note", "The Northwind review call lands on 9 October 2026 right after the vendor demo."),
            ],
            ("mtg-project-ironwood", "Project Ironwood — pilot", "2026-05-21", Some("product")),
            vec![
                ev("iw-pilot", "note", "Ironwood stays a two-store pilot until the hardware refresh budget clears."),
                ev("iw-goal", "summary", "Ironwood targets shelf-scan errors dropping by half during the pilot window."),
            ],
            &["weekly sync calls and a single decision log", "on 9 October 2026"],
            &["company-wide rollout before the pilot readout"],
            ("mtg-other-initiatives", "Portfolio — other initiatives", "2026-04-16", None,
                "Other portfolio initiatives keep named owners in the quarterly deck."),
            false,
        ),
        multi_case(
            "pt-multi-vesper-zenite",
            Language::Portuguese,
            "andamento do Vesper e do Zênite?",
            ("mtg-projeto-vesper", "Projeto Vesper — status", "2026-07-08", Some("engenharia")),
            vec![
                ev("ve-status", "summary", "Vesper concluiu migração de filas e agora compartilha métricas diárias com o suporte."),
                ev("ve-marco", "note", "Marco seguinte do Vesper é o congelamento de esquema em 21 dezembro 2026."),
            ],
            ("mtg-projeto-zenite", "Projeto Zênite — direção", "2026-06-01", Some("engenharia")),
            vec![
                ev("ze-direcao", "note", "Zênite adota o roteiro curto de duas fases com revisão externa no meio."),
                ev("ze-time", "summary", "Time do Zênite incorpora uma pessoa de dados desde a última alocação."),
            ],
            &["congelamento de esquema em 21 dezembro 2026", "roteiro curto de duas fases"],
            &["prazos esticados sem marco intermediário"],
            ("mtg-backlog-engenharia", "Backlog de engenharia", "2026-05-11", None,
                "O backlog de engenharia recebe triagem semanal com rótulos de esforço."),
            false,
        ),
        multi_case(
            "en-multi-bluefin-kestrel",
            Language::English,
            "Bluefin and Kestrel rollout notes?",
            ("mtg-project-bluefin", "Project Bluefin — rollout", "2026-06-25", Some("operations")),
            vec![
                ev("bf-rollout", "summary", "Bluefin rollout reaches the northern depots first because of carrier contracts."),
                ev("bf-window", "note", "The Bluefin depot switch happens overnight on 14 November 2026."),
            ],
            ("mtg-project-kestrel", "Project Kestrel — scope", "2026-05-30", Some("product")),
            vec![
                ev("ke-scope", "note", "Kestrel narrows scope to the picker app only, leaving label printers untouched."),
                ev("ke-owner", "summary", "Kestrel gains a dedicated analyst for the duration of the field trial."),
            ],
            &["overnight on 14 November 2026", "narrows scope to the picker app"],
            &["flipping all depots in a single weekend"],
            ("mtg-depot-notes", "Depot operations notes", "2026-04-24", None,
                "Depot operation notes feed the monthly reliability review."),
            false,
        ),
        multi_case(
            "pt-multi-coral-sargaco",
            Language::Portuguese,
            "o que decidiu Coral e Sargaço?",
            ("mtg-programa-coral", "Programa Coral — decisão", "2026-06-17", Some("sucesso-cliente")),
            vec![
                ev("co-decisao", "summary", "Coral aprovou cartilha unificada de boas-vindas para contas médias."),
                ev("co-prazo", "note", "Cartilha do Coral vai à gráfica depois da revisão jurídica em 8 outubro 2026."),
            ],
            ("mtg-programa-sargaco", "Programa Sargaço — recorte", "2026-05-13", Some("sucesso-cliente")),
            vec![
                ev("sa-recorte", "note", "Sargaço atende apenas contas industriais na primeira onda do programa."),
                ev("sa-meta", "summary", "Meta inicial do Sargaço reduz retrabalho de cadastro pela metade."),
            ],
            &["cartilha unificada de boas-vindas", "apenas contas industriais na primeira onda"],
            &["abrir todas as frentes ao mesmo tempo"],
            ("mtg-rituais-sucesso", "Rituais do sucesso do cliente", "2026-04-27", None,
                "Os rituais do sucesso do cliente incluem revisão mensal de carteiras."),
            false,
        ),
        multi_case(
            "en-multi-larkspur-basalt",
            Language::English,
            "Larkspur and Basalt budget lines?",
            ("mtg-project-larkspur", "Project Larkspur — funding", "2026-07-10", Some("finance")),
            vec![
                ev("la-funding", "summary", "Larkspur keeps its training fund intact despite the broader spending freeze."),
                ev("la-cap", "note", "Larkspur caps individual course reimbursements at four hundred coins per semester."),
            ],
            ("mtg-project-basalt", "Project Basalt — hardware", "2026-05-26", Some("it")),
            vec![
                ev("ba-hardware", "note", "Basalt defers laptop refreshes one quarter pending supplier quotes."),
                ev("ba-loaner", "summary", "Basalt maintains a loaner pool of six machines for critical failures."),
            ],
            &["four hundred coins per semester", "defers laptop refreshes one quarter"],
            &["unlimited reimbursement requests mid-semester"],
            ("mtg-spending-freeze", "Spending freeze context", "2026-04-22", None,
                "The spending freeze exempts safety-related purchases explicitly."),
            false,
        ),
        multi_case(
            "pt-multi-jacaranda-guavira",
            Language::Portuguese,
            "resultados do Jacarandá e da Guavira?",
            ("mtg-piloto-jacaranda", "Piloto Jacarandá — resultados", "2026-06-29", Some("crescimento")),
            vec![
                ev("ja-resultados", "summary", "Jacarandá elevou conversão do primeiro contato em dez pontos percentuais."),
                ev("ja-nota", "note", "Relatório final do Jacarandá sai em 19 janeiro 2027 com coorte completa."),
            ],
            ("mtg-piloto-guavira", "Piloto Guavira — encerramento", "2026-05-28", Some("crescimento")),
            vec![
                ev("gv-encerramento", "note", "Guavira encerra sem continuidade por custo de manutenção acima do ganho."),
                ev("gv-aprendizado", "summary", "Aprendizado do Guavira orienta critérios de corte de pilotos futuros."),
            ],
            &["dez pontos percentuais", "encerra sem continuidade"],
            &["manter os dois pilotos indefinidamente"],
            ("mtg-criterios-pilotos", "Critérios de pilotos", "2026-04-30", None,
                "Critérios de pilotos exigem meta numérica e data de corte desde o início."),
            false,
        ),
        multi_case(
            "en-multi-quartz-meadow",
            Language::English,
            "Quartz and Meadow staffing calls?",
            ("mtg-project-quartz", "Project Quartz — staffing", "2026-07-03", Some("people")),
            vec![
                ev("qu-staffing", "summary", "Quartz adds one recruiter dedicated to senior engineering searches."),
                ev("qu-start", "note", "The Quartz recruiter starts on 2 March 2027 after garden leave ends."),
            ],
            ("mtg-project-meadow", "Project Meadow — rotation", "2026-06-05", Some("people")),
            vec![
                ev("me-rotation", "note", "Meadow rotates two support specialists into product discovery sessions monthly."),
                ev("me-feedback", "summary", "Meadow feedback loops shorten from quarterly to biweekly during the trial."),
            ],
            &["one recruiter dedicated to senior engineering", "rotates two support specialists"],
            &["freezing all hiring including critical roles"],
            ("mtg-hiring-context", "Hiring plan context", "2026-05-08", None,
                "The hiring plan distinguishes growth roles from backfill roles clearly."),
            false,
        ),
        multi_case(
            "pt-multi-onca-pintada",
            Language::Portuguese,
            "o que rolou no Onça e na Pintada?",
            ("mtg-frente-onca", "Frente Onça — segurança", "2026-06-21", Some("seguranca")),
            vec![
                ev("on-seguranca", "summary", "Onça fecha as contas de administrador compartilhadas em todos os sistemas."),
                ev("on-prazo", "note", "Contas compartilhadas do Onça desligam definitivamente em 28 fevereiro 2027."),
            ],
            ("mtg-frente-pintada", "Frente Pintada — acessos", "2026-05-24", Some("seguranca")),
            vec![
                ev("pi-acessos", "note", "Pintada revisa acessos de ex-colaboradores com varredura automatizada mensal."),
                ev("pi-relatorio", "summary", "Relatório da Pintada alimenta o comitê de segurança a cada seis semanas."),
            ],
            &["desligam definitivamente em 28 fevereiro 2027", "varredura automatizada mensal"],
            &["manter contas compartilhadas por praticidade"],
            ("mtg-comite-seguranca", "Comitê de segurança", "2026-04-23", None,
                "O comitê de segurança reúne representantes de plataforma e infraestrutura."),
            false,
        ),
        multi_case(
            "en-multi-harbor-lantern",
            Language::English,
            "Harbor and Lantern launch dates?",
            ("mtg-project-harbor", "Project Harbor — launch", "2026-06-19", Some("product")),
            vec![
                ev("hb-launch", "summary", "Harbor launches to the waitlist cohort with staged invitations over two weeks."),
                ev("hb-date", "note", "First Harbor invites go out on 4 February 2027 after load checks."),
            ],
            ("mtg-project-lantern", "Project Lantern — beta", "2026-05-15", Some("product")),
            vec![
                ev("ln-beta", "note", "Lantern stays an internal beta until accessibility fixes clear the backlog item."),
                ev("ln-notes", "summary", "Lantern session notes stream into the research repository automatically."),
            ],
            &["invites go out on 4 February 2027", "stays an internal beta until accessibility fixes"],
            &["public launch without load checks"],
            ("mtg-release-train", "Release train schedule", "2026-04-17", None,
                "The release train departs every second Tuesday with named conductors."),
            false,
        ),
        multi_case(
            "pt-multi-ipueira-taboca",
            Language::Portuguese,
            "decisões da Ipueira e da Taboca?",
            ("mtg-frente-ipueira", "Frente Ipueira — dados", "2026-07-12", Some("dados")),
            vec![
                ev("ip-dados", "summary", "Ipueira padroniza catálogo de métricas com dono único por indicador."),
                ev("ip-curso", "note", "Curso de catálogo da Ipueira abre turma extra em 11 abril 2027."),
            ],
            ("mtg-frente-taboca", "Frente Taboca — relatórios", "2026-06-02", Some("dados")),
            vec![
                ev("ta-relatorios", "note", "Taboca aposenta relatórios duplicados após inventário de painéis."),
                ev("ta-migracao", "summary", "Migração da Taboca preserva links antigos com redirecionamentos permanentes."),
            ],
            &["dono único por indicador", "aposenta relatórios duplicados"],
            &["dois catálogos paralelos convivendo"],
            ("mtg-inventario-paineis", "Inventário de painéis", "2026-05-06", None,
                "Inventário de painéis roda trimestralmente com exportação de uso."),
            false,
        ),
        multi_case(
            "en-multi-summit-valley",
            Language::English,
            "Summit and Valley retro outcomes?",
            ("mtg-project-summit", "Project Summit — retrospective", "2026-07-15", Some("engineering")),
            vec![
                ev("su-retro", "summary", "Summit adopts trunk-based merging after a contentious retro debate."),
                ev("su-training", "note", "Summit training session on short-lived branches runs on 6 May 2027."),
            ],
            ("mtg-project-valley", "Project Valley — follow-up", "2026-06-10", Some("engineering")),
            vec![
                ev("va-followup", "note", "Valley tracks flaky tests weekly until the quarantine list shrinks twice."),
                ev("va-dashboard", "summary", "Valley dashboard highlights the top five offenders with owner tags."),
            ],
            &["training session on short-lived branches", "tracks flaky tests weekly"],
            &["ignoring flaky tests until the next rewrite"],
            ("mtg-engineering-forum", "Engineering forum notes", "2026-05-12", None,
                "Engineering forum notes collect cross-team proposals between retros."),
            false,
        ),
        multi_case(
            "pt-multi-cerrado-mangue",
            Language::Portuguese,
            "fechamento do Cerrado e do Mangue?",
            ("mtg-frente-cerrado", "Frente Cerrado — campo", "2026-06-27", Some("operacoes")),
            vec![
                ev("ce-campo", "summary", "Cerrado padroniza checklist de campo com fotos obrigatórias por parada."),
                ev("ce-treinamento", "note", "Treinamento do checklist do Cerrado ocorre em 17 maio 2027 para as equipes regionais."),
            ],
            ("mtg-frente-mangue", "Frente Mangue — monitoramento", "2026-05-25", Some("operacoes")),
            vec![
                ev("ma-monitoramento", "note", "Mangue troca planilhas de maré por sensores alugados com calibração mensal."),
                ev("ma-alerta", "summary", "Alertas do Mangue chegam ao plantão com nível, foto e coordenada no mesmo cartão."),
            ],
            &["fotos obrigatórias por parada", "sensores alugados com calibração mensal"],
            &["substituir visitas de campo por ligação telefônica"],
            ("mtg-operacoes-campo", "Rotinas de operações de campo", "2026-04-29", None,
                "As rotinas de campo ganham revisão única anual em workshop presencial."),
            false,
        ),
        multi_case(
            "en-multi-fjord-dune",
            Language::English,
            "Fjord and Dune pilot wrap-ups?",
            ("mtg-project-fjord", "Project Fjord — wrap-up", "2026-07-18", Some("product")),
            vec![
                ev("fj-wrapup", "summary", "Fjord graduates from pilot with a documented runbook and on-call pairing."),
                ev("fj-readout", "note", "The Fjord readout session is booked for 23 June 2027 in the large atrium."),
            ],
            ("mtg-project-dune", "Project Dune — decision", "2026-06-08", Some("product")),
            vec![
                ev("du-decision", "note", "Dune pauses expansion until battery replacements arrive for the field kits."),
                ev("du-inventory", "summary", "Dune inventory counts happen Fridays with a two-person witness rule."),
            ],
            &["documented runbook and on-call pairing", "pauses expansion until battery replacements arrive"],
            &["scaling pilots without runbooks or spares"],
            ("mtg-pilot-review-board", "Pilot review board", "2026-05-21", None,
                "The pilot review board meets monthly with a standing decision log."),
            false,
        ),
        multi_case(
            "en-multi-opal-topaz",
            Language::English,
            "Opal and Topaz onboarding changes?",
            ("mtg-project-opal", "Project Opal — onboarding", "2026-07-22", Some("people")),
            vec![
                ev("op-onboarding", "summary", "Opal trims first-week onboarding to three mandatory sessions only."),
                ev("op-survey", "note", "The Opal feedback survey reopens on 30 July 2027 for the summer cohort."),
            ],
            ("mtg-project-topaz", "Project Topaz — mentoring", "2026-06-16", Some("people")),
            vec![
                ev("to-mentoring", "note", "Topaz pairs every newcomer with a mentor outside their direct team."),
                ev("to-handbook", "summary", "Topaz handbook chapters ship as short pages instead of one long PDF."),
            ],
            &["three mandatory sessions only", "mentor outside their direct team"],
            &["returning to the single marathon orientation day"],
            ("mtg-newcomer-lunch", "Newcomer lunch rotation", "2026-05-27", None,
                "The newcomer lunch rotation mixes departments deliberately each month."),
            false,
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn multi_case(
    id: &str,
    language: Language,
    question: &str,
    first: (&str, &str, &str, Option<&str>),
    first_evidence: Vec<Evidence>,
    second: (&str, &str, &str, Option<&str>),
    second_evidence: Vec<Evidence>,
    facts: &[&str],
    forbidden: &[&str],
    distractor: (&str, &str, &str, Option<&str>, &str),
    critical: bool,
) -> EvaluationCase {
    let first_meeting = mtg(
        first.0,
        first.1,
        first.2,
        first.3,
        MeetingState::Current,
        first_evidence,
    );
    let second_meeting = mtg(
        second.0,
        second.1,
        second.2,
        second.3,
        MeetingState::Current,
        second_evidence,
    );
    let distractor_meeting = dm(
        distractor.0,
        distractor.1,
        distractor.2,
        distractor.3,
        distractor.4,
    );
    let ordinal = id.chars().last().map(|c| c as usize).unwrap_or(0);
    let kind = if ordinal % 2 == 0 {
        ScopeKind::Snapshot
    } else {
        ScopeKind::Today
    };
    let allowed = [first.0, second.0, distractor.0];
    let scope = match kind {
        ScopeKind::Snapshot => scope(ScopeKind::Snapshot, None, None, &allowed),
        _ => scope(ScopeKind::Today, None, None, &allowed),
    };
    let subtype = match (ordinal + id.len()) % 3 {
        0 => "exact_number",
        1 => "exact_date",
        _ => "exact_name",
    };
    let mut required = first_meeting
        .evidence
        .iter()
        .map(|e| e.id.clone())
        .collect::<Vec<_>>();
    required.extend(second_meeting.evidence.iter().map(|e| e.id.clone()));
    let required_refs = required.iter().map(String::as_str).collect::<Vec<_>>();
    case(
        id,
        language,
        question,
        &[],
        None,
        "multi_meeting_synthesis",
        &[
            "exact_term",
            "multi_meeting_synthesis",
            "number_date_name",
            subtype,
        ],
        critical,
        scope,
        vec![first_meeting, second_meeting, distractor_meeting],
        &[first.0, second.0],
        &[(first.0, second.0)],
        &required_refs,
        facts,
        forbidden,
    )
}
