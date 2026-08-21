// ponytail: groundwork — a minimal string-lookup primitive. No library; the app is pre-i18n. Once the team decides on app-wide i18n (next-intl vs react-i18next vs a homegrown solution), swap this file for the chosen provider. Keeping t() shape stable makes the migration mechanical.
// ponytail: Sidebar still has hardcoded PT-BR strings (Sidebar/index.tsx:775, 985, 1024; FolderTreeItem.tsx:252). Notes + chat are now EN-via-dict. The app-wide decision needed: (a) adopt a real i18n lib (next-intl, react-i18next) and migrate Sidebar + Settings too, or (b) port Sidebar to the homegrown dict. Deferred — flagged for the team.
import { en } from "./strings/en";

type StringMap = Record<string, string>;

const DICT: StringMap = en;

export function t(key: string, vars?: Record<string, string | number>): string {
    let s = DICT[key] ?? key;
    if (vars) {
        for (const [k, v] of Object.entries(vars)) {
            s = s.replace(new RegExp(`\\{${k}\\}`, "g"), String(v));
        }
    }
    return s;
}
