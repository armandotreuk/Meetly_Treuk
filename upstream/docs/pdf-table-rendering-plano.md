# PDF Table Rendering Fix — Plano (Sprint G, Items 32-38)

> Referência: `upstream/docs/multi-template-summaries-progress.md` (entrada "Sprint G" no topo).

## Goal

Exportar PDF de reuniões com seção em formato tabela (`standard_meeting` → "Action Items", `project_sync` → "Milestones & Status" / "Top Risks" / etc.) renderizando como **grid real** (colunas alinhadas, bordas, células com wrap interno), não como texto quebrado com `│` enterrado.

## Root cause

`render_markdown_table` (`frontend/src-tauri/src/export/pdf.rs:497-530`) junta as células com `"  │  "` e passa a string inteira para `write_wrapped`. Para tabelas com 5 colunas e células longas (ex: "Reference Transcript Segment" com frase completa), a linha concatenada excede `CONTENT_WIDTH_MM` (170mm). `write_wrapped` quebra em 3-4 linhas físicas; separadores `│` ficam dentro de parágrafos quebrados. Tabela deixa de parecer tabela.

Bug secundário: `looks_like_structured_list` (`pdf.rs:431-441`) não detecta `- [ ] Task - [[Owner]] - Due: Date` (formato checkbox usado por outputs mais antigos / outras LLMs). Cai no bullet-list fallback.

**LLM output está correto** — verificado diretamente em `meeting_minutes.sqlite`. Tabelas em pipe-table válido são armazenadas; o renderer é o único defeito.

## Decisões travadas

| # | Decisão | Escolha |
|---|---|---|
| 1 | Layout de tabela | Grid real (colunas calculadas, bordas, wrap por célula) |
| 2 | Paginação | Por linha: linha que não cabe → page break com header re-renderizado |
| 3 | Detecção de checklist | Adicionada: `- [ ]` + `[[Owner]]` + `Due:` parseados como grid 2-3 colunas |
| 4 | Header synthesis | Se LLM esquece header e `item_format` é pipe → usar 1ª linha de `item_format` como header |
| 5 | Dependencies | Nenhuma nova. `printpdf` já tem `Line` + retângulos |
| 6 | Zebra-striping | Não (YAGNI) |
| 7 | Coluna < 18mm | Floor aplicado; se 8+ colunas, log warning + fallback ao renderer atual (ponytail ceiling) |

## Sequência de implementação

```
32 (constants + write_wrapped_at)
 ├─→ 33 (render_table_grid: parse + col widths + render + pagination)
 │    └─→ 34 (wire em render_list, passa item_format)
 └─→ 35 (checkbox-list detection, independente)
       └─→ 36 (6 testes novos)
              └─→ 37 (cargo test)
                     └─→ 38 (build + install + visual verify)
```

## Itens

| # | Item | LOC est. |
|---|---|---|
| 32 | Layout constants + `write_wrapped_at` helper em `pdf.rs` | ~30 |
| 33 | `render_table_grid` (parse rows, col widths, render, pagination) | ~150 |
| 34 | Wire `render_table_grid` em `render_list` (substitui `render_markdown_table`); passa `item_format` | ~15 |
| 35 | `- [ ]` checklist detection + parser `[[Owner]]` / `- Due:` | ~30 |
| 36 | 6 testes (grid, wrap, pagination, header synthesis, checkbox, regression) | ~120 |
| 37 | `cargo test -p meetily --lib` (full suite + novos) | — |
| 38 | Build release CUDA + install + re-export meeting `d934686d-...` + visual verify | — |

## Riscos / ceilings

- **8+ colunas**: floor de `TABLE_MIN_COL_WIDTH_MM` (18mm) pode exceder `CONTENT_WIDTH_MM`. `ponytail:` log warning + fallback ao renderer atual (text-com-`│`). Upgrade path: aumentar para 6+ colunas suportadas via scaling proporcional, ou transpor a tabela.
- **Wrap de célula muito longa**: cada célula pode gerar N linhas wrapped. Row height = max(wrapped lines) × LINE_HEIGHT_BODY. `ensure_space` antes de cada row. Row que não cabe → page break.
- **Header repetition**: simples, ~10 LOC. Bottom rule + re-render do header no topo da próxima página.
- **Codificação**: `printpdf` builtin Helvetica não suporta `ç`, `ã`, `é` — código já carrega DejaVu Sans via `FontSet::load`. Grid usa as mesmas fonts. Sem regressão.

## Arquivos

**Modificar** (1):
- `frontend/src-tauri/src/export/pdf.rs` — adicionar layout constants, `write_wrapped_at`, `render_table_grid`; modificar `render_list` para receber `item_format`; estender `looks_like_structured_list` e `render_structured_list_as_table`; adicionar 6 testes.

**Não modificar** (verificado):
- `frontend/src-tauri/src/export/commands.rs` (extrai `result.markdown`, passa para `SectionContent.content` — funciona)
- `frontend/src-tauri/src/summary/templates/types.rs` (prompt já correto)
- `frontend/src-tauri/src/summary/processor.rs` (LLM call já correto)
- `frontend/src/lib/blocknote-markdown.ts` (BlockNote não tem table block; markdown armazenado é o do LLM direto)

## Verificação

1. **Unit tests** (item 37): todos 222 existentes + 6 novos = 228 passing.
2. **Build** (item 38): `npx --no-install @tauri-apps/cli build --no-bundle -- --features cuda` exit 0.
3. **Visual** (item 38): re-export meeting `d934686d-53e1-4a59-95fc-0c057aeda8b0` (data real, 5-col Action Items com "Reference Transcript Segment"). Confirmar:
   - Grid com bordas visíveis
   - Colunas alinhadas verticalmente
   - Células longas quebram dentro de sua largura de coluna
   - Header em negrito
   - Footer em todas as páginas
   - Sem regressão em seções paragraph/string

## Fora de escopo

- DOCX export (`docx.rs:15` é stub).
- Templates `psychiatric_session` e `sales_marketing_client_call` — não usam `item_format` pipe, continuam com bullet-list. Sprint G não muda comportamento deles.
- Tabelas BlockNote nativas (BlockNote core 0.36.0 não tem table block; sem mudança no frontend).
