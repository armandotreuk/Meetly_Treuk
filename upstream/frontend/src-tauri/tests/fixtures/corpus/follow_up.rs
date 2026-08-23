// Follow-up family: pronoun questions whose rewritten query carries the
// concrete terms. Exact no-regression contract applies to every case here.

use super::super::EvaluationCase;
use super::{case, dm, ev, mtg, scope, Evidence, Language, MeetingState, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    vec![
        follow_up(
            "pt-followup-parcela-orcamento",
            Language::Portuguese,
            "e o valor da segunda parcela?",
            &[
                "User: qual o orçamento aprovado para eventos?",
                "Assistant: o comitê aprovou verba em duas parcelas anuais.",
            ],
            "valor da segunda parcela do orçamento de eventos",
            ("mtg-eventos-verba", "Orçamento de eventos — parcelas", "2026-06-13", Some("financeiro")),
            vec![
                ev("f1-parcela", "note", "A segunda parcela do orçamento de eventos é de quarenta mil moedas, liberada após o relatório do primeiro semestre."),
                ev("f1-contexto", "summary", "Comitê aprovou verba total de eventos em duas parcelas anuais com prestação obrigatória."),
            ],
            &[],
            &["segunda parcela do orçamento de eventos é de quarenta mil moedas"],
            &["liberação imediata sem relatório"],
            [
                ("mtg-eventos-logistica", "Logística de eventos internos", "2026-05-09", Some("administrativo"),
                    "A logística de eventos internos ganhou checklist único de credenciamento."),
                ("mtg-relatorio-semestral", "Relatório do primeiro semestre", "2026-07-16", None,
                    "O relatório do primeiro semestre consolida despesas por área e projeto."),
            ],
            false,
            "exact_number",
        ),
        follow_up(
            "pt-followup-edital-concurso",
            Language::Portuguese,
            "e quando sai o edital?",
            &[
                "User: teremos concurso interno este ano?",
                "Assistant: sim, o comitê confirmou o concurso interno de carreiras.",
            ],
            "data de publicação do edital do concurso interno",
            ("mtg-concurso-carreiras", "Concurso interno de carreiras", "2026-05-27", Some("pessoas")),
            vec![
                ev("f2-edital", "note", "O edital do concurso interno será publicado em 12 outubro 2026 no mural digital da empresa."),
                ev("f2-inscricoes", "summary", "Inscrições do concurso interno abrem um dia após a publicação e fecham em duas semanas."),
            ],
            &[],
            &["publicado em 12 outubro 2026"],
            &["publicação surpresa sem mural"],
            [
                ("mtg-carreiras-palestras", "Palestras sobre carreiras", "2026-06-18", Some("pessoas"),
                    "As palestras sobre carreiras internas acontecem nas quintas de almoço."),
                ("mtg-mural-digital", "Mural digital das unidades", "2026-07-21", None,
                    "O mural digital das unidades exibe avisos operacionais e aniversários."),
            ],
            false,
            "exact_date",
        ),
        follow_up(
            "pt-followup-parecer-auditoria",
            Language::Portuguese,
            "e quem assina o parecer final?",
            &[
                "User: como fica a auditoria externa deste ciclo?",
                "Assistant: a auditoria externa entra na semana 42 com amostra ampliada.",
            ],
            "responsável por assinar o parecer final da auditoria externa",
            ("mtg-auditoria-externa", "Auditoria externa — encerramento", "2026-06-02", Some("financeiro")),
            vec![
                ev("f3-parecer", "note", "O parecer final da auditoria externa será assinado pela coordenadora Helena Braga antes da assembleia."),
                ev("f3-prazos", "summary", "Amostra ampliada da auditoria cobre compras acima do teto usual e contratos recorrentes."),
            ],
            &[],
            &["assinado pela coordenadora Helena Braga"],
            &["assinatura coletiva sem responsável nomeado"],
            [
                ("mtg-amostragem-compras", "Regras de amostragem de compras", "2026-05-14", Some("financeiro"),
                    "A amostragem de compras prioriza fornecedores novos e valores atípicos."),
                ("mtg-assembleia-resultados", "Assembleia de resultados", "2026-07-11", None,
                    "A assembleia de resultados apresenta indicadores auditados aos conselheiros."),
            ],
            false,
            "exact_name",
        ),
        follow_up(
            "pt-followup-vagas-estagio",
            Language::Portuguese,
            "quantos entram na turma?",
            &[
                "User: teremos programa de estágio no verão?",
                "Assistant: sim, o verão terá trilha própria de estágio.",
            ],
            "número de vagas da turma de estágio de verão",
            ("mtg-estagio-verao", "Estágio de verão — turma", "2026-05-30", Some("pessoas")),
            vec![
                ev("f4-vagas", "note", "A turma de estágio de verão terá doze vagas distribuídas entre três áreas técnicas."),
                ev("f4-bolsa", "summary", "Bolsa do estágio acompanha a tabela vigente e inclui vale transporte integral."),
            ],
            &[],
            &["terá doze vagas distribuídas entre três áreas"],
            &["vagas ilimitadas conforme demanda"],
            [
                ("mtg-trilhas-tecnicas", "Trilhas técnicas de entrada", "2026-06-24", Some("ti"),
                    "As trilhas técnicas de entrada foram reorganizadas por nível de experiência."),
                ("mtg-tabela-bolsas", "Tabela de bolsas vigente", "2026-07-08", None,
                    "A tabela de bolsas vigente considera tempo de trajeto e regime de estudo."),
            ],
            false,
            "exact_number",
        ),
        follow_up(
            "pt-followup-comprovante-prazo",
            Language::Portuguese,
            "e qual a data limite dele?",
            &[
                "User: preciso atualizar meu cadastro residencial?",
                "Assistant: sim, o cadastro residencial passa por confirmação anual.",
            ],
            "prazo final para envio do comprovante de residência atualizado",
            ("mtg-cadastro-residencial", "Confirmação cadastral anual", "2026-06-20", Some("pessoas")),
            vec![
                ev("f5-prazo", "note", "Envio do comprovante de residência atualizado termina em 30 setembro 2026 pelo portal do colaborador."),
                ev("f5-canais", "summary", "Confirmação anual aceite contas de consumo digitais emitidas nos últimos noventa dias."),
            ],
            &[],
            &["termina em 30 setembro 2026"],
            &["aceitação de contas vencidas há anos"],
            [
                ("mtg-portal-colaborador", "Portal do colaborador — novidades", "2026-05-17", Some("pessoas"),
                    "O portal do colaborador estreou assinatura eletrônica de documentos internos."),
                ("mtg-contas-digitais", "Contas digitais aceites", "2026-07-03", None,
                    "Contas digitais de energia e água são aceites em processos administrativos."),
            ],
            false,
            "exact_date",
        ),
        follow_up(
            "pt-followup-visita-planta",
            Language::Portuguese,
            "e quem conduz a visita?",
            &[
                "User: a visita técnica da planta foi confirmada?",
                "Assistant: confirmada, com roteiro completo pelas linhas de produção.",
            ],
            "nome do especialista que conduz a visita técnica da planta",
            ("mtg-visita-planta", "Visita técnica à planta industrial", "2026-07-01", Some("operacoes")),
            vec![
                ev("f6-condutor", "note", "A visita técnica da planta será conduzida pelo especialista Marcos Tavares com apoio dos líderes de turno."),
                ev("f6-roteiro", "summary", "Roteiro cobre recebimento, linha de envase, laboratório e expedição com pausas técnicas."),
            ],
            &[],
            &["conduzida pelo especialista Marcos Tavares"],
            &["visita livre sem acompanhamento técnico"],
            [
                ("mtg-linhas-producao", "Linhas de produção — turnos", "2026-05-25", Some("operacoes"),
                    "As linhas de produção ganham quarto turno durante a alta temporada."),
                ("mtg-laboratorio-qualidade", "Laboratório de qualidade", "2026-06-29", None,
                    "O laboratório de qualidade ampliou testes de resistência de embalagens."),
            ],
            false,
            "exact_name",
        ),
        follow_up(
            "pt-followup-salas-congresso",
            Language::Portuguese,
            "e quantas salas ficaram reservadas?",
            &[
                "User: vamos levar estande ao congresso regional?",
                "Assistant: sim, presença confirmada com palestra principal.",
            ],
            "quantidade de salas reservadas para o congresso regional",
            ("mtg-congresso-regional", "Congresso regional — logística", "2026-06-06", Some("marketing")),
            vec![
                ev("f7-salas", "note", "Foram reservadas sete salas para o congresso regional no centro de convenções, além do estande principal."),
                ev("f7-palestra", "summary", "Palestra principal apresenta o caso de automação premiado no último ano."),
            ],
            &[],
            &["reservadas sete salas para o congresso regional"],
            &["reserva de auditório inteiro sem uso"],
            [
                ("mtg-estande-feiras", "Estandes em feiras", "2026-05-22", Some("marketing"),
                    "Estandes em feiras seguem guia visual unificado desde a rebranding."),
                ("mtg-centro-convencoes", "Centro de convenções — acessos", "2026-07-19", None,
                    "O centro de convenções libera acesso de montagem um dia antes de cada evento."),
            ],
            false,
            "exact_number",
        ),
        follow_up(
            "en-followup-community-budget",
            Language::English,
            "And the total budget figure?",
            &[
                "User: are we running the community program again?",
                "Assistant: yes, the community program returns with local chapters.",
            ],
            "total budget approved for the community program this year",
            ("mtg-community-program", "Community program — funding", "2026-06-15", Some("finance")),
            vec![
                ev("f8-budget", "note", "The community program budget approved for the year totals sixty thousand coins across local chapters."),
                ev("f8-chapters", "summary", "Local chapters submit quarterly spending reports with photos and attendance counts."),
            ],
            &[],
            &["totals sixty thousand coins across local chapters"],
            &["unlimited chapter budgets without reporting"],
            [
                ("mtg-local-chapters", "Local chapters kickoff", "2026-05-20", Some("operations"),
                    "The local chapters kickoff introduced the shared activity calendar."),
                ("mtg-spending-reports", "Quarterly spending reports", "2026-07-10", None,
                    "Quarterly spending reports feed the annual transparency page."),
            ],
            false,
            "exact_number",
        ),
        follow_up(
            "en-followup-workshop-registration",
            Language::English,
            "When does registration open?",
            &[
                "User: is the leadership workshop happening this fall?",
                "Assistant: yes, the leadership workshop runs two cohorts in the fall.",
            ],
            "registration opening date for the leadership workshop cohorts",
            ("mtg-leadership-workshop", "Leadership workshop — logistics", "2026-05-29", Some("people")),
            vec![
                ev("f9-registration", "note", "Registration for the leadership workshop opens on 14 September 2026 through the learning hub."),
                ev("f9-cohorts", "summary", "Two cohorts run back to back with capped seats and waitlist automation."),
            ],
            &[],
            &["opens on 14 September 2026 through the learning hub"],
            &["walk-in seats without registration"],
            [
                ("mtg-learning-hub", "Learning hub refresh", "2026-06-23", Some("people"),
                    "The learning hub refresh added offline downloads for travel weeks."),
                ("mtg-waitlist-automation", "Waitlist automation rules", "2026-07-15", None,
                    "Waitlist automation promotes the oldest confirmed-interest entries first."),
            ],
            false,
            "exact_date",
        ),
        follow_up(
            "en-followup-audit-partner",
            Language::English,
            "And who leads the briefing?",
            &[
                "User: what about the external audit this year?",
                "Assistant: the external audit starts week 42 with an expanded sample.",
            ],
            "name of the partner leading the external audit briefing",
            ("mtg-external-audit", "External audit — kickoff", "2026-06-04", Some("finance")),
            vec![
                ev("f10-partner", "note", "Partner Alice Nery leads the audit briefing scheduled for the board week."),
                ev("f10-sample", "summary", "Expanded sample covers recurring contracts and purchases above the usual threshold."),
            ],
            &[],
            &["Partner Alice Nery leads the audit briefing"],
            &["anonymous junior staff running the briefing alone"],
            [
                ("mtg-board-week", "Board week agenda", "2026-05-16", Some("board"),
                    "The board week agenda packs committee reviews before the plenary."),
                ("mtg-threshold-review", "Threshold review cycle", "2026-07-08", None,
                    "The threshold review cycle adjusts purchase limits twice a year."),
            ],
            false,
            "exact_name",
        ),
        follow_up(
            "en-followup-license-count",
            Language::English,
            "How many licenses came back?",
            &[
                "User: did we finish the tool consolidation?",
                "Assistant: yes, the tool consolidation wrapped up last sprint.",
            ],
            "number of licenses released after the tool consolidation sprint",
            ("mtg-tool-consolidation", "Tool consolidation — results", "2026-06-27", Some("it")),
            vec![
                ev("f11-licenses", "note", "Tool consolidation released thirty-one licenses back to the shared pool for reuse."),
                ev("f11-savings", "summary", "Consolidation retired duplicate trackers and unified access reviews into one flow."),
            ],
            &[],
            &["released thirty-one licenses back to the shared pool"],
            &["permanent deletion of every legacy tool"],
            [
                ("mtg-access-reviews", "Access review cadence", "2026-05-23", Some("security"),
                    "The access review cadence moved to quarterly with owner attestations."),
                ("mtg-shared-pool", "Shared license pool rules", "2026-07-22", None,
                    "The shared license pool prioritizes onboarding classes during peak months."),
            ],
            false,
            "exact_number",
        ),
        follow_up(
            "en-followup-expense-cutoff",
            Language::English,
            "What's the cutoff then?",
            &[
                "User: are June expense claims handled normally?",
                "Assistant: yes, June expense claims follow the standard flow.",
            ],
            "cutoff date for submitting expense claims from June",
            ("mtg-expense-claims", "Expense claims — June window", "2026-07-02", Some("finance")),
            vec![
                ev("f12-cutoff", "note", "Expense claims from June must be submitted by noon on 5 July 2026 to join the monthly batch."),
                ev("f12-flow", "summary", "Standard flow pairs receipts automatically with card statements before approval."),
            ],
            &[],
            &["submitted by noon on 5 July 2026"],
            &["late claims accepted without exception notes"],
            [
                ("mtg-card-statements", "Card statement pairing", "2026-05-31", Some("finance"),
                    "Card statement pairing flags mismatches for reviewer attention early."),
                ("mtg-monthly-batches", "Monthly payment batches", "2026-06-18", None,
                    "Monthly payment batches release on the tenth working day consistently."),
            ],
            false,
            "exact_date",
        ),
        follow_up(
            "en-followup-vendor-owner",
            Language::English,
            "Who owns that review now?",
            &[
                "User: is the vendor review still quarterly?",
                "Assistant: yes, the vendor review stays quarterly with scorecards.",
            ],
            "person accountable for the quarterly vendor review scorecards",
            ("mtg-quarterly-vendor-review", "Quarterly vendor review", "2026-06-11", Some("operations")),
            vec![
                ev("f13-owner", "note", "The quarterly vendor review is owned by operations manager Bruno Costa with procurement support."),
                ev("f13-scorecards", "summary", "Scorecards weigh delivery reliability, incident history, and pricing stability equally."),
            ],
            &[],
            &["owned by operations manager Bruno Costa"],
            &["ownership rotating without a named accountable person"],
            [
                ("mtg-procurement-support", "Procurement support desk", "2026-05-27", Some("operations"),
                    "The procurement support desk answers contract questions within two days."),
                ("mtg-scorecard-weights", "Scorecard weighting tweaks", "2026-07-17", None,
                    "Scorecard weighting tweaks require sign-off from both category leads."),
            ],
            false,
            "exact_name",
        ),
        follow_up(
            "en-followup-annex-desks",
            Language::English,
            "And how many desks fit?",
            &[
                "User: is the annex floor opening soon?",
                "Assistant: yes, the annex floor opens after the safety inspection.",
            ],
            "desk capacity of the new annex floor after inspection",
            ("mtg-annex-floor", "Annex floor — capacity plan", "2026-06-30", Some("facilities")),
            vec![
                ev("f14-desks", "note", "The new annex floor holds forty-eight desks with bookable monitors at every seat."),
                ev("f14-zones", "summary", "Zones split into quiet focus rows, collaboration pods, and a phone booth cluster."),
            ],
            &[],
            &["holds forty-eight desks with bookable monitors"],
            &["hot-desking chaos without zone labels"],
            [
                ("mtg-safety-inspection", "Safety inspection checklist", "2026-05-19", Some("facilities"),
                    "The safety inspection checklist covers exits, sensors, and load limits."),
                ("mtg-phone-booths", "Phone booth placement", "2026-07-13", None,
                    "Phone booths sit away from quiet rows to keep noise contained."),
            ],
            false,
            "exact_number",
        ),
        follow_up(
            "en-followup-staging-build",
            Language::English,
            "Which build goes out then?",
            &[
                "User: does staging get an update this sprint?",
                "Assistant: yes, staging receives the sprint candidate build.",
            ],
            "build number promoted to the staging environment this sprint",
            ("mtg-staging-promotion", "Staging promotion — sprint candidate", "2026-07-06", Some("engineering")),
            vec![
                ev("f15-build", "note", "Build 2026.14 ships to staging after regression sign-off this sprint."),
                ev("f15-regression", "summary", "Regression suite covers checkout flows, notifications, and the migration dry run."),
            ],
            &[],
            &["Build 2026.14 ships to staging after regression sign-off"],
            &["direct production push skipping staging entirely"],
            [
                ("mtg-regression-suite", "Regression suite ownership", "2026-06-12", Some("quality"),
                    "Regression suite ownership rotates between platform and product squads."),
                ("mtg-migration-dry-run", "Migration dry run findings", "2026-07-20", None,
                    "Migration dry run findings fed two schema fixes into the same build."),
            ],
            false,
            "exact_number",
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn follow_up(
    id: &str,
    language: Language,
    question: &str,
    history: &[&str],
    rewritten: &str,
    target: (&str, &str, &str, Option<&str>),
    evidence: Vec<Evidence>,
    order: &[(&str, &str)],
    facts: &[&str],
    forbidden: &[&str],
    neighbours: [(&str, &str, &str, Option<&str>, &str); 2],
    critical: bool,
    subtype: &str,
) -> EvaluationCase {
    let target_meeting = mtg(
        target.0,
        target.1,
        target.2,
        target.3,
        MeetingState::Current,
        evidence,
    );
    let neighbour_meetings = neighbours
        .iter()
        .map(|(nid, ntitle, ndate, nfolder, ntext)| dm(nid, ntitle, ndate, *nfolder, ntext))
        .collect::<Vec<_>>();
    let kind = if id.rsplit('-').next().unwrap_or("").len() % 2 == 0 {
        ScopeKind::Today
    } else {
        ScopeKind::Snapshot
    };
    let allowed = [target.0, neighbours[0].0, neighbours[1].0];
    let scope = match kind {
        ScopeKind::Snapshot => scope(ScopeKind::Snapshot, None, None, &allowed),
        _ => scope(ScopeKind::Today, None, None, &allowed),
    };
    let first_required = target_meeting.evidence[0].id.clone();
    case(
        id,
        language,
        question,
        history,
        Some(rewritten),
        "follow_up_exact",
        &[
            "exact_term",
            "follow_up_rewrite",
            "number_date_name",
            subtype,
        ],
        critical,
        scope,
        std::iter::once(target_meeting)
            .chain(neighbour_meetings)
            .collect(),
        &[target.0],
        order,
        &[first_required.as_str()],
        facts,
        forbidden,
    )
}
