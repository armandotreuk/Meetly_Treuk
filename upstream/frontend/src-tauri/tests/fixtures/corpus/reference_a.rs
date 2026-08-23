// Reference family part A: the pinned WhatsApp case and the superseded-draft
// dunning case. Both reproduce evidence-completeness failures under FTS.

use super::super::EvaluationCase;
use super::{case, dm, ev, mtg, Language, MeetingState, ScopeKind, REFERENCE_CATEGORY};

pub(super) fn cases() -> Vec<EvaluationCase> {
    vec![whatsapp_retention(), cobranca_regua()]
}

// Critical 1: pinned evidence-completeness failure. The topical summary keeps
// the meeting findable while bounded fragment retrieval returns neither the
// final schedule nor the day-one distinction; superseded and partial neighbour
// cadences supply the misleading 3/4-day fragments.
fn whatsapp_retention() -> EvaluationCase {
    case(
        "fixture-whatsapp-retention",
        Language::Portuguese,
        "quais os dias de comunicacao por whatsapp para o fluxo de retencao?",
        &[],
        None,
        "reference_schedule",
        &[REFERENCE_CATEGORY],
        true,
        super::scope(
            ScopeKind::All,
            None,
            None,
            &[
                "mtg-whatsapp-retention",
                "mtg-onboarding-ativacao",
                "mtg-reativacao-fria",
                "mtg-cobranca-avisos",
                "mtg-acompanhamento-contas",
                "mtg-suporte-fila",
                "mtg-pesquisa-disparos",
                "mtg-retensao-piloto",
                "mtg-renovacoes-ciclo",
                "mtg-webinar-convites",
                "mtg-parcerias-alinhamento",
            ],
        ),
        vec![
            mtg(
                "mtg-whatsapp-retention",
                "Programa de retenção — comunicação por WhatsApp",
                "2026-07-14",
                Some("sucesso-cliente"),
                MeetingState::Current,
                vec![
                    ev("ref-visao", "summary", "Alinhamento do programa de retenção: definimos a régua de comunicação por WhatsApp, os responsáveis por cada toque e o gancho de reengajamento."),
                    ev("ref-regua-final", "note", "Minuta final da régua aprovada na revisão de cadência: contatos nos dias 1, 3, 7, 10 e 15 após o cadastro, sem novos canais nesta fase do programa."),
                    ev("ref-dia-um", "note", "No dia um, unidades MPV enviam boas-vindas; unidades não MPV iniciam confirmação cadastral pelo canal padrão definido pelo comitê."),
                    ev("ref-supressao", "transcript", "Na gravação ficou acordado que contas com risco alto de rebote ficam fora da trilha de e-mail até a próxima revisão de supressão."),
                ],
            ),
            dm("mtg-onboarding-ativacao", "Onboarding digital — sequência de ativação", "2026-06-02", Some("crescimento"),
                "A sequência de ativação prevê um toque após 3 dias de conta criada e reforço depois de 4 dias sem login."),
            dm("mtg-reativacao-fria", "Campanha de reativação — segmento frio", "2026-05-28", Some("crescimento"),
                "O teste da campanha compara janelas de 3 dias e de 7 dias entre disparos."),
            dm("mtg-cobranca-avisos", "Comunicação de cobrança — régua de avisos", "2026-06-19", Some("financeiro"),
                "A régua de avisos envia lembrete por WhatsApp 2 dias antes do vencimento."),
            dm("mtg-acompanhamento-contas", "Sucesso do cliente — rituais de acompanhamento", "2026-07-01", Some("sucesso-cliente"),
                "Contas estratégicas recebem revisão a cada 15 dias e chamada mensal de rotina."),
            dm("mtg-suporte-fila", "Fluxo de suporte — fila e prioridades", "2026-06-25", Some("operacoes"),
                "O fluxo de suporte responde chamados normais em até 3 dias úteis."),
            dm("mtg-pesquisa-disparos", "Pesquisa de satisfação — disparos", "2026-07-08", Some("experiencia"),
                "Disparos da pesquisa saem 4 dias após a reunião de kickoff com o time."),
            dm("mtg-retensao-piloto", "Retenção antecipada — piloto de ofertas", "2026-05-12", Some("crescimento"),
                "O piloto testa gatilhos de retenção antes do sexagésimo dia de contrato."),
            dm("mtg-renovacoes-ciclo", "Renovações — avisos do ciclo", "2026-06-11", Some("sucesso-cliente"),
                "O aviso de renovação sai 10 dias antes do fim do ciclo vigente."),
            dm("mtg-webinar-convites", "Webinar de produto — convites", "2026-04-30", Some("marketing"),
                "Os convites do webinar saem 3 dias antes da sessão ao vivo."),
            dm("mtg-parcerias-alinhamento", "Parcerias — integrações e alinhamento", "2026-07-03", None,
                "Reuniões de alinhamento com parceiros ocorrem a cada 4 dias úteis durante a integração."),
        ],
        &["mtg-whatsapp-retention"],
        &[],
        &["ref-regua-final", "ref-dia-um"],
        &["dias 1, 3, 7, 10 e 15", "unidades MPV enviam boas-vindas"],
        &["apenas 3 dias", "apenas 4 dias"],
    )
}

// Critical 2: evidence completeness against a superseded draft. The approved
// sequence (2, 8, 20, 35) and the SMS step lose the top-10 to the discarded
// draft (5 and 15) plus finance-cadence neighbours.
fn cobranca_regua() -> EvaluationCase {
    case(
        "pt-ref-cobranca-regua",
        Language::Portuguese,
        "qual a régua de cobrança para faturas vencidas?",
        &[],
        None,
        "reference_schedule",
        &[REFERENCE_CATEGORY],
        true,
        super::scope(
            ScopeKind::Folder,
            Some("financeiro"),
            None,
            &[
                "mtg-cobranca-vencidas",
                "mtg-aging-receber",
                "mtg-negociacao-dividas",
                "mtg-boletos-baixa",
                "mtg-credito-limites",
                "mtg-juros-mora",
                "mtg-recuperacao-creditos",
                "mtg-descontos-pontualidade",
                "mtg-renegociacao-acordos",
            ],
        ),
        vec![
            mtg(
                "mtg-cobranca-vencidas",
                "Régua de cobrança — faturas vencidas",
                "2026-06-18",
                Some("financeiro"),
                MeetingState::Current,
                vec![
                    ev("cobr-ancora", "summary", "A régua de cobrança para faturas vencidas foi fechada na reunião financeira com etapas, canais e responsáveis definidos."),
                    ev("cobr-final", "note", "Sequência final aprovada para a régua de cobrança: lembretes nos dias 2, 8, 20 e 35 depois do vencimento em aberto, com revisão mensal dos textos usados."),
                    ev("cobr-rascunho", "note", "Rascunho descartado da régua previa cobrança nos dias 5 e 15; foi substituído pela sequência final após teste com carteira pequena."),
                    ev("cobr-canais", "note", "Canais por etapa do calendário de avisos: contato inicial via SMS e negociação somente ao telefone com o time de cartões."),
                ],
            ),
            dm("mtg-aging-receber", "Contas a receber — relatório de aging", "2026-06-30", Some("financeiro"),
                "O relatório de aging mostra faturas em atraso há mais de 60 dias."),
            dm("mtg-negociacao-dividas", "Negociação de dívidas — política", "2026-05-22", Some("financeiro"),
                "A política de negociação permite parcelar débitos de cobrança com mais de 30 dias."),
            dm("mtg-boletos-baixa", "Boletos — baixa automática", "2026-07-02", Some("financeiro"),
                "A baixa automática de boletos ocorre 2 dias após o pagamento confirmado."),
            dm("mtg-credito-limites", "Política de crédito e limites", "2026-06-12", Some("financeiro"),
                "A revisão de limites usa o histórico de cobrança dos últimos 90 dias."),
            dm("mtg-juros-mora", "Juros e multa — aplicação", "2026-05-30", Some("financeiro"),
                "Juros de mora correm sobre faturas não pagas a partir do quinto dia."),
            dm("mtg-recuperacao-creditos", "Recuperação de créditos", "2026-04-25", Some("financeiro"),
                "Créditos recuperados voltam ao relatório consolidado depois de 45 dias."),
            dm("mtg-impostos-guias", "Impostos — guias mensais", "2026-06-08", None,
                "As guias de impostos vencem 10 dias antes do fim do período de apuração."),
            dm("mtg-descontos-pontualidade", "Descontos por pontualidade", "2026-07-06", Some("financeiro"),
                "Desconto por pagamento adiantado de faturas vale dois por cento e não se acumula entre ciclos."),
            dm("mtg-renegociacao-acordos", "Renegociação de acordos", "2026-04-21", Some("financeiro"),
                "Acordos de renegociação priorizam os maiores valores em cobrança há mais tempo em aberto."),
        ],
        &["mtg-cobranca-vencidas"],
        &[],
        &["cobr-final", "cobr-canais"],
        &["dias 2, 8, 20 e 35", "contato inicial via SMS"],
        &["dias 5 e 15"],
    )
}
