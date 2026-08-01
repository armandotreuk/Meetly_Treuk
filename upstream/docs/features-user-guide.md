# Meeting Dates — How to Use & Smoke Tests

Each meeting now shows its creation date so you can find a specific meeting in a long list without opening it.

### Where the date appears

- **Sidebar list** — under each meeting title, in a short format (e.g. `Jul 31, 2:30 PM`).
- **Meeting-details header** — under the editable title, in a full format (e.g. `July 31, 2026 at 2:30 PM`).

The intro row (`+ New Call`) does not show a date — it isn't a real meeting.

### Locale

The date is formatted using your browser's default locale, so it adapts to your system language (e.g. `31 de julho de 2026` in pt-BR, `31 juillet 2026` in fr-FR).

### Smoke tests

| # | Test | Expected |
|---|------|----------|
| 1 | Open a meeting | Full-format date appears under the title in the meeting-details header |
| 2 | Edit the meeting title (pencil icon) | The date stays the same (only the title changes) |
| 3 | Look at the sidebar | Short-format date appears under each meeting title |
| 4 | Switch the OS language to pt-BR (or another locale) and re-open the app | Date format changes accordingly |
| 5 | Create a brand-new meeting and look at the sidebar | Short date appears immediately under the title |