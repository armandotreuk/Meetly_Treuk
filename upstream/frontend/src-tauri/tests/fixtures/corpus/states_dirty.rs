// Dirty-state family: an in-edit meeting stays lexically available while its
// authoritative text has moved on, pinning the dirty fallback behaviour.

use super::super::EvaluationCase;
use super::{case, dm, ev, mtg, scope, Language, MeetingState, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    let topics: [(&str, &str, &str, &str, &str, &str, &str); 15] = [
        (
            "pt-dirty-agenda-conselho", "agenda do conselho de março",
            "Conselho de março — pauta", "2026-06-05",
            "A agenda do conselho de março traz orçamento, promoções e revisão de estatuto na ordem do dia.",
            "Rascunho em edição da agenda do conselho ainda lista item antigo de crédito coletivo.",
            "item antigo de crédito coletivo na pauta final",
        ),
        (
            "pt-dirty-venda-ferias", "regras de venda de férias",
            "Regras de férias — ajuste", "2026-07-02",
            "Venda de férias permite até dez dias úteis com aprovação da liderança e saldo mínimo remanescente.",
            "Versão em edição mantém o teto antigo de cinco dias úteis para a venda de férias.",
            "teto antigo de cinco dias úteis vigente",
        ),
        (
            "pt-dirty-conta-ativa", "definição de conta ativa no glossário",
            "Glossário do produto — sessão", "2026-05-23",
            "Conta ativa é aquela com login autenticado nos últimos trinta dias corridos.",
            "Glossário do produto em revisão ainda define conta ativa por sessão semanal.",
            "conta ativa por sessão semanal ainda publicada",
        ),
        (
            "pt-dirty-rota-zona-sul", "rota de entregas da zona sul",
            "Rotas da zona sul — planejamento", "2026-06-30",
            "A rota de entregas da zona sul passa a sair às seis com dois veículos elétricos fixos.",
            "Planejamento em edição da rota sul ainda mostra três veículos a combustão.",
            "três veículos a combustão no desenho atual",
        ),
        (
            "pt-dirty-desconto-volume", "desconto por volume de licenças",
            "Tabela de preços — revisão", "2026-07-18",
            "Desconto de volume vale para pedidos acima de cinquenta licenças com contrato anual.",
            "Tabela de preços em edição conserva faixa antiga a partir de cem licenças contratadas.",
            "faixa antiga a partir de cem licenças",
        ),
        (
            "pt-dirty-abertura-loja", "checklist de abertura de loja",
            "Abertura de lojas — padronização", "2026-05-09",
            "Checklist de abertura de loja inclui teste de alarme, conferência de caixa e foto da vitrine.",
            "Checklist de abertura em edição ainda pede relógio de ponto manual na lista.",
            "relógio de ponto manual permanece listado",
        ),
        (
            "pt-dirty-prazo-estorno", "prazo de estorno no cartão",
            "Prazos de estorno — conferência", "2026-06-14",
            "Estorno em cartão conclui em até cinco dias úteis após a confirmação interna do pedido.",
            "Documento de estornos em edição repete prazo antigo de dez dias úteis.",
            "prazo antigo de dez dias úteis repetido",
        ),
        (
            "pt-dirty-trilha-dados", "trilha de dados para analistas",
            "Trilhas de dados — mapeamento", "2026-07-25",
            "Trilha de dados para analistas cobre SQL básico, modelagem e leitura de painéis.",
            "Mapeamento em edição das trilhas ainda exclui o módulo de modelagem.",
            "módulo de modelagem fora do mapa atual",
        ),
        (
            "en-dirty-holiday-calendar", "holiday calendar regional holidays",
            "Holiday planning sync", "2026-06-08",
            "The holiday calendar adds two regional holidays to the operations planning sheet this year.",
            "Draft copy of the holiday calendar still omits both new regional holidays.",
            "both new regional holidays missing from draft",
        ),
        (
            "en-dirty-badge-photo", "badge photo renewal interval",
            "Badge renewal logistics", "2026-07-11",
            "Badge photos renew every two years during the scheduled lobby photo week.",
            "Working draft of badge rules still says photos renew every five years instead.",
            "photos renew every five years in draft",
        ),
        (
            "en-dirty-snack-budget", "snack budget per head weekly",
            "Snack budget review", "2026-05-16",
            "The snack budget holds steady at six coins per head each week through winter.",
            "Unpublished budget notes still carry the old four coin per head figure.",
            "old four coin per head figure persists",
        ),
        (
            "en-dirty-deskpool-release", "hot desk return time policy",
            "Hot desk flow tuning", "2026-06-27",
            "Hot desks return to the bookable pool automatically at the end-of-day slot.",
            "Draft flow chart still releases desks at noon rather than the end of day.",
            "desks release at noon in draft chart",
        ),
        (
            "en-dirty-referral-bonus", "referral bonus payment timing",
            "Referral rules refresh", "2026-07-30",
            "Referral bonus pays after the referred colleague completes probation successfully.",
            "Draft referral page still promises payout on the first signed contract day.",
            "payout promised on first signed contract day",
        ),
        (
            "en-dirty-cycle-count-day", "inventory count day scheduling",
            "Cycle count scheduling", "2026-06-19",
            "Store inventory counts move to the second Tuesday with cycle sheets printed overnight.",
            "Editable count day schedule still lists counts on the last Friday like last year.",
            "counts listed on last Friday still",
        ),
        (
            "en-dirty-support-banner", "support hours weekend banner text",
            "Support banner copy", "2026-05-31",
            "The support hours banner now reads weekends from nine to one with chat priority on.",
            "Banner draft still shows the retired closed-on-weekends message verbatim.",
            "retired closed-on-weekends message shown verbatim",
        ),
    ];
    topics
        .iter()
        .enumerate()
        .map(|(index, topic)| {
            let (id, question, tm_title, tm_date, answer_text, dirty_indexed, forbidden) = *topic;
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
                dirty_indexed,
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
    candidate_indexed: &str,
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
        &format!("mtg-{id}-edicao"),
        &format!("Em edição — rascunho {index}"),
        "2026-03-19",
        Some("records"),
        MeetingState::Dirty,
        vec![ev(&format!("{id}-draft-ev"), "note", candidate_indexed)],
    );
    let d1 = dm(
        &format!("mtg-{id}-vizinho"),
        &format!("Briefing vizinho {index}"),
        "2026-04-16",
        Some("records"),
        "Briefing de rotina sobre arquivamento, etiquetas e prazos de guarda.",
    );
    let d2 = dm(
        &format!("mtg-{id}-acervo"),
        &format!("Notas de acervo {index}"),
        "2026-02-20",
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
        &["exact_term", "number_date_name", subtype, "state_dirty"],
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
