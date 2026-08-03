# Plano: Multi-Template Summaries por Meeting

## Goal

Permitir **N sumários independentes por meeting**, um por template. Usuário gera "Daily Standup" e "Standard Meeting" separadamente, pode excluir cada um individualmente. UX reusa o botão "Template" no header do `SummaryPanel`, renomeado para "Summaries" + badge com contagem.

## Decisões travadas

| # | Decisão | Escolha |
|---|---|---|
| 1 | Modelo | N templates/meeting, 1 sumário por template, delete individual |
| 2 | UX | Reusar botão "Template" → renomear para "Summaries" + badge |
| 3 | Regeneração | Manual por template |
| 4 | Template lifecycle | Soft reference (aceita broken refs) |
| 5 | Default ao abrir | Último visualizado (localStorage), fallback mais recente |
| 6 | Dirty switch | Confirm dialog com "Save & switch" |
| 7 | Backfill | Sentinela `template_id='legacy'` |
| 8 | Concorrência | Serializar — menu locked durante geração |
| 9 | Botão primário | Sempre "Regenerate" quando ativo existe |
| 10 | Delete | Dialog de confirmação |
| 11 | Export | Apenas sumário ativo |
| 12 | Analytics | Sem mudanças |
| 13 | Delete do ativo | Fallback: próximo da lista, senão empty state |
| 14 | Regenerate legacy | Dialog "Choose template" → cria NOVA row (legacy intacto) |
| 15 | Persistência | localStorage por meeting (`meeting.activeSummary.<meetingId>`) |
| 16 | Poll fetch | Passa templateId no startSummaryPolling |
| 17 | Tooltip lock | "A summary is already being generated" |

## Correções obrigatórias (identificadas pelos reviews)

### SEV-1 #1: Manter `result_backup` + `result_backup_timestamp`
As 4 queries em `summary.rs` (linhas 102, 135, 170, 204) dependem dessas colunas. Removê-las quebraria o build em runtime.

### SEV-1 #2: Todas as queries com `WHERE meeting_id = ? AND template_id = ?`
`update_process_completed`, `update_process_failed`, `update_process_cancelled`, `update_meeting_summary`, `create_or_reset_process`. 6 call sites em `service.rs` (328, 345, 351, 392, 397, 480, 635) propagam `template_id`.

### SEV-1 #3: `ON CONFLICT(meeting_id, template_id)`
O upsert atual falha pós-migração.

### SEV-3: Default alinhado para `"standard_meeting"`
`summary/commands.rs:354` usa `"daily_standup"` quando `template_id` é None. Alinhar.

### SEV-3: Rename meeting title condicional
`service.rs:583-594` só renomeia se `meetings.title` vazio OU nenhum row `completed` existe.

### SEV-2: `service.rs:518` cache lookup
`get_summary_data(&pool, &meeting_id)` → passa `template_id`.

## Schema (corrigido)

```sql
-- 20260721000001_multi_template_summaries.sql
ALTER TABLE summary_processes RENAME TO summary_processes_old;

CREATE TABLE summary_processes (
    meeting_id TEXT NOT NULL,
    template_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error TEXT,
    result TEXT,
    start_time TEXT,
    end_time TEXT,
    chunk_count INTEGER DEFAULT 0,
    processing_time REAL DEFAULT 0.0,
    metadata TEXT,
    result_backup TEXT,
    result_backup_timestamp TEXT,
    PRIMARY KEY (meeting_id, template_id),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

INSERT INTO summary_processes
  (meeting_id, template_id, status, created_at, updated_at, error, result,
   start_time, end_time, chunk_count, processing_time, metadata,
   result_backup, result_backup_timestamp)
SELECT meeting_id, 'legacy', status, created_at, updated_at, error, result,
       start_time, end_time, chunk_count, processing_time, metadata,
       result_backup, result_backup_timestamp
FROM summary_processes_old;

DROP TABLE summary_processes_old;
```

**Mudanças vs. plano original**:
- PK composta `(meeting_id, template_id)` — sem surrogate `id`, sem índice redundante.
- Mantém `result_backup*` (4 queries dependem).
- Backfill `'legacy'` (honesto sobre origem).

## Backend

### `database/models.rs`
```rust
pub struct SummaryProcess {
    pub meeting_id: String,
    pub template_id: String,  // NOVO
    // resto inalterado (sem id surrogate)
}
```

### `database/repositories/summary.rs`
Assinaturas novas:
```rust
get_summary_data(pool, meeting_id, template_id) -> Option<SummaryProcess>
get_summary_data_for_meeting(pool, meeting_id, template_id) -> Option<SummaryProcess>
list_summaries_for_meeting(pool, meeting_id) -> Vec<SummaryProcess>  // NOVO
create_or_reset_process(pool, meeting_id, template_id)  // ON CONFLICT(meeting_id, template_id)
update_meeting_summary(pool, meeting_id, template_id, summary)
update_process_completed(pool, meeting_id, template_id, ...)
update_process_failed(pool, meeting_id, template_id, ...)
update_process_cancelled(pool, meeting_id, template_id)
delete_summary(pool, meeting_id, template_id) -> bool  // NOVO
```

### `summary/commands.rs`
- `api_process_transcript`: default `template_id` = `"standard_meeting"`.
- `api_get_summary(meeting_id, template_id: Option<String>)`:
  - `Some(id)` → busca esse.
  - `None` → `ORDER BY updated_at DESC LIMIT 1`.
- `api_save_meeting_summary(meeting_id, summary, template_id: Option<String>)` → default `"standard_meeting"`.
- **NOVO** `api_list_meeting_summaries(meeting_id) -> Vec<MeetingSummaryInfo>`:
  ```rust
  struct MeetingSummaryInfo { template_id, status, updated_at, error }
  ```
  Não retorna `result` completo (seria pesado). Frontend busca conteúdo via `api_get_summary`.
- **NOVO** `api_delete_meeting_summary(meeting_id, template_id) -> bool`:
  - Se PENDING → cancela via `CANCELLATION_REGISTRY` primeiro.
- `SummaryResponse` ganha `template_id: String`.
- `api_cancel_summary(meeting_id, template_id: Option<String>)` → aceita, ignora (serialização).

### `summary/service.rs`
- `update_process_failed(pool, meeting_id, template_id, error_msg)` — propaga para 7 call sites.
- Rename meeting title: só se vazio OU primeiro completed.
- `register_cancellation_token(meeting_id)` / `cleanup_cancellation_token(meeting_id)`: inalterados (serialização).

### `export/commands.rs:74`
Passa `request.template_id` para `get_summary_data`.

### `lib.rs`
Registrar `api_list_meeting_summaries` e `api_delete_meeting_summary`.

## Frontend

### Types (`types/meeting.ts`)
```ts
interface MeetingSummaryInfo {
  template_id: string;
  status: "PENDING" | "completed" | "failed" | "cancelled";
  updated_at: string;
  error: string | null;
}
```

### Hooks novos

**`hooks/meeting-details/useMeetingSummaries.ts`**:
```ts
const useMeetingSummaries = (meetingId: string) => {
  const [summaries, setSummaries] = useState<MeetingSummaryInfo[]>([]);
  const refresh = useCallback(async () => {
    const list = await invoke<MeetingSummaryInfo[]>("api_list_meeting_summaries", { meetingId });
    setSummaries(list);
  }, [meetingId]);
  useEffect(() => { refresh(); }, [refresh]);
  return { summaries, refresh };
};
```

**`hooks/meeting-details/useActiveSummaryTemplate.ts`** (localStorage-backed):
```ts
const useActiveSummaryTemplate = (meetingId: string, summaries: MeetingSummaryInfo[]) => {
  // 1. localStorage.getItem(`meeting.activeSummary.${meetingId}`)
  // 2. Fallback: mais recentemente atualizado
  // 3. Fallback: 'standard_meeting'
};
```

### `SummaryPanel.tsx`
- Estado: `activeTemplateId` (via hook acima).
- Ao trocar: se `blockNoteSummaryRef.current?.isDirty` → `ConfirmSwitchSummaryDialog`:
  - "Você tem alterações não salvas em '{templateName}'."
  - Botões: [Descartar] [Cancelar] [Salvar e trocar]
- Passa `activeTemplateId` para: `api_get_summary`, `api_save_meeting_summary`, `ExportMenu.templateId`, `api_process_transcript.templateId`.
- Delete do ativo: fallback para próximo da lista, senão empty state.

### `SummaryGeneratorButtonGroup.tsx`
- Trigger: `FileText` + "Summaries" + badge `{summaries.length}` quando ≥1.
- Dropdown:
  ```
  ─────────────────────────────
  Summaries              (label)
  ─────────────────────────────
  ✓ Standard Meeting    2h 🗑
    Daily Standup       3d 🗑
    ⚠ legacy (original)     🗑
  ─────────────────────────────
  Generate new summary   (label)
  ─────────────────────────────
    Project Sync           ✦
    Retrospective          ✦
  ```
- Rows existentes: click → troca `activeTemplateId` + carrega via `api_get_summary`.
- Trash: `ConfirmDeleteSummaryDialog` → `api_delete_meeting_summary` → refresh lista.
- Rows disponíveis: click no ✦ → `api_process_transcript` com esse template_id.
- **Lock**: se `summaryStatus ∈ {"processing","summarizing","regenerating"}` → menu disabled + tooltip "A summary is already being generated".
- Template deletado: row com id cru + ⚠ + tooltip "Template was removed. You can view, copy, export, or delete this summary, but regenerating requires a template with the same id."
- `legacy`: renderiza "Summary (original)"; click "Regenerate" → dialog "Choose template" → cria NOVA row (legacy intacto).

### Botão primário
- Se `activeTemplateId` tem row → label "Regenerate Summary", age no ativo.
- Se não tem → label "Generate Summary", age no ativo (default).

### Dialogs novos (2)
- `ConfirmSwitchSummaryDialog.tsx`: dirty + switch.
- `ConfirmDeleteSummaryDialog.tsx`: confirma delete.

### `useSummaryGeneration.ts`
- Passa `activeTemplateId` para `api_process_transcript`, `api_cancel_summary`, e nos fetches de restore (linhas 184, 217).

### `useMeetingData.ts:109`
Passa `activeTemplateId` para `api_save_meeting_summary`.

### `page.tsx:207`
`api_get_summary(meetingId)` (sem template) → backend retorna mais recente; hook ajusta depois.

### `SidebarProvider.tsx`
`startSummaryPolling(meetingId, processId, templateId, onUpdate)` — poll usa `templateId` para fetch. Polling continua por meeting (serialização garante 1 por vez).

### `ExportMenu.tsx`
Recebe `templateId = activeTemplateId` (já tem prop, só corrigir origem em `SummaryPanel:338`).

## Edge cases

| Caso | Comportamento |
|---|---|
| Meeting sem sumários | Empty state; menu esconde zona 1 |
| Só `legacy` | Menu mostra "Summary (original)"; regenerate → dialog choose template |
| Template deletado | Row com ⚠; view/export/delete OK; regenerate desabilitado |
| Delete do ativo | Fallback próximo da lista, senão empty state |
| Durante geração | Menu locked; só polling/cancel |
| Regenerate dirty | Confirm dialog (mesmo de switch) |
| Language por meeting | Inalterado. Trocar language afeta próxima geração apenas. |
| Sidebar polling após delete | Deletar row PENDING cancela o process primeiro, remove poll depois |

## Testes

### Rust (4 testes mínimos)
1. **Migration test** (`#[tokio::test]` com in-memory SQLite + `sqlx::Migrator`):
   - Seed pre-migration row com `result_backup` set.
   - Roda todas migrations.
   - Assert: row tem `template_id='legacy'`, backup existe, `create_or_reset_process(m, t1)` + `(m, t2)` = 2 rows independentes, regenerar t1 não toca t2.
2. **Per-template isolation**: completar t1, falhar t2, cancelar t3 → verificar isolation.
3. **Delete**: `delete_summary` → row some, lista atualiza.
4. **Default resolution**: `api_get_summary` com `None` → mais recente.

### Manual smoke (10 passos)
1. Criar meeting → gerar Daily → 1 row no menu.
2. Gerar Standard → 2 rows.
3. Trocar entre os 2 → editor carrega correto.
4. Editar Daily → trocar → confirm dialog → "Save & switch".
5. Delete Standard → dialog → some → fallback Daily.
6. Regenerate Daily → backup/restore funciona.
7. Gerar durante geração → menu locked.
8. Deletar template usado → row com ⚠.
9. Reload app → abre no último visualizado.
10. Export PDF → usa ativo.

## Sequência de implementação

1. **Migration** (com teste #1 rodando).
2. **`SummaryProcess` model + repo** (todas assinaturas novas).
3. **`api_list_meeting_summaries` + `api_delete_meeting_summary`** (commands + lib.rs).
4. **`api_get_summary`/`api_save_meeting_summary`/`api_process_transcript`/`api_cancel_summary`** ganham `template_id` opcional.
5. **`export/commands.rs`** passa template_id.
6. **Testes Rust #2-#4**.
7. **Frontend types + `useMeetingSummaries` + `useActiveSummaryTemplate`**.
8. **`SummaryPanel` integra activeTemplateId + dialogs**.
9. **`SummaryGeneratorButtonGroup` transformação do dropdown**.
10. **Propagação de `activeTemplateId` em todos os call sites**.
11. **Smoke test manual completo**.

## Arquivos (~16)

**Backend** (7):
- `frontend/src-tauri/migrations/20260721000001_multi_template_summaries.sql` (novo)
- `frontend/src-tauri/src/database/models.rs`
- `frontend/src-tauri/src/database/repositories/summary.rs` (+tests)
- `frontend/src-tauri/src/summary/commands.rs`
- `frontend/src-tauri/src/summary/service.rs`
- `frontend/src-tauri/src/export/commands.rs`
- `frontend/src-tauri/src/lib.rs`

**Frontend** (9):
- `frontend/src/types/meeting.ts`
- `frontend/src/hooks/meeting-details/useMeetingSummaries.ts` (novo)
- `frontend/src/hooks/meeting-details/useActiveSummaryTemplate.ts` (novo)
- `frontend/src/components/MeetingDetails/SummaryPanel.tsx`
- `frontend/src/components/MeetingDetails/SummaryGeneratorButtonGroup.tsx`
- `frontend/src/components/MeetingDetails/ConfirmSwitchSummaryDialog.tsx` (novo)
- `frontend/src/components/MeetingDetails/ConfirmDeleteSummaryDialog.tsx` (novo)
- `frontend/src/hooks/meeting-details/useSummaryGeneration.ts`
- `frontend/src/hooks/meeting-details/useMeetingData.ts`
- `frontend/src/components/Sidebar/SidebarProvider.tsx`
- `frontend/src/app/meeting-details/page.tsx`