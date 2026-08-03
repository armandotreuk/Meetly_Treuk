# Pastas Lógicas Multi-nível — Plano Decidido

> Artefato gerado para a feature de sistema de pastas para organizar artefatos de meetings.
> Decisões confirmadas em 2026-07-21. Implementação iniciada em paralelo.

---

## 1. Memorial de Decisões

| # | Decisão | Status |
|---|---|---|
| 1 | Pastas **lógicas** (só no DB; `folder_path` no disco intocado) | ✅ Confirmado |
| 2 | Estrutura **multi-nível** (árvore recursiva) | ✅ Confirmado |
| 3 | Sidebar abre mostrando **só pastas**; meetings sem pasta em "Sem pasta" | ✅ Confirmado |
| 4 | Mover via **drag-and-drop + menu** "Mover para..." | ✅ Confirmado |
| 5 | Deletar pasta com meetings → meetings vão p/ **"Sem pasta"** | ✅ Confirmado |
| 6 | Meeting novo sempre cai em **"Sem pasta"** (zero mudança no fluxo de gravação) | ✅ Confirmado |
| 7 | `expandedFolders` **só em memória** (reseta entre sessões) | ✅ Confirmado |
| 8 | "Sem pasta" = seção especial, topo, não renomeável/deletável | ✅ Confirmado |
| 9 | Pastas também se movem (drag + menu) — cycle detection no backend | ✅ Confirmado |
| 10 | Nomes duplicados permitidos (sem `UNIQUE`) | ✅ Confirmado |
| 11 | Deletar pasta → subpastas em cascata; todos os meetings afetados → "Sem pasta" | ✅ Confirmado |
| 12 | Menu "Mover para..." = **modal próprio** (escalável p/ níveis fundos) | ✅ Confirmado |
| 13 | Drop em pasta colapsada cai na própria pasta (não em subpasta oculta) | ✅ Confirmado |
| 14 | Empty states incluídos (pasta vazia + "Sem pasta" vazia) | ✅ Confirmado |
| 15 | FK declarada com `ON DELETE SET NULL` no schema (cascade real é Rust; FKs não enforced) | ✅ Confirmado |
| 16 | Estado **otimista** no move (single-user dogfooding) | ✅ Confirmado |
| 17 | `sort_order` **cortado** da migration (YAGNI) | ✅ Confirmado |
| 18 | Hook `useSidebarTree` **isolado** (testável) | ✅ Confirmado |
| 19 | Busca global = **flat** (não preserva árvore — 90% do valor, 40% do código) | ✅ Confirmado |
| 20 | Auto-expand-on-hover durante drag = **não** (YAGNI) | ✅ Confirmado |

---

## 2. Plano técnico (completo, pós-decisões)

### Fase 1 — Backend (Rust)

**1.1 Migration `20260721000000_add_meeting_folders.sql`**
```sql
CREATE TABLE IF NOT EXISTS meeting_folders (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT REFERENCES meeting_folders(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL
);
ALTER TABLE meetings ADD COLUMN folder_id TEXT REFERENCES meeting_folders(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_meetings_folder_id ON meetings(folder_id);
CREATE INDEX IF NOT EXISTS idx_folders_parent ON meeting_folders(parent_id);
```
Sem `sort_order` (cortado). FKs declaradas com `ON DELETE SET NULL` (documentação); cascade real é explícito em Rust (FKs não enforced; padrão `delete_meeting_with_transaction`).

**1.2 `src-tauri/src/database/models.rs`**
- `MeetingModel` += `folder_id: Option<String>`
- Novo `MeetingFolderModel { id, name, parent_id: Option<String>, created_at: DateTimeUtc }`

**1.3 Novo `src-tauri/src/database/repositories/folder.rs`** (~160 ln + 2 testes)
- `get_all` → `ORDER BY name ASC`
- `create(name, parent_id)` → id `format!("folder-{}", Uuid::new_v4())`
- `rename(id, name)`
- `move_folder(id, new_parent_id: Option<&str>)` — cycle detection via recursive CTE subindo ancestrais; rejeita `new_parent_id == id` e ancestrais:
  ```sql
  WITH RECURSIVE ancestors(id) AS (
      SELECT parent_id FROM meeting_folders WHERE id = ?1
      UNION ALL
      SELECT f.parent_id FROM meeting_folders f
      JOIN ancestors a ON f.id = a.id
      WHERE f.parent_id IS NOT NULL
  )
  SELECT 1 FROM ancestors WHERE id = ?2
  ```
- `delete_with_cascade(id)` — transação: recursive CTE coleta `id` + descendentes → `UPDATE meetings SET folder_id = NULL WHERE folder_id IN (subtree)` → `DELETE FROM meeting_folders WHERE id IN (subtree)`. Padrão `delete_meeting_with_transaction`.
- `set_meeting_folder(meeting_id, folder_id: Option<&str>)` — valida existência de `folder_id`.

**1.4 `database/repositories/meeting.rs`**: zero mudança de SQL (`SELECT *` pega `folder_id`).

**1.5 `src-tauri/src/api/api.rs`**: `Meeting` struct += `folder_id: Option<String>`; mapear em `api_get_meetings`.

**1.6 Novo `src-tauri/src/api/folders.rs`** — 6 commands: `api_get_folders`, `api_create_folder`, `api_rename_folder`, `api_move_folder`, `api_delete_folder`, `api_set_meeting_folder`. Cycle → mensagem p/ toast. Registrar em `lib.rs`.

**1.7 Check**: `cargo check` + 2 unit tests (cycle A→B→A erro; deletar pai → meetings dos filhos NULL).

### Fase 2 — Frontend

**2.1 `types`**: `MeetingFolder`; `CurrentMeeting` += `folder_id?: string | null`.

**2.2 `SidebarProvider.tsx`**: estado `folders` + `fetchFolders`; ações otimistas `createFolder`/`renameFolder`/`moveFolder`/`deleteFolder`/`moveMeetingToFolder`.

**2.3 Novo hook `src/hooks/useSidebarTree.ts`** (isolado, testável): constrói árvore recursiva de `folders + meetings`; raiz = `unfiled` (meetings `folder_id == null`) + pastas `parent_id == null` como irmãs abaixo. Busca **flat**: retorna lista filtrada ignorando árvore (mais simples).

**2.4 Novo `Sidebar/FolderTreeItem.tsx`** (~120 ln): recursivo; **drag source E drop target** (`application/x-meetily` payload `{kind, id}`); menu "..." (Nova subpasta / Renomear / **Mover para... modal** / Excluir com `ConfirmationModal` mostrando "subpastas vão junto"); highlight visual em `onDragOver`; empty state "Arraste meetings aqui ou use Move to...".

**2.5 Novo `Sidebar/MeetingTreeItem.tsx`** (~60 ln): draggable; click navega; menu "..." → "Mover para..." abre modal.

**2.6 Novo `Sidebar/MoveToFolderModal.tsx`** (~80 ln): modal Radix já existe; árvore indentada de pastas + "Sem pasta"; seleciona e confirma.

**2.7 `Sidebar/index.tsx`** (refatoração): `renderItem` → dispatch (`unfiled`/`folder`/`meeting`); "Sem pasta" drop target (meeting→desanexa, folder→raiz); `expandedFolders` default `{"unfiled"}`; uso de `useSidebarTree`; botão "+ Nova Pasta".

**2.8 Check**: `npx tsc --noEmit`.

### Fase 3 — Build, install, verificação manual

**Build**: `npx --no-install @tauri-apps/cli build --no-bundle -- --features cuda` (com `CARGO_TARGET_DIR=C:\Users\arman\cargo-target`).

**Install**: copiar `cargo-target\release\meetily.exe` → `AppData\Local\meetily\meetily.exe`.

**Verificação manual (8 passos):**
1. Criar "Trabalho" → subpasta "Projeto X"; renomear; duplicata de nome em níveis diferentes funciona.
2. Arrastar meeting → "Projeto X"; arrastar de volta p/ "Sem pasta".
3. Arrastar "Projeto X" p/ raiz; arrastar "Trabalho" p/ dentro de "Projeto X" → **erro (ciclo)** com toast.
4. Menu "Mover para..." em pasta e em meeting (caminho por teclado).
5. Deletar "Trabalho" contendo subpasta + meetings → tudo em "Sem pasta".
6. Gravar meeting novo → cai em "Sem pasta".
7. Busca filtra meetings (flat) e mostra resultados sem dependência de árvore.
8. `folder_path` no disco **intocado** — abrir pasta do meeting continua funcionando.

### Arquivos envolvidos (12 editados/criados, ~535 ln)

| Arquivo | Ação |
|---|---|
| `frontend/src-tauri/migrations/20260721000000_add_meeting_folders.sql` | novo |
| `frontend/src-tauri/src/database/models.rs` | editar |
| `frontend/src-tauri/src/database/repositories/folder.rs` | novo (~160 ln + 2 testes) |
| `frontend/src-tauri/src/database/repositories/mod.rs` | editar (1 linha) |
| `frontend/src-tauri/src/api/api.rs` | editar (struct Meeting + map) |
| `frontend/src-tauri/src/api/folders.rs` | novo (~90 ln) |
| `frontend/src-tauri/src/lib.rs` | editar (6 commands) |
| `frontend/src/types` (onde `CurrentMeeting`/`Meeting` vivem) | editar |
| `frontend/src/components/Sidebar/SidebarProvider.tsx` | editar (~80 ln) |
| `frontend/src/components/Sidebar/FolderTreeItem.tsx` | novo (~120 ln) |
| `frontend/src/components/Sidebar/MeetingTreeItem.tsx` | novo (~60 ln) |
| `frontend/src/components/Sidebar/MoveToFolderModal.tsx` | novo (~80 ln) |
| `frontend/src/hooks/useSidebarTree.ts` | novo (~40 ln) |
| `frontend/src/components/Sidebar/index.tsx` | editar (renderItem + busca + drop zones) |

Zero dependências novas. Zero mudança em disco/`folder_path`.

---

## 3. Próximos passos NÃO priorizados (backlog pós-implementação)

| # | Item | Justificativa de não priorizar |
|---|---|---|
| 1 | "Expandir tudo" + colapsar tudo | Útil só em árvores profundas; usuário dogfooding ainda não chegou lá |
| 2 | Filtro por pasta-raiz ("ver só Trabalho/*") | Hoje busca flat cobre acidentalmente |
| 3 | **Sincronizar estrutura de pastas no disco** (mirrored folders) | Resolve deslocamento Expectativa-vs-realidade ao anexar `audio.mp4` em email; faria em exportação, não em tempo real |
| 4 | Reordenação manual por drag (setar `sort_order`) | Hoje `ORDER BY name ASC`; reintroduz `sort_order` quando surgir necessidade |
| 5 | Persistência de `expandedFolders` entre sessões (localStorage) | In-memory suficiente para sessão única |
| 6 | Auto-expand-on-hover durante drag | Evitaria sensação de drop em colapsado poder cair em subpasta oculta — vira orgânico |
| 7 | Busca preservando árvore (auto-expande pais de match) | Hoje flat; melhora só em caso de muitos matches repetidos em mesma pasta |
| 8 | Atalho de teclado para "Nova Pasta" | Faltante por ora; cobriria criar subpasta rapidamente |
| 9 | Bulk move (selecionar múltiplas meetings → mover) | Operações hoje são 1-a-1 |
| 10 | Filtro "ver só subpastas/filhos diretos" (não recursivo) | YAGNI sem medida de uso |
| 11 | Compartilhar pastas entre dispositivos via sync backend | Offline-first; não há sync hoje |
| 12 | Exportar árvore de pastas como JSON/CSV p/ backup manual | Sem pedido explícito |

---

## 4. Riscos técnicos identificados (mitigados no plano)

1. **FK pragma desativado** → cascade explícito em Rust (não depende de FK enforcement). Padrão já usado em `delete_meeting_with_transaction`.
2. **Race condition no estado otimista** → aceito (single-user dogfooding; refetch em erro).
3. **Cycle em CTE** → testes unitários cobrem (A→B→A rejeita).
4. **Conflicto `dataTransfer` × `usePanelResize` mousedown** → verificação manual na Fase 3.
5. **Sidebar 871 ln refatorada** → hook `useSidebarTree` isolado mantém `index.tsx` enxuto e testável.
6. **`ALTER TABLE ADD COLUMN` em SQLite** → default NULL é o comportamento; meetings existentes ficam automaticamente em "Sem pasta". Zero migração de dados.

---

## 5. Estado de execução

- [ ] Fase 1.1 Migration
- [ ] Fase 1.2 models.rs
- [ ] Fase 1.3 folder.rs + testes
- [ ] Fase 1.4-1.6 commands + lib.rs
- [ ] Fase 1.7 cargo check + testes
- [ ] Fase 2.1-2.2 types + SidebarProvider
- [ ] Fase 2.3 useSidebarTree
- [ ] Fase 2.4-2.6 FolderTreeItem + MeetingTreeItem + MoveToFolderModal
- [ ] Fase 2.7 Sidebar/index.tsx refatoração
- [ ] Fase 2.8 tsc --noEmit
- [ ] Fase 3 Build + install
- [ ] Fase 3 Verificação manual (8 passos)
