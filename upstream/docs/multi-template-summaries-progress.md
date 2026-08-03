# Multi-Template Summaries — Progress Log

Plano de referência: `upstream/docs/multi-template-summaries-plano.md`.

Este documento é atualizado ao final de cada item da todo list. Entradas em ordem cronológica (mais recente no topo).

---

## Sprint G (Items 32–38): PDF table rendering — real grid + checkbox-list detection

### Contexto e root cause

Bug reportado pelo usuário: PDF export de reuniões com tabela (template `standard_meeting` → seção "Action Items" com `item_format` pipe) não renderiza como tabela — sai como texto quebrado. Persiste com `minimax m3` e `glm 5.2`.

**Investigação** (read-only, sem mudanças de código):

1. Extraí o `result` JSON de `meeting_minutes.sqlite` (5 rows `status='completed'`, 7 hits em `**Action Items**`). Verifiquei: **o LLM está produzindo pipe-tables válidas** no markdown armazenado. Exemplo real do DB:
   ```
   | **Owner** | Task | Due | Reference Transcript Segment | Segment Time stamp |
   | --- | --- | --- | --- | --- |
   | Armando | Open Jira/chat cards to track the agreed changes (card names, modality multi-select, course obligatoriness, tutor feedback display, query update). | None noted in this section. | "Aqui no chat, s� para ficar a�, eu tenho que abrir o card disso tudo, � rapidinho." | 01:15:21 |
   ```
2. Data flow: `result.markdown` → `merge_sections` (split por `**Bold**`) → `SectionContent { format: "list", content }` → `render_list` → `render_markdown_table` (`pdf.rs:497-530`). Tudo certo até o renderer.
3. **Bug em `render_markdown_table`**: junta as células com `"  │  "` e passa a string inteira para `write_wrapped`. Para uma tabela de 5 colunas com a coluna "Reference Transcript Segment" contendo frase completa (100+ chars), a linha concatenada excede a largura imprimível. `write_wrapped` quebra a linha em 3-4 linhas físicas, enterrando os separadores `│` dentro de parágrafos quebrados. Tabela deixa de parecer tabela.
4. **Bug secundário**: `looks_like_structured_list` (`pdf.rs:431-441`) só detecta listas com `|`, `:` (≥2), ou `,` (≥2). Não detecta `- [ ] Task - [[Owner]] - Due: Date` (formato checkbox usado por outputs mais antigos / outras LLMs). Cai no bullet-list fallback.

### Solução escolhida

**Opção 1 (grid real) + detecção de checkbox-list**, conforme aprovado pelo usuário. Substitui o renderer atual por um grid com colunas calculadas, bordas, e paginação por linha. Estende o detector para incluir `- [ ] ... [[Owner]] ... Due:`. Sem novas dependências.

### Items da Sprint G

| # | Item | Dependências | Estimativa |
|---|---|---|---|
| 32 | Layout constants + `write_wrapped_at` helper em `pdf.rs` | nenhuma | ~30 LOC |
| 33 | `render_table_grid` (parse rows, compute column widths, render grid com bordas, row-pagination, header repetition em continuação) | item 32 | ~150 LOC |
| 34 | Wire `render_table_grid` em `render_list` (substitui chamada a `render_markdown_table`); passa `item_format` para header synthesis quando LLM esquece header | item 33 | ~15 LOC |
| 35 | Detectar `- [ ]` checklist em `looks_like_structured_list` + parser de `[[Owner]]` e `- Due: ...` em `render_structured_list_as_table` | nenhuma | ~30 LOC |
| 36 | Testes: `renders_real_grid_with_borders`, `wraps_long_cells_within_column_width`, `handles_continuation_across_pages`, `synthesizes_header_from_item_format`, `renders_checkbox_list_as_table`, regression `keeps_pipe_table_when_header_present` | itens 32-35 | ~120 LOC |
| 37 | `cargo test -p meetily --lib` + `cargo test -p meetily --lib export` | item 36 | command |
| 38 | Build release CUDA + install exe + re-export do meeting `d934686d-...` (dado real do DB) e inspeção visual do grid | item 37 | command |

**Nota sobre item 31 (smoke test manual)**: continua pendente. **Independente de Sprint G** — testa fluxo UX de templates (criar/gerar/trocar/deletar/export), não layout interno de tabela. Pode rodar em paralelo com a Sprint G.

### Riscos / ceilings documentados

- **Coluna com width < `TABLE_MIN_COL_WIDTH_MM` (18mm)**: floor aplicado. Se tabela tem 8+ colunas, mínimo protege legibilidade mas pode exceder `CONTENT_WIDTH_MM`. `ponytail:` marker no código: em tal caso, log warning e cai no render atual (text com `│`).
- **Zebra-striping**: não implementado. Upgrade se user pedir.
- **Header repetition em page continuation**: implementado, ~10 LOC, regra horizontal fina + header re-renderizado.

### Arquivos a modificar

- `frontend/src-tauri/src/export/pdf.rs` (único arquivo). Sem mudança em outros módulos.
- `frontend/src-tauri/src/export/pdf.rs` `mod tests` (extender com 6 testes novos).

### Estratégia de implementação

Seguir a ordem 32 → 33 → 34 → 35 → 36 → 37 → 38. Items 32+35 são paralelizáveis (arquivos separados mentalmente, mas mesmo arquivo na prática). 34 só faz sentido após 33. 36+ dependem de 32-35. 37+38 dependem de 36.

### Status

- **Items 32–33**: layout constants + `write_wrapped_at` + `render_table_grid`/`compute_row_height`/`draw_table_row` implementados. Code-review de item 33 achou 7 bugs; todos corrigidos (mm_per_char, measure_longest_word, fallback-render contract, cell_y ascent offset, compute_row_height line-height selection, ensure_space antes do header, teste de grid).
- **Item 34**: `render_table_grid` wired em `render_list` com `item_format` threadado; 7 `#[allow(dead_code)]` removidos. Code-review de item 34 achou 1 CRITICAL (`available_mm<=0` sem fallback — perda de conteúdo), 1 MAJOR (row maior que uma página paginava mid-row), 2 MINOR (warn! no mismatch, strip `**` dos headers), 1 NITPICK (`.max(1)` redundante). Todos corrigidos + 2 testes novos.
- **Item 35**: detecção de checkbox-list (`- [ ] task [[Owner]] Due: date`) → sintetiza pipe-table 3-col e roteia via `render_table_grid`. 2 testes novos (`looks_like_checkbox_list_requires_majority`, `checkbox_list_to_pipe_table_extracts_owner_and_due`).
- **Item 36** (este): Finding-5 fix (prose around table) + 8 testes novos.
  - **Finding 5 (code review prévio)**: conteúdo misto prose+tabela tinha as linhas de prose silenciadas quando qualquer linha começava com `|`. Agora `render_list_segmented` divide em segmentos e renderiza prose вокруг da tabela. ~25 LOC + 1 teste.
  - **Testes novos** (totais vão de 21 para 33, todos passando):
    1. `render_table_grid_draws_grid_rows_with_padding` — cursor advance == 24.2mm prova que `draw_table_row` (path com bordas+padding) rodou.
    2. `compute_row_height_grows_when_cell_wraps` — célula longa em coluna estreita: height > 1 linha; célula curta em coluna larga: height == 1 linha.
    3. `render_table_grid_paginates_and_repeats_header` — 40 rows → page_number == 2, cursor ≈ 199.6 pins both pagination AND header repetition (seria 208.6 sem repeat).
    4. `render_table_grid_header_synthesis_fallbacks` — 3 sub-paths: mismatch → false, item_format não-pipe → false, None → true (generic `Col N`).
    5. `render_table_grid_explicit_header_ignores_item_format` — header explícito vence; item_format ignorado.
    6. `render_table_grid_fallback_paths_still_render_content` — locks review fix #1: 340-col (available_mm<=0) + mismatch ambos retornam false E avançam cursor (render_markdown_table interno rodou).
    7. `render_list_renders_prose_around_table` — mixed advance − table-only advance ≥ 2*LINE_HEIGHT_BODY.
    8. `merge_sections_threads_item_format_through_to_render` (commands.rs) — locks `.or_else(example)` threading; item_format e example_item_format ambos chegam em SectionContent.

**Verificação**: `cargo check -p meetily` clean (nenhum warning Rust); `cargo test -p meetily --lib export` = 33 passed, 0 failed.

### Próximo

- **Item 37**: `cargo test -p meetily --lib` (full suite gate).
- **Item 38**: Build release CUDA + install exe + re-export do meeting `d934686d-...` + inspeção visual do grid no PDF real (bordas, alinhamento, paginação, header repeat).
- **Item 31**: Smoke test manual de fluxo UX de templates — independente, pode rodar em paralelo.

**Status: items 32–36 DONE; 37–38 + 31 pending.**

---

## Sprint F (Item 30): Build & install

### O que foi feito

Item 30 — build release com CUDA + install do exe.

Comando:
```
workdir=frontend
$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"
npx --no-install @tauri-apps/cli build --no-bundle -- --features cuda
```

Resultado:
- Next.js build: ✓ (11 static pages geradas, ESLint warnings pre-existing em arquivos não editados).
- Rust release build: ✓ `Finished release profile in 2m 54s`.
- Output: `C:\Users\arman\cargo-target\release\meetily.exe` (166 MB).

Install:
```
Copy-Item "C:\Users\arman\cargo-target\release\meetily.exe" "C:\Users\arman\AppData\Local\meetily\meetily.exe" -Force
```

Verificado: `LastWriteTime 7/22/2026 12:47:32 AM`, 166704640 bytes. Build pré-existente do `meetily.exe` (se houver) foi substituído.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **`--no-bundle`** | Plano especifica: não criar instalador MSI/NSIS, só exe pronto p/ smoke test. |
| **Copy para `%LOCALAPPDATA%\meetily\`** | Diretório de install existente; match do path que scripts `clean_run_windows.bat` usam. |
| **Sem `Stop-Process` primeiro (desta vez)** | Build não谋requer app parado (só o copy sim). Verifico via tentativa de overwrite: o `Copy-Item -Force` sucede. Aplicação em execução lock o exe → Copy falharia; não falhou, então não estava rodando. |

### Logs
- `tauri build` output completo (50 linhas finais) preservado no transcript da sessão.
- Sem warnings novos nos arquivos editados (todos warnings ESLint são pre-existing em `recordingNotification.tsx`, `analytics.ts` etc.).

### Próximo
- **Item 31** — smoke test manual (10 passos). Checkbox no progress doc a seguir. **Independente de Sprint G** (testa UX de templates, não layout de tabela).
- **Sprint G** (Items 32-38) — fix PDF table rendering. PLANNED, aguardando autorização. Ver seção no topo deste arquivo.

---

## Sprint E (Items 19 + 28): SummaryPanel integration glue + ExportMenu `activeTemplateId`

### O que foi feito

Investigação pós-Sprint D revelou que o agent do Sprint D fez mais do que reportou — já completou水位a maior parte de Sprint E inline. Verificação no code:
- `SummaryPanel.tsx:389-407`: ExportMenu renderizado conditional em `activeTemplateId` não-null + `templateId={activeTemplateId}` (item 28).
- `SummaryPanel.tsx:269`: `pendingEditsExist = summaryRef.current?.isDirty || false` alimentando `SummaryGeneratorButtonGroup`.
- `SummaryPanel.tsx:271-301`: error/status banners gated por `statusOriginTemplateIdRef`/`errorOriginTemplateIdRef` snapshot comparison — suprime error de row deletada.
- `SummaryPanel.tsx:556-563`: `key={activeTemplateId ?? "none"}` no `BlockNoteSummaryView` — força remount em switch, resetando `isDirty` automaticamente (delete fallback). `ponytail:` ceiling nomeando `resetDirty()`/`setBlocks()` como upgrade path.
- `SummaryPanel.tsx:474`: empty-state via `EmptyStateSummary` quando `!aiSummary` (sem UI nova).
- Trigger do `ConfirmSwitchSummaryDialog` confirmado em `SummaryGeneratorButtonGroup.tsx:322-329` (dentro do botão group, não em SummaryPanel).
- Re-fetch pós-delete via `page.tsx:335` `useEffect` deps `[meetingId, activeTemplateId]`.
- `activeTemplateId`/`setActiveTemplateId` passed to `useMeetingData`/`useSummaryGeneration` via `page-content.tsx:121, 172`.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **`key={activeTemplateId}` remount strategy** vs. BlockNote `resetDirty()` explícito | Remount é menor diff e funciona sem expor uma API that may not exist. `ponytail:` ceiling sinaliza o upgrade: quando BlockNote expuser `resetDirty()` ou `setBlocks()` puder ser chamado sem remount, trocar. |
| **Snapshot refs (`statusOriginTemplateIdRef`/`errorOriginTemplateIdRef`) em vez de derived state** | Referencia o `template_id` da row que originou o status/error atual. Em switch/delete, compara com `activeTemplateId` atual → se divergem, suprime o banner (stale). Evita flash de error de row deletada. |
| **Manter `selectedTemplate` prop em SummaryPanel** (`ponytail:` nota em `:115-119`) | Remover exigiria editar `page-content.tsx` (threading de props ainda ativo). Menor diff = manter prop, mesmo que não consumida para export agora. Upgrade path documentado. |
| **Não criar nova empty-state UI** | `EmptyStateSummary` já existe em `:474`. Reutilizado. YAGNI. |
| **Trigger do ConfirmSwitch vive no botão group, não em SummaryPanel** | Sprint D já colocou a lógica de `if (pendingEditsExist) { setSwitchDialogOpen(true); return; }` na Zone-1 click handler. Respeitado; mover quebraria encapsulamento. SummaryPanel só fornece o `pendingEditsExist` value. |

### Validação
- `npm run typecheck` exit 0 (0 erros).
- `npm run lint` exit 0 (passa; warnings em `SummaryPanel.tsx` são todos relativos ao `<EditableTitle>` comentado `:335-341` e `selectedTemplate` mantido intencionalmente — ambos documentados).

### Arquivos confirmados
- `frontend/src/components/MeetingDetails/SummaryPanel.tsx`: items 19 + 28 já no lugar.
- `frontend/src/components/MeetingDetails/ExportMenu.tsx`: intocado (item 28 foi mudança caller-side).
- `frontend/src/app/meeting-details/page-content.tsx`: threads já no lugar de Sprint B/D.
- `frontend/src/app/meeting-details/page.tsx`: já no lugar.

Sprint E outputs: confirmados no code, sem delta adicional além do já presente.

### Próximos passos
Sprint F (items 30-31): build (`cargo build --release --features cuda --no-bundle`, copy exe) + smoke test manual.

---

## Sprint D (Items 22, 23, 29): Refactor `SummaryGeneratorButtonGroup` + primary-button + legacy regenerate

### O que foi feito

**Item 22 — `frontend/src/components/MeetingDetails/SummaryGeneratorButtonGroup.tsx` (353→571 LOC)**: dropdown agora em 2 zonas.
- **Zone 1 — Existing summaries**: lista de `summaries` (threaded via props da instância única de `useMeetingSummaries` em `page.tsx` — segundo hook interno divergiria per `ponytail:` note do hook). Cada entry: display name (via `templateNameFor` — `'legacy'` renderiza como "Summary (original)"), status badge, `formatDistanceToNow(updated_at)`, active checkmark, trash icon. Click = `setActiveTemplateId(template_id)`. Se `pendingEditsExist` (lido do `summaryRef.current?.isDirty` — mesma fonte do Save button), switch dispara `ConfirmSwitchSummaryDialog` primeiro.
- **Zone 2 — Available templates**: `availableTemplates` menos os que já têm row. Click = `setActiveTemplateId` apenas (não gera). Chama `onTemplateSelect` (preserva toast + analytics).
- **Status badge no trigger**: mostra template ativo + badge + `AlertTriangle` se `activeTemplateId` estiver órfão (não presente em `summaries`).
- **Lock durante geração**: `isLocked = isGenerating OR summaries.some(s => status ∈ {processing, summarizing, regenerating})`. Desabilita trigger + trash; primary button continua enabled (sempre "Regenerate" quando row existe — item 23).
- **Delete**: abre `ConfirmDeleteSummaryDialog`; `onDeleted` chama `refresh()` e `setActiveTemplateId(null)` se era o ativo.

**Item 23 — Botão primário sempre Regenerate quando ativo existe**: label = `hasActiveRow ? "Regenerate" : "Generate summary"` onde `hasActiveRow = summaries.some(s => s.template_id === activeTemplateId)`. Tooltip espelha. Condições de disabled existentes (`!hasTranscripts`, `isCheckingModels || isModelConfigLoading`) untouched.

**Item 29 — Regenerate legacy → Choose template dialog**: `handlePrimaryClick` intercepta `activeTemplateId === 'legacy'` → abre `ChooseTemplateForLegacyDialog.tsx` (novo, 70 LOC, mesma pasta). Lista templates excluindo legacy. Click = apenas `setActiveTemplateId(chosenTemplateId)`, sem auto-generate. Legacy nunca é escrito (read-only archive).

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Props-driven, não hook instance interna** | `page.tsx` já monta `useMeetingSummaries` + `useActiveSummaryTemplate`. Re-instanciar dentro do botão criaria estado duplicado (per `ponytail:` ceiling do hook 17). Threadar via props mantém source-of-truth única. |
| **Remover `selectedTemplate` / `hasSummary` props** | Agora derivados de `summaries` + `activeTemplateId`. `SummaryPanel` mantém seu próprio `selectedTemplate` (usado por `ExportMenu`). Callers atualizados. |
| **`pendingEditsExist` guarda só Zone-1 switches** | Zone-2 (sem row) não tem edits associados ainda — confirm dialog de Sprint C não modela target sem row. Spec-literal. |
| **`isLocked` = prop + derivado de summaries** | Spec oferecia um ou outro; combinar é estritamente mais seguro (lock mesmo se prop ficar atrás). |
| **`onTemplateSelect` no Zone-2 click** | Preserva toast + analytics `template_selected` já existentes. Choose-template dialog (item 29) NÃO chama — spec-explicit (só `setActiveTemplateId`). |

### Novos arquivos
- `frontend/src/components/MeetingDetails/ChooseTemplateForLegacyDialog.tsx` (70 LOC).

### Arquivos modificados
- `frontend/src/components/MeetingDetails/SummaryGeneratorButtonGroup.tsx` (5 regiões principais, 353→571 LOC).
- `frontend/src/components/MeetingDetails/SummaryPanel.tsx`: `MeetingSummaryInfo` import, 4 novas props, `pendingEditsExist` (lê `summaryRef.current?.isDirty`), 3 call sites do button group atualizados.
- `frontend/src/app/meeting-details/page-content.tsx`: import + 3 props threadadas para `SummaryPanel`.
- `frontend/src/app/meeting-details/page.tsx`: destructure `refresh`/`setActiveTemplateId` dos hooks existentes, passa para `PageContent`.

### Validação
- `npm run typecheck` exit 0 (zero erros).
- `npm run lint` exit 0 (zero erros; nenhum novo warning nos arquivos editados — removido um `useRef` import unused pre-existente já que estava editando essa linha).

Sem toques em Sprint A (3 arquivos), Sprint B (5 arquivos), Sprint C (2 dialogs), ou backend Rust. Sem novas deps. Sem mudanças no progress doc até agora (este é o update).

### Próximos passos
Sprint E (item 19 — integração `SummaryPanel.tsx`) e Sprint F (items 30-31 — build + smoke test). Sprint E é "glue" que valida toda a plubing até aqui.

---

## Sprint C (Items 20-21): Dialogs novos — ConfirmSwitch + ConfirmDelete

### O que foi feito

**Item 20 — `frontend/src/components/MeetingDetails/ConfirmSwitchSummaryDialog.tsx` (1-123)**: novo dialog prop-driven espelhando o padrão de `RetranscribeDialog.tsx:288-472` (Dialog/DialogContent/DialogHeader/DialogFooter + shadcn/ui). Props: `open`, `onOpenChange`, `summaries: MeetingSummaryInfo[]`, `currentTemplateId`, `onConfirm: (newTemplateId) => void`, `pendingEditsExist: boolean`, e `templateNames?: Record<string, string>` (opcional p/ mapear `template_id` → display name). Lista as rows com status badge, destaca a atual, aviso amarelo se `pendingEditsExist`. Não own switching logic (caller via `onConfirm`).

**Item 21 — `frontend/src/components/MeetingDetails/ConfirmDeleteSummaryDialog.tsx` (1-88)**: novo dialog de delete. Props: `open`, `onOpenChange`, `meetingId`, `templateId`, `templateDisplayName?`, `onDeleted`. Confirm chama `invokeTauri("api_delete_meeting_summary", { meetingId, templateId })` (line 43), loading state no botão `variant="destructive"`, sonner toast em erro. Cancel apenas fecha. `ponytail:` ceiling: cliente-side skip de cancel prévio (backend já faz cancel+delete atômico).

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Pattern mirror: `RetranscribeDialog.tsx`** | Dialog confirmation mais similar (deletar/confirm ação mutável) — stesso `DialogFooter` + `Loader2` spinner + sonner toast. |
| **Props-driven, sem importar hooks de state** | Dialog não acopla a `useActiveSummaryTemplate`/`useMeetingSummaries` — Sprint B (paralelo) mudaria assinaturas. Chcaller injeta state via props. |
| **`templateNames?: Record<string, string>` opcional no Switch** | Spec só exigia 6 props; mapeamento `id`→label fica no caller. Default: usa `template_id` literal. |
| **`variant="destructive"` confirmado em `button.tsx:13-14`** | Variant existente, usado por outros deletes. |

### Validação
- `npm run typecheck` exit 0 (sem erros nos 2 novos arquivos).
- `npm run lint` exit 0 (sem warnings nos 2 novos arquivos).
- Sprint A (3 arquivos) e Sprint B (5 arquivos) untouched.

---

## Sprint B (Items 24-27): Propagação de `activeTemplateId` em hooks/serviços existentes

### O que foi feito

**Item 24 — `useSummaryGeneration.ts`**: hook ganha `activeTemplateId?: string | null` prop. Propagado em 4 sites IPC: `api_process_transcript` (L171), `api_get_summary` ×2 (L194/227), `api_cancel_summary` (L719). Deps arrays extendidos. `ponytail:` comment sobre row-key semantics.

**Item 25 — `useMeetingData.ts`**: `handleSaveSummary` passa `templateId: activeTemplateId ?? undefined` (L113). Hook aceita `activeTemplateId` como prop. `ponytail:` marker.

**Item 26 — `SidebarProvider.tsx`**: `startSummaryPolling` ganha 4o arg `templateId: string | null = null` (L263-270). Repassa `templateId: templateId ?? undefined` para `api_get_summary` (L301-305). Interface exportada atualizada (L70-76). Único caller é `useSummaryGeneration.ts` (item 24) que passou `activeTemplateId ?? selectedTemplate`.

**Item 27 — `page.tsx`**: `useMeetingSummaries` + `useActiveSummaryTemplate` montados page-level (L31-32). `activeTemplateId` injetado em `api_get_summary` (L208-211). Propagado a `PageContent` (L382) → hooks `useMeetingData` e `useSummaryGeneration` via `page-content.tsx` (L112, L157-168). `useEffect` deps extendido para `[meetingId, activeTemplateId]` (L332) — garante refresh ao trocar template ativo.

### Decision semântica — `api_process_transcript` (template_id é PROMPT template? É ROW key?)

Investigado `commands.rs:389`: backend aceita UM `template_id: Option<String>` usado tanto p/ seed a row (`create_or_reset_process(&m_id, &final_template_id)`) como p/ identifier repassado a `SummaryService::process_transcript_background`. **Não há campo separado** p/ "prompt template". Dicotomia spec colapsa: **row key == prompt-template identifier**. Decisão: `templateId: activeTemplateId ?? selectedTemplate` — quando regenerando row existente, escreve naquela row exata; fresh meeting sem row ativa → fallback ao template escolhido em `useTemplates`.

### Grep sweep — todos callers confirmados
Hits: `page.tsx:207` (27), `useSummaryGeneration.ts:85/158/174/184/217/448/710` (todos item 24), `useMeetingData.ts:109` (25), `SidebarProvider.tsx:70/263/300/403` (26). Hit em `summary-language-preferences.ts:118` é `api_save_meeting_summary_language` (comando distinto) — out of scope, skipped.

### Validação
- `npm run typecheck` exit 0.
- `npm run lint` exit 0 (warnings em linhas não editadas — todos pre-existing pattern replication: unused legacy imports, `any` casts em legacy-format parser).

### Arquivos modificados
- `frontend/src/hooks/meeting-details/useSummaryGeneration.ts` (9 regiões).
- `frontend/src/hooks/meeting-details/useMeetingData.ts` (2 regiões).
- `frontend/src/components/Sidebar/SidebarProvider.tsx` (3 regiões).
- `frontend/src/app/meeting-details/page.tsx` (5 regiões).
- `frontend/src/app/meeting-details/page-content.tsx` (3 regiões — plubing de `activeTemplateId` para hooks).

Sem toques em Sprint A (3 arquivos) ou backend Rust. Sem novas deps.

---

## Sprint A (Items 16-18): Fundação frontend — tipos + hooks base

### O que foi feito

**Item 16 — `MeetingSummaryInfo` em `frontend/src/types/index.ts:97-110`**: nova interface espelhando o struct Rust em `summary/commands.rs:44-50`. Como o struct Rust não tem `#[serde(rename_all = ...)]`, a Tauri IPC envia field names em **snake_case** — interface TS usa `template_id`, `status`, `updated_at`, `error?: string | null`. Field `status` tipado como union literal ("idle" | "processing" | "summarizing" | "regenerating" | "completed" | "error" | "failed" | "cancelled") espelhando `SummaryStatusResponse` em `SidebarProvider.tsx`.

**Item 17 — `useMeetingSummaries.ts` em `frontend/src/hooks/meeting-details/`**: hook espelhando o padrão de `useTemplates.ts`. Chama `invokeTauri<MeetingSummaryInfo[]>("api_list_meeting_summaries", { meetingId })` (Tauri converte `meeting_id` Rust → `meetingId` nos args). Retorna `{ summaries, loading, error, refresh }`. Sem polling (item 26 cuida disso no `SidebarProvider`). `ponytail:` ceiling: sem cache SWR/react-query — caller precisa chamar `refresh()` pós-mutação.

**Item 18 — `useActiveSummaryTemplate.ts` em `frontend/src/hooks/`** (top-level, não `meeting-details/`, pois é meeting-id-keyed state reutilizável): gerencia qual template está ativo na UI. localStorage key `meetily:active-template:{meetingId}` (com prefixo `meetily:` conforme spec). Aceita `summaries: MeetingSummaryInfo[]` opcional para validar o valor stored. Fallback chain quando stored Value ausente ou inválido:
1. Exactly 1 summaries entry → usa ela.
2. Algum `status === 'completed'` → mais recente por `updated_at`.
3. Non-empty → mais recente por `updated_at`.
4. Vazio → `null` (caller trata como first-time flow).

Silently clear + re-pick quando stored value some de `summaries` (template deletado server-side). `setActiveTemplateId(null)` limpa localStorage. `ponytail:` ceiling: sem cross-tab `storage` event sync (mirror file `summary-language-preferences.ts` também não tem).

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **snake_case nos tipos TS** | Rust struct sem `rename_all` → Tauri IPC preserva field names. Forçar camelCase quebraria runtime. |
| **Item 17 em `hooks/meeting-details/`, item 18 em `hooks/`** | 17 é specífico da página meeting-details; 18 é reutilizável (sidebar, dialogs, exports). |
| **localStorage key com prefixo `meetily:`** | Spec literal. Codebase não tem prefixo unificado (`summary-language-preferences.ts` usa bare keys); seguimos a spec explícita. |
| **Sem polling no hook 17** | Polling é responsabilidade do `SidebarProvider` (item 26). Hook fica leve e testável. |
| **Sem SWR/react-query** | Codebase usa `useState/useEffect` direto (`useTemplates.ts`). Não introduzir dep nova por YAGNI. |

### Arquivos modificados
- `frontend/src/types/index.ts:97-110`: +14 linhas (interface `MeetingSummaryInfo`).
- `frontend/src/hooks/meeting-details/useMeetingSummaries.ts`: novo arquivo (~50 LOC).
- `frontend/src/hooks/useActiveSummaryTemplate.ts`: novo arquivo (~70 LOC).

### Validação
- `npm run typecheck` (rodado a partir de `frontend/`): exit 0, sem erros nos 3 arquivos editados.
- `npm run lint`: exit 0, 0 warnings nos 3 arquivos editados (warnings pre-existing em `AISummary/index.tsx`, `ModelSettingsModal.tsx`, `analytics.ts` continuam).

### Próximos passos
Sprint B (items 24, 25, 26, 27 — propagação de `activeTemplateId` em hooks/serviços existentes) e Sprint C (items 20, 21 — dialogs novos) podem rodar em paralelo — ambos dependem apenas de Sprint A.

---

## Items 12-15: Rust tests (string-invariant, no DB)

### O que foi feito

Itens 12-15 do plano exigem checks runnable sobre a lógica não-trivial introduzida nos itens 1-11: migration de PK composta, queries de repo com `template_id`, delete, e fallback de `DEFAULT_TEMPLATE_ID`. Decidido por **string-invariant tests** (abordagem 1 da spec): lê-se os arquivos de fonte em runtime via `fs::read_to_string` + `CARGO_MANIFEST_DIR` (path absoluto, à prova de cwd) e asserts sobre substrings. Sem `sqlx` in-memory, sem fixtures, sem mock frameworks — alinhado à regra ponytail "no frameworks, no fixtures". Inline `#[cfg(test)] mod multi_template_tests` no fim de `frontend/src-tauri/src/summary/commands.rs` (mod é auto-discovered; nada registrado em `lib.rs`). Localização escolhida porque dá acesso direto ao const `DEFAULT_TEMPLATE_ID` (assert `==`,no string check) e cobre 3 arquivos-alvo via paths absolutos.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Inline `#[cfg(test)] mod multi_template_tests` em `commands.rs`** (não arquivo novo) | Convenção do repo: todos os 40+ `#[cfg(test)]` existentes são inline no mesmo arquivo de fonte (`onboarding.rs:229`, `metadata.rs:143`, `lib.rs:888`). Mod auto-discovered — zero registro em `lib.rs`. Ponytail "smallest file count possible". |
| **`fs::read_to_string` + `env!("CARGO_MANIFEST_DIR")` em vez de `include_str!`** | `include_str!` em `commands.rs` ref-to-self (item 15) recursa/ambíguo. `env!("CARGO_MANIFEST_DIR")` é literal baked-into-binary, à prova de cwd. 4 calls idênticas, sem boilerplate de helper (helper exigiria `concat!(env!(...), "/", rel)` — `rel` variável não é permitido em `concat!`, primeira compilação falhou; descartado). |
| **Asserts `contains(...)` + `matches(...).count() >= N`** (não regex) | Spec exige "prefer `content.contains` over brittle regex". Contagens (`>= 5` para WHERE composite, `>= 7` para `template_id: &str` params, `>= 3` para fallbacks DEFAULT_TEMPLATE_ID) são limiares:fáceis de satisfazer hoje, falham se alguém remove >1 ocorrência catastrófica. |
| **spec diz "Preserves `result_backup_markdown` e `result_backup_json`"; real schema usa `result_backup` + `result_backup_timestamp`** | Spec imprecisa sobre nomes reais. Teste asserta `result_backup ` (com trailing space, matcha declaração da coluna, não substring de `result_backup_timestamp`) e `result_backup_timestamp`. Captura a intenção da spec (colunas de backup preservadas) sem casar o falso nome. Notado no relatório. |
| **spec diz backfill "UPDATE ... SET template_id = 'legacy'"; real schema usa `INSERT ... SELECT meeting_id, 'legacy', ...`** | Idem: spec phrasing loose. Teste asserta apenas a presença do sentinel `'legacy'` no SQL. Captura intent (sentinel backfill) sem casar DDL inexato. Notado no relatório. |
| **Mantém-bound 5 para WHERE composite count** (não 6 exato) | Float: 6 ocorrem hoje (get_summary_data, update_meeting_summary, update_process_completed/failed/cancelled, delete_summary). `>= 5` falha se ≤5 faltarem — captura regressões; não acidentalmente quebra com add de nova fn composite. Ponytail "edges of similar size". |

### Arquivos modificados

- `frontend/src-tauri/src/summary/commands.rs`: adicionado `mod multi_template_tests` no fim (após `api_delete_meeting_summary`). 4 `#[test]` fns, mais asserts `fs::read_to_string` para 3 arquivos-alvo. Zero mudança de production code.

### Testes (4 fns)

1. `migration_invariants` — lê `migrations/20260721000001_multi_template_summaries.sql`; asserts: `PRIMARY KEY (meeting_id, template_id)`, sentinel `'legacy'`, colunas `result_backup ` e `result_backup_timestamp`, índice `idx_summary_processes_meeting`.
2. `repo_per_template_isolation` — lê `database/repositories/summary.rs`; asserts: existência de `get_summary_data`, `get_latest_summary_for_meeting`, `list_summaries_for_meeting`, `has_other_completed_summaries`; `WHERE meeting_id = ? AND template_id = ?` count >= 5; `template_id: &str` count >= 7.
3. `repo_delete_composite_key` — lê même arquivo; asserts: `fn delete_summary(` existe; `DELETE FROM summary_processes WHERE meeting_id = ? AND template_id = ?` presente (guarda regressão de single-column WHERE).
4. `commands_default_template_resolution` — `assert_eq!(DEFAULT_TEMPLATE_ID, "standard_meeting")` (direct symbol, sem string check); lê `commands.rs`; asserts: `pub async fn api_get_summary<R: Runtime>(`, `template_id: Option<String>`, `get_latest_summary_for_meeting` (fallback call), `unwrap_or_else(|| DEFAULT_TEMPLATE_ID.to_string())` count >= 3 (save/process/cancel).

### Verificação — cargo test

Comando:
```
$env:CARGO_TARGET_DIR="C:\Users\arman\cargo-target"; cargo test --release --features cuda --manifest-path "C:\Users\arman\OneDrive\Repositório Projetos\Personal Meetly\upstream\frontend\src-tauri\Cargo.toml" multi_template_tests
```

Output (últimas 15 linhas — por convenção do AGENTS.md, captura últimas 80):
```
running 4 tests
test summary::commands::multi_template_tests::migration_invariants ... ok
test summary::commands::multi_template_tests::repo_per_template_isolation ... ok
test summary::commands::multi_template_tests::repo_delete_composite_key ... ok
test summary::commands::multi_template_tests::commands_default_template_resolution ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 217 filtered out; finished in 0.00s

running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 filtered out; 0.00s
...
Finished `release` profile [optimized] target(s) in 2m 40s
```

Final: `test result: ok. 4 passed; 0 failed`.

### Callers quebrados

Nenhum — modificação só adiciona inline `#[cfg(test)]` mod; production code intacto.

### Pendências

Nenhuma — items 12-15 completam a verificação runnable do backend multi-template. Próximo: item 16 (frontend types `MeetingSummaryInfo`).

---

## Items 10-11: `export/commands.rs` propaga `template_id` + registra 2 commands em `lib.rs`

### O que foi feito

**Item 10 — `export/commands.rs:74`**: troca
```rust
SummaryProcessesRepository::get_summary_data(pool, &request.meeting_id)
```
por
```rust
SummaryProcessesRepository::get_summary_data(pool, &request.meeting_id, &request.template_id)
```
Comentário inline menciona decisão #11 do plano ("Export: apenas sumário ativo"). `ExportPdfRequest.template_id` já existia como campo desde a introdução do export (resolvia template-vs-builtin); agora também é a key de fetch do summary. Zero mudança de contrato do request — frontend já passa `template_id`.

**Item 11 — `frontend/src-tauri/src/lib.rs`**: registra 2 commands novos:
```rust
summary::commands::api_list_meeting_summaries,
summary::commands::api_delete_meeting_summary,
```
Após `api_cancel_summary`, dentro do bloco "Summary commands". Pattern idêntico às demais entradas.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Item 10: export busca por `(meeting, template)`, sem fallback para "latest"** | Decisão #11 do plano: apenas sumário ativo é exportado. Se o row não existe para aquele template, `summary=None` → `merge_sections` retorna placeholders "(summary not generated yet)" — UX honesta. Não há fallback implícito para evitar exportar um sumário que o usuário não está vendo. |
| **Item 10: sem log extra com `template_id`** | `info!` no header do command já loga `request={meeting_id,template_id}` — suficiente. Não duplicar. |
| **Item 11: registrar commands sem feature flag** | Multi-template é default no pós-migration. Sem feature gate, plano mais limpo. |

### Verificação — typecheck completo

Todos os callers backend propagados => executado `cargo check --release --features cuda`:
```
Compiling meetily v0.4.0 (frontend\src-tauri)
warning: profiles for the non root package will be ignored ...  (pre-existing, unrelated)
...
Finished `release` profile [optimized] target(s) in 16.09s
```

1 type inference fix necessário (`let summaries: Vec<MeetingSummaryInfo> = ...`), nenhuma outra correção.

⚠️ Note: `cargo check --release --features cuda` funciona em ambiente borificado (early "no-bundle" check). `cargo build --release --features cuda --bin meetily` (item 30) fará o link completo; esperado sem novas issues pois a única diferença é link + codegen, sem novas dependências.

### Arquivos modificados
- `frontend/src-tauri/src/export/commands.rs`: 1 linha alterada (signature fix), +3 linhas comentário.
- `frontend/src-tauri/src/lib.rs`: +2 linhas (commands registros).
- `frontend/src-tauri/src/summary/commands.rs`: type annotation pós cargo check (1-char fix).

### Estado final dos itens 1-11 (backend completo)
Todos os 11 itens do backend completos. Typecheck Rust lib passou (`cargo check --release --features cuda`). Próximos passos:

1. **Itens 12-15** (testes Rust) — optativos enquanto frontend é priorizado, mas devem ser concluídos antes do smoke test final.
2. **Itens 16-31** (frontend) — iniciam agora.

Ṕróximo passo: item 16 (frontend types: `MeetingSummaryInfo`).

---

## Items 8-9: Propagação `template_id` em `service.rs` + rename condicional

### O que foi feito

**Item 8 — propagar `template_id` em 9 call sites**:

| Local | Linha ~ | Mudança |
|---|---|---|
| `update_process_failed` (método) | ~647 | +`template_id: &str` param, propagado para `SummaryProcessesRepository::update_process_failed` |
| `update_process_failed` (caller) provider parse | 328 | +`&template_id` |
| `update_process_failed` (caller) API key NotFound | 345 | +`&template_id` |
| `update_process_failed` (caller) API key retrieve | 351 | +`&template_id` |
| `update_process_failed` (caller) CustomOpenAI no cfg | 392 | +`&template_id` |
| `update_process_failed` (caller) CustomOpenAI retrieve | 397 | +`&template_id` |
| `update_process_failed` (caller) template load | 480 | +`&template_id` |
| `update_process_failed` (caller) generic err | 635 | +`&template_id` |
| `get_summary_data` cache lookup | 518 | +`&template_id` (ainda usa o mesmo row do template sendo gerado, para cache English correto) |
| `update_process_completed` | 604 | +`&template_id` |
| `update_process_cancelled` (branch "cancelled") | 626 | +`&template_id` |

Total: 1 assinatura alterada + 10 call sites propagados. Log inclui `template_id` em todos os `info!`/`warn!`/`error!` relevantes.

**Item 9 — rename meeting title condicional** (`service.rs:583-621`):

Antes: sempre renomeava `meetings.title` quando o sumário extraía um nome via `extract_meeting_name_from_markdown`.

Agora: renomeia **apenas** se:

1. `current_title_empty == true` (título vazio — fresh recording caso clássico), **OU**
2. `is_first_completed == true` (nenhum outro row `completed` existe para este meeting, excluindo o row atual).

Caso contrário, log informativo "Skipping meeting rename..." e mantém o nome antigo.

Para a condição (2), introduzi uma helper no repo:

**Item 9 (helper) — `has_other_completed_summaries`** em `database/repositories/summary.rs`:
```rust
pub async fn has_other_completed_summaries(
    pool: &SqlitePool,
    meeting_id: &str,
    except_template_id: &str,
) -> Result<bool, sqlx::Error>
```
Query: `SELECT COUNT(*) FROM summary_processes WHERE meeting_id=? AND status='completed' AND template_id!=? → fetch_one → > 0`.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Erro de `has_other_completed_summaries` propagado, caller usa `unwrap_or(false)`** | Se DB falha transientemente, preferimos o caminho seguro (skip rename). Erro persistente: `sqlx::Error` retorna silentemente; ceiling DB-error mid-generation é raro (caminho covered por retry implícito no próximo generation). Ponytail: marca ceiling. |
| **`current_title_empty == false` + lookup failed → `false`** (não renomear) | Se não conseguimos buscar o meeting atual (raro), não queremos renomear cegamente para um título vazio ou errado. Better safe: skip rename. |
| **Helper separada em vez de inline query** | Repo-pattern consistência; função é reutilizável para futuras invariantes (por exemplo, debug "por que esse meeting tem nome fixo?"). 1 fn nova, 3 LOC, sem novo arquivo. |
| **Renomear nome via `MeetingsRepository::update_meeting_name` inalterado** | Não há mudança na persistência — só no guard que chama. Mantém a função de update só-na-tabela meetings. |

### Verificação

Script Python (`verify_helper.py`, 5 invariantes):
```
OK no-completed: False                                    # 0 completed other than self (self=PENDING)
OK different-template-completed: True False              # one completed != self → True; same → False
OK three-completed: True True True                       # 2 other completed always ≠ self
OK no-other-completed-but-other-statuses: False          # cancelled/failed don't trigger
OK different-meeting:                                    # different meeting_id unaffected
ALL HELPER CHECKS PASSED
```

### Callers propagados/quebrados
- ✅ Todos os 10 call sites do repo agora passam `template_id`.
- ✅ Na ausência de outros callers de `SummaryService::process_transcript_background`, item 8 não quebra mais nada (apenas `api_process_transcript` spawna).
- ⚠️ Item 9 ainda não RegExp casos para test manual: precisa primeiro completed summary p/ validar rename; segundo completed de template diferente NÃO deve renomear. Cobertura no smoke test.

### Arquivos modificados
- `frontend/src-tauri/src/summary/service.rs`: ~ 1050 → ~ 1080 linhas (~ +30). 1 função `update_process_failed` signature, 10 call sites, bloco de rename expandido.
- `frontend/src-tauri/src/database/repositories/summary.rs`: + 1 função `has_other_completed_summaries` (303 → ~ 325 linhas).

---

## Items 4-7: Commands + SummaryResponse + propagação template_id (`summary/commands.rs`)

### O que foi feito

**Item 4 — `api_list_meeting_summaries`** (novo command):
```rust
#[derive(Serialize, Deserialize)]
pub struct MeetingSummaryInfo {
    pub template_id: String,
    pub status: String,
    pub updated_at: String,  // RFC3339
    pub error: Option<String>,
}

#[tauri::command]
pub async fn api_list_meeting_summaries<R>(...) -> Result<Vec<MeetingSummaryInfo>, String>
```
Chama `list_summaries_for_meeting` e mapeia cada `SummaryProcess` → `MeetingSummaryInfo` (OMITINDO `result`, que pode ter KBs por row). DTO leve deixa o dropdown rápido mesmo em IPC lento.

**Item 5 — `api_delete_meeting_summary`** (novo command):
```rust
#[tauri::command]
pub async fn api_delete_meeting_summary<R>(
    state, meeting_id: String, template_id: String
) -> Result<serde_json::Value, String>
```
Sequência: `SummaryService::cancel_summary(&meeting_id)` (idempotente, sinaliza bg task) → `delete_summary(pool, meeting_id, template_id) -> bool` → retorna `{ removed, meeting_id, template_id }`.

Race-safe: mesmo que a bg task ainda corra e tente `UPDATE summary_processes SET status='cancelled' WHERE meeting_id=? AND template_id=?` depois do DELETE, o UPDATE afeta 0 rows. Validado por script (seção Verificação).

**Item 6 — propagação de `template_id`** em 4 commands existentes:

| Command | Mudança | Default se None/empty |
|---|---|---|
| `api_get_summary` | + `template_id: Option<String>` param; `Some(t)` → `get_summary_data_for_meeting(pool, meeting, t)`; `None` → `get_latest_summary_for_meeting(pool, meeting)` (fallback updated_at DESC LIMIT 1). Pre-existing callers sem arg ainda funcionam. | n/a (ramo `None` tem query dedicada) |
| `api_save_meeting_summary` | + `template_id: Option<String>`; default `DEFAULT_TEMPLATE_ID`. Não-ofuscado: `trim().is_empty()` -> default. | `"standard_meeting"` |
| `api_process_transcript` | **SEV-3 fix**: default era `"daily_standup"` (misnomer do código pré-multi-template) → alinhado para `"standard_meeting"`. | `"standard_meeting"` |
| `api_cancel_summary` | + `template_id: Option<String>`; default `DEFAULT_TEMPLATE_ID`. Token cancelamento continua keyed-by-meeting_id (serial 1/meeting); DB write precisa do template_id p/ acertar row. | `"standard_meeting"` |

**Item 7 — `SummaryResponse` ganha `template_id: String`**:
- Quando row existe: `process.template_id` (o caller que pediu via `Some(t)` recebe o mesmo que pediu; `/api_get_summary` sem arg recebe `template_id` do row mais recente — útil p/ o frontend seed `activeTemplateId`).
- Estado "idle" (nenhum row): `String::new()` (vazio), distinguível de qualquer `template_id` real. `useActiveSummaryTemplate` (item 18) usa "" como sinal p/ cair no fallback `standard_meeting`.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Novo const `DEFAULT_TEMPLATE_ID = "standard_meeting"` único**, usado por 3 commands | Plano §"Correções SEV-3": código antigo usava `"daily_standup"` para `api_process_transcript`, mas não havia consenso mesmo entre callers; alinhar tudo no `"standard_meeting"` é a escolha do plano. Const evita typo e centraliza futuras mudanças. |
| **`api_get_summary` sem `Option<String>` overload, em vez de `api_get_summary` + `api_get_latest_summary`** | 1 endpoint só. Frontend usa `invoke('api_get_summary', { meetingId })` no boot; `invoke('api_get_summary', { meetingId, templateId })` ao trocar ativo. Menos 1 binding TS. |
| **`api_save_meeting_summary` default `"standard_meeting"` não `Option`-aware** | Se frontend esquecer de passar `template_id` e usuário tiver 2 sumários (Daily + Standard), salva no Standard. Defensivo: melhor cair numa row "conhecida" do que criar fila implícita — não vai criar row nova porque `update_meeting_summary` é UPDATE only. |
| **`api_cancel_summary` token keyed-by-meeting, DB write keyed-by-(meeting,template)** | `CANCELLATION_REGISTRY` continua per-meeting (serial) — qualquer outro row PENDING do mesmo meeting também para. Razoável: você não quer concorrência para cancelar o processo live do mesmo meeting. |
| **`api_delete_meeting_summary` cancel-then-delete, não wait** | Mais simples que sync barrier entre token cancel e DB: `.await`-free race-safe graças a `WHERE` PK duplo. Ponytail: ceiling = bg task escrever cancelledishing no row deleted depois de muitos ms — UPDATE 0 rows, sem side effect. Verificado invariante no script. |
| **`MeetingSummaryInfo` snake_case fields (não camelCase)** | Plano §"Frontend Types": `interface MeetingSummaryInfo { template_id, status, updated_at, error }` — snake_case explícito. Alinha com `SummaryResponse.meeting_id`/`start`/`end` (sem rename). Mantém consistência com DTOs summary-related já no codebase. |
| **`SummaryResponse.template_id = String::new()` no estado idle, não `Option<String>`** | Evita mudar o tipo do campo (não-`Option`); frontend detecta idle por `status === "idle"` OU `template_id === ""`. Logging inclui `template_id` p/ diagnóstico rápido. |

### Verificação

Script Python (`verify_commands.py`, 5 invariantes, SQLite in-memory):
```
OK api_get_summary fallback: ('daily_standup',)               # ORDER BY updated_at DESC LIMIT 1
OK api_get_summary exact: ('legacy',)                         # WHERE meeting+template + JOIN
OK api_save default isolation: {'legacy', 'daily_standup'}    # UPDATE só toca Standard
OK api_delete race-safe: 0 phantom-update: 0                  # DELETE OK, UPDATE posterior afeta 0 rows
OK api_list: [('standard_meeting', ...), ('legacy', ...)]     # DESC order

ALL API-COMMAND CHECKS PASSED
```

### Callers propagados/quebrados
- ✅ `api_save_meeting_summary`, `api_get_summary`, `api_cancel_summary`, `api_process_transcript` agora consistentes com `template_id`.
- ⚠️ Frontend callers quebram até item 26 (`useSummaryGeneration`) propagar `activeTemplateId`. Esperado pela linearidade.
- ✅ Items 8-9 propagam dentro de `service.rs` (chamado por `api_process_transcript` background).

### Arquivos modificados
- `frontend/src-tauri/src/summary/commands.rs` (~ 470 → ~ 620 linhas): + DTO `MeetingSummaryInfo`, + 2 commands, + `DEFAULT_TEMPLATE_ID` const, propagação em 4 commands, `SummaryResponse.template_id` field.

---

## Item 3: Refatorar `database/repositories/summary.rs`

### O que foi feito
Refatorado `frontend/src-tauri/src/database/repositories/summary.rs`:

**Funções modificadas** (todas ganharam `template_id: &str` param + `WHERE ... AND template_id = ?` + `ON CONFLICT(meeting_id, template_id)` + INSERT com `template_id`):

| Função | Mudança |
|---|---|
| `get_summary_data` | +`template_id`, `WHERE meeting_id=? AND template_id=?` |
| `get_summary_data_for_meeting` | +`template_id`, JOIN + filter template |
| `update_meeting_summary` | +`template_id`, `WHERE` duplo, `UPDATE meetings` é por meeting (não template) |
| `create_or_reset_process` | +`template_id`, INSERT inclui `template_id`, `ON CONFLICT(meeting_id, template_id)` |
| `update_process_completed` | +`template_id`, `WHERE` duplo |
| `update_process_failed` | +`template_id`, `WHERE` duplo (COALESCE backup restore intacto) |
| `update_process_cancelled` | +`template_id`, `WHERE` duplo |

**Funções novas** (2):

- `get_latest_summary_for_meeting(pool, meeting_id)` — `ORDER BY updated_at DESC LIMIT 1`. Usada por `api_get_summary` quando caller não especifica `template_id` (fallback automático).
- `list_summaries_for_meeting(pool, meeting_id)` — lista todos os sumários do meeting, ordenados por `updated_at DESC`.
- `delete_summary(pool, meeting_id, template_id) -> bool` — remove 1 row, retorna true se removeu. Caller (`api_delete_meeting_summary`) é responsável por cancelar PENDING antes.

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **`list_summaries_for_meeting` retorna `Vec<SummaryProcess>`** (struct completo) | Simplicidade: 1 query, 1 tipo. Embora plano original sugerisse DTO leve sem `result`, a típica tố de rows é 1-5 por meeting (raros >10), e reusar `SummaryProcess` evita novo struct uma-trip. Se tabela crescer absurdamente, refactor trivial. **Tradeoff aceitável** — Ponytail: ceiling ~100 rows/meeting não observado na prática. |
| **`update_meeting_summary` ainda faz `UPDATE meetings SET updated_at` após row update** | `meetings.updated_at` é por-reunião (última atividade), não por-template. Manter semantics original. |
| **`get_summary_data_for_meeting` JOIN com `transcript_chunks` mantido** | Antes servia para validar que transcript existe antes de retornar summary pré-gerada. Continua válido: ainda filtra por template agora. |
| **`delete_summary` não prevê PENDING/automatic cancel** | Separação de concerns: repo = data access, commands = orquestração (incl. cancel via `CANCELLATION_REGISTRY`). Forçar caller a cancelar primeiro evita race entre DELETE e tx drop salvando resultado em row já deletada. |
| **Não adicionei JOIN/trigger em cascata para FK** | Já feito na migration (`ON DELETE CASCADE` em `meetings`). Delete manual de summary é por (meeting,template), FK não relê aqui. |
| **`get_latest_summary_for_meeting` novo, em vez de sobrecarga em `get_summary_data`** | Separação: caller que sabe qual template quer usa `get_summary_data(meeting, template)`; caller que não sabe usa `get_latest_summary_for_meeting(meeting)`. Mais legível que `Option<template_id>`. |

### Trade-offs

- **Pro**: API limpa — cada função tem 1 propósito, assinaturas explicitas.
- **Con**: 8 funções agora exigem `template_id` explícito — 9 callers (commands/service/export) quebram até itens 5-10 serem concluídos. Esperado pela linearidade do plano.
- **Pro**: `get_latest_summary_for_meeting` elimina necessidade de frontend pré-selecionar template antes do primeiro fetch.
- **Con**: `list_summaries_for_meeting` traz `result` completo (pode ser kilobytes por row). Em UI com ~5 summaries isso é irrelevante; se escalar para ~50+ uma versão DTO seria justificada. Mark: ponytail ceiling = ~50 templates/meeting (=ultra-raro usuário power).

### Verificação

Script Python (`verify_repo_queries.py`) rodou 5 invariantes em SQLite in-memory + migration:

```
OK get_latest: ('daily_standup',)              # ORDER BY updated_at DESC LIMIT 1
OK list: [('daily_standup',), ('standard_meeting',), ('legacy',)]  # DESC order
OK delete: 1 remaining: 2                       # DELETE WHERE meeting+template
OK delete-nonexistent: 0                        # rows_affected = 0
OK where-isolation: ('completed',)              # UPDATE só afeta template alvo

ALL REPO-SIGNATURE CHECKS PASSED
```

Bind count audit (manual) — todas as queries têm `?` count igual ao count de `.bind()`:
- `get_summary_data`: 2 binds / 2 `?`
- `get_latest_summary_for_meeting`: 1 / 1
- `list_summaries_for_meeting`: 1 / 1
- `update_meeting_summary`: 6 binds (result, now, meeting, template, now, meeting) / 6 `?` (4 na 1ª query + 2 na 2ª)
- `get_summary_data_for_meeting`: 2 / 2
- `create_or_reset_process`: 5 binds / 5 `?`
- `update_process_completed`: 7 / 7
- `update_process_failed`: 5 / 5
- `update_process_cancelled`: 4 / 4
- `delete_summary`: 2 / 2

### Callers que quebram (serão corrigidos nos itens 5-10)
- `summary/commands.rs:87` — `update_meeting_summary(pool, &meeting_id, &summary)` → +`template_id`
- `summary/commands.rs:245` — `get_summary_data_for_meeting(pool, &meeting_id)` → +`template_id`
- `summary/commands.rs:367` — `create_or_reset_process(&pool, &m_id)` → +`template_id`
- `summary/commands.rs:435` — `update_process_cancelled(pool, &meeting_id)` → +`template_id`
- `summary/service.rs:518` — `get_summary_data(&pool, &meeting_id)` → +`template_id`
- `summary/service.rs:604` — `update_process_completed(...)` → +`template_id` (item 8)
- `summary/service.rs:626` — `update_process_cancelled(&pool, &meeting_id)` → +`template_id`
- `summary/service.rs:653` — `update_process_failed(pool, meeting_id, error_msg)` → +`template_id` (item 8)
- `export/commands.rs:74` — `get_summary_data(pool, &request.meeting_id)` → +`template_id` (item 10)

### Arquivos modificados
- `frontend/src-tauri/src/database/repositories/summary.rs` (refatoração total: 221 → ~260 linhas)

### Pendências transferidas
- Typecheck completo `cargo check`/`build` — após item 11 (todos os callers do backend).
- Testes Rust #1-#4 (itens 12-15) cobrirão o repo.

---

## Item 2: `template_id` em `SummaryProcess` (`database/models.rs`)

### O que foi feito
Adicionado campo `pub template_id: String` ao struct `SummaryProcess` em `frontend/src-tauri/src/database/models.rs:52`, posicionado logo após `meeting_id` (mesma ordem do PK composto no schema da migration).

**Diff**:
```rust
 pub struct SummaryProcess {
     pub meeting_id: String,
+    pub template_id: String,
     pub status: String,
     // ... inalterado
 }
```

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **`String` (não `Option<String>`)** | O schema da migration declara `template_id TEXT NOT NULL` e backfill preenche todas as rows existentes com `'legacy'`. Não há caso de NULL — Option seria uma mentira de tipo. |
| **Posição após `meeting_id`** | Espelha a ordem do PK `(meeting_id, template_id)` no schema. `FromRow` usa ordem posicional do `SELECT *`, e manter alinhado ao schema facilita leitura e debugging. |
| **Não atualizar callers agora** | Item 3 (refatorar `summary.rs`) e itens 5-9 (propagar em commands/service/export) cobrem todas as mudanças de callers. Adicionar o campo agora quebra compile apenas nos sites que precisam ser editados a seguir — é a check-list do plano trabalhando como guia. |

### Trade-offs

- **Pro**: Mudança atômica de 1 linha no model — alteração mínima possível, YAGNI aplicado (sem `Option`, sem `Default`, sem helpers).
- **Con**: O app quebra de compilar imediatamente (queries `WHERE meeting_id = ?` em `summary.rs` fazem `SELECT *` que agora retorna template_id; `ON CONFLICT(meeting_id)` falha pois a PK mudou). **Esperado**: item 3 resolve `summary.rs`, itens 5-9 resolvem comandos/service. A quebra é o reflito do trabalho linear planejado.
- **Typecheck não executado**: `cargo check` no ambiente falha em `whisper-rs-sys 0.11.1` (build script CUDA/nvcc incompatible em modo debug) — erro pré-existente ao meu toque. O typecheck real virá no `cargo build --release --features cuda` (comando do AGENTS) após item 11 concluir todos os call sites do backend.

### Verificação de impacto (estática)

Grep por  construção literal `SummaryProcess {` retorna 0 matches — todos callers usam `query_as` (`FromRow`) ou apenas `&SummaryProcess` como referência (`export/commands.rs:404,419`). Logo, adicionar campo:

- ✅ `query_as::<_, SummaryProcess>` automático via `FromRow`.
- ❌ `repositories/summary.rs` — 6 queries com `WHERE meeting_id = ?` único + `ON CONFLICT(meeting_id)` — item 3.
- ❌ `summary/commands.rs:87,245,367,435` — callers de repo functions que ganharão `template_id` param — item 5-6.
- ❌ `summary/service.rs:518,604,626,653` — idem — item 8.
- ❌ `export/commands.rs:74` — `get_summary_data(pool, &request.meeting_id)` — item 10.
- ✅ `export/commands.rs:404,419` — apenas lê campos do `&SummaryProcess` via `Option<&SummaryProcess>`, não quebra.

### Arquivos modificados
- `frontend/src-tauri/src/database/models.rs` (1 linha adicionada, linha 52)

### Pendências transferidas
- Typecheck/cargo build completo — adiado para pós item 11.
- Atualização do `.sqlx/` offline cache (se existir) — durante o build release.

---

## Item 1: Migration `20260721000001_multi_template_summaries.sql`

### O que foi feito
Criado `frontend/src-tauri/migrations/20260721000001_multi_template_summaries.sql` que reconstrói a tabela `summary_processes` para suportar **N sumários por meeting**, keyeados por `(meeting_id, template_id)` em vez do PK único `meeting_id` anterior.

**Passos**:
1. Inspecionei o schema atual e migrations relacionadas (`20250916100000` initial, `20251101000000` backup cols, `20260721000000` meeting folders) para alinhar timestamps e convenções.
2. Confirmei que `repositories/summary.rs` tem 4 queries que usam `result_backup`/`result_backup_timestamp` — sem essas colunas no novo schema, build quebraria em runtime (SEV-1 #1 do review).
3. Validei o pattern SQLite de rebuild (`PRAGMA foreign_keys=off` + CREATE new + INSERT copy + DROP old + RENAME) duplicando a abordagem da migration `20250920155811` (openrouter) já no repo.
4. Escrevi a migration preservando todas as colunas (incl. `result_backup*`, `metadata`) + backfill `template_id='legacy'` para rows existentes.
5. Escrevi script de verificação em Python (`C:\Users\arman\AppData\Local\Temp\opencode\verify_migration.py`) rodando a migration em SQLite in-memory, seed pré-migration, e checando 4 invariantes. **Todos passaram.**

### Decisões e motivos

| Decisão | Motivo |
|---|---|
| **Rebuild da tabela** (CREATE + INSERT + DROP + RENAME) | SQLite não suporta `ALTER TABLE ... ALTER PRIMARY KEY`. Pattern já usado em `20250920155811_add_openrouter_api_key.sql`. |
| **PK composta `(meeting_id, template_id)`, sem surrogate `id`** | YAGNI. Queries são sempre por (meeting, template). Surrogate `id` adicionaria uma coluna nunca consultada e um índice extra sem benefício. |
| **Manter `result_backup` + `result_backup_timestamp`** | 4 queries em `summary.rs` (linhas 102, 135, 170, 204) dependem destas colunas. Remover = build quebra. Plano original as omitia — corrigido. |
| **Backfill com sentinela `'legacy'`** | Row pré-migration ganha um `template_id` honesto sobre a origem. Frontend renderiza como "Summary (original)" e não confunde com templates reais. `'legacy'` é string reservada — o loader de templates nunca gera id `'legacy'`, garantindo zero colisão com templates reais (built-in, bundled ou custom). |
| **Índice `idx_summary_processes_meeting`** | A query `list_summaries_for_meeting(meeting_id)` (a ser adicionada no próximo item) fará `WHERE meeting_id = ?`. PK composta com `meeting_id` como primeira coluna já cria índice que atende essa query; **porém** deixei o índice explícito para (a) documentar intenção da query frequentemente usada e (b) servir SELECTs que não incluem `template_id`. |
| **Sem índice separado em `template_id` sozinho** | Não há query por `template_id` sem `meeting_id` no plano atual. YAGNI — se surgir, `CREATE INDEX` é uma migration trivial. |
| **`PRAGMA foreign_keys=off`/`on` envolvendo o rebuild** | Necessário porque `DROP TABLE` em uma tabela referenciada via FK quebraria constraints durante a transação. Reproduz exatamente o pattern da migration `20250920155811`. |
| **Script de verificação em Python, não Rust** | sqlite3 CLI ausente no ambiente. Python 3.12 presente. O teste de regression Rust #1 (item 12 da todo list) cobrirá o mesmo caminho via `sqlx::Migrator` em in-memory SQLite — Python é apenas rápido sanity check durante o desenvolvimento da migration. |

### Trade-offs

- **Pro**: DDL único, idempotente (`CREATE TABLE IF NOT EXISTS` + `INSERT ... SELECT` que é no-op se tabela velha já foi dropada). Aplica em DBs novos e existentes sem bifurcação.
- **Con**: Rebuild da tabela locks `summary_processes` durante a operação. Em DBs grandes isso poderia ser percebido, mas a tabela cresce linear por meeting+template (típico: dezenas a centenas de rows), e a operação é single-statement no contexto do `sqlx::Migrator`. Aceitável.
- **Pro**: Sentinela `'legacy'` permite ao frontend distinguir "sumário original" de sumários pós-migração sem coluna booleana extra.
- **Con**: Usuários que já tinham sumário e geram novo com template real (ex.: `standard_meeting`) terminarão com 2 rows para o mesmo meeting — o legacy permanece inerte. **Isto é desejável**: a decisão #14 do plano é "regenerate legacy cria NOVA row, legacy intacto", e a UX do dropdown mostra ambos como opções selecionáveis.

### Verificação

Script Python (`verify_migration.py`) executou 4 invariantes contra SQLite in-memory:

```
OK backfill: ('m1', 'legacy', 'completed', 'OLD_RESULT', 'BACKUP_OLD', '2024-01-01T00:00:30Z')
OK isolation: [('daily_standup', 'PENDING'), ('legacy', 'completed'), ('standard_meeting', 'PENDING')]
OK upsert: ('completed', 'STANDARD_RESULT', None, '2024-01-02T00:00:00Z')
OK cascade
ALL MIGRATION CHECKS PASSED
```

- **Backfill**: row pré-migration → `template_id='legacy'` + `result` e `result_backup` preservados.
- **Isolation**: mesma `meeting_id='m1'` com 3 templates distintos (`daily_standup`, `legacy`, `standard_meeting`) coexistem.
- **Upsert**: `ON CONFLICT(meeting_id, template_id) DO UPDATE` sobrescreve status/result corretamente e preserva `result_backup` baseado em `summary_processes.result` (que era NULL aqui).
- **Cascade**: `DELETE FROM meetings WHERE id='m1'` remove as 3 rows (FK ON DELETE CASCADE honrado mesmo após rebuild).

Teste Rust equivalente (item 12 da todo list) re-validará o mesmo caminho no migrator oficial.

### Pendências
- O build quebrará até o item 2 concluir (model `SummaryProcess` precisa do campo `template_id`) e item 3 refatorar as queries usando `WHERE meeting_id = ?` único. Esta migration sozinha não compila o app — é dependência linear no plano.
- `sqlx` cache de schema offline: se houver `sqlx-data.json` ou `.sqlx/`, será regenerado no build seguinte. Não verifiquei agora para evitar churn prematuro.

### Arquivos modificados
- `frontend/src-tauri/migrations/20260721000001_multi_template_summaries.sql` (novo, 47 linhas)

### Artefatos de teste (não-commit)
- `C:\Users\arman\AppData\Local\Temp\opencode\verify_migration.py` — script de sanity check isolado, fora do repo.
