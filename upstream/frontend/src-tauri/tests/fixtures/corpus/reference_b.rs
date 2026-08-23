// Reference family part B: critical cases 3–5, each with a named failure mode
// (terminological gap, stale-version contamination, cross-section join).

use super::super::EvaluationCase;
use super::{dm, ev, mtg, sibling_case, stale_ev, MeetingState, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    vec![chaves_acesso(), sla_suporte(), nps_detrator()]
}

// Critical 3: terminological gap. The decision speaks of rotating credentials
// quarterly while the question says trocar/chaves; the inventory and access
// neighbours own the surface vocabulary and push the target below rank four.
fn chaves_acesso() -> EvaluationCase {
    sibling_case(
        "pt-ref-chaves-acesso",
        "quando trocar as chaves de acesso dos ambientes?",
        "Segurança de acesso — políticas de credenciais",
        "2026-07-21",
        Some("plataforma"),
        ScopeKind::All,
        "",
        vec![
            ev("chv-discussao", "transcript", "Nesta conversa a equipe revisou a política de credenciais e decidiu formalizar o calendário de rotação periódica nos ambientes servidos pela área de plataforma."),
            ev("chv-ciclo", "note", "Registro oficial da decisão: credenciais rotacionam a cada trimestre corrido em todos os ambientes, com peça escrita anexada ao inventário."),
            ev("chv-rascunho", "note", "Proposta antiga sugeria renovação mensal das credenciais; descartada por custo operacional elevado para times pequenos."),
        ],
        &["chv-discussao", "chv-ciclo"],
        &["política de credenciais", "rotacionam a cada trimestre corrido"],
        &["renovação mensal"],
        vec![
            dm("mtg-inventario-chaves", "Inventário de chaves de acesso", "2026-07-09", Some("plataforma"),
                "O inventário de chaves de acesso lista credenciais de todos os ambientes e seus responsáveis nomeados."),
            dm("mtg-revisao-acessos", "Revisão de acessos dos ambientes", "2026-06-15", Some("plataforma"),
                "A revisão de acessos dos ambientes de homologação e dos ambientes de produção acontece trimestralmente com relatório assinado."),
            dm("mtg-acesso-administrativo", "Acesso administrativo — bastião", "2026-04-14", Some("plataforma"),
                "O acesso administrativo aos ambientes internos passa pelo bastião com auditoria ligada."),
            dm("mtg-vpn-senhas", "VPN corporativa e senhas", "2026-06-08", Some("seguranca"),
                "O acesso remoto por VPN exige senha forte com renovação semestral obrigatória."),
            dm("mtg-certificados-tls", "Certificados TLS dos domínios", "2026-05-19", Some("seguranca"),
                "A renovação de certificados dos domínios públicos acontece antes do prazo apontado pelo monitor."),
        ],
        true,
    )
}

// Critical 4: stale-version contamination. The renewed agreement talks about
// faster turnaround without repeating the question nouns; the stale derived
// summary of the legacy contract keeps circulating with the old figure.
fn sla_suporte() -> EvaluationCase {
    sibling_case(
        "pt-ref-sla-suporte",
        "qual o prazo de primeira resposta acordado com os clientes?",
        "SLA de suporte — contrato vigente",
        "2026-07-06",
        Some("operacoes"),
        ScopeKind::Folder,
        "operacoes",
        vec![
            ev("sla-atual", "summary", "Renovação do acordo de nível fechou tempos de retorno ao usuário bem mais curtos no plano avançado, conforme texto aprovado com o jurídico."),
            ev("sla-antigo", "note", "Texto anterior do compromisso previa devolução de contato em um dia inteiro; caiu fora na revisão deste ano."),
        ],
        &["sla-atual"],
        &["tempos de retorno ao usuário bem mais curtos"],
        &["em um dia inteiro"],
        vec![
            dm("mtg-prazos-resposta", "Prazos de resposta por canal", "2026-06-20", Some("operacoes"),
                "Todo prazo de atendimento ao cliente consta da tabela publicada no portal de ajuda."),
            dm("mtg-primeiras-mensagens", "Primeira mensagem em filas triadas", "2026-05-11", Some("operacoes"),
                "A primeira mensagem da fila sai após triagem automática de severidade do cliente."),
            dm("mtg-acordos-nivel", "Acordos de nível revisitados", "2026-07-13", Some("operacoes"),
                "Acordos de nível foram revisitados com clientes maiores durante a feira anual."),
            mtg(
                "mtg-sla-legado",
                "SLA de suporte — legado pré-renovação",
                "2026-03-30",
                Some("operacoes"),
                MeetingState::StaleDerived,
                vec![stale_ev(
                    "sla-stale",
                    "summary",
                    "Resumo desatualizado do compromisso antigo: primeira resposta apenas em um dia inteiro para qualquer plano assinado.",
                    "Resumo regenerado removeu o compromisso antigo de um dia inteiro herdado do contrato legado.",
                )],
            ),
            dm("mtg-relatorio-semanal", "Relatório semanal de operações", "2026-04-22", None,
                "O relatório semanal consolida volume de chamados, reincidências e pendências por squad."),
        ],
        true,
    )
}

// Critical 5: cross-section join inside one meeting. Neither the threshold nor
// the callback commitment repeats the question words; survey-logistics
// neighbours own them.
fn nps_detrator() -> EvaluationCase {
    sibling_case(
        "pt-ref-nps-detrator",
        "o que decidimos sobre clientes detratores da pesquisa?",
        "Escuta ativa — plano para notas baixas",
        "2026-06-29",
        Some("experiencia"),
        ScopeKind::All,
        "",
        vec![
            ev("nps-plano", "summary", "Fechamos o ciclo trimestral de escuta e montamos um fluxo de contato para quem responde mal o formulário de opinião."),
            ev("nps-limiar", "note", "Decisão registrada: recebe selo vermelho quem dá nota menor que seis no formulário de opinião da rodada atual."),
            ev("nps-contato", "note", "Abertura do bloco de encerramento: o grupo revisou os comentários enviados pelas unidades da região, separou elogios pontuais de reclamações recorrentes, validou os critérios de prioridade combinados na rodada anterior, alinhou as pendências com a coordenação regional e revisou também os pedidos de reabertura ignorados no ciclo passado, confirmando os donos de cada fila de atendimento; na sequência ficou definido que o time telefona ao cliente em dois dias úteis e registra o motivo no painel de retorno."),
            ev("nps-cupom", "note", "Ideia descartada: cupom como resposta padrão para notas baixas, por mascarar causas raiz."),
        ],
        &["nps-limiar", "nps-contato"],
        &["nota menor que seis", "telefona ao cliente em dois dias úteis"],
        &["cupom como resposta padrão"],
        vec![
            dm("mtg-pesquisa-envio", "Envio da pesquisa de clima", "2026-06-02", Some("pessoas"),
                "O envio da pesquisa de clima abre na próxima segunda para todos os times."),
            dm("mtg-pesquisa-usabilidade", "Pesquisa de usabilidade guiada", "2026-07-05", Some("experiencia"),
                "A pesquisa de usabilidade guiada entrevistou operadores do módulo fiscal."),
            dm("mtg-clientes-entrevista", "Entrevistas com clientes fiéis", "2026-05-19", Some("sucesso-cliente"),
                "As entrevistas com clientes fiéis revelaram elogios ao novo painel de autoatendimento."),
            dm("mtg-combinados-reuniao", "Combinados da reunião geral", "2026-04-28", None,
                "Os combinados da reunião geral ficaram registrados na ata compartilhada do mês."),
            dm("mtg-relatos-insatisfacao", "Relatos públicos de insatisfação", "2026-03-26", Some("experiencia"),
                "Relatos públicos de insatisfação viraram pauta fixa do comitê de experiência deste trimestre."),
            dm("mtg-pesquisa-rodadas", "Rodadas de escuta da pesquisa", "2026-03-11", Some("experiencia"),
                "A pesquisa interna de opinião subsidia o relatório trimestral da área de experiência."),
            dm("mtg-conselho-clientes", "Conselho consultivo com clientes", "2026-02-25", Some("sucesso-cliente"),
                "Clientes do conselho consultivo elogiaram o atendimento e renovaram os contratos anuais."),
            dm("mtg-pesquisa-net-promoter", "Pesquisa net promoter — cadência", "2026-02-11", Some("experiencia"),
                "A pesquisa net promoter passa a rodar mensalmente com amostra completa das contas atendidas."),
            dm("mtg-combinados-clientes", "Combinados com clientes estratégicos", "2026-03-04", Some("sucesso-cliente"),
                "Os combinados com clientes estratégicos ficam registrados na ata da diretoria executiva."),
            dm("mtg-formularios-feedback", "Formulários de feedback contínuo", "2026-01-28", None,
                "Nova regra sobre formulários de feedback substitui os disparos antigos de opinião a partir de fevereiro."),
        ],
        true,
    )
}
