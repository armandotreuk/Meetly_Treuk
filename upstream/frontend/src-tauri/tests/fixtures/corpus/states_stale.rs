// Stale-derived family: a derived summary still projects the retired value
// with strong query overlap, so the baseline retrieves and contaminates while
// the current answer stays reachable.

use super::super::EvaluationCase;
use super::{case, dm, ev, mtg, scope, Language, MeetingState, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    let topics: [(&str, &str, &str, &str, &str, &str, &str); 15] = [
        (
            "pt-stale-politica-almoco", "política de horário de almoço",
            "Política de pessoas — pausas", "2026-06-26",
            "A política de horário de almoço fixa pausa de sessenta minutos entre doze e catorze.",
            "Resumo derivado desatualizado da política de horário de almoço ainda indica pausa de trinta minutos.",
            "pausa de trinta minutos",
        ),
        (
            "pt-stale-meta-vendas", "meta trimestral de vendas",
            "Metas comerciais — trimestre", "2026-07-04",
            "A meta trimestral de vendas sobe para quatrocentas unidades por carteira ativa.",
            "Painel derivado antigo da meta de vendas ainda exibe trezentas unidades.",
            "exibe trezentas unidades",
        ),
        (
            "pt-stale-feriado-municipal", "fechamento no feriado municipal",
            "Calendário operacional — feriados", "2026-05-28",
            "Fechamento no feriado municipal mantém só o plantão remoto de emergências acionável por telefone.",
            "Versão derivada velha do fechamento no feriado ainda promete atendimento presencial.",
            "promete atendimento presencial",
        ),
        (
            "pt-stale-limite-reembolso", "limite mensal de reembolso",
            "Reembolsos — teto vigente", "2026-06-13",
            "O limite mensal de reembolso sobe para duzentas moedas com notas digitais obrigatórias.",
            "Extrato derivado desatualizado do limite de reembolso segue mostrando cento e cinquenta moedas.",
            "cento e cinquenta moedas",
        ),
        (
            "pt-stale-versao-app", "versão mínima do aplicativo",
            "App — política de versões", "2026-07-16",
            "A versão mínima do aplicativo passa a ser sete ponto dois com aviso na tela inicial.",
            "Nota derivada antiga da versão do aplicativo ainda cita seis ponto cinco.",
            "ainda cita seis ponto cinco",
        ),
        (
            "pt-stale-sala-reserva", "reserva da sala de conselho",
            "Salas — regras de reserva", "2026-05-17",
            "Reserva da sala de conselho exige aprovação da diretoria e libera com quarenta e oito horas.",
            "Guia derivado velho da reserva da sala de conselho ainda aponta confirmação automática.",
            "aponta confirmação automática",
        ),
        (
            "pt-stale-desconto-aniversario", "desconto de aniversário na loja",
            "Loja — campanhas internas", "2026-06-08",
            "Desconto de aniversário na loja vinte por cento vale também para itens de promoção cruzada.",
            "Folha derivada desatualizada do desconto de aniversário na loja ainda diz dez por cento.",
            "ainda diz dez por cento",
        ),
        (
            "pt-stale-jornada-remota", "jornada no regime remoto",
            "Regimes — registro de jornada", "2026-07-29",
            "Jornada no regime remoto registra marcações por confiança com auditoria amostral mensal.",
            "Manual derivado antigo da jornada no regime remoto ainda exige ponto digital às nove.",
            "exige ponto digital às nove",
        ),
        (
            "en-stale-password-expiry", "password expiry window days",
            "Credentials — expiry policy", "2026-06-22",
            "The password expiry window extends to one hundred eighty days for standard accounts.",
            "Stale derived summary of the password expiry window still shows ninety days everywhere.",
            "still shows ninety days",
        ),
        (
            "en-stale-office-capacity", "office capacity per floor limit",
            "Facilities — capacity plan", "2026-07-09",
            "Office capacity per floor rises to ninety seats after the annex inspection clears.",
            "Old derived dashboard for office capacity per floor still lists seventy seats.",
            "still lists seventy seats",
        ),
        (
            "en-stale-shipping-cutoff", "shipping cutoff time orders",
            "Fulfilment — daily cutoff", "2026-06-03",
            "The shipping cutoff time moves to four in the afternoon for same-day courier orders.",
            "Derived note for the shipping cutoff time still states two in the afternoon.",
            "still states two in the afternoon",
        ),
        (
            "en-stale-training-allowance", "training allowance yearly amount",
            "Learning — allowance policy", "2026-07-24",
            "The training allowance yearly amount doubles to eight hundred coins from January.",
            "Cached derived page for the training allowance yearly amount still reads four hundred coins.",
            "still reads four hundred coins",
        ),
        (
            "en-stale-meeting-length", "meeting length default calendar",
            "Calendar — default durations", "2026-05-25",
            "Meeting length default drops to twenty-five minutes leaving recovery gaps between calls.",
            "Derived settings snapshot for meeting length default still shows fifty minutes.",
            "still shows fifty minutes",
        ),
        (
            "en-stale-invoice-discount", "early invoice discount percent",
            "Billing — early payment terms", "2026-06-30",
            "Early invoice discount settles at three percent for payments inside ten days.",
            "Superseded derived terms for early invoice discount still advertise five percent.",
            "still advertise five percent",
        ),
        (
            "en-stale-badge-access-doors", "badge access doors after hours",
            "Security — door access matrix", "2026-07-31",
            "Badge access doors after hours now require the paired mobile confirmation step.",
            "Legacy derived matrix for badge access doors after hours still allows single tap entry.",
            "allows single tap entry",
        ),
    ];
    topics
        .iter()
        .enumerate()
        .map(|(index, topic)| {
            let (id, question, tm_title, tm_date, answer_text, stale_indexed, forbidden) = *topic;
            let language = if index < 8 {
                Language::Portuguese
            } else {
                Language::English
            };
            state_case(
                id,
                language,
                question,
                tm_title,
                tm_date,
                answer_text,
                stale_indexed,
                forbidden,
                index,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn state_case(
    id: &str,
    language: Language,
    question: &str,
    target_title: &str,
    target_date: &str,
    answer_text: &str,
    stale_indexed: &str,
    forbidden: &str,
    index: usize,
) -> EvaluationCase {
    let evidence_id = format!("{id}-answer");
    let summary_id = format!("{id}-context");
    let target_meeting = mtg(
        &format!("mtg-{id}"),
        target_title,
        target_date,
        Some("records"),
        MeetingState::Current,
        vec![
            ev(
                &evidence_id,
                match index % 3 {
                    0 => "summary",
                    1 => "note",
                    _ => "transcript",
                },
                answer_text,
            ),
            ev(
                &summary_id,
                "summary",
                "Sessão registrou a decisão vigente com responsável e data da próxima revisão.",
            ),
        ],
    );
    let candidate = mtg(
        &format!("mtg-{id}-derivado"),
        &format!("Derivado desatualizado {index}"),
        "2026-03-02",
        Some("records"),
        MeetingState::StaleDerived,
        vec![super::stale_ev(
            &format!("{id}-stale-ev"),
            "summary",
            stale_indexed,
            "Sumário regenerado removeu a alegação antiga herdada do ciclo anterior de revisão.",
        )],
    );
    let d1 = dm(
        &format!("mtg-{id}-vizinho"),
        &format!("Briefing vizinho {index}"),
        "2026-04-11",
        Some("records"),
        "Briefing de rotina sobre arquivamento, etiquetas e prazos de guarda.",
    );
    let d2 = dm(
        &format!("mtg-{id}-acervo"),
        &format!("Notas de acervo {index}"),
        "2026-02-14",
        None,
        "Notas de acervo descrevem caixas, inventário e formulários de consulta.",
    );
    let allowed_base = [
        target_meeting.id.as_str(),
        candidate.id.as_str(),
        d1.id.as_str(),
        d2.id.as_str(),
    ];
    let kind_scope = match index % 4 {
        0 => scope(ScopeKind::All, None, None, &allowed_base),
        1 => scope(ScopeKind::Folder, Some("records"), None, &allowed_base),
        2 => scope(ScopeKind::Snapshot, None, None, &allowed_base),
        _ => scope(ScopeKind::Today, None, None, &allowed_base),
    };
    let subtype = match index % 3 {
        0 => "exact_number",
        1 => "exact_date",
        _ => "exact_name",
    };
    case(
        id,
        language,
        question,
        &[],
        None,
        "exact_lookup",
        &[
            "exact_term",
            "number_date_name",
            subtype,
            "state_stale_derived",
        ],
        false,
        kind_scope,
        vec![target_meeting, candidate, d1, d2],
        &[&format!("mtg-{id}")],
        &[],
        &[evidence_id.as_str()],
        &[short_fact(answer_text).as_str()],
        &[forbidden],
    )
}

fn short_fact(answer: &str) -> String {
    let words = answer.split_whitespace().collect::<Vec<_>>();
    let take = words.len().min(8);
    words[..take].join(" ").trim_end_matches(',').to_string()
}
