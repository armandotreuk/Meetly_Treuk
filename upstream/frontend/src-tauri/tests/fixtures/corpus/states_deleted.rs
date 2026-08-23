// Deleted-state family: a removed candidate still carries the query terms and
// a wrong figure in the index, but must never surface through production FTS
// because the parent meeting row is gone.

use super::super::EvaluationCase;
use super::{case, dm, ev, mtg, scope, Language, MeetingState, ScopeKind};

pub(super) fn cases() -> Vec<EvaluationCase> {
    let topics: [(&str, &str, &str, &str, &str, &str, &str, &str); 15] = [
        (
            "en-deleted-invoice-threshold",
            "invoice threshold approval limit",
            "Approval limits for invoice batches",
            "Batch approvals above the invoice threshold of two thousand coins need a second signer.",
            "mtg-invoice-batch", "Invoice batch approvals", "2026-06-11",
            "The invoice threshold was lowered from five thousand coins last quarter.",
        ),
        (
            "en-deleted-onboarding-guide",
            "onboarding guide revision number",
            "Revision history of the onboarding guide",
            "The onboarding guide reached revision nine with the new benefits chapter attached.",
            "mtg-onboarding-rev", "Onboarding guide revisions", "2026-05-30",
            "An early draft claimed revision twelve before chapters were merged back.",
        ),
        (
            "en-deleted-server-window",
            "server maintenance window saturday",
            "Server maintenance window planning",
            "Server maintenance stays on Saturday 02:00 with a two-hour maximum disruption window.",
            "mtg-server-window", "Maintenance window schedule", "2026-06-24",
            "A scrapped proposal suggested moving the window to Sunday 04:00.",
        ),
        (
            "en-deleted-refund-policy-code",
            "refund policy code R17",
            "Refund policy code registry",
            "Refund code R17 covers damaged-in-transit claims with automatic store credit.",
            "mtg-refund-registry", "Refund code registry", "2026-07-08",
            "A withdrawn draft mapped R17 to manager approval instead of automatic credit.",
        ),
        (
            "en-deleted-badge-colors",
            "visitor badge colors orange",
            "Visitor badge color legend",
            "Orange visitor badges mark escorted guests valid until end of business day.",
            "mtg-badge-legend", "Badge legend refresh", "2026-05-19",
            "A discarded palette made every visitor badge neon green regardless of escort.",
        ),
        (
            "en-deleted-quota-regional",
            "regional quota numbers northeast",
            "Regional quotas for the northeast territory",
            "Northeast regional quotas rise four percent after the depot expansion finishes.",
            "mtg-regional-quotas", "Territory quota review", "2026-06-02",
            "An obsolete sheet listed the northeast quota flat at zero percent growth.",
        ),
        (
            "en-deleted-training-room-cap",
            "training room capacity twenty",
            "Training room capacity rules",
            "The main training room caps at twenty seats until the projector mount is reinforced.",
            "mtg-training-room", "Room capacity audit", "2026-07-15",
            "A stale flyer advertised capacity thirty-five from the old annex layout.",
        ),
        (
            "en-deleted-api-version-date",
            "api version sunset date v9",
            "API version support calendar",
            "Version v9 of the public API sunsets on 31 March 2027 with notices at login.",
            "mtg-api-calendar", "API support calendar", "2026-06-18",
            "A retired plan moved the v9 sunset to December before partners pushed back.",
        ),
        (
            "en-deleted-expense-per-diem",
            "per diem rate forty coins",
            "Per diem rate adjustments",
            "Domestic per diem holds at forty coins while lodging receipts stay mandatory.",
            "mtg-perdiem-rates", "Per diem policy check-in", "2026-05-27",
            "A rejected memo floated sixty coins including lodging in the flat rate.",
        ),
        (
            "en-deleted-hiring-freeze-exempt",
            "hiring freeze exemptions support",
            "Hiring freeze exemption list",
            "Support engineering remains exempt from the hiring freeze for critical backfill.",
            "mtg-freeze-exemptions", "Freeze exemption review", "2026-07-01",
            "An earlier draft stripped every exemption before leadership restored them.",
        ),
        (
            "en-deleted-partner-tier-names",
            "partner tier names platinum",
            "Partner tier naming decision",
            "Platinum partner tier requires two certified engineers and quarterly reviews.",
            "mtg-partner-tiers", "Tier naming workshop", "2026-06-25",
            "A fun-but-shelved idea renamed tiers after constellations instead of metals.",
        ),
        (
            "en-deleted-backup-frequency",
            "backup frequency weekly archive",
            "Backup frequency confirmation",
            "Nightly backups continue with the weekly archive moving to cold storage.",
            "mtg-backup-plan", "Backup plan review", "2026-05-14",
            "A cancelled option dropped nightly snapshots for weekly-only coverage.",
        ),
        (
            "en-deleted-office-parking-pass",
            "parking pass fee ten coins",
            "Parking pass fee structure",
            "Monthly parking passes cost ten coins with motorcycle spots staying free.",
            "mtg-parking-fees", "Parking fee tuning", "2026-07-20",
            "A voided proposal charged visitors twenty coins per entry at the gate.",
        ),
        (
            "en-deleted-release-notes-style",
            "release notes style numbered",
            "Release notes style guide",
            "Numbered release notes ship with every deploy, grouped by component area.",
            "mtg-notes-style", "Notes style alignment", "2026-06-09",
            "An abandoned experiment wrote release notes as haiku summaries instead.",
        ),
        (
            "en-deleted-table-reservation-limit",
            "table reservation limit three days",
            "Table reservation policy update",
            "Table reservations cap at three consecutive days so visiting teams find space.",
            "mtg-table-reservation", "Booking policy sync", "2026-05-21",
            "A lapsed draft extended bookings to ten days before the fair-share rule won.",
        ),
    ];
    topics
        .iter()
        .enumerate()
        .map(|(index, topic)| {
            let (id, question, _topic_title, answer_text, tm_id, tm_title, tm_date, deleted_text) =
                *topic;
            deleted_case(
                id,
                question,
                answer_text,
                (tm_id, tm_title, tm_date),
                deleted_text,
                index,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn deleted_case(
    id: &str,
    question: &str,
    answer_text: &str,
    target: (&str, &str, &str),
    deleted_text: &str,
    index: usize,
) -> EvaluationCase {
    let evidence_id = format!("{id}-answer");
    let summary_id = format!("{id}-context");
    let target_meeting = mtg(
        target.0,
        target.1,
        target.2,
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
                "Working session recorded the current decision with owners and next review date.",
            ),
        ],
    );
    let deleted_meeting = mtg(
        &format!("mtg-{id}-removed"),
        &format!("Removed draft {}", index + 1),
        "2026-03-05",
        Some("records"),
        MeetingState::Deleted,
        vec![ev(&format!("{id}-removed-ev"), "note", deleted_text)],
    );
    let d1 = dm(
        &format!("mtg-{id}-near"),
        &format!("Adjacent records briefing {index}"),
        "2026-04-12",
        Some("records"),
        "Routine records briefing covering filing hygiene and retention labels.",
    );
    let d2 = dm(
        &format!("mtg-{id}-archive"),
        &format!("Archive intake notes {index}"),
        "2026-02-17",
        None,
        "Archive intake notes describe boxing schedules and retrieval request forms.",
    );
    let ordinal = (index % 4) as u8;
    let allowed_base = [target.0, d1.id.as_str(), d2.id.as_str()];
    let kind_scope = match ordinal {
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
    let fact = short_fact(answer_text);
    case(
        id,
        Language::English,
        question,
        &[],
        None,
        "exact_lookup",
        &["exact_term", "number_date_name", subtype, "state_deleted"],
        false,
        kind_scope,
        vec![target_meeting, deleted_meeting, d1, d2],
        &[target.0],
        &[],
        &[evidence_id.as_str()],
        &[fact.as_str()],
        &[short_forbidden(deleted_text).as_str()],
    )
}

fn short_fact(answer: &str) -> String {
    let words = answer.split_whitespace().collect::<Vec<_>>();
    let take = words.len().min(8);
    words[..take].join(" ").trim_end_matches(',').to_string()
}

fn short_forbidden(deleted_text: &str) -> String {
    let words = deleted_text.split_whitespace().collect::<Vec<_>>();
    let start = words.len().saturating_sub(8);
    words[start..].join(" ").trim_end_matches('.').to_string()
}
